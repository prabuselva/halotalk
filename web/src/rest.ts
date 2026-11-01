import type {
  AuthStatus,
  Chat,
  Health,
  KbDocEntry,
  Message,
  ModelInfo,
  Preset,
  Prompt,
  Provider,
  ProviderTestResult,
  Recovery,
  RegistryEntry,
  SearchHit,
  User,
} from "./types";

function unauthorized() {
  window.dispatchEvent(new Event("halo-unauthorized"));
}

async function errorMessage(res: Response): Promise<string> {
  try {
    const j = await res.json();
    if (j?.error) return j.error as string;
  } catch {
    /* use status */
  }
  return `request failed (${res.status})`;
}

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  if (res.status === 401 && !path.startsWith("/api/auth/")) unauthorized();
  if (!res.ok) throw new Error(await errorMessage(res));
  if (res.status === 204) return undefined as T;
  return res.json() as Promise<T>;
}

const post = <T,>(path: string, body: unknown) =>
  req<T>(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
const patch = <T,>(path: string, body: unknown) =>
  req<T>(path, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });

export const rest = {
  health: () => req<Health>("/api/health"),
  authStatus: () => req<AuthStatus>("/api/auth/status"),
  me: () => req<User>("/api/auth/me"),
  setup: (username: string, password: string, recovery: Recovery) =>
    post<User>("/api/auth/setup", { username, password, ...recovery }),
  signup: (username: string, password: string, recovery: Recovery) =>
    post<User>("/api/auth/signup", { username, password, ...recovery }),
  login: (username: string, password: string) => post<User>("/api/auth/login", { username, password }),
  logout: () => req<void>("/api/auth/logout", { method: "POST" }),
  setOpenSignup: (open: boolean) =>
    patch<void>("/api/auth/open-signup", { open_signup: open }),
  userSettings: () => req<{ settings: Record<string, string> }>("/api/settings").then((r) => r.settings),
  setUserSettings: (settings: Record<string, string>) =>
    req<{ settings: Record<string, string> }>("/api/settings", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ settings }),
    }),

  listProviders: () => req<{ providers: Provider[] }>("/api/providers").then((r) => r.providers),
  createProvider: (body: Record<string, unknown>) =>
    post<Provider>("/api/providers", body),
  updateProvider: (id: string, body: Record<string, unknown>) =>
    patch<Provider>(`/api/providers/${id}`, body),
  deleteProvider: (id: string) => req<void>(`/api/providers/${id}`, { method: "DELETE" }),
  models: (providerId: string) =>
    req<{ models: ModelInfo[] }>(`/api/providers/${providerId}/models`).then((r) => r.models),
  testProvider: (id: string) =>
    post<ProviderTestResult>(`/api/providers/${id}/test`, {}),
  providerMetrics: (id: string) =>
    req<{ metrics: string | null }>(`/api/providers/${id}/metrics`).then((r) => r.metrics),

  listKb: () => req<{ documents: KbDocEntry[] }>("/api/kb").then((r) => r.documents),
  addKb: (source: string, text: string) =>
    post<{ ids: string[]; chunks: number; embedded: boolean }>("/api/kb", { source, text }),
  deleteKbDoc: (id: string) => req<void>(`/api/kb/${id}`, { method: "DELETE" }),
  deleteKbFile: (source: string) =>
    req<{ deleted: number }>(`/api/kb?source=${encodeURIComponent(source)}`, { method: "DELETE" }),

  listChats: (archived = false) =>
    req<{ chats: Chat[] }>(archived ? "/api/chats?archived=1" : "/api/chats").then(
      (r) => r.chats,
    ),
  createChat: (body: Record<string, unknown> = {}) => post<Chat>("/api/chats", body),
  getChat: (id: string) =>
    req<{ chat: Chat; messages: Message[] }>(`/api/chats/${id}`),
  updateChat: (id: string, body: Record<string, unknown>) =>
    patch<Chat>(`/api/chats/${id}`, body),
  deleteChat: (id: string) => req<void>(`/api/chats/${id}`, { method: "DELETE" }),
  activateMessage: (chatId: string, mid: string) =>
    post<{ changed: boolean; index: number; total: number }>(
      `/api/chats/${chatId}/messages/${mid}/activate`,
      {},
    ),

  search: (q: string) =>
    req<{ hits: SearchHit[] }>(`/api/search?q=${encodeURIComponent(q)}`).then((r) => r.hits),

  listPrompts: () => req<{ prompts: Prompt[] }>("/api/prompts").then((r) => r.prompts),
  createPrompt: (title: string, body: string) =>
    post<Prompt>("/api/prompts", { title, body }),
  updatePrompt: (id: string, title: string, body: string) =>
    req<void>(`/api/prompts/${id}`, {
      method: "PATCH",
      body: JSON.stringify({ title, body }),
    }),
  deletePrompt: (id: string) =>
    req<void>(`/api/prompts/${id}`, { method: "DELETE" }),

  listPresets: () => req<{ presets: Preset[] }>("/api/presets").then((r) => r.presets),
  createPreset: (name: string, params: Record<string, unknown>) =>
    post<Preset>("/api/presets", { name, params }),
  deletePreset: (id: string) =>
    req<void>(`/api/presets/${id}`, { method: "DELETE" }),

  registry: (providerId: string) =>
    req<{ registry: RegistryEntry[] }>(
      `/api/registry?provider_id=${encodeURIComponent(providerId)}`,
    ).then((r) => r.registry),
  setRegistry: (
    providerId: string,
    modelId: string,
    vision: boolean,
    reasoning: boolean,
    ctxLen: number | null,
  ) =>
    req<void>("/api/registry", {
      method: "PUT",
      body: JSON.stringify({
        provider_id: providerId,
        model_id: modelId,
        vision,
        reasoning,
        ctx_len: ctxLen,
      }),
    }),

  settings: () => req<{ settings: Record<string, string> }>("/api/settings").then((r) => r.settings),
  putSettings: (settings: Record<string, string>) =>
    req<void>("/api/settings", { method: "PUT", body: JSON.stringify({ settings }) }),

  speech: async (text: string): Promise<Blob> => {
    const res = await fetch("/api/audio/speech", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ text }),
    });
    if (res.status === 401) unauthorized();
    if (!res.ok) throw new Error(await errorMessage(res));
    return res.blob();
  },
  transcribe: async (blob: Blob, filename = "speech.webm"): Promise<string> => {
    const form = new FormData();
    form.append("file", blob, filename);
    const res = await fetch("/api/audio/transcribe", { method: "POST", body: form });
    if (res.status === 401) unauthorized();
    if (!res.ok) throw new Error(await errorMessage(res));
    const j = (await res.json()) as { text?: string };
    return j.text ?? "";
  },

  exportConfig: (includeSecrets: boolean) =>
    req<unknown>(`/api/config/export${includeSecrets ? "?include_secrets=1" : ""}`),
  importConfig: (doc: unknown) =>
    post<{ imported: Record<string, number> }>("/api/config/import", doc),
};
