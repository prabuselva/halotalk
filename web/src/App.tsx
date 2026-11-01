import { useCallback, useEffect, useRef, useState } from "react";
import {
  editUserMessage,
  regenerateChat,
  sendMessage,
  stopGeneration as apiStop,
  subscribeChatEvents,
} from "./api";
import AuthGate from "./components/AuthGate";
import CommandPalette from "./components/CommandPalette";
import Composer from "./components/Composer";
import HaloChatEmpty from "./components/EmptyState";
import { HaloLogo, IconGear, IconMenu, IconMoon, IconSun } from "./components/Icons";
import MessageList, { type PendingItem, type PendingStatus } from "./components/MessageList";
import SettingsDrawer, {
  type ChatDraft,
  draftFromChat,
  draftToPatch,
  setPendingDraft,
  takePendingDraft,
} from "./components/SettingsDrawer";
import Sidebar from "./components/Sidebar";
import { exportChatJson, exportChatMarkdown } from "./export";
import { computeSpeeds, parsePrometheus, type RunSpeeds } from "./metrics";
import { rest } from "./rest";
import type {
  AuthStatus,
  Chat,
  Message,
  Prompt,
  Provider,
  RegistryEntry,
  User,
} from "./types";

export default function App() {
  const [user, setUser] = useState<User | null | undefined>(undefined);
  const [authStatus, setAuthStatus] = useState<AuthStatus | null>(null);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [prompts, setPrompts] = useState<Prompt[]>([]);
  const [registry, setRegistry] = useState<RegistryEntry[]>([]);
  const [chats, setChats] = useState<Chat[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [activeChat, setActiveChat] = useState<Chat | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [streamingId, setStreamingId] = useState<string | null>(null);
  const [perfBadge, setPerfBadge] = useState<{ firstMs: number; tps: number } | null>(null);
  const perfRef = useRef<{ startedAt: number; firstAt: number; chars: number }>({
    startedAt: 0,
    firstAt: 0,
    chars: 0,
  });
  const [speeds, setSpeeds] = useState<Record<string, RunSpeeds | null>>({});
  const metricsBefore = useRef<Record<string, Promise<Map<string, number> | null>>>({});
  const activeChatRef = useRef<Chat | null>(null);
  activeChatRef.current = activeChat;
  const takeMetrics = useCallback((chatId: string, providerId?: string | null) => {
    const pid = providerId ?? activeChatRef.current?.provider_id ?? "";
    metricsBefore.current[chatId] = pid
      ? rest
          .providerMetrics(pid)
          .then((t) => (t ? parsePrometheus(t) : null))
          .catch(() => null)
      : Promise.resolve(null);
  }, []);

  const finishMetrics = useCallback((chatId: string) => {
    const pending = metricsBefore.current[chatId];
    if (!pending) return;
    delete metricsBefore.current[chatId];
    const liveId = streamingIdRef.current;
    const pid = activeChatRef.current?.provider_id ?? "";
    void (async () => {
      const before = await pending;
      const after = pid
        ? await rest
            .providerMetrics(pid)
            .then((t) => (t ? parsePrometheus(t) : null))
            .catch(() => null)
        : null;
      const latest = await rest
        .getChat(chatId)
        .then((c) => c.messages)
        .catch(() => null);
      const lastAssistant = latest?.filter((m) => m.role === "assistant").pop();
      const doneId = liveId ?? lastAssistant?.id ?? null;
      if (doneId) setSpeeds((s) => ({ ...s, [doneId]: computeSpeeds(before, after) }));
    })();
  }, []);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [mobileSidebar, setMobileSidebar] = useState(false);
  const [toast, setToastMsg] = useState<string | null>(null);
  const toastTimer = useRef<number | undefined>(undefined);

  const [theme, setTheme] = useState<"light" | "dark">(() =>
    document.documentElement.dataset.theme === "dark" ? "dark" : "light",
  );

  const showToast = useCallback((m: string) => {
    setToastMsg(m);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToastMsg(null), 3200);
  }, []);

  const refreshProviders = useCallback(async () => {
    try {
      setProviders(await rest.listProviders());
    } catch (e) {
      showToast(`Providers: ${(e as Error).message}`);
    }
  }, [showToast]);

  const [voice, setVoice] = useState({ tts: false, stt: false });
  const refreshVoice = useCallback(() => {
    rest
      .userSettings()
      .then((s) =>
        setVoice({
          tts: !!s.tts_provider_id && !!s.tts_model,
          stt: !!s.stt_provider_id && !!s.stt_model,
        }),
      )
      .catch(() => setVoice({ tts: false, stt: false }));
  }, []);

  const refreshChats = useCallback(async () => {
    try {
      setChats(await rest.listChats());
    } catch (e) {
      showToast(`Chats: ${(e as Error).message}`);
    }
  }, [showToast]);

  const [archived, setArchived] = useState<Chat[]>([]);
  const [archivedLoaded, setArchivedLoaded] = useState(false);

  const loadArchived = useCallback(async () => {
    try {
      setArchived((await rest.listChats(true)).filter((c) => c.archived));
      setArchivedLoaded(true);
    } catch (e) {
      showToast(`Archived: ${(e as Error).message}`);
    }
  }, [showToast]);

  const patchChat = useCallback(
    async (id: string, body: Record<string, unknown>) => {
      try {
        await rest.updateChat(id, body);
        if (id === activeId) setActiveChat((c) => (c ? { ...c, ...body } : c));
        await refreshChats();
        if (archivedLoaded) loadArchived();
      } catch (e) {
        showToast((e as Error).message);
      }
    },
    [activeId, archivedLoaded, loadArchived, refreshChats, showToast],
  );

  const togglePin = (c: Chat) => void patchChat(c.id, { pinned: !c.pinned });
  const archiveChat = (c: Chat) => void patchChat(c.id, { archived: true });
  const unarchiveChat = (c: Chat) => void patchChat(c.id, { archived: false });
  const setChatTags = (c: Chat) => {
    const v = window.prompt("Tags (comma separated; search with #tag)", c.tags.join(", "));
    if (v === null) return;
    const tags = [
      ...new Set(
        v
          .split(",")
          .map((t) => t.trim().toLowerCase().replace(/^#/, ""))
          .filter((t) => /^[a-z0-9._-]+$/.test(t)),
      ),
    ].sort();
    void patchChat(c.id, { tags });
  };

  useEffect(() => {
    const onUnauthorized = () => {
      setUser(null);
      setChats([]);
      setArchived([]);
      setArchivedLoaded(false);
      setActiveId(null);
      setActiveChat(null);
      setMessages([]);
      setProviders([]);
    };
    window.addEventListener("halo-unauthorized", onUnauthorized);
    return () => window.removeEventListener("halo-unauthorized", onUnauthorized);
  }, []);

  useEffect(() => {
    rest.authStatus().then(setAuthStatus).catch(() => setAuthStatus(null));
    rest.me().then(setUser).catch(() => setUser(null));
  }, []);

  const refreshPrompts = useCallback(async () => {
    try {
      setPrompts(await rest.listPrompts());
    } catch (e) {
      showToast(`Prompts: ${(e as Error).message}`);
    }
  }, [showToast]);

  useEffect(() => {
    if (!user) return;
    refreshProviders();
    refreshChats();
    refreshPrompts();
    refreshVoice();
  }, [user, refreshProviders, refreshChats, refreshPrompts, refreshVoice]);

  useEffect(() => {
    const pid = activeChat?.provider_id;
    if (!pid) {
      setRegistry([]);
      return;
    }
    let stale = false;
    rest
      .registry(pid)
      .then((r) => !stale && setRegistry(r))
      .catch(() => !stale && setRegistry([]));
    return () => {
      stale = true;
    };
  }, [activeChat?.provider_id]);

  const modelEntry = registry.find((r) => r.model_id === activeChat?.model);
  const visionBlocked = !!activeChat?.model && modelEntry !== undefined && !modelEntry.vision;

  const selectChat = useCallback(async (id: string) => {
    setMobileSidebar(false);
    try {
      const { chat, messages } = await rest.getChat(id);
      setActiveId(id);
      setActiveChat(chat);
      setMessages(messages);
    } catch (e) {
      showToast((e as Error).message);
    }
  }, [showToast]);

  const openNew = useCallback(() => {
    setActiveId(null);
    setActiveChat(null);
    setMessages([]);
    setMobileSidebar(false);
  }, []);

  const toggleTheme = () => {
    const t = theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = t;
    localStorage.setItem("halotalk-theme", t);
    setTheme(t);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen((p) => !p);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const pickModel = async (providerId: string, modelId: string) => {
    if (activeChat) {
      try {
        const updated = await rest.updateChat(activeChat.id, {
          provider_id: providerId,
          model: modelId,
        });
        setActiveChat(updated);
        refreshChats();
        showToast(`Model → ${modelId}`);
      } catch (e) {
        showToast((e as Error).message);
      }
    } else {
      const nd = { ...draftFromChat(null, providerId), model: modelId };
      setPendingDraft(nd);
      setPendingDraftState(nd);
      showToast(`Model → ${modelId}`);
    }
  };

  const renameChat = async (id: string) => {
    const current = chats.find((c) => c.id === id);
    const title = window.prompt("Chat title", current?.title ?? "");
    if (!title?.trim()) return;
    try {
      await rest.updateChat(id, { title: title.trim() });
      if (id === activeId) setActiveChat((c) => (c ? { ...c, title: title.trim() } : c));
      refreshChats();
    } catch (e) {
      showToast((e as Error).message);
    }
  };

  const deleteChat = async (id: string) => {
    if (!window.confirm("Delete this chat?")) return;
    try {
      await rest.deleteChat(id);
      if (id === activeId) openNew();
      refreshChats();
      if (archivedLoaded) loadArchived();
    } catch (e) {
      showToast((e as Error).message);
    }
  };

  const stopGeneration = () => {
    if (activeId) void apiStop(activeId).catch((e) => showToast((e as Error).message));
  };

  const onLogout = async () => {
    try {
      await rest.logout();
    } catch {
      /* session already gone */
    }
    setUser(null);
    setChats([]);
    setActiveId(null);
    setActiveChat(null);
    setMessages([]);
    setProviders([]);
  };

  const refreshStatus = useCallback(() => {
    rest.authStatus().then(setAuthStatus).catch(() => {});
  }, []);

  const toggleSignup = async (open: boolean) => {
    setAuthStatus((s) => (s ? { ...s, open_signup: open } : s));
    try {
      await rest.setOpenSignup(open);
    } catch (e) {
      showToast((e as Error).message);
      refreshStatus();
    }
  };

  const streamingIdRef = useRef<string | null>(null);
  const seqFloors = useRef(new Map<string, number>());
  const [queues, setQueues] = useState<
    Record<string, { content: string; images: string[]; pendingId: string }[]>
  >({});
  // Local send-status bubbles (WhatsApp-style clock/checks), kept outside
  // `messages` so mergeMessages/reloads never drop them.
  const [pending, setPending] = useState<Record<string, PendingItem[]>>({});
  const pendingRef = useRef(pending);
  useEffect(() => {
    pendingRef.current = pending;
  }, [pending]);
  const [delivered, setDelivered] = useState<Set<string>>(new Set());
  const markPending = useCallback((chatId: string, id: string, status: PendingStatus) => {
    setPending((ps) => ({
      ...ps,
      [chatId]: (ps[chatId] ?? []).map((p) => (p.id === id ? { ...p, status } : p)),
    }));
  }, []);
  /// Reconcile local pending sends with the authoritative transcript: a pending
  /// already visible as a server user message (e.g. the hub closed our events
  /// stream at run start and the echo was lost in the 3s EventSource retry)
  /// gets dropped and its server bubble gets the delivered double-tick.
  const correlatePendings = useCallback((chatId: string, msgs: Message[]) => {
    const q = pendingRef.current[chatId] ?? [];
    if (!q.length) return;
    const drop = new Set<string>();
    const taken = new Set<string>();
    const echoes = msgs.filter((m) => m.role === "user");
    for (const p of q) {
      if (p.status === "failed") continue;
      const m = echoes.find(
        (e) =>
          !taken.has(e.id) &&
          e.content === p.content &&
          (e.images?.length ?? 0) === p.images.length,
      );
      if (m) {
        drop.add(p.id);
        taken.add(m.id);
      }
    }
    if (!drop.size) return;
    pendingRef.current = { ...pendingRef.current, [chatId]: q.filter((p) => !drop.has(p.id)) };
    setPending((ps) => ({ ...ps, [chatId]: (ps[chatId] ?? []).filter((p) => !drop.has(p.id)) }));
    setDelivered((d0) => {
      const add = [...taken].filter((id) => !d0.has(id));
      if (!add.length) return d0;
      const d = new Set(d0);
      add.forEach((id) => d.add(id));
      return d;
    });
  }, []);
  const setStreaming = useCallback((id: string | null) => {
    streamingIdRef.current = id;

    setStreamingId(id);
  }, []);

  const startPerf = () => {
    perfRef.current = { startedAt: performance.now(), firstAt: 0, chars: 0 };
    setPerfBadge(null);
  };

  const patchHead = useCallback((fn: (m: Message) => Message) => {
    const id = streamingIdRef.current;
    if (!id) return;
    setMessages((ms) => ms.map((m) => (m.id === id ? fn(m) : m)));
  }, []);

  const send = async (content: string, images: string[]) => {
    if (activeId && streamingId) {
      const lid = `pend-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
      setQueues((q) => ({ ...q, [activeId]: [...(q[activeId] ?? []), { content, images, pendingId: lid }] }));
      setPending((ps) => ({
        ...ps,
        [activeId]: [...(ps[activeId] ?? []), { id: lid, content, images, status: "queued" }],
      }));
      return;
    }
    let chatId = activeId;
    let providerId = activeChat?.provider_id ?? null;
    if (!chatId) {
      try {
        const pendingDraft = takePendingDraft();
        setPendingDraftState(null);
        const created = await rest.createChat(pendingDraft ? draftToPatch(pendingDraft) : {});
        setActiveId(created.id);
        setActiveChat(created);
        providerId = created.provider_id;
        chatId = created.id;
      } catch (e) {
        showToast((e as Error).message);
        return;
      }
    }
    const lid = `pend-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    setPending((ps) => ({
      ...ps,
      [chatId]: [...(ps[chatId] ?? []), { id: lid, content, images, status: "sending" }],
    }));
    startPerf();
    takeMetrics(chatId, providerId);
    try {
      await sendMessage(chatId, content, images);
      markPending(chatId, lid, "sent");
    } catch (e) {
      showToast((e as Error).message);
      markPending(chatId, lid, "failed");
    }
  };

  const reloadMessages = useCallback(
    async (chatId: string) => {
      try {
        const { messages } = await rest.getChat(chatId);
        setMessages(messages);
        correlatePendings(chatId, messages);
      } catch {
        /* keep optimistic view */
      }
    },
    [correlatePendings],
  );

  const regenerate = async () => {
    if (streamingId || !activeId) return;
    startPerf();
    try {
      takeMetrics(activeId);
      await regenerateChat(activeId);
    } catch (e) {
      showToast((e as Error).message);
    }
  };

  /// Reconnect-safe message merge: never let a server snapshot shorter than
  /// what is already rendered (live deltas) delete text.
  const mergeMessages = useCallback((fresh: Message[]) => {
    setMessages((prev) => {
      const byId = new Map(prev.map((m) => [m.id, m]));
      return fresh.map((m) => {
        const old = byId.get(m.id);
        if (!old) return m;
        const content =
          (m.error === null || !m.error) && old.content.length > m.content.length
            ? old.content
            : m.content;
        const reasoning =
          (old.reasoning?.length ?? 0) > (m.reasoning?.length ?? 0) ? old.reasoning : m.reasoning;
        return { ...m, content, reasoning };
      });
    });
  }, []);

  const snapshotMessage = useCallback((id: string, patch: Partial<Message>): void => {
    setMessages((ms) =>
      ms.map((m) => {
        if (m.id !== id) return m;
        return {
          ...m,
          content: (patch.content?.length ?? 0) > m.content.length ? (patch.content as string) : m.content,
          reasoning:
            (patch.reasoning?.length ?? 0) > (m.reasoning?.length ?? 0)
              ? (patch.reasoning as string)
              : m.reasoning,
        };
      }),
    );
  }, []);

  /// Live view of the active chat: turns run server-side; this subscription
  /// is the single source of truth for message updates while it is mounted.
  useEffect(() => {
    if (!user || !activeId) return;
    const chatId = activeId;
    setStreaming(null);
    return subscribeChatEvents(chatId, {
      onState: (s) => {
        if (!s.streaming || !s.message_id) {
          finishMetrics(chatId);
          setStreaming(null);
          // Hub closed an idle-held stream at run start (or the run finished
          // during the EventSource retry gap) — pull the transcript so frames
          // we missed can't leave the view blank.
          void rest
            .getChat(chatId)
            .then(({ messages }) => {
              mergeMessages(messages);
              correlatePendings(chatId, messages);
            })
            .catch(() => {});
          return;
        }
        const id = s.message_id;
        seqFloors.current.set(id, s.seq ?? 0);
        if (streamingIdRef.current !== id) {
          startPerf();
          setStreaming(id);
        }
        void rest
          .getChat(chatId)
          .then(({ messages }) => {
            if (streamingIdRef.current !== id) return;
            mergeMessages(messages);
            correlatePendings(chatId, messages);
            snapshotMessage(id, { content: s.content, reasoning: s.reasoning });
          })
          .catch(() => {});
      },
      onUser: (m) => {
        setMessages((ms) => (ms.some((x) => x.id === m.id) ? ms : [...ms, m]));
        // Echo of our own send → drop the local pending bubble and double-tick
        // the server-side message. ("sent": the HTTP 202 can beat the echo.)
        const mine = (pendingRef.current[chatId] ?? []).find(
          (p) =>
            p.status !== "failed" &&
            p.content === m.content &&
            p.images.length === (m.images?.length ?? 0),
        );
        if (mine) {
          setPending((ps) => ({
            ...ps,
            [chatId]: (ps[chatId] ?? []).filter((p) => p.id !== mine.id),
          }));
          setDelivered((d) => (d.has(m.id) ? d : new Set(d).add(m.id)));
        }
      },
      onResync: () => {
        void rest
          .getChat(chatId)
          .then(({ messages }) => {
            mergeMessages(messages);
            correlatePendings(chatId, messages);
          })
          .catch(() => {});
      },
      onMeta: (meta) => {
        if (meta.title) {
          setActiveChat((c) => (c && c.id === chatId ? { ...c, title: meta.title } : c));
        }
        const id = meta.message_id;
        seqFloors.current.set(id, 0);
        setStreaming(id);
        setMessages((ms) =>
          ms.some((m) => m.id === id)
            ? ms
            : [
                ...ms,
                {
                  id,
                  role: "assistant",
                  content: "",
                  reasoning: "",
                  images: [],
                  error: null,
                  reply_to: null,
                  active: true,
                  created_at: new Date().toISOString(),
                },
              ],
        );
      },
      onReasoning: (d, seq) => {
        const id = streamingIdRef.current;
        if (id && seq && seq <= (seqFloors.current.get(id) ?? 0)) return;
        patchHead((m) => ({ ...m, reasoning: (m.reasoning ?? "") + d }));
      },
      onToken: (d, seq) => {
        if (!d) return;
        const id = streamingIdRef.current;
        if (!id) return;
        if (seq && seq <= (seqFloors.current.get(id) ?? 0)) return;
        const p = perfRef.current;
        if (!p.firstAt) p.firstAt = performance.now();
        p.chars += d.length;
        const el = performance.now() - p.firstAt;
        // tokens approximated as chars/4 (no per-provider tokenizer)
        setPerfBadge({
          firstMs: p.firstAt - p.startedAt,
          tps: el > 200 ? p.chars / 4 / (el / 1000) : 0,
        });
        patchHead((m) => ({ ...m, content: m.content + d }));
      },
      onTitle: (t) => {
        if (!t) return;
        setActiveChat((c) => (c && c.id === chatId ? { ...c, title: t } : c));
        setChats((cs) => cs.map((c) => (c.id === chatId ? { ...c, title: t } : c)));
      },
      onSources: (sources) => patchHead((m) => ({ ...m, sources })),
      onError: (msg) => {
        if (!msg) return; // EventSource network error event has no payload
        patchHead((m) => ({ ...m, error: msg }));
      },
      onTool: (t) => {
        if (t.phase === "call") {
          setMessages((ms) =>
            ms.some((m) => m.id === `tool-${t.id}`)
              ? ms
              : [
                  ...ms,
                  {
                    id: `tool-${t.id}`,
                    role: "tool",
                    content: `searching the web…`,
                    reasoning: null,
                    images: [],
                    error: null,
                    reply_to: null,
                    active: true,
                    created_at: new Date().toISOString(),
                    tool_call_id: t.id,
                  },
                ],
          );
        } else {
          setMessages((ms) =>
            ms.map((m) =>
              m.id === `tool-${t.id}`
                ? { ...m, content: t.ok ? (t.preview ?? "") : `failed: ${t.preview ?? ""}` }
                : m,
            ),
          );
        }
      },
      onDone: () => {
        finishMetrics(chatId);
        setStreaming(null);
        window.setTimeout(() => setPerfBadge(null), 2500);
        refreshChats();
        void reloadMessages(chatId);
      },
    });
  }, [
    user,
    activeId,
    mergeMessages,
    correlatePendings,
    patchHead,
    refreshChats,
    reloadMessages,
    setStreaming,
    finishMetrics,
  ]);

  /// Flush the per-chat message queue whenever the chat goes idle.
  useEffect(() => {
    if (!activeId || streamingId) return;
    const q = queues[activeId];
    if (!q?.length) return;
    const [head, ...rest] = q;
    setQueues((prev) => ({ ...prev, [activeId]: rest }));
    markPending(activeId, head.pendingId, "sending");
    startPerf();
    takeMetrics(activeId);
    sendMessage(activeId, head.content, head.images)
      .then(() => markPending(activeId, head.pendingId, "sent"))
      .catch((e) => {
        showToast((e as Error).message);
        markPending(activeId, head.pendingId, "failed");
        setQueues((prev) => ({ ...prev, [activeId]: [head, ...(prev[activeId] ?? [])] }));
      });
  }, [activeId, streamingId, queues, markPending]);

  const activateSibling = async (mid: string) => {
    if (streamingId || !activeId) return;
    try {
      await rest.activateMessage(activeId, mid);
    } catch (e) {
      showToast((e as Error).message);
    }
    reloadMessages(activeId);
  };

  /// Failed bubble retry: drop the pending row and re-dispatch the send.
  const retryPending = (id: string) => {
    if (!activeId) return;
    const p = (pending[activeId] ?? []).find((x) => x.id === id);
    if (!p) return;
    setPending((ps) => ({ ...ps, [activeId]: (ps[activeId] ?? []).filter((x) => x.id !== id) }));
    void send(p.content, p.images);
  };

  const editMessage = async (mid: string, content: string) => {
    if (streamingId || !activeId) return;
    startPerf();
    takeMetrics(activeId);
    try {
      await editUserMessage(activeId, mid, content);
    } catch (e) {
      showToast((e as Error).message);
    }
  };

  const [pendingDraft, setPendingDraftState] = useState<ChatDraft | null>(null);
  const saveSettings = async (patch: Record<string, unknown>) => {
    if (!activeChat) {
      const d = draftFromChat(null, providers.find((p) => p.is_default)?.id ?? null);
      const next: ChatDraft = {
        ...d,
        provider_id: (patch.provider_id as string | null) ?? d.provider_id,
        model: (patch.model as string | null) ?? d.model,
        temperature: typeof patch.temperature === "number" ? patch.temperature : d.temperature,
        top_p: typeof patch.top_p === "number" ? patch.top_p : d.top_p,
        max_tokens: patch.max_tokens == null ? "" : String(patch.max_tokens),
        system_prompt: (patch.system_prompt as string | null) ?? "",
        reasoning_effort: (patch.reasoning_effort as string | null) ?? "off",
        tools_enabled: patch.tools_enabled === true,
        kb_enabled: patch.kb_enabled === true,
      };
      setPendingDraft(next);
      setPendingDraftState(next);
      setDrawerOpen(false);
      showToast("Will apply to your next chat");
      return;
    }
    try {
      const updated = await rest.updateChat(activeChat.id, patch);
      setActiveChat(updated);
      setActiveId((id) => (id ? id : updated.id));
      setDrawerOpen(false);
      refreshChats();
      showToast("Settings saved");
    } catch (e) {
      showToast((e as Error).message);
    }
  };

  const providerName =
    providers.find((p) => p.id === activeChat?.provider_id)?.name ?? null;
  const modelMissing = activeChat
    ? !activeChat.model || !activeChat.provider_id
    : !(
        (pendingDraft && pendingDraft.model && pendingDraft.provider_id) ||
        providers.some((p) => p.is_default)
      );
  const showEmpty = messages.length === 0;

  if (user === undefined) {
    return (
      <div className="flex h-full items-center justify-center bg-bg text-sm text-muted">
        Loading HaloTalk…
      </div>
    );
  }
  if (user === null) {
    return (
      <AuthGate
        status={authStatus ?? { needs_setup: false, open_signup: false }}
        onAuthed={(u) => {
          setUser(u);
          refreshStatus();
        }}
      />
    );
  }

  return (
    <div className="relative flex h-full overflow-hidden">
      {/* desktop sidebar */}
      <div className="hidden md:flex">
        <Sidebar
          chats={chats}
          activeId={activeId}
          theme={theme}
          user={user}
          openSignup={authStatus?.open_signup ?? false}
          onSelect={selectChat}
          onNew={openNew}
          onRename={renameChat}
          onDelete={deleteChat}
          archived={archived}
          archivedLoaded={archivedLoaded}
          onTogglePin={togglePin}
          onSetTags={setChatTags}
          onArchive={archiveChat}
          onUnarchive={unarchiveChat}
          onLoadArchived={() => void loadArchived()}
          onToggleTheme={toggleTheme}
          onToggleSignup={(v) => void toggleSignup(v)}
          onLogout={() => void onLogout()}
        />
      </div>
      {/* mobile overlay sidebar */}
      {mobileSidebar && (
        <div className="absolute inset-0 z-40 flex md:hidden">
          <div className="absolute inset-0 bg-black/40" onClick={() => setMobileSidebar(false)} />
          <div className="relative z-50">
            <Sidebar
              chats={chats}
              activeId={activeId}
              theme={theme}
              user={user}
              openSignup={authStatus?.open_signup ?? false}
              onSelect={selectChat}
              onNew={openNew}
              onRename={renameChat}
              onDelete={deleteChat}
              archived={archived}
              archivedLoaded={archivedLoaded}
              onTogglePin={togglePin}
              onSetTags={setChatTags}
              onArchive={archiveChat}
              onUnarchive={unarchiveChat}
              onLoadArchived={() => void loadArchived()}
              onToggleTheme={toggleTheme}
              onToggleSignup={(v) => void toggleSignup(v)}
              onLogout={() => void onLogout()}
              onCloseMobile={() => setMobileSidebar(false)}
            />
          </div>
        </div>
      )}

      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-2 border-b border-line bg-surface/80 px-4 py-2.5 backdrop-blur">
          <button
            className="rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-ink md:hidden"
            aria-label="Open sidebar"
            onClick={() => setMobileSidebar(true)}
          >
            <IconMenu />
          </button>
          {!mobileSidebar && <HaloLogo className="h-5 w-5 md:hidden" />}
          <h1 className="min-w-0 truncate text-sm font-semibold">
            {activeChat?.title ?? "New chat"}
          </h1>
          {streamingId && perfBadge && (
            <span
              className="shrink-0 rounded-full bg-surface-2 px-2 py-0.5 text-[0.65rem] text-muted tabular-nums"
              title="first-token latency · generation speed (~tokens = chars/4)"
            >
              first {(perfBadge.firstMs / 1000).toFixed(2)}s
              {perfBadge.tps > 0 ? ` · ${perfBadge.tps.toFixed(0)} tok/s` : ""}
            </span>
          )}
          <div className="ml-auto flex items-center gap-1.5">
            {activeChat && messages.length > 0 && (
              <>
                <button
                  className="hidden rounded-full border border-line px-2.5 py-1 text-xs text-muted transition-colors hover:border-accent/50 hover:text-accent sm:inline"
                  title="Export chat as Markdown"
                  onClick={() => exportChatMarkdown(activeChat, messages)}
                >
                  MD
                </button>
                <button
                  className="hidden rounded-full border border-line px-2.5 py-1 text-xs text-muted transition-colors hover:border-accent/50 hover:text-accent sm:inline"
                  title="Export chat as JSON"
                  onClick={() => exportChatJson(activeChat, messages)}
                >
                  JSON
                </button>
              </>
            )}
            <button
              className={`flex max-w-[45vw] items-center gap-1.5 truncate rounded-full border border-line px-3 py-1 text-xs transition-colors hover:border-accent/50 hover:text-accent ${
                modelMissing ? "text-muted" : ""
              }`}
              onClick={() => setDrawerOpen(true)}
              title="Model settings"
            >
              {providerName ? `${providerName} · ${activeChat?.model}` : "Choose model"}
            </button>
            <button
              className="rounded-md p-2 text-muted transition-colors hover:bg-surface-2 hover:text-ink"
              aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
              title={theme === "dark" ? "Light mode" : "Dark mode"}
              onClick={toggleTheme}
            >
              {theme === "dark" ? <IconSun /> : <IconMoon />}
            </button>
            <button
              className="rounded-md p-2 text-muted transition-colors hover:bg-surface-2 hover:text-ink"
              aria-label="Open settings"
              onClick={() => setDrawerOpen(true)}
            >
              <IconGear />
            </button>
          </div>
        </header>

        {showEmpty ? (
          <HaloChatEmpty
            hasProviders={providers.length > 0}
            onOpenSettings={() => setDrawerOpen(true)}
          />
        ) : (
          <MessageList
            messages={messages}
            streamingId={streamingId}
            ttsReady={voice.tts}
            speeds={speeds}
            pending={activeId ? (pending[activeId] ?? []) : []}
            delivered={delivered}
            onRetryPending={retryPending}
            onRegenerate={() => void regenerate()}
            onEditMessage={(id, content) => void editMessage(id, content)}
            onActivate={(id) => void activateSibling(id)}
            onToast={showToast}
          />
        )}

        <Composer
          queueCount={activeId ? (queues[activeId]?.length ?? 0) : 0}
          onClearQueue={() => {
            if (!activeId) return;
            setQueues((q) => ({ ...q, [activeId]: [] }));
            setPending((ps) => ({
              ...ps,
              [activeId]: (ps[activeId] ?? []).filter((p) => p.status !== "queued"),
            }));
          }}
          streaming={streamingId !== null}
          modelMissing={modelMissing}
          visionBlocked={visionBlocked}
          prompts={prompts}
          sttReady={voice.stt}
          onSend={(c, imgs) => void send(c, imgs)}
          onStop={stopGeneration}
          onFixSettings={() => setDrawerOpen(true)}
          onToast={showToast}
        />
      </main>

      {drawerOpen && (
        <SettingsDrawer
          chat={activeChat}
          hasChatId={activeId !== null}
          providers={providers}
          prompts={prompts}
          onClose={() => {
            setDrawerOpen(false);
            refreshVoice();
          }}
          onSave={(p) => void saveSettings(p)}
          onProvidersChanged={() => refreshProviders()}
          onPromptsChanged={() => refreshPrompts()}
          onToast={showToast}
        />
      )}

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        chats={chats}
        providers={providers}
        theme={theme}
        onNewChat={openNew}
        onSelectChat={(id) => void selectChat(id)}
        onToggleTheme={toggleTheme}
        onOpenSettings={() => setDrawerOpen(true)}
        onPickModel={(p, m) => void pickModel(p, m)}
      />

      {toast && (
        <div className="pointer-events-none fixed bottom-20 left-1/2 z-50 -translate-x-1/2 rounded-lg bg-ink px-4 py-2 text-sm text-bg shadow-lg">
          {toast}
        </div>
      )}
    </div>
  );
}
