import { ChangeEvent, ClipboardEvent, DragEvent, KeyboardEvent, useMemo, useRef, useState } from "react";
import { rest } from "../rest";
import type { Prompt } from "../types";
import { IconImage, IconMic, IconSend, IconStop, IconTag, IconX } from "./Icons";

export type Attachment = { name: string; dataUrl: string };

const MAX_IMAGES = 4;
const MAX_BYTES = 6 * 1024 * 1024;

type Props = {
  queueCount: number;
  onClearQueue: () => void;
  streaming: boolean;
  modelMissing: boolean;
  visionBlocked: boolean;
  prompts: Prompt[];
  sttReady: boolean;
  onSend: (content: string, images: string[]) => void;
  onStop: () => void;
  onFixSettings: () => void;
  onToast: (msg: string) => void;
};

export default function Composer({
  queueCount,
  onClearQueue,
  streaming,
  modelMissing,
  visionBlocked,
  prompts,
  sttReady,
  onSend,
  onStop,
  onFixSettings,
  onToast,
}: Props) {
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [slashIdx, setSlashIdx] = useState(0);
  const [recording, setRecording] = useState(false);
  const [sttBusy, setSttBusy] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const recRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);

  // `/`-menu: open while the text is a single `/query` with at least one matching
  // prompt title. Enter/Tab inserts the prompt body, arrows navigate, Esc closes.
  const slash = text.startsWith("/") && !text.slice(1).includes(" ")
    ? prompts.filter((p) => p.title.toLowerCase().includes(text.slice(1).toLowerCase()))
    : null;
  const menuOpen = slash !== null && slash.length > 0;
  const shown = slash ? slash.slice(0, 8) : [];
  const idx = useMemo(
    () => (shown.length > 0 ? Math.min(slashIdx, shown.length - 1) : 0),
    [shown.length, slashIdx],
  );

  const insertPrompt = (p: Prompt) => {
    setText(p.body);
    if (areaRef.current) {
      areaRef.current.style.height = "auto";
      autoGrow();
      areaRef.current.focus();
    }
  };

  const addFiles = (files: FileList | File[]) => {
    if (visionBlocked) {
      onToast("Selected model does not advertise vision — images cannot be sent");
      return;
    }
    const list = Array.from(files).filter((f) => f.type.startsWith("image/"));
    const next = [...attachments];
    for (const f of list) {
      if (next.length >= MAX_IMAGES) {
        onToast(`Max ${MAX_IMAGES} images per message`);
        break;
      }
      if (f.size > MAX_BYTES) {
        onToast(`${f.name} is larger than 6 MB`);
        continue;
      }
      const reader = new FileReader();
      reader.onload = () =>
        setAttachments((a) =>
          a.length < MAX_IMAGES ? [...a, { name: f.name, dataUrl: String(reader.result) }] : a,
        );
      reader.readAsDataURL(f);
    }
  };

  const onPasteFiles = (e: ClipboardEvent) => {
    if (e.clipboardData?.files?.length) {
      e.preventDefault();
      addFiles(e.clipboardData.files);
    }
  };

  const onDrop = (e: DragEvent) => {
    e.preventDefault();
    if (e.dataTransfer?.files?.length) addFiles(e.dataTransfer.files);
  };

  const canSend = !modelMissing && (text.trim().length > 0 || attachments.length > 0);

  const send = () => {
    if (!canSend) return;
    onSend(
      text.trim(),
      attachments.map((a) => a.dataUrl),
    );
    setText("");
    setAttachments([]);
    if (areaRef.current) areaRef.current.style.height = "auto";
  };

  const onKey = (e: KeyboardEvent) => {
    if (menuOpen && slash) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSlashIdx((idx + 1) % slash.length);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSlashIdx((idx - 1 + slash.length) % slash.length);
        return;
      }
      if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
        e.preventDefault();
        insertPrompt(slash[idx]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setText((t) => t.slice(1));
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  };

  const onPick = (e: ChangeEvent<HTMLInputElement>) => {
    if (e.target.files) addFiles(e.target.files);
    e.target.value = "";
  };

  const autoGrow = () => {
    const el = areaRef.current;
    if (!el) return;
    const max = Math.round(Math.max(140, Math.min(420, window.innerHeight * 0.35)));
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, max)}px`;
    el.style.maxHeight = `${max}px`;
  };

  const startRec = async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const rec = new MediaRecorder(stream);
      chunksRef.current = [];
      rec.ondataavailable = (e) => e.data.size > 0 && chunksRef.current.push(e.data);
      rec.onstop = async () => {
        stream.getTracks().forEach((t) => t.stop());
        const blob = new Blob(chunksRef.current, { type: rec.mimeType || "audio/webm" });
        setRecording(false);
        recRef.current = null;
        if (blob.size === 0) return;
        setSttBusy(true);
        try {
          const transcript = await rest.transcribe(blob, "speech.webm");
          if (transcript.trim()) {
            setText((t) => (t.trim() ? `${t.trim()} ${transcript.trim()}` : transcript.trim()));
            areaRef.current?.focus();
          } else {
            onToast("No speech recognized");
          }
        } catch (e) {
          onToast(`Transcription: ${(e as Error).message}`);
        } finally {
          setSttBusy(false);
        }
      };
      recRef.current = rec;
      rec.start();
      setRecording(true);
    } catch {
      onToast("Microphone unavailable or permission denied");
    }
  };

  return (
    <div className="px-4 pb-4 pt-1">
      <div className="mx-auto w-full max-w-3xl xl:max-w-4xl 2xl:max-w-5xl">
        {modelMissing && (
          <div className="mb-2 flex items-center gap-2 rounded-lg border border-accent/40 bg-accent-soft px-3 py-2 text-xs">
            <span>No model selected for this chat.</span>
            <button className="font-semibold text-accent underline" onClick={onFixSettings}>
              Open settings
            </button>
          </div>
        )}
        {menuOpen && (
          <div className="mb-1.5 overflow-hidden rounded-xl border border-line bg-surface shadow-lg">
            <div className="px-3 pb-1 pt-2 text-[0.6rem] font-semibold uppercase tracking-wider text-muted">
              Prompts
            </div>
            {shown.map((p, i) => (
              <button
                key={p.id}
                className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm transition-colors ${
                  i === idx ? "bg-accent-soft text-ink" : "text-muted hover:bg-surface-2"
                }`}
                onMouseEnter={() => setSlashIdx(i)}
                onClick={() => insertPrompt(p)}
              >
                <IconTag className="h-3.5 w-3.5 shrink-0 text-accent" />
                <span className="shrink-0 font-medium">{p.title}</span>
                <span className="min-w-0 flex-1 truncate text-[0.7rem] text-muted">
                  {p.body.replace(/\s+/g, " ").slice(0, 90)}
                </span>
              </button>
            ))}
          </div>
        )}
        <div
          className="rounded-2xl border border-line bg-surface p-2 shadow-sm transition-shadow focus-within:border-accent/60"
          onDrop={onDrop}
          onDragOver={(e) => e.preventDefault()}
        >
          {attachments.length > 0 && (
            <div className="flex flex-wrap gap-2 px-1.5 pb-2">
              {attachments.map((a, i) => (
                <div key={i} className="relative">
                  <img
                    src={a.dataUrl}
                    alt={a.name}
                    className="h-14 w-14 rounded-lg border border-line object-cover"
                  />
                  <button
                    className="absolute -right-1.5 -top-1.5 rounded-full bg-surface-2 p-0.5 text-muted shadow hover:text-danger"
                    aria-label={`Remove ${a.name}`}
                    onClick={() => setAttachments((att) => att.filter((_, j) => j !== i))}
                  >
                    <IconX className="h-3 w-3" />
                  </button>
                </div>
              ))}
            </div>
          )}
          <div className="flex items-end gap-2">
            {!visionBlocked && (
              <>
                <button
                  className="rounded-lg p-2 text-muted transition-colors hover:bg-surface-2 hover:text-ink"
                  aria-label="Attach image"
                  title="Attach image"
                  onClick={() => fileRef.current?.click()}
                >
                  <IconImage />
                </button>
                <input
                  ref={fileRef}
                  type="file"
                  accept="image/*"
                  multiple
                  className="hidden"
                  onChange={onPick}
                />
              </>
            )}
            {sttReady &&
              (recording ? (
                <button
                  className="flex animate-pulse items-center gap-1.5 rounded-lg bg-danger px-2.5 py-2 text-xs font-medium text-white transition-colors hover:opacity-90"
                  aria-label="Stop recording"
                  title="Stop recording"
                  onClick={() => recRef.current?.stop()}
                >
                  <IconStop className="h-4 w-4" /> Record
                </button>
              ) : (
                <button
                  className="rounded-lg p-2 text-muted transition-colors hover:bg-surface-2 hover:text-ink disabled:cursor-not-allowed disabled:opacity-40"
                  aria-label="Dictate with microphone"
                  title={sttBusy ? "Transcribing…" : "Dictate"}
                  disabled={sttBusy}
                  onClick={() => void startRec()}
                >
                  <IconMic />
                </button>
              ))}
            <textarea
              ref={areaRef}
              rows={1}
              className="max-h-[40vh] flex-1 resize-none bg-transparent py-2 text-sm outline-none placeholder:text-muted"
              placeholder={
                streaming
                  ? "Keep typing — Enter queues your next message…"
                  : "Message HaloTalk — / for prompts, images via paste/drop"
              }
              value={text}
              onChange={(e) => {
                setText(e.target.value);
                setSlashIdx(0);
                autoGrow();
              }}
              onPaste={onPasteFiles}
              onKeyDown={onKey}
            />
            {streaming && (
              <button
                className="rounded-lg bg-danger p-2 text-white transition-colors hover:opacity-90"
                aria-label="Stop generating"
                title="Stop generating"
                onClick={onStop}
              >
                <IconStop />
              </button>
            )}
            <button
              className="rounded-lg bg-accent p-2 text-white transition-colors hover:bg-accent-strong disabled:cursor-not-allowed disabled:opacity-40"
              aria-label={streaming ? "Queue message" : "Send message"}
              title={streaming ? "Queue this message (sends when the reply finishes)" : "Send"}
              disabled={!canSend}
              onClick={send}
            >
              <IconSend />
            </button>
          </div>
        </div>
        <p className="mt-1.5 text-center text-[0.65rem] text-muted">
          {queueCount > 0 ? (
            <>
              {queueCount} queued — sent as soon as the reply finishes ·{" "}
              <button className="font-semibold text-accent underline" onClick={onClearQueue}>
                clear queue
              </button>
            </>
          ) : (
            <>
              Enter to send · Shift+Enter for a new line · / for prompts · images ≤ {MAX_IMAGES}, ≤{" "}
              6 MB each
            </>
          )}
        </p>
      </div>
    </div>
  );
}
