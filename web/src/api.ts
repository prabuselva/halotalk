import type { Message } from "./types";

export type StateFrame = {
  streaming: boolean;
  message_id?: string | null;
  seq?: number;
  content?: string;
  reasoning?: string;
};

export type StreamHandlers = {
  onState?: (s: StateFrame) => void;
  onUser?: (message: Message) => void;
  onResync?: (info: { reason?: string }) => void;
  onMeta?: (meta: { chat_id: string; message_id: string; title: string }) => void;
  onReasoning?: (delta: string, seq: number) => void;
  onToken?: (delta: string, seq: number) => void;
  onTitle?: (title: string) => void;
  onTool?: (tool: {
    phase: "call" | "result";
    id: string;
    name?: string;
    arguments?: string;
    ok?: boolean;
    preview?: string;
  }) => void;
  onSources?: (sources: { title: string; score: number }[]) => void;
  onDone?: () => void;
  onError?: (message: string) => void;
};

async function post(path: string, body?: unknown): Promise<void> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body ?? {}),
  });
  if (res.status === 401) window.dispatchEvent(new Event("halo-unauthorized"));
  if (!res.ok) {
    let msg = `request failed (${res.status})`;
    try {
      const j = await res.json();
      if (j?.error) msg = j.error;
    } catch {
      /* keep status message */
    }
    throw new Error(msg);
  }
}

export function sendMessage(chatId: string, content: string, images: string[]): Promise<void> {
  return post(`/api/chats/${encodeURIComponent(chatId)}/messages`, { content, images });
}

export function regenerateChat(chatId: string): Promise<void> {
  return post(`/api/chats/${encodeURIComponent(chatId)}/regenerate`, {});
}

export function editUserMessage(
  chatId: string,
  messageId: string,
  content: string,
): Promise<void> {
  return post(`/api/chats/${encodeURIComponent(chatId)}/messages/${encodeURIComponent(messageId)}/edit`, {
    content,
  });
}

export function stopGeneration(chatId: string): Promise<void> {
  return post(`/api/chats/${encodeURIComponent(chatId)}/stop`);
}

/// Live view of a chat's event bus. Generation is device-decoupled server-side;
/// this client just watches. The first `state` frame of every (re)connection
/// carries a snapshot + `seq` floor so deltas already reflected are skipped;
/// the browser's EventSource reconnects automatically (manual retry when it
/// gives up, e.g. after a fatal status).
export function subscribeChatEvents(chatId: string, h: StreamHandlers): () => void {
  let es: EventSource | null = null;
  let timer: number | undefined;
  let closed = false;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const parse = (ev: Event): any => {
    const data = (ev as MessageEvent).data;
    if (typeof data !== "string" || !data) return {};
    try {
      return JSON.parse(data);
    } catch {
      return {};
    }
  };
  const open = () => {
    if (closed) return;
    es = new EventSource(`/api/chats/${encodeURIComponent(chatId)}/events`);
    es.addEventListener("state", (e) => h.onState?.(parse(e) as StateFrame));
    es.addEventListener("user", (e) => h.onUser?.(parse(e) as Message));
    es.addEventListener("resync", (e) => h.onResync?.({ reason: parse(e).reason }));
    es.addEventListener("meta", (e) =>
      h.onMeta?.(parse(e) as { chat_id: string; message_id: string; title: string }),
    );
    es.addEventListener("reasoning", (e) => {
      const d = parse(e);
      h.onReasoning?.(String(d.delta ?? ""), Number(d.seq ?? 0));
    });
    es.addEventListener("token", (e) => {
      const d = parse(e);
      h.onToken?.(String(d.delta ?? ""), Number(d.seq ?? 0));
    });
    es.addEventListener("title", (e) => h.onTitle?.(String(parse(e).title ?? "")));
    es.addEventListener("tool", (e) => h.onTool?.(parse(e)));
    es.addEventListener("sources", (e) => {
      const d = parse(e);
      h.onSources?.(Array.isArray(d) ? d : []);
    });
    es.addEventListener("error", (e) => h.onError?.(String(parse(e).message ?? "")));
    es.addEventListener("done", () => h.onDone?.());
    es.onerror = () => {
      if (closed) return;
      if (es && es.readyState === EventSource.CONNECTING) return; // browser retries
      es?.close();
      es = null;
      timer = window.setTimeout(open, 3000);
    };
  };
  open();
  return () => {
    closed = true;
    window.clearTimeout(timer);
    es?.close();
  };
}
