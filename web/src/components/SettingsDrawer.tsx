import { useEffect, useState } from "react";
import { rest } from "../rest";
import type {
  Chat,
  KbDocEntry,
  ModelInfo,
  Preset,
  Prompt,
  Provider,
  ProviderTestResult,
  RegistryEntry,
} from "../types";
import { IconPlus, IconTrash, IconX } from "./Icons";

export type ChatDraft = {
  provider_id: string | null;
  model: string | null;
  temperature: number;
  top_p: number;
  max_tokens: string; // "" = provider default
  system_prompt: string;
  reasoning_effort: string; // off | low | medium | high
  tools_enabled: boolean;
  kb_enabled: boolean;
};

export function draftFromChat(chat: Chat | null, fallbackProvider: string | null): ChatDraft {
  return {
    provider_id: chat?.provider_id ?? fallbackProvider,
    model: chat?.model ?? null,
    temperature: chat?.temperature ?? 0.7,
    top_p: chat?.top_p ?? 1,
    max_tokens: chat?.max_tokens != null ? String(chat.max_tokens) : "",
    system_prompt: chat?.system_prompt ?? "",
    reasoning_effort: chat?.reasoning_effort ?? "off",
    tools_enabled: chat?.tools_enabled ?? false,
    kb_enabled: chat?.kb_enabled ?? false,
  };
}

export function draftToPatch(d: ChatDraft): Record<string, unknown> {
  return {
    provider_id: d.provider_id,
    model: d.model,
    temperature: d.temperature,
    top_p: d.top_p,
    max_tokens: d.max_tokens === "" ? null : Number(d.max_tokens),
    system_prompt: d.system_prompt.trim() ? d.system_prompt : null,
    reasoning_effort: d.reasoning_effort === "off" ? null : d.reasoning_effort,
    tools_enabled: d.tools_enabled,
    kb_enabled: d.kb_enabled,
  };
}

type Props = {
  chat: Chat | null;
  hasChatId: boolean;
  providers: Provider[];
  prompts: Prompt[];
  onClose: () => void;
  onSave: (patch: Record<string, unknown>) => void;
  onProvidersChanged: () => void;
  onPromptsChanged: () => void;
  onToast: (msg: string) => void;
};

type PromptEdit = { id: string | null; title: string; body: string };

// Presets + Prompts + Account (2.3)

function CapsBadge({ label }: { label: string }) {
  return (
    <span className="shrink-0 rounded bg-accent-soft px-1 py-px text-[0.6rem] font-medium text-accent">
      {label}
    </span>
  );
}

function parseCtx(v: string): number | null {
  const n = Number(v);
  return v.trim() !== "" && Number.isFinite(n) && n > 0 ? Math.round(n) : null;
}

export default function SettingsDrawer({
  chat,
  hasChatId,
  providers,
  prompts,
  onClose,
  onSave,
  onProvidersChanged,
  onPromptsChanged,
  onToast,
}: Props) {
  const fallback = providers.find((p) => p.is_default)?.id ?? providers[0]?.id ?? null;
  const [draft, setDraft] = useState<ChatDraft>(() => draftFromChat(chat, fallback));
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [modelsNonce, setModelsNonce] = useState(0);
  const [modelQuery, setModelQuery] = useState("");
  const [loadingModels, setLoadingModels] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [addingProvider, setAddingProvider] = useState(false);
  const [editingProvider, setEditingProvider] = useState<string | null>(null);
  const [pName, setPName] = useState("");
  const [pUrl, setPUrl] = useState("");
  const [pKey, setPKey] = useState("");
  const [pKind, setPKind] = useState("openai");
  const [savingProvider, setSavingProvider] = useState(false);
  const [testingProvider, setTestingProvider] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, ProviderTestResult>>({});
  const [presets, setPresets] = useState<Preset[]>([]);
  const [registry, setRegistry] = useState<RegistryEntry[]>([]);
  const [capCtx, setCapCtx] = useState("");
  const [promptEdit, setPromptEdit] = useState<PromptEdit | null>(null);
  const [savingPrompt, setSavingPrompt] = useState(false);
  const [defaultSys, setDefaultSys] = useState("");
  const [savingAccount, setSavingAccount] = useState(false);
  const [exportSecrets, setExportSecrets] = useState(false);
  const [ioBusy, setIoBusy] = useState(false);
  const [kbDocs, setKbDocs] = useState<KbDocEntry[]>([]);
  const [embedProvider, setEmbedProvider] = useState("");
  const [embedModel, setEmbedModel] = useState("");
  const [kbBusy, setKbBusy] = useState(false);
  const [kbNote, setKbNote] = useState<string | null>(null);
  const [ttsProvider, setTtsProvider] = useState("");
  const [ttsModel, setTtsModel] = useState("");
  const [sttProvider, setSttProvider] = useState("");
  const [sttModel, setSttModel] = useState("");
  const [voiceBusy, setVoiceBusy] = useState(false);

  useEffect(() => {
    rest.listPresets().then(setPresets).catch(() => setPresets([]));
    rest
      .settings()
      .then((s) => {
        setDefaultSys(s.default_system_prompt ?? "");
        setEmbedProvider(s.embed_provider_id ?? "");
        setEmbedModel(s.embed_model ?? "");
        setTtsProvider(s.tts_provider_id ?? "");
        setTtsModel(s.tts_model ?? "");
        setSttProvider(s.stt_provider_id ?? "");
        setSttModel(s.stt_model ?? "");
      })
      .catch(() => setDefaultSys(""));
    rest.listKb().then(setKbDocs).catch(() => setKbDocs([]));
  }, []);

  useEffect(() => {
    if (!draft.provider_id) {
      setRegistry([]);
      return;
    }
    let cancelled = false;
    rest
      .registry(draft.provider_id)
      .then((r) => !cancelled && setRegistry(r))
      .catch(() => !cancelled && setRegistry([]));
    return () => {
      cancelled = true;
    };
  }, [draft.provider_id]);

  const selectedCaps =
    draft.provider_id && draft.model
      ? registry.find((r) => r.model_id === draft.model)
      : undefined;

  useEffect(() => {
    setCapCtx(selectedCaps?.ctx_len != null ? String(selectedCaps.ctx_len) : "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft.provider_id, draft.model, registry]);

  const capsFor = new Map(registry.map((r) => [r.model_id, r]));

  // Re-seed when the drawer opens on a different chat.
  useEffect(() => {
    setDraft(draftFromChat(chat, fallback));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chat?.id, chat?.provider_id, chat?.model]);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      if (!draft.provider_id) {
        setModels([]);
        return;
      }
      setLoadingModels(true);
      setModelError(null);
      try {
        const ms = await rest.models(draft.provider_id);
        if (!cancelled) setModels(ms);
      } catch (e) {
        if (!cancelled) {
          setModels([]);
          setModelError((e as Error).message);
        }
      } finally {
        if (!cancelled) setLoadingModels(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [draft.provider_id, modelsNonce]);

  const shownModels = models.filter(
    (m) =>
      !modelQuery ||
      m.id.toLowerCase().includes(modelQuery.toLowerCase()) ||
      m.name.toLowerCase().includes(modelQuery.toLowerCase()),
  );

  const closeProviderForm = () => {
    setPName("");
    setPUrl("");
    setPKey("");
    setPKind("openai");
    setAddingProvider(false);
    setEditingProvider(null);
  };

  const startEditProvider = (p: Provider) => {
    setPName(p.name);
    setPUrl(p.base_url);
    setPKey("");
    setPKind(p.kind);
    setEditingProvider(p.id);
    setAddingProvider(true);
  };

  const saveProviderForm = async () => {
    if (!pName.trim() || !pUrl.trim()) {
      onToast("Provider needs a name and base URL");
      return;
    }
    setSavingProvider(true);
    try {
      if (editingProvider) {
        const body: Record<string, unknown> = {
          name: pName.trim(),
          base_url: pUrl.trim(),
          kind: pKind,
        };
        if (pKey.trim()) body.api_key = pKey.trim();
        await rest.updateProvider(editingProvider, body);
        if (draft.provider_id === editingProvider) {
          setDraft((d) => ({ ...d, model: null }));
          setModelsNonce((n) => n + 1);
        }
        onToast("Provider updated");
      } else {
        await rest.createProvider({
          name: pName.trim(),
          base_url: pUrl.trim(),
          api_key: pKey.trim(),
          kind: pKind,
        });
        onToast("Provider added");
      }
      closeProviderForm();
      onProvidersChanged();
    } catch (e) {
      onToast((e as Error).message);
    } finally {
      setSavingProvider(false);
    }
  };

  const removeProvider = async (id: string) => {
    if (!confirm("Delete this provider? Chats using it will need a new provider.")) return;
    try {
      await rest.deleteProvider(id);
      if (draft.provider_id === id) setDraft((d) => ({ ...d, provider_id: null, model: null }));
      onProvidersChanged();
    } catch (e) {
      onToast((e as Error).message);
    }
  };

  const testProvider = async (id: string) => {
    setTestingProvider(id);
    try {
      const r = await rest.testProvider(id);
      setTestResults((m) => ({ ...m, [id]: r }));
    } catch (e) {
      setTestResults((m) => ({ ...m, [id]: { ok: false, error: (e as Error).message } }));
    } finally {
      setTestingProvider(null);
    }
  };

  const saveCaps = async (vision: boolean, reasoning: boolean, ctxLen: number | null) => {
    if (!draft.provider_id || !draft.model) return;
    try {
      await rest.setRegistry(draft.provider_id, draft.model, vision, reasoning, ctxLen);
      setRegistry(await rest.registry(draft.provider_id));
    } catch (e) {
      onToast((e as Error).message);
    }
  };

  const savePreset = async () => {
    const name = window.prompt("Preset name");
    if (!name?.trim()) return;
    try {
      await rest.createPreset(name.trim(), {
        model: draft.model,
        temperature: draft.temperature,
        top_p: draft.top_p,
        max_tokens: draft.max_tokens === "" ? null : Number(draft.max_tokens),
        system_prompt: draft.system_prompt.trim() ? draft.system_prompt : null,
        reasoning_effort: draft.reasoning_effort === "off" ? null : draft.reasoning_effort,
      });
      setPresets(await rest.listPresets());
      onToast("Preset saved");
    } catch (e) {
      onToast((e as Error).message);
    }
  };

  const applyPreset = (p: Preset) => {
    const v = p.params;
    setDraft((d) => {
      const n = { ...d };
      if (typeof v.model === "string") n.model = v.model;
      if (typeof v.temperature === "number") n.temperature = v.temperature;
      if (typeof v.top_p === "number") n.top_p = v.top_p;
      if (v.max_tokens == null) n.max_tokens = "";
      else if (typeof v.max_tokens === "number") n.max_tokens = String(v.max_tokens);
      if (typeof v.system_prompt === "string") n.system_prompt = v.system_prompt;
      if (typeof v.reasoning_effort === "string") n.reasoning_effort = v.reasoning_effort;
      return n;
    });
    onToast(`Preset "${p.name}" applied — press Save to keep`);
  };

  const removePreset = async (id: string) => {
    try {
      await rest.deletePreset(id);
      setPresets((ps) => ps.filter((p) => p.id !== id));
    } catch (e) {
      onToast((e as Error).message);
    }
  };

  const savePrompt = async () => {
    if (!promptEdit) return;
    const title = promptEdit.title.trim();
    if (!title || !promptEdit.body) {
      onToast("Prompt needs a title and body");
      return;
    }
    setSavingPrompt(true);
    try {
      if (promptEdit.id) {
        await rest.updatePrompt(promptEdit.id, title, promptEdit.body);
      } else {
        await rest.createPrompt(title, promptEdit.body);
      }
      onPromptsChanged();
      setPromptEdit(null);
    } catch (e) {
      onToast((e as Error).message);
    } finally {
      setSavingPrompt(false);
    }
  };

  const removePrompt = async (id: string) => {
    try {
      await rest.deletePrompt(id);
      onPromptsChanged();
    } catch (e) {
      onToast((e as Error).message);
    }
  };

  const saveAccount = async () => {
    setSavingAccount(true);
    try {
      await rest.putSettings({ default_system_prompt: defaultSys });
      onToast("Account settings saved");
    } catch (e) {
      onToast((e as Error).message);
    } finally {
      setSavingAccount(false);
    }
  };

  const saveEmbedConfig = async () => {
    setKbBusy(true);
    try {
      await rest.putSettings({
        embed_provider_id: embedProvider,
        embed_model: embedModel,
      });
      onToast("Embeddings config saved");
    } catch (e) {
      onToast((e as Error).message);
    } finally {
      setKbBusy(false);
    }
  };

  const saveVoiceConfig = async () => {
    setVoiceBusy(true);
    try {
      await rest.putSettings({
        tts_provider_id: ttsProvider,
        tts_model: ttsModel,
        stt_provider_id: sttProvider,
        stt_model: sttModel,
      });
      onToast("Voice config saved");
    } catch (e) {
      onToast((e as Error).message);
    } finally {
      setVoiceBusy(false);
    }
  };

  const uploadKbFile = async (file: File) => {
    setKbBusy(true);
    setKbNote(null);
    try {
      const text = await file.text();
      const r = await rest.addKb(file.name, text);
      setKbNote(
        r.embedded
          ? `Indexed ${file.name} (${r.chunks} chunks).`
          : `Stored ${file.name} without vectors — configure embeddings below and re-upload.`,
      );
      setKbDocs(await rest.listKb());
    } catch (e) {
      setKbNote(`Upload failed — ${(e as Error).message}`);
    } finally {
      setKbBusy(false);
    }
  };

  const removeKbFile = async (source: string) => {
    await rest.deleteKbFile(source).catch(() => undefined);
    setKbDocs(await rest.listKb().catch(() => []));
  };

  const kbSources = kbDocs.reduce<{ source: string; docs: KbDocEntry[] }[]>((acc, d) => {
    const hit = acc.find((a) => a.source === d.source);
    if (hit) hit.docs.push(d);
    else acc.push({ source: d.source, docs: [d] });
    return acc;
  }, []);

  const doExport = async () => {
    setIoBusy(true);
    try {
      const doc = await rest.exportConfig(exportSecrets);
      const blob = new Blob([JSON.stringify(doc, null, 2)], { type: "application/json" });
      const a = document.createElement("a");
      a.href = URL.createObjectURL(blob);
      a.download = "halotalk-config.json";
      a.click();
      URL.revokeObjectURL(a.href);
    } catch (e) {
      onToast((e as Error).message);
    } finally {
      setIoBusy(false);
    }
  };

  const doImport = async (file: File | undefined) => {
    if (!file) return;
    setIoBusy(true);
    try {
      const doc = JSON.parse(await file.text());
      const r = await rest.importConfig(doc);
      const n = r.imported;
      onToast(
        `Imported ${n.providers} provider(s), ${n.prompts} prompt(s), ${n.presets} preset(s)`,
      );
      onProvidersChanged();
      onPromptsChanged();
      setPresets(await rest.listPresets());
      const s = await rest.settings();
      setDefaultSys(s.default_system_prompt ?? "");
    } catch (e) {
      onToast((e as Error).message);
    } finally {
      setIoBusy(false);
    }
  };

  const save = () => {
    if (!hasChatId) {
      // No persisted chat yet: remember as defaults for the next new chat.
      setPendingDraft(draft);
      onToast("Saved as defaults for new chats");
      onClose();
      return;
    }
    onSave(draftToPatch(draft));
  };

  return (
    <aside className="drawer-in absolute inset-y-0 right-0 z-30 flex w-[340px] max-w-[92vw] flex-col border-l border-line bg-surface shadow-2xl">
      <header className="flex items-center gap-2 border-b border-line px-4 py-3">
        <h2 className="text-sm font-semibold">Chat settings</h2>
        <button
          className="ml-auto rounded-md p-1.5 text-muted hover:bg-surface-2 hover:text-ink"
          onClick={onClose}
          aria-label="Close settings"
        >
          <IconX />
        </button>
      </header>

      <div className="min-h-0 flex-1 space-y-6 overflow-y-auto px-4 py-4">
        {/* provider ------------------------------------------------ */}
        <section>
          <h3 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-wider text-muted">
            Provider
          </h3>
          {providers.length === 0 && !addingProvider && (
            <p className="text-xs text-muted">No providers yet. Add an OpenAI-compatible endpoint.</p>
          )}
          {providers.length > 0 && (
            <select
              className="select"
              value={draft.provider_id ?? ""}
              onChange={(e) =>
                setDraft((d) => ({ ...d, provider_id: e.target.value || null, model: null }))
              }
            >
              <option value="">— select provider —</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} {p.is_default ? "(default)" : ""}
                </option>
              ))}
            </select>
          )}
          {providers.length > 0 && (
            <ul className="mt-2 space-y-1.5">
              {providers.map((p) => (
                <li key={p.id} className="text-[0.72rem] text-muted">
                  <div className="flex items-center gap-2">
                    {p.kind !== "openai" && (
                      <span className="shrink-0 rounded bg-surface px-1.5 py-0.5 text-[10px] font-medium uppercase text-muted">
                        {p.kind}
                      </span>
                    )}
                    <span className="truncate">{p.base_url}</span>
                    <button
                      className="ml-auto shrink-0 rounded px-1.5 py-0.5 font-medium text-accent hover:bg-accent-soft disabled:opacity-50"
                      onClick={() => testProvider(p.id)}
                      disabled={testingProvider === p.id}
                    >
                      {testingProvider === p.id ? "Testing…" : "Test"}
                    </button>
                    <button
                      className="shrink-0 rounded px-1.5 py-0.5 font-medium text-accent hover:bg-accent-soft"
                      onClick={() => startEditProvider(p)}
                    >
                      Edit
                    </button>
                    <button
                      className="shrink-0 rounded p-1 hover:text-danger"
                      aria-label={`Delete provider ${p.name}`}
                      onClick={() => removeProvider(p.id)}
                    >
                      <IconTrash className="h-3.5 w-3.5" />
                    </button>
                  </div>
                  {testResults[p.id] && (
                    <p
                      className={
                        testResults[p.id].ok ? "mt-0.5 text-accent" : "mt-0.5 text-danger"
                      }
                    >
                      {testResults[p.id].ok
                        ? `Reachable — ${testResults[p.id].models} models in ${testResults[p.id].ms} ms`
                        : `Unreachable — ${testResults[p.id].error}`}
                    </p>
                  )}
                </li>
              ))}
            </ul>
          )}
          {addingProvider ? (
            <div className="mt-3 space-y-2 rounded-lg border border-line bg-surface-2 p-3">
              <input
                className="input"
                placeholder="Name (e.g. Ollama)"
                value={pName}
                onChange={(e) => setPName(e.target.value)}
              />
              <input
                className="input"
                placeholder="Base URL (e.g. http://localhost:11434/v1)"
                value={pUrl}
                onChange={(e) => setPUrl(e.target.value)}
              />
              <select
                className="input"
                aria-label="API type"
                value={pKind}
                onChange={(e) => setPKind(e.target.value)}
              >
                <option value="openai">OpenAI-compatible</option>
                <option value="anthropic">Anthropic (Messages API)</option>
                <option value="gemini">Gemini (generateContent)</option>
              </select>
              <input
                className="input"
                placeholder={editingProvider ? "API key (leave empty to keep)" : "API key (optional)"}
                type="password"
                value={pKey}
                onChange={(e) => setPKey(e.target.value)}
              />
              <div className="flex justify-end gap-2 pt-1">
                <button
                  className="rounded-md px-3 py-1.5 text-xs text-muted hover:bg-surface"
                  onClick={closeProviderForm}
                >
                  Cancel
                </button>
                <button
                  className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-strong disabled:opacity-50"
                  disabled={savingProvider}
                  onClick={saveProviderForm}
                >
                  {savingProvider ? "Testing…" : editingProvider ? "Save provider" : "Add provider"}
                </button>
              </div>
            </div>
          ) : (
            <button
              className="mt-2 flex items-center gap-1 text-xs font-medium text-accent hover:underline"
              onClick={() => setAddingProvider(true)}
            >
              <IconPlus className="h-3.5 w-3.5" /> Add provider
            </button>
          )}
        </section>

        {/* knowledge base --------------------------------------------- */}
        <section>
          <h3 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-wider text-muted">
            Knowledge base
          </h3>
          <div className="space-y-2 rounded-lg border border-line bg-surface-2 p-3">
            <label className="block">
              <div className="mb-1 text-xs">Embeddings provider</div>
              <select
                className="input"
                value={embedProvider}
                onChange={(e) => setEmbedProvider(e.target.value)}
              >
                <option value="">— none —</option>
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
            <label className="block">
              <div className="mb-1 text-xs">Embeddings model</div>
              <input
                className="input"
                placeholder="e.g. nomic-embed-text"
                value={embedModel}
                onChange={(e) => setEmbedModel(e.target.value)}
              />
            </label>
            <div className="flex justify-end">
              <button
                className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-strong disabled:opacity-50"
                disabled={kbBusy}
                onClick={saveEmbedConfig}
              >
                Save embeddings config
              </button>
            </div>
          </div>
          <label className="mt-2 flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-dashed border-line px-3 py-3 text-xs text-muted hover:border-accent hover:text-accent">
            <IconPlus className="h-3.5 w-3.5" />
            {kbBusy ? "Working…" : "Upload .txt / .md document"}
            <input
              type="file"
              accept=".txt,.md,text/plain,text/markdown"
              className="hidden"
              onChange={(e) => {
                const f = e.target.files?.[0];
                e.target.value = "";
                if (f) void uploadKbFile(f);
              }}
            />
          </label>
          {kbNote && <p className="mt-1.5 text-xs text-muted">{kbNote}</p>}
          {kbSources.length > 0 && (
            <ul className="mt-2 space-y-1">
              {kbSources.map((g) => (
                <li key={g.source} className="flex items-center gap-2 text-xs">
                  <span className="truncate">{g.source}</span>
                  <span className="shrink-0 rounded bg-surface px-1.5 py-0.5 text-[10px] text-muted">
                    {g.docs.length} chunks
                  </span>
                  {!g.docs.every((d) => d.has_vector) && (
                    <span className="shrink-0 rounded bg-danger/10 px-1.5 py-0.5 text-[10px] text-danger">
                      not indexed
                    </span>
                  )}
                  <button
                    className="ml-auto shrink-0 rounded p-1 hover:text-danger"
                    aria-label={`Delete ${g.source}`}
                    onClick={() => removeKbFile(g.source)}
                  >
                    <IconTrash className="h-3.5 w-3.5" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        {/* voice (TTS / STT) ------------------------------------------- */}
        <section>
          <h3 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-wider text-muted">
            Voice (TTS / STT)
          </h3>
          <div className="space-y-2 rounded-lg border border-line bg-surface-2 p-3">
            <div className="grid grid-cols-2 gap-2">
              <label className="block">
                <div className="mb-1 text-xs">TTS provider</div>
                <select
                  className="input"
                  value={ttsProvider}
                  onChange={(e) => setTtsProvider(e.target.value)}
                >
                  <option value="">— none —</option>
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="block">
                <div className="mb-1 text-xs">TTS model</div>
                <input
                  className="input"
                  placeholder="e.g. tts-1"
                  value={ttsModel}
                  onChange={(e) => setTtsModel(e.target.value)}
                />
              </label>
              <label className="block">
                <div className="mb-1 text-xs">STT provider</div>
                <select
                  className="input"
                  value={sttProvider}
                  onChange={(e) => setSttProvider(e.target.value)}
                >
                  <option value="">— none —</option>
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="block">
                <div className="mb-1 text-xs">STT model</div>
                <input
                  className="input"
                  placeholder="e.g. whisper-1"
                  value={sttModel}
                  onChange={(e) => setSttModel(e.target.value)}
                />
              </label>
            </div>
            <div className="flex justify-end">
              <button
                className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-strong disabled:opacity-50"
                disabled={voiceBusy}
                onClick={saveVoiceConfig}
              >
                Save voice config
              </button>
            </div>
          </div>
        </section>

        {/* model --------------------------------------------------- */}
        <section>
          <h3 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-wider text-muted">
            Model {draft.model ? `· ${draft.model}` : ""}
          </h3>
          {draft.provider_id ? (
            <>
              <input
                className="input"
                placeholder={loadingModels ? "Loading models…" : "Search models"}
                value={modelQuery}
                onChange={(e) => setModelQuery(e.target.value)}
              />
              {modelError && <p className="mt-1 text-xs text-danger">{modelError}</p>}
              <div className="mt-1.5 max-h-52 overflow-y-auto rounded-lg border border-line">
                {loadingModels && <p className="px-3 py-2 text-xs text-muted">Fetching…</p>}
                {!loadingModels && shownModels.length === 0 && !modelError && (
                  <p className="px-3 py-2 text-xs text-muted">No models found.</p>
                )}
                {shownModels.map((m) => (
                  <button
                    key={m.id}
                    className={`block w-full px-3 py-1.5 text-left text-xs transition-colors ${
                      draft.model === m.id
                        ? "bg-accent-soft font-medium"
                        : "hover:bg-surface-2"
                    }`}
                    onClick={() => setDraft((d) => ({ ...d, model: m.id }))}
                  >
                    <span className="flex items-center gap-1.5">
                      <span className="truncate font-mono">{m.id}</span>
                      {capsFor.get(m.id)?.vision && <CapsBadge label="vision" />}
                      {capsFor.get(m.id)?.reasoning && <CapsBadge label="reasoning" />}
                      {capsFor.get(m.id)?.ctx_len != null && (
                        <CapsBadge label={`${Math.round(capsFor.get(m.id)!.ctx_len! / 1000)}k ctx`} />
                      )}
                    </span>
                    {m.name !== m.id && (
                      <span className="block truncate text-[0.68rem] text-muted">{m.name}</span>
                    )}
                  </button>
                ))}
              </div>
              {draft.model && (
                <div className="mt-2 rounded-lg border border-line bg-surface-2 p-3">
                  <div className="mb-2 text-[0.68rem] font-semibold uppercase tracking-wider text-muted">
                    Capabilities · {draft.model}
                  </div>
                  <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                    <label className="flex items-center gap-1.5 text-xs">
                      <input
                        type="checkbox"
                        checked={selectedCaps?.vision ?? false}
                        onChange={(e) =>
                          saveCaps(e.target.checked, selectedCaps?.reasoning ?? false, parseCtx(capCtx))
                        }
                      />
                      Vision
                    </label>
                    <label className="flex items-center gap-1.5 text-xs">
                      <input
                        type="checkbox"
                        checked={selectedCaps?.reasoning ?? false}
                        onChange={(e) =>
                          saveCaps(selectedCaps?.vision ?? false, e.target.checked, parseCtx(capCtx))
                        }
                      />
                      Reasoning
                    </label>
                    <label className="flex items-center gap-1.5 text-xs">
                      Context
                      <input
                        className="input w-24 py-0.5 text-xs"
                        type="number"
                        min={1}
                        placeholder="unknown"
                        value={capCtx}
                        onChange={(e) => setCapCtx(e.target.value)}
                        onBlur={() =>
                          saveCaps(
                            selectedCaps?.vision ?? false,
                            selectedCaps?.reasoning ?? false,
                            parseCtx(capCtx),
                          )
                        }
                      />
                    </label>
                  </div>
                </div>
              )}
            </>
          ) : (
            <p className="text-xs text-muted">Select a provider first.</p>
          )}
        </section>

        {/* params ---------------------------------------------------- */}
        <section className="space-y-4">
          <h3 className="text-[0.7rem] font-semibold uppercase tracking-wider text-muted">
            Parameters
          </h3>
          <label className="block">
            <div className="mb-1 flex justify-between text-xs">
              <span>Temperature</span>
              <span className="font-mono text-muted">{draft.temperature.toFixed(2)}</span>
            </div>
            <input
              type="range"
              min={0}
              max={2}
              step={0.05}
              className="w-full"
              value={draft.temperature}
              onChange={(e) => setDraft((d) => ({ ...d, temperature: Number(e.target.value) }))}
            />
          </label>
          <label className="block">
            <div className="mb-1 flex justify-between text-xs">
              <span>Top P</span>
              <span className="font-mono text-muted">{draft.top_p.toFixed(2)}</span>
            </div>
            <input
              type="range"
              min={0}
              max={1}
              step={0.05}
              className="w-full"
              value={draft.top_p}
              onChange={(e) => setDraft((d) => ({ ...d, top_p: Number(e.target.value) }))}
            />
          </label>
          <label className="block">
            <div className="mb-1 text-xs">Max tokens (blank = provider default)</div>
            <input
              className="input"
              type="number"
              min={1}
              placeholder="e.g. 2048"
              value={draft.max_tokens}
              onChange={(e) => setDraft((d) => ({ ...d, max_tokens: e.target.value }))}
            />
          </label>
          <label className="block">
            <div className="mb-1 text-xs">Reasoning effort</div>
            <select
              className="select"
              value={draft.reasoning_effort}
              onChange={(e) => setDraft((d) => ({ ...d, reasoning_effort: e.target.value }))}
            >
              <option value="off">Off / model default</option>
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
            </select>
          </label>
          <label className="block">
            <div className="mb-1 text-xs">System prompt</div>
            <textarea
              className="textarea min-h-24"
              placeholder="You are a concise, rigorous assistant…"
              value={draft.system_prompt}
              onChange={(e) => setDraft((d) => ({ ...d, system_prompt: e.target.value }))}
            />
          </label>
          <label className="flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={draft.tools_enabled}
              onChange={(e) => setDraft((d) => ({ ...d, tools_enabled: e.target.checked }))}
            />
            <span>
              Web search tool{" "}
              <span className="text-muted">(model can call web_search; needs HALO_WEB_SEARCH_URL)</span>
            </span>
          </label>
          <label className="flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={draft.kb_enabled}
              onChange={(e) => setDraft((d) => ({ ...d, kb_enabled: e.target.checked }))}
            />
            <span>
              Knowledge base{" "}
              <span className="text-muted">
                (retrieve from uploaded documents; configure them in the Knowledge base section)
              </span>
            </span>
          </label>
        </section>

        {/* presets --------------------------------------------------- */}
        <section>
          <h3 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-wider text-muted">
            Presets
          </h3>
          {presets.length === 0 && (
            <p className="text-xs text-muted">No presets yet. Save the current parameters as one.</p>
          )}
          {presets.length > 0 && (
            <ul className="space-y-1">
              {presets.map((p) => (
                <li key={p.id} className="flex items-center gap-2 text-xs">
                  <span className="truncate">{p.name}</span>
                  <button
                    className="ml-auto shrink-0 rounded px-1.5 py-0.5 font-medium text-accent hover:bg-accent-soft"
                    onClick={() => applyPreset(p)}
                  >
                    Apply
                  </button>
                  <button
                    className="shrink-0 rounded p-1 hover:text-danger"
                    aria-label={`Delete preset ${p.name}`}
                    onClick={() => removePreset(p.id)}
                  >
                    <IconTrash className="h-3.5 w-3.5" />
                  </button>
                </li>
              ))}
            </ul>
          )}
          <button
            className="mt-2 flex items-center gap-1 text-xs font-medium text-accent hover:underline"
            onClick={savePreset}
          >
            <IconPlus className="h-3.5 w-3.5" /> Save current as preset
          </button>
        </section>

        {/* prompts --------------------------------------------------- */}
        <section>
          <h3 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-wider text-muted">
            Prompts
          </h3>
          <p className="mb-2 text-[0.68rem] text-muted">
            Insert a prompt body in the composer with <code>/title</code>.
          </p>
          {promptEdit ? (
            <div className="space-y-2 rounded-lg border border-line bg-surface-2 p-3">
              <input
                className="input"
                placeholder="Title (used for /autocomplete)"
                value={promptEdit.title}
                onChange={(e) => setPromptEdit({ ...promptEdit, title: e.target.value })}
              />
              <textarea
                className="textarea min-h-20"
                placeholder="Prompt body…"
                value={promptEdit.body}
                onChange={(e) => setPromptEdit({ ...promptEdit, body: e.target.value })}
              />
              <div className="flex justify-end gap-2 pt-1">
                <button
                  className="rounded-md px-3 py-1.5 text-xs text-muted hover:bg-surface"
                  onClick={() => setPromptEdit(null)}
                >
                  Cancel
                </button>
                <button
                  className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-strong disabled:opacity-50"
                  disabled={savingPrompt}
                  onClick={savePrompt}
                >
                  {savingPrompt ? "Saving…" : "Save prompt"}
                </button>
              </div>
            </div>
          ) : (
            <>
              {prompts.length === 0 && (
                <p className="text-xs text-muted">No prompts yet.</p>
              )}
              {prompts.length > 0 && (
                <ul className="space-y-1">
                  {prompts.map((p) => (
                    <li key={p.id} className="flex items-center gap-2 text-xs">
                      <button
                        className="min-w-0 truncate text-left font-medium hover:text-accent"
                        onClick={() =>
                          setPromptEdit({ id: p.id, title: p.title, body: p.body })
                        }
                      >
                        {p.title}
                      </button>
                      <span className="min-w-0 flex-1 truncate text-[0.68rem] text-muted">
                        {p.body.slice(0, 40)}
                      </span>
                      <button
                        className="shrink-0 rounded p-1 hover:text-danger"
                        aria-label={`Delete prompt ${p.title}`}
                        onClick={() => removePrompt(p.id)}
                      >
                        <IconTrash className="h-3.5 w-3.5" />
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              <button
                className="mt-2 flex items-center gap-1 text-xs font-medium text-accent hover:underline"
                onClick={() => setPromptEdit({ id: null, title: "", body: "" })}
              >
                <IconPlus className="h-3.5 w-3.5" /> Add prompt
              </button>
            </>
          )}
        </section>

        {/* account --------------------------------------------------- */}
        <section>
          <h3 className="mb-2 text-[0.7rem] font-semibold uppercase tracking-wider text-muted">
            Account
          </h3>
          <label className="block">
            <div className="mb-1 text-xs">
              Default system prompt (used for new chats without one)
            </div>
            <textarea
              className="textarea min-h-20"
              placeholder="Leave empty to send no default system prompt"
              value={defaultSys}
              onChange={(e) => setDefaultSys(e.target.value)}
            />
          </label>
          <div className="mt-2 flex justify-end">
            <button
              className="rounded-md bg-accent px-3 py-1.5 text-xs font-medium text-white hover:bg-accent-strong disabled:opacity-50"
              disabled={savingAccount}
              onClick={saveAccount}
            >
              {savingAccount ? "Saving…" : "Save"}
            </button>
          </div>
          <div className="mt-4 border-t border-line pt-3">
            <div className="mb-1 text-xs">Config</div>
            <p className="mb-2 text-[0.68rem] text-muted">
              Export/import providers, prompts, presets and settings (not chats).
            </p>
            <label className="mb-2 flex items-center gap-1.5 text-xs">
              <input
                type="checkbox"
                checked={exportSecrets}
                onChange={(e) => setExportSecrets(e.target.checked)}
              />
              Include API keys in export
            </label>
            <div className="flex gap-2">
              <button
                className="rounded-md border border-line px-3 py-1.5 text-xs font-medium hover:bg-surface-2 disabled:opacity-50"
                disabled={ioBusy}
                onClick={doExport}
              >
                Export JSON
              </button>
              <label className="cursor-pointer rounded-md border border-line px-3 py-1.5 text-xs font-medium hover:bg-surface-2">
                Import JSON
                <input
                  type="file"
                  accept="application/json,.json"
                  className="hidden"
                  onChange={(e) => {
                    void doImport(e.target.files?.[0]);
                    e.target.value = "";
                  }}
                />
              </label>
            </div>
          </div>
        </section>
      </div>

      <footer className="flex gap-2 border-t border-line px-4 py-3">
        <button
          className="flex-1 rounded-lg bg-accent px-3 py-2 text-sm font-medium text-white hover:bg-accent-strong"
          onClick={save}
        >
          Save
        </button>
        <button
          className="rounded-lg px-3 py-2 text-sm text-muted hover:bg-surface-2 hover:text-ink"
          onClick={onClose}
        >
          Cancel
        </button>
      </footer>
    </aside>
  );
}

// New-chat defaults kept client-side until a chat exists (Phase 2 moves server-side).
let pendingDraft: ChatDraft | null = null;
export const setPendingDraft = (d: ChatDraft) => {
  pendingDraft = d;
};
export const takePendingDraft = () => {
  const d = pendingDraft;
  pendingDraft = null;
  return d;
};
