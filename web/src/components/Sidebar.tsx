import { useEffect, useMemo, useState } from "react";
import { rest } from "../rest";
import type { Chat, SearchHit, User } from "../types";
import {
  IconArchive,
  HaloLogo,
  IconMoon,
  IconPencil,
  IconPin,
  IconPlus,
  IconSearch,
  IconSun,
  IconTag,
  IconTrash,
  IconX,
} from "./Icons";

export type ChatGroup = { label: string; chats: Chat[] };

function groupChats(chats: Chat[]): ChatGroup[] {
  const now = new Date();
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  const day = 86_400_000;
  const buckets: ChatGroup[] = [
    { label: "Today", chats: [] },
    { label: "Yesterday", chats: [] },
    { label: "Previous 7 days", chats: [] },
    { label: "Older", chats: [] },
  ];
  for (const c of chats) {
    const t = new Date(c.updated_at).getTime();
    if (t >= startOfToday) buckets[0].chats.push(c);
    else if (t >= startOfToday - day) buckets[1].chats.push(c);
    else if (t >= startOfToday - 7 * day) buckets[2].chats.push(c);
    else buckets[3].chats.push(c);
  }
  const pinned = chats.filter((c) => c.pinned);
  const groups = buckets.filter((b) => b.chats.length > 0);
  return pinned.length > 0 ? [{ label: "Pinned", chats: pinned }, ...groups] : groups;
}

/// Title filter that also understands `#tag` tokens (AND-ed, exact match).
function matchesQuery(c: Chat, query: string): boolean {
  const toks = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (toks.length === 0) return true;
  const title = c.title.toLowerCase();
  return toks.every((t) =>
    t.startsWith("#") ? c.tags.includes(t.slice(1)) : title.includes(t),
  );
}

type Props = {
  chats: Chat[];
  archived: Chat[];
  archivedLoaded: boolean;
  activeId: string | null;
  theme: "light" | "dark";
  user: User;
  openSignup: boolean;
  onSelect: (id: string) => void;
  onNew: () => void;
  onRename: (id: string) => void;
  onDelete: (id: string) => void;
  onTogglePin: (c: Chat) => void;
  onSetTags: (c: Chat) => void;
  onArchive: (c: Chat) => void;
  onUnarchive: (c: Chat) => void;
  onLoadArchived: () => void;
  onToggleTheme: () => void;
  onToggleSignup: (open: boolean) => void;
  onLogout: () => void;
  onCloseMobile?: () => void;
};

export default function Sidebar({
  chats,
  archived,
  archivedLoaded,
  activeId,
  theme,
  user,
  openSignup,
  onSelect,
  onNew,
  onRename,
  onDelete,
  onTogglePin,
  onSetTags,
  onArchive,
  onUnarchive,
  onLoadArchived,
  onToggleTheme,
  onToggleSignup,
  onLogout,
  onCloseMobile,
}: Props) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [archOpen, setArchOpen] = useState(false);
  const groups = useMemo(
    () => groupChats(query.trim() ? chats.filter((c) => matchesQuery(c, query)) : chats),
    [chats, query],
  );

  // Full-text message search (debounced) once the query is meaningful.
  useEffect(() => {
    const q = query.trim();
    if (q.length < 2) {
      setHits([]);
      return;
    }
    let stale = false;
    const timer = window.setTimeout(() => {
      rest
        .search(q)
        .then((h) => !stale && setHits(h))
        .catch(() => !stale && setHits([]));
    }, 250);
    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r border-line bg-surface">
      <div className="flex items-center gap-2.5 px-4 pb-2 pt-4">
        <HaloLogo />
        <div className="min-w-0">
          <div className="text-[0.95rem] font-semibold tracking-tight">HaloTalk</div>
        </div>
        <button
          className="ml-auto rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-ink md:hidden"
          onClick={onCloseMobile}
          aria-label="Close sidebar"
        >
          <IconX />
        </button>
      </div>

      <div className="px-3 pt-2">
        <button
          onClick={onNew}
          className="flex w-full items-center gap-2 rounded-lg bg-accent px-3 py-2 text-sm font-medium text-white shadow-sm transition-colors hover:bg-accent-strong"
        >
          <IconPlus />
          New chat
        </button>
      </div>

      <div className="px-3 py-2.5">
        <div className="relative">
          <IconSearch className="pointer-events-none absolute left-2.5 top-1/2 w-3.5 -translate-y-1/2 text-muted" />
          <input
            className="input !py-1.5 pl-8 text-[0.8rem]"
            placeholder="Search chats, #tags, messages"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {groups.length === 0 && (
          <p className="px-3 py-6 text-center text-xs text-muted">
            {query ? "No chats match." : "No conversations yet."}
          </p>
        )}
        {groups.map((g) => (
          <div key={g.label} className="mb-2">
            <div className="px-2 pb-1 pt-2 text-[0.65rem] font-semibold uppercase tracking-wider text-muted">
              {g.label}
            </div>
            {g.chats.map((c) => (
              <div
                key={c.id}
                className={`group flex cursor-pointer items-center gap-0.5 rounded-lg px-2 py-1.5 text-sm transition-colors ${
                  c.id === activeId
                    ? "bg-accent-soft font-medium text-ink"
                    : "text-muted hover:bg-surface-2 hover:text-ink"
                }`}
                onClick={() => onSelect(c.id)}
                title={c.tags.length > 0 ? c.tags.map((t) => `#${t}`).join(" ") : c.title}
              >
                {c.pinned && <IconPin className="h-3 w-3 shrink-0 text-accent" />}
                <span className="min-w-0 flex-1 truncate">{c.title}</span>
                <button
                  className={`rounded p-0.5 text-muted hover:text-ink ${
                    c.pinned ? "block" : "hidden group-hover:block"
                  }`}
                  aria-label={c.pinned ? "Unpin chat" : "Pin chat"}
                  title={c.pinned ? "Unpin" : "Pin to top"}
                  onClick={(e) => {
                    e.stopPropagation();
                    onTogglePin(c);
                  }}
                >
                  <IconPin className="h-3.5 w-3.5" />
                </button>
                <button
                  className="hidden rounded p-0.5 text-muted hover:text-ink group-hover:block"
                  aria-label="Edit tags"
                  title="Edit tags (search with #tag)"
                  onClick={(e) => {
                    e.stopPropagation();
                    onSetTags(c);
                  }}
                >
                  <IconTag className="h-3.5 w-3.5" />
                </button>
                <button
                  className="hidden rounded p-0.5 text-muted hover:text-ink group-hover:block"
                  aria-label="Archive chat"
                  title="Archive"
                  onClick={(e) => {
                    e.stopPropagation();
                    onArchive(c);
                  }}
                >
                  <IconArchive className="h-3.5 w-3.5" />
                </button>
                <button
                  className="hidden rounded p-0.5 text-muted hover:text-ink group-hover:block"
                  aria-label="Rename chat"
                  onClick={(e) => {
                    e.stopPropagation();
                    onRename(c.id);
                  }}
                >
                  <IconPencil className="h-3.5 w-3.5" />
                </button>
                <button
                  className="hidden rounded p-0.5 text-muted hover:text-danger group-hover:block"
                  aria-label="Delete chat"
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(c.id);
                  }}
                >
                  <IconTrash className="h-3.5 w-3.5" />
                </button>
              </div>
            ))}
          </div>
        ))}

        {query.trim().length >= 2 && (
          <div className="mb-2">
            <div className="px-2 pb-1 pt-2 text-[0.65rem] font-semibold uppercase tracking-wider text-muted">
              Messages & tags
            </div>
            {hits.length === 0 && (
              <p className="px-3 py-2 text-xs text-muted">No message matches.</p>
            )}
            {hits.map((h) => (
              <div
                key={h.message_id}
                className="cursor-pointer rounded-lg px-2 py-1.5 transition-colors hover:bg-surface-2"
                onClick={() => onSelect(h.chat_id)}
                title={h.snippet}
              >
                <div className="truncate text-xs font-medium text-ink">{h.chat_title}</div>
                <div className="truncate text-[0.7rem] text-muted">{h.snippet}</div>
              </div>
            ))}
          </div>
        )}

        <div className="mb-2 border-t border-line pt-1">
          <button
            className="flex w-full items-center gap-1.5 rounded-lg px-2 py-1.5 text-xs text-muted transition-colors hover:bg-surface-2 hover:text-ink"
            onClick={() => {
              const next = !archOpen;
              setArchOpen(next);
              if (next) onLoadArchived();
            }}
          >
            <IconArchive className="h-3.5 w-3.5" />
            Archived{archivedLoaded && archived.length > 0 ? ` (${archived.length})` : ""}
          </button>
          {archOpen && (
            <div className="ml-2">
              {archived.length === 0 && (
                <p className="px-2 py-1.5 text-xs text-muted">
                  {archivedLoaded ? "No archived chats." : "Loading…"}
                </p>
              )}
              {archived.map((c) => (
                <div
                  key={c.id}
                  className="group flex cursor-pointer items-center gap-0.5 rounded-lg px-2 py-1.5 text-sm text-muted transition-colors hover:bg-surface-2 hover:text-ink"
                  onClick={() => onSelect(c.id)}
                >
                  <span className="min-w-0 flex-1 truncate">{c.title}</span>
                  <button
                    className="hidden rounded p-0.5 text-muted hover:text-ink group-hover:block"
                    aria-label="Unarchive chat"
                    title="Unarchive"
                    onClick={(e) => {
                      e.stopPropagation();
                      onUnarchive(c);
                    }}
                  >
                    <IconArchive className="h-3.5 w-3.5" />
                  </button>
                  <button
                    className="hidden rounded p-0.5 text-muted hover:text-danger group-hover:block"
                    aria-label="Delete chat"
                    onClick={(e) => {
                      e.stopPropagation();
                      onDelete(c.id);
                    }}
                  >
                    <IconTrash className="h-3.5 w-3.5" />
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      </nav>

      <div className="border-t border-line px-3 py-2.5">
        {user.is_admin && (
          <label className="mb-2 flex cursor-pointer items-center justify-between text-[0.7rem] text-muted">
            <span>Public sign-up</span>
            <input
              type="checkbox"
              checked={openSignup}
              onChange={(e) => onToggleSignup(e.target.checked)}
              className="h-3.5 w-3.5 accent-current"
              title="Allow anyone with the URL to create an account"
            />
          </label>
        )}
        <div className="flex items-center gap-1.5">
          <span
            className="min-w-0 flex-1 truncate text-xs font-medium text-ink"
            title={user.username}
          >
            {user.username}
            {user.is_admin && <span className="ml-1 text-[0.65rem] text-muted">· admin</span>}
          </span>
          <button
            onClick={onLogout}
            className="rounded-md px-2 py-1 text-[0.7rem] text-muted transition-colors hover:bg-surface-2 hover:text-ink"
            title="Log out"
          >
            Log out
          </button>
          <button
            onClick={onToggleTheme}
            className="rounded-md p-1.5 text-muted transition-colors hover:bg-surface-2 hover:text-ink"
            aria-label="Toggle light/dark theme"
            title={theme === "dark" ? "Switch to light" : "Switch to dark"}
          >
            {theme === "dark" ? <IconSun /> : <IconMoon />}
          </button>
        </div>
      </div>
    </aside>
  );
}
