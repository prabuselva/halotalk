//! Per-chat event hub: generation runs detached from any HTTP connection and
//! fans frames out to every subscriber (multi-device live sync). A reconnecting
//! client re-attaches with a snapshot + sequence guard so no deltas are lost
//! or duplicated when a mobile tab sleeps, switches apps, or blips offline.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};
use tokio::sync::{broadcast, watch};

const CAP: usize = 2048;
/// Throttled snapshot cadence for reconnecting subscribers.
const SNAP_MS: u128 = 600;

/// One live generation turn. `id` is `None` for an idle placeholder created by
/// a viewer subscribing before anyone started a turn.
#[derive(Clone, Debug)]
pub struct Turn {
    pub id: Option<String>,
    tx: broadcast::Sender<(String, String)>,
    stop_tx: watch::Sender<bool>,
    stop_rx: watch::Receiver<bool>,
    seq: Arc<AtomicU64>,
    snap: Arc<Mutex<Snapshot>>,
}

#[derive(Debug)]
struct Snapshot {
    seq: u64,
    content: String,
    reasoning: String,
    at: Instant,
}

impl Turn {
    fn blank() -> Self {
        let (tx, _rx) = broadcast::channel(CAP);
        let (stop_tx, stop_rx) = watch::channel(false);
        Self {
            id: None,
            tx,
            stop_tx,
            stop_rx,
            seq: Arc::new(AtomicU64::new(0)),
            snap: Arc::new(Mutex::new(Snapshot {
                seq: 0,
                content: String::new(),
                reasoning: String::new(),
                at: Instant::now(),
            })),
        }
    }

    pub fn live(&self) -> bool {
        self.id.is_some()
    }

    pub fn stopping(&self) -> bool {
        *self.stop_rx.borrow()
    }

    /// Resolves once stop is requested (used with `tokio::select!` for prompt
    /// cancellation even while a slow upstream chunk is pending).
    pub async fn stop_signal(&self) {
        let mut rx = self.stop_rx.clone();
        loop {
            if *rx.borrow_and_update() {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    /// Broadcast a frame (no receiver, no problem — subscribers are best-effort;
    /// lagged/closed subscribers catch up via snapshot on reconnect).
    pub fn emit(&self, event: &str, data: impl Into<Value>) {
        let _ = self.tx.send((event.to_string(), data.into().to_string()));
    }

    /// Emit a content/reasoning delta with a monotonic sequence number so a
    /// reconnecting client can drop frames already covered by its snapshot.
    pub fn emit_delta(&self, event: &str, delta: &str, content: &str, reasoning: &str) {
        let n = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        {
            let mut s = self.snap.lock().unwrap();
            if event == "token" {
                s.content = content.to_string();
            } else {
                s.reasoning = reasoning.to_string();
            }
            s.seq = n;
            let fresh = s.at.elapsed().as_millis() >= SNAP_MS;
            if fresh {
                s.at = Instant::now();
            }
        }
        self.emit(event, json!({ "delta": delta, "seq": n }));
    }

    pub fn snapshot_frame(&self) -> Value {
        let s = self.snap.lock().unwrap();
        json!({
            "streaming": true,
            "message_id": self.id,
            "seq": s.seq,
            "content": s.content,
            "reasoning": s.reasoning,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<(String, String)> {
        self.tx.subscribe()
    }
}

pub struct Hub {
    inner: Mutex<HashMap<String, Turn>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn current(&self, chat_id: &str) -> Option<Turn> {
        self.inner.lock().unwrap().get(chat_id).cloned()
    }

    /// Live (assistant-attached) turn for a chat, if generation is running.
    pub fn live_turn(&self, chat_id: &str) -> Option<Turn> {
        self.current(chat_id).filter(|t| t.live())
    }

    /// Begin a turn. Any idle placeholder (or stale entry) is replaced; its
    /// subscribers see their stream close and re-attach to the live turn.
    /// Returns `Err(current)` if a generation is already running for the chat.
    pub fn start(&self, chat_id: &str, message_id: &str) -> Result<Turn, Turn> {
        let mut map = self.inner.lock().unwrap();
        if let Some(cur) = map.get(chat_id)
            && cur.live()
        {
            return Err(cur.clone());
        }
        let (tx, _rx) = broadcast::channel(CAP);
        let (stop_tx, stop_rx) = watch::channel(false);
        let turn = Turn {
            id: Some(message_id.to_string()),
            tx,
            stop_tx,
            stop_rx,
            seq: Arc::new(AtomicU64::new(0)),
            snap: Arc::new(Mutex::new(Snapshot {
                seq: 0,
                content: String::new(),
                reasoning: String::new(),
                at: Instant::now(),
            })),
        };
        map.insert(chat_id.to_string(), turn.clone());
        Ok(turn)
    }

    /// Viewer joined with no turn running: install an idle placeholder that
    /// keeps the SSE open until a real turn replaces it.
    pub fn join_idle(&self, chat_id: &str) -> Turn {
        let mut map = self.inner.lock().unwrap();
        map.entry(chat_id.to_string())
            .or_insert_with(Turn::blank)
            .clone()
    }

    /// End a turn (only if it is still the current one), releasing the map's
    /// sender so idle-held subscribers close and reconnect.
    pub fn finish(&self, chat_id: &str, turn: &Turn) {
        let mut map = self.inner.lock().unwrap();
        let same = map
            .get(chat_id)
            .is_some_and(|c| c.tx.same_channel(&turn.tx));
        if same {
            map.remove(chat_id);
        }
    }

    /// Request stop for the running turn; `true` when a live turn was flagged.
    pub fn stop(&self, chat_id: &str) -> bool {
        if let Some(t) = self.current(chat_id)
            && t.live()
        {
            t.stop_tx.send_replace(true);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_rejects_second_turn_and_finish_releases() {
        let hub = Hub::new();
        let t = hub.start("c1", "m1").unwrap();
        assert!(hub.start("c1", "m2").is_err());
        t.emit("token", json!({ "delta": "x" }));
        let mut rx = t.subscribe();
        t.emit("token", json!({ "delta": "y" }));
        let (ev, data) = rx.recv().await.unwrap();
        assert_eq!(ev, "token");
        assert!(data.contains("y"));
        hub.finish("c1", &t);
        assert!(hub.current("c1").is_none());
        // Once every sender is gone (hub entry + producer), the stream closes
        // and subscribers reconnect.
        drop(t);
        assert!(rx.recv().await.is_err());
        assert!(hub.start("c1", "m3").is_ok());
    }

    #[tokio::test]
    async fn join_idle_is_replaced_by_live_turn() {
        let hub = Hub::new();
        let idle = hub.join_idle("c1");
        assert!(!idle.live());
        assert!(hub.join_idle("c1").id.is_none());
        let live = hub.start("c1", "m1").unwrap();
        assert_eq!(hub.live_turn("c1").unwrap().id.as_deref(), Some("m1"));
        // Idle subscriber's sender gets dropped on replacement.
        let mut rx = idle.subscribe();
        drop(idle);
        assert!(rx.recv().await.is_err());
        assert!(!live.stopping());
    }

    #[tokio::test]
    async fn seq_snapshot_and_replay_guard() {
        let hub = Hub::new();
        let mut content = String::new();
        let t = hub.start("c1", "m1").unwrap();
        let mut rx = t.subscribe();
        for w in ["a", "b", "c"] {
            content.push_str(w);
            t.emit_delta("token", w, &content, "");
        }
        let mut last_seq = 0;
        for _ in 0..3 {
            let (_ev, data): (_, String) = rx.recv().await.unwrap();
            last_seq = serde_json::from_str::<Value>(&data).unwrap()["seq"]
                .as_u64()
                .unwrap();
        }
        assert_eq!(last_seq, 3);
        let snap = t.snapshot_frame();
        assert_eq!(snap["seq"], 3);
        assert_eq!(snap["content"], "abc");
        // A client whose snapshot says seq=3 would drop every delivered frame;
        // only frames beyond 3 apply.
        t.emit_delta("token", "d", &format!("{content}d"), "");
        let (_ev, data) = rx.recv().await.unwrap();
        let n = serde_json::from_str::<Value>(&data).unwrap()["seq"]
            .as_u64()
            .unwrap();
        assert!(n > last_seq);
    }

    #[tokio::test]
    async fn stop_flags_turn() {
        let hub = Hub::new();
        let t = hub.start("c1", "m1").unwrap();
        assert!(!t.stopping());
        assert!(hub.stop("c1"));
        assert!(t.stopping());
        hub.finish("c1", &t);
        assert!(!hub.stop("c1"));
    }
}
