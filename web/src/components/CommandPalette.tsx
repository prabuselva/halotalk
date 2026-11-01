import { useEffect, useMemo, useRef, useState } from "react";
import { rest } from "../rest";
import type { Chat, Provider } from "../types";

type Item = {
  id: string;
  group: string;
  label: string;
  hint?: string;
  run: () => void;
};

type Props = {
  open: boolean;
  onClose: () => void;
  chats: Chat[];
  providers: Provider[];
  theme: "light" | "dark";
  onNewChat: () => void;
  onSelectChat: (id: string) => void;
  onToggleTheme: () => void;
  onOpenSettings: () => void;
  onPickModel: (providerId: string, modelId: string) => void;
};

export default function CommandPalette({
  open,
  onClose,
  chats,
  providers,
  theme,
  onNewChat,
  onSelectChat,
  onToggleTheme,
  onOpenSettings,
  onPickModel,
}: Props) {
  const [q, setQ] = useState("");
  const [models, setModels] = useState<{ provider: Provider; models: string[] }[]>([]);
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      setQ("");
      setSel(0);
      return;
    }
    inputRef.current?.focus();
    let stale = false;
    Promise.all(
      providers.map(async (p) => {
        try {
          const rows = await rest.registry(p.id);
          return { provider: p, models: rows.map((r) => r.model_id) };
        } catch {
          return { provider: p, models: [] as string[] };
        }
      }),
    ).then((m) => !stale && setModels(m));
    return () => {
      stale = true;
    };
  }, [open, providers]);

  const items = useMemo<Item[]>(() => {
    const run = (f: () => void) => () => {
      f();
      onClose();
    };
    const all: Item[] = [
      { id: "new", group: "Actions", label: "New chat", run: run(onNewChat) },
      {
        id: "theme",
        group: "Actions",
        label: theme === "dark" ? "Switch to light theme" : "Switch to dark theme",
        run: run(onToggleTheme),
      },
      { id: "settings", group: "Actions", label: "Open settings", run: run(onOpenSettings) },
    ];
    const query = q.trim().toLowerCase();
    const match = (s: string) => !query || s.toLowerCase().includes(query);

    for (const c of chats) {
      if (match(c.title)) {
        all.push({ id: `c-${c.id}`, group: "Chats", label: c.title, run: run(() => onSelectChat(c.id)) });
      }
    }
    for (const pm of models) {
      for (const m of pm.models) {
        if (match(m)) {
          all.push({
            id: `m-${pm.provider.id}-${m}`,
            group: "Models",
            label: m,
            hint: pm.provider.name,
            run: run(() => onPickModel(pm.provider.id, m)),
          });
        }
      }
    }
    return all;
  }, [q, chats, models, theme, onNewChat, onSelectChat, onToggleTheme, onOpenSettings, onPickModel, onClose]);

  useEffect(() => {
    setSel((s) => Math.min(s, Math.max(items.length - 1, 0)));
  }, [items.length]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "ArrowDown") {
        e.preventDefault();
        setSel((s) => (items.length ? (s + 1) % items.length : 0));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSel((s) => (items.length ? (s - 1 + items.length) % items.length : 0));
      } else if (e.key === "Enter") {
        e.preventDefault();
        items[sel]?.run();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, items, sel, onClose]);

  useEffect(() => {
    listRef.current
      ?.querySelector(`[data-idx="${sel}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  if (!open) return null;

  let lastGroup = "";
  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/40 pt-[12vh] px-4"
      onMouseDown={onClose}
    >
      <div
        className="w-full max-w-xl overflow-hidden rounded-xl border border-line bg-surface shadow-2xl"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <input
          ref={inputRef}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="Search chats, models, actions…"
          className="w-full border-b border-line bg-transparent px-4 py-3 text-sm text-ink outline-none placeholder:text-muted"
        />
        <div ref={listRef} className="max-h-[50vh] overflow-y-auto py-1">
          {items.length === 0 && (
            <div className="px-4 py-6 text-center text-sm text-muted">No matches</div>
          )}
          {items.map((it, i) => {
            const header = it.group !== lastGroup ? it.group : null;
            lastGroup = it.group;
            return (
              <div key={it.id}>
                {header && (
                  <div className="px-4 pb-1 pt-2 text-[10px] font-semibold uppercase tracking-wider text-muted">
                    {header}
                  </div>
                )}
                <button
                  data-idx={i}
                  onMouseEnter={() => setSel(i)}
                  onClick={it.run}
                  className={`flex w-full items-center gap-2 px-4 py-2 text-left text-sm ${
                    i === sel ? "bg-surface-2 text-ink" : "text-ink/80"
                  }`}
                >
                  <span className="min-w-0 flex-1 truncate">{it.label}</span>
                  {it.hint && <span className="shrink-0 text-xs text-muted">{it.hint}</span>}
                </button>
              </div>
            );
          })}
        </div>
        <div className="flex gap-3 border-t border-line px-4 py-2 text-[11px] text-muted">
          <span>↑↓ navigate</span>
          <span>↵ select</span>
          <span>esc close</span>
        </div>
      </div>
    </div>
  );
}
