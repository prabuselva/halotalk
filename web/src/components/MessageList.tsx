import { useEffect, useRef, useState } from "react";
import { rest } from "../rest";
import type { Message } from "../types";
import Markdown from "./Markdown";
import {
  HaloLogo,
  IconAlert,
  IconBrain,
  IconCheck,
  IconCheckDouble,
  IconChevron,
  IconClock,
  IconPencil,
  IconRefresh,
  IconSpeaker,
} from "./Icons";

export type PendingStatus = "queued" | "sending" | "sent" | "failed";
export type PendingItem = { id: string; content: string; images: string[]; status: PendingStatus };

function StatusTick({ status, delivered }: { status?: PendingStatus; delivered?: boolean }) {
  if (status === "queued")
    return (
      <span data-testid="tick-queued" title="Queued" className="text-muted">
        <IconClock className="h-3.5 w-3.5" />
      </span>
    );
  if (status === "sending")
    return (
      <span data-testid="tick-sending" title="Sending…" className="text-muted">
        <IconCheck className="h-3.5 w-3.5" />
      </span>
    );
  if (status === "failed")
    return (
      <span data-testid="tick-failed" title="Failed — retry?" className="text-danger">
        <IconAlert className="h-3.5 w-3.5" />
      </span>
    );
  if (status === "sent" || delivered)
    return (
      <span
        data-testid={delivered ? "tick-delivered" : "tick-sent"}
        title={delivered ? "Delivered" : "Sent"}
        className={delivered ? "text-accent" : "text-muted"}
      >
        <IconCheckDouble className="h-3.5 w-3.5" />
      </span>
    );
  return null;
}

function Reasoning({ text, streaming }: { text: string; streaming?: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="mb-2 overflow-hidden rounded-lg border border-line bg-surface-2">
      <button
        className="flex w-full items-center gap-1.5 px-2.5 py-1.5 text-xs font-medium text-muted hover:text-ink"
        onClick={() => setOpen((o) => !o)}
      >
        <IconBrain className="h-3.5 w-3.5" />
        {streaming && !text ? "Thinking…" : "Thought for a moment"}
        <IconChevron
          className={`ml-auto h-3.5 w-3.5 transition-transform ${open ? "rotate-90" : ""}`}
        />
      </button>
      {open && (
        <div className="border-t border-line px-3 py-2">
          <pre className="whitespace-pre-wrap font-sans text-xs leading-relaxed text-muted">
            {text}
          </pre>
        </div>
      )}
    </div>
  );
}

function Images({ refs }: { refs: string[] }) {
  return (
    <div className="mb-2 flex flex-wrap gap-2">
      {refs.map((src, i) => (
        <a key={i} href={src} target="_blank" rel="noreferrer" className="block">
          <img
            src={src}
            alt={`attachment ${i + 1}`}
            className="max-h-44 max-w-[220px] cursor-zoom-in rounded-lg border border-line object-cover"
          />
        </a>
      ))}
    </div>
  );
}

function MessageRow({
  m,
  streaming,
  canEdit,
  branch,
  ttsReady,
  playing,
  delivered,
  onEdit,
  onActivate,
  onSpeak,
}: {
  m: Message;
  streaming: boolean;
  canEdit: boolean;
  branch: { ids: string[]; index: number } | null;
  ttsReady: boolean;
  playing: boolean;
  delivered?: boolean;
  onEdit: (id: string, content: string) => void;
  onActivate: (id: string) => void;
  onSpeak: (id: string, text: string, isPlaying: boolean) => void;
}) {
  const isUser = m.role === "user";
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(m.content);

  const saveEdit = () => {
    const text = draft.trim();
    setEditing(false);
    if (text && text !== m.content) onEdit(m.id, text);
  };

  if (m.role === "tool") {
    const preview = m.content.length > 140 ? `${m.content.slice(0, 140)}…` : m.content;
    return (
      <div className="flex justify-center py-1.5">
        <span
          title={m.content}
          className="max-w-[82%] truncate rounded-full border border-line bg-surface-2 px-3 py-1 text-[0.7rem] text-muted"
        >
          tool result · {preview}
        </span>
      </div>
    );
  }

  return (
    <div className={`group flex gap-3 py-4 ${isUser ? "justify-end" : ""}`}>
      {!isUser && (
        <div className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-accent-soft">
          <HaloLogo className="h-4 w-4" />
        </div>
      )}
      <div className={`min-w-0 max-w-[82%] ${isUser ? "order-1" : ""}`}>
        {!isUser && m.reasoning ? <Reasoning text={m.reasoning} streaming={streaming} /> : null}
        {!isUser && !m.content && streaming && !m.reasoning ? (
          <div className="text-sm text-muted">Thinking…</div>
        ) : null}
        {m.images.length > 0 && <Images refs={m.images} />}
        {editing ? (
          <div className="rounded-2xl rounded-br-md bg-accent-soft px-3.5 py-2.5">
            <textarea
              className="w-full resize-none bg-transparent text-sm leading-relaxed outline-none"
              rows={Math.min(10, Math.max(2, draft.split("\n").length + 1))}
              value={draft}
              autoFocus
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  saveEdit();
                } else if (e.key === "Escape") setEditing(false);
              }}
            />
            <div className="mt-1.5 flex justify-end gap-2 text-xs">
              <button
                className="rounded-md px-2 py-1 text-muted hover:text-ink"
                onClick={() => setEditing(false)}
              >
                Cancel
              </button>
              <button
                className="rounded-md bg-accent px-2.5 py-1 font-medium text-white hover:bg-accent-strong"
                onClick={saveEdit}
              >
                Save &amp; resend
              </button>
            </div>
          </div>
        ) : (
          m.content &&
          (isUser ? (
            <div className="rounded-2xl rounded-br-md bg-accent-soft px-3.5 py-2.5 text-sm leading-relaxed">
              <Markdown>{m.content}</Markdown>
              {delivered && (
                <div className="mt-0.5 flex justify-end">
                  <StatusTick delivered />
                </div>
              )}
            </div>
          ) : (
            <div className={`text-sm ${streaming ? "caret" : ""}`}>
              <Markdown streaming={streaming}>{m.content}</Markdown>
            </div>
          ))
        )}
        {m.error && (
          <div className="mt-1.5 rounded-lg border !border-danger/40 bg-danger/10 px-3 py-2 text-xs text-danger">
            {m.error}
          </div>
        )}
        {!isUser && m.sources && m.sources.length > 0 && (
          <div className="mt-1.5 flex flex-wrap items-center gap-1">
            <span className="text-[0.65rem] text-muted">sources:</span>
            {m.sources.map((s) => (
              <span
                key={s.title}
                title={`relevance ${s.score}`}
                className="rounded-full border border-line bg-surface-2 px-2 py-0.5 text-[0.65rem] text-muted"
              >
                {s.title}
              </span>
            ))}
          </div>
        )}
        {!isUser && !streaming && ttsReady && m.content.trim() && (
          <div className="mt-1 flex justify-end">
            <button
              className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-[0.65rem] transition-colors hover:text-ink ${
                playing ? "text-accent" : "text-muted opacity-0 group-hover:opacity-100"
              }`}
              title={playing ? "Stop reading" : "Read aloud"}
              onClick={() => onSpeak(m.id, m.content, playing)}
            >
              <IconSpeaker className="h-3 w-3" /> {playing ? "Stop" : "Speak"}
            </button>
          </div>
        )}
        {!isUser && branch && branch.ids.length > 1 && (
          <div className="mt-1 flex items-center gap-0.5 text-[0.68rem] text-muted">
            <button
              className="rounded px-1.5 py-0.5 transition-colors hover:bg-surface-2 hover:text-ink disabled:pointer-events-none disabled:opacity-30"
              aria-label="Previous response"
              disabled={branch.index === 0}
              onClick={() => onActivate(branch.ids[branch.index - 1])}
            >
              ‹
            </button>
            <span className="min-w-8 text-center tabular-nums">
              {branch.index + 1}/{branch.ids.length}
            </span>
            <button
              className="rounded px-1.5 py-0.5 transition-colors hover:bg-surface-2 hover:text-ink disabled:pointer-events-none disabled:opacity-30"
              aria-label="Next response"
              disabled={branch.index === branch.ids.length - 1}
              onClick={() => onActivate(branch.ids[branch.index + 1])}
            >
              ›
            </button>
          </div>
        )}
        {isUser && !editing && canEdit && (
          <div className="mt-1 flex justify-end">
            <button
              className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[0.65rem] text-muted opacity-0 transition-opacity hover:text-ink group-hover:opacity-100"
              title="Edit and resend from here"
              onClick={() => {
                setDraft(m.content);
                setEditing(true);
              }}
            >
              <IconPencil className="h-3 w-3" /> Edit
            </button>
          </div>
        )}
      </div>
      {isUser && (
        <div className="mt-0.5 hidden h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-surface-2 text-[0.65rem] font-semibold text-muted sm:flex">
          You
        </div>
      )}
    </div>
  );
}

type ShownMessage = { m: Message; branch: { ids: string[]; index: number } | null };

function PendingRow({ p, onRetry }: { p: PendingItem; onRetry: (id: string) => void }) {
  return (
    <div data-testid="pending-row" className="flex justify-end gap-3 py-4 opacity-80">
      <div className="min-w-0 max-w-[82%] rounded-2xl rounded-br-md bg-accent-soft px-3.5 py-2.5 text-sm leading-relaxed">
        {p.images.length > 0 && <Images refs={p.images} />}
        <div className="whitespace-pre-wrap break-words">{p.content}</div>
        <div className="mt-0.5 flex items-center justify-end gap-2">
          <StatusTick status={p.status} />
          {p.status === "failed" && (
            <button
              data-testid="pending-retry"
              className="text-[0.65rem] font-semibold text-accent underline"
              onClick={() => onRetry(p.id)}
            >
              Retry
            </button>
          )}
        </div>
      </div>
      <div className="mt-0.5 hidden h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-surface-2 text-[0.65rem] font-semibold text-muted sm:flex">
        You
      </div>
    </div>
  );
}

/// Hide inactive branch siblings and annotate the active one with its pager
/// position (`‹ i/N ›`). Legacy replies without a parent render unpaged.
function buildView(messages: Message[]): ShownMessage[] {
  const groups = new Map<string, string[]>();
  for (const m of messages) {
    if (m.role === "assistant" && m.reply_to) {
      const g = groups.get(m.reply_to);
      if (g) g.push(m.id);
      else groups.set(m.reply_to, [m.id]);
    }
  }
  const out: ShownMessage[] = [];
  for (const m of messages) {
    if (m.role === "assistant" && m.reply_to) {
      if (!m.active) continue;
      const ids = groups.get(m.reply_to) ?? [m.id];
      out.push({ m, branch: { ids, index: Math.max(0, ids.indexOf(m.id)) } });
    } else {
      out.push({ m, branch: null });
    }
  }
  return out;
}

type Props = {
  messages: Message[];
  streamingId: string | null;
  ttsReady: boolean;
  pending: PendingItem[];
  delivered: Set<string>;
  onRetryPending: (id: string) => void;
  onRegenerate: () => void;
  onEditMessage: (id: string, content: string) => void;
  onActivate: (id: string) => void;
  onToast: (msg: string) => void;
};

export default function MessageList({
  messages,
  streamingId,
  ttsReady,
  pending,
  delivered,
  onRetryPending,
  onRegenerate,
  onEditMessage,
  onActivate,
  onToast,
}: Props) {
  const anchor = useRef<HTMLDivElement>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
  const [pinned, setPinned] = useState(true);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    if (atBottom !== pinnedRef.current) {
      pinnedRef.current = atBottom;
      setPinned(atBottom);
    }
  };

  const jumpToLatest = () => {
    pinnedRef.current = true;
    setPinned(true);
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: "smooth" });
  };
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [playingId, setPlayingId] = useState<string | null>(null);

  const stopVoice = () => {
    audioRef.current?.pause();
    audioRef.current = null;
    setPlayingId(null);
  };

  const speak = async (id: string, text: string, isPlaying: boolean) => {
    if (isPlaying) {
      stopVoice();
      return;
    }
    stopVoice();
    setPlayingId(id);
    try {
      const blob = await rest.speech(text);
      const url = URL.createObjectURL(blob);
      const audio = new Audio(url);
      audio.onended = () => {
        URL.revokeObjectURL(url);
        audioRef.current = null;
        setPlayingId(null);
      };
      audio.onerror = () => {
        URL.revokeObjectURL(url);
        audioRef.current = null;
        setPlayingId(null);
        onToast("Audio playback failed");
      };
      audioRef.current = audio;
      await audio.play();
    } catch (e) {
      setPlayingId(null);
      onToast(`TTS: ${(e as Error).message}`);
    }
  };

  useEffect(() => stopVoice, []);

  useEffect(() => {
    if (pinnedRef.current) anchor.current?.scrollIntoView({ block: "end" });
  }, [messages, pending, streamingId]);

  const firstMsgId = messages[0]?.id ?? null;
  useEffect(() => {
    pinnedRef.current = true;
    setPinned(true);
    requestAnimationFrame(() => anchor.current?.scrollIntoView({ block: "end" }));
  }, [firstMsgId]);

  const view = buildView(messages);
  const last = view[view.length - 1]?.m;
  const canRegenerate = !streamingId && last?.role === "assistant";
  const busy = streamingId !== null;

  return (
    <div
      ref={scroller}
      onScroll={onScroll}
      data-testid="message-scroll"
      className="relative mx-auto w-full max-w-3xl flex-1 overflow-y-auto px-4 py-2 xl:max-w-4xl 2xl:max-w-5xl"
    >
      {view.map(({ m, branch }) => (
        <MessageRow
          key={m.id}
          m={m}
          streaming={m.id === streamingId}
          canEdit={!busy}
          branch={branch}
          ttsReady={ttsReady}
          playing={playingId === m.id}
          delivered={delivered.has(m.id)}
          onEdit={onEditMessage}
          onActivate={onActivate}
          onSpeak={(id, text, isPlaying) => void speak(id, text, isPlaying)}
        />
      ))}
      {pending.map((p) => (
        <PendingRow key={p.id} p={p} onRetry={onRetryPending} />
      ))}
      {canRegenerate && (
        <div className="flex justify-center pb-2">
          <button
            className="flex items-center gap-1.5 rounded-full border border-line bg-surface px-3 py-1.5 text-xs text-muted shadow-sm transition-colors hover:border-accent/50 hover:text-accent"
            onClick={onRegenerate}
          >
            <IconRefresh className="h-3.5 w-3.5" /> Regenerate
          </button>
        </div>
      )}
      {!pinned && (
        <div className="sticky bottom-3 z-10 flex h-0 justify-center">
          <button
            onClick={jumpToLatest}
            className="pointer-events-auto rounded-full border border-line bg-bg px-3 py-1.5 text-xs font-medium text-accent shadow-md hover:bg-surface"
          >
            ↓ Jump to latest
          </button>
        </div>
      )}
      <div className="h-6" ref={anchor} />
    </div>
  );
}
