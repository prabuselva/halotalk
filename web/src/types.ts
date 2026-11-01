export interface Provider {
  id: string;
  name: string;
  base_url: string;
  kind: string;
  is_default: boolean;
  user_id: string | null;
  created_at: string;
}

export interface ModelInfo {
  id: string;
  name: string;
}

export interface ProviderTestResult {
  ok: boolean;
  models?: number;
  ms?: number;
  error?: string;
}

export interface Chat {
  id: string;
  title: string;
  provider_id: string | null;
  model: string | null;
  temperature: number | null;
  top_p: number | null;
  max_tokens: number | null;
  system_prompt: string | null;
  reasoning_effort: string | null;
  pinned: boolean;
  archived: boolean;
  tags: string[];
  tools_enabled: boolean;
  kb_enabled: boolean;
  created_at: string;
  updated_at: string;
}

export interface User {
  id: string;
  username: string;
  is_admin: boolean;
}

export interface Recovery {
  recovery_question: string;
  recovery_answer: string;
}

export interface Health {
  app: string;
  ok: boolean;
  version: string;
}

export interface AuthStatus {
  needs_setup: boolean;
  open_signup: boolean;
}

export type Role = "user" | "assistant" | "system" | "tool";
export interface SourceRef {
  title: string;
  score: number;
}
export interface KbDocEntry {
  id: string;
  source: string;
  title: string;
  chars: number;
  has_vector: boolean;
  embed_model: string;
  created_at: string;
}
export interface Message {
  id: string;
  role: Role;
  content: string;
  reasoning: string | null;
  images: string[];
  error: string | null;
  reply_to: string | null;
  active: boolean;
  tool_call_id?: string | null;
  sources?: SourceRef[];
  created_at: string;
}

export interface SearchHit {
  chat_id: string;
  message_id: string;
  chat_title: string;
  role: Role;
  snippet: string;
}

export interface Prompt {
  id: string;
  title: string;
  body: string;
  created_at: string;
}

export interface Preset {
  id: string;
  name: string;
  params: Record<string, unknown>;
  created_at: string;
}

export interface RegistryEntry {
  model_id: string;
  vision: boolean;
  reasoning: boolean;
  ctx_len: number | null;
}
