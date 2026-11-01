# HaloTalk — Architecture

## Workspace layout
```
halotalk/
├── Cargo.toml                 # [workspace] members = crates/*
├── crates/
│   ├── halo-core/             # Domain types + DB (sqlx) + migrations. No HTTP.
│   ├── halo-providers/        # OpenAI + Anthropic + Gemini clients; AnyClient dispatches by kind.
│   └── halo-server/           # axum binary: routes, SSE, uploads, static file serving.
├── web/                       # React + Vite + Tailwind (talks to halo-server over /api)
├── docs/development_phases/
└── data/                      # runtime: halotalk.db + uploads/
```

Dependency direction: `halo-server → {halo-core, halo-providers}`; the two leaf crates never
depend on each other except via halo-core types.

## Data model (SQLite)
```sql
providers(id, name, base_url, api_key, is_default, kind, user_id, created_at)
chats(id, title, provider_id, model, temperature REAL, top_p REAL,
      max_tokens INT, system_prompt TEXT, reasoning_effort TEXT,
      user_id, pinned INT, archived INT, tags TEXT /* JSON array */,
       tools_enabled INT /* web_search opt-in */,
       kb_enabled INT /* RAG v1 retrieval opt-in */,
       created_at, updated_at)
messages(id, chat_id, role /* user|assistant|tool */, content TEXT, reasoning TEXT,
         images TEXT /* JSON: ["/uploads/<file>", ...] */,
         reply_to TEXT /* parent user msg id on assistant replies */,
         active INT /* branch selection */,
         tool_calls TEXT /* JSON tool-call group, repeated on each tool row */,
         tool_call_id TEXT /* id on role=tool rows */,
         sources TEXT /* JSON citations [{title,score}] on assistant replies */,
         created_at, error TEXT)
kb_documents(id, user_id, source /* uploaded file name */, title /* "file · N" when chunked */,
         text TEXT, vector TEXT /* JSON f32 array; NULL until embedded */,
         embed_model, created_at)
users(id, username, pass_hash, is_admin, created_at) + sessions(token_hash, …)
```
Settings travel with each chat (`chats` row) so per-chat model config "just works".

## API (all JSON under /api; SSE for chat)
| Method | Path | Purpose |
|---|---|---|
| GET | /api/health | liveness `{ok, app, version}` (public; UI shows `version` on AuthGate + sidebar footer) |
| GET | /api/auth/status | `{needs_setup, open_signup}` (public, used by AuthGate) |
| POST | /api/auth/setup | first-run admin `{username, password, recovery_question, recovery_answer}` → session cookie; adopts legacy rows |
| POST | /api/auth/signup | register when `open_signup` on; recovery question required (answer stored argon2-normalized, never serialized) → session cookie |
| POST | /api/auth/login / /logout | `{username, password}` → cookie / clears session (204) |
| GET | /api/auth/me | current user `{id, username, is_admin}` |
| PATCH | /api/auth/open-signup | admin-only `{open_signup: bool}` |
| GET/PUT | /api/settings | per-user settings key/value map (`default_system_prompt` applied to new chats without one; `embed_provider_id` + `embed_model` configure RAG embeddings; `tts_provider_id` + `tts_model` + `stt_provider_id` + `stt_model` configure voice) |
| GET/POST | /api/providers | list / add providers |
| PATCH/DELETE | /api/providers/{id} | update (api_key optional) / remove |
| GET | /api/providers/{id}/models | proxy upstream `GET /v1/models` (advertised caps seeded into registry once per model; manual edits win) |
| POST | /api/providers/{id}/test | connectivity check → `{ok, models, ms}` or `{ok:false, error, ms}` |
| GET/POST | /api/prompts | per-user prompt library list / create `{title, body}` |
| PATCH/DELETE | /api/prompts/{id} | rename/edit body / delete |
| GET/POST | /api/presets | named chat-param sets list / create `{name, params}` (params allowlist) |
| DELETE | /api/presets/{id} | remove preset |
| GET/PUT | /api/registry | model capabilities `?provider_id=` / upsert `{provider_id, model_id, vision, reasoning, ctx_len}` |
| GET | /api/config/export | user config bundle (`?include_secrets=1` to embed provider API keys; chats excluded) |
| POST | /api/config/import | merge config bundle; providers matched by name+URL (registry remapped by name); prompts/presets deduped; settings upserted |
| GET/POST | /api/kb | knowledge-base chunk list / upload `{source, text}` → `{ids, chunks, embedded}` (chunks ≤1200 chars; embeds only when configured) |
| DELETE | /api/kb/{id} · /api/kb?source= | remove one chunk / all chunks of one uploaded file |
| GET/POST | /api/chats | list (`?archived=1` includes archived) / create |
| GET/PATCH/DELETE | /api/chats/{id} | fetch (with messages, incl. role=tool rows + `sources` citations) / update settings, pinned, archived, tags, tools_enabled, kb_enabled / delete |
| POST | /api/chats/{id}/messages | body `{content, images:[dataURL…]}` → **202** `{ok:true}`; reply streams on `/events`. 409 if a turn is already running for this chat |
| POST | /api/chats/{id}/regenerate | body `{message_id?}` → 202; new sibling reply (old ones kept as branches); drops turns after that position; 409 while a turn runs |
| POST | /api/chats/{id}/messages/{mid}/edit | body `{content}` → 202; user msg only, truncates after it (discards sibling replies); 409 while a turn runs |
| GET | /api/chats/{id}/events | **SSE hub channel** — open for the whole chat lifetime: joins the live turn or an idle placeholder; first frame `state` (snapshot), stream ends when a live turn finishes so EventSource reconnects fresh. Browser retry gap is ~3 s, so a faster-than-that turn can finish unseen: the client refetches the transcript on every `state {"streaming":false}` join and on `resync` |
| POST | /api/chats/{id}/stop | cancel the running turn → partial answer is persisted and finalized on `/events`; `{ok:true,"stopped":bool}` |
| POST | /api/chats/{id}/messages/{mid}/activate | switch which sibling assistant reply is active on that branch |
| GET | /api/search?q= | FTS5 prefix search; `#tag` tokens match chat tags (AND-ed, work with text terms, archived excluded) → `{hits:[{chat_id, chat_title, message_id, role, snippet}]}` |
| GET | /api/uploads/{file} | uploaded images |
| GET, HEAD | /uploads/{file} | same, public path for `<img src>` |
| static / | web/dist in release; vite dev server proxies /api in dev |

All `/api/*` routes require the `halo_session` cookie, except `/api/auth/*` and `/api/health`.
Cross-user resource access returns **404** (existence is not leaked). API keys live in the DB as
ChaCha20-Poly1305 ciphertext (`xc1:` prefix; legacy fernet `fer1:` rows decrypt to empty, forcing
key re-entry — the pure-Rust migration in 3.3b dropped openssl).

### Chat event protocol (`GET /api/chats/{id}/events`, `text/event-stream`)
Every generation runs **detached from HTTP** in a per-chat hub `Turn`
(`crates/halo-server/src/hub.rs`): producing, persisting and broadcasting frames
to any number of subscribers (multi-device sync) with client disconnects
never affecting the run. Send/regenerate/edit only enqueue the turn (202).
```
event: state      data: {"streaming":true,…,"seq":N}                    (join frame: live snapshot — content/reasoning so far + seq floor; or {"streaming":false} when idle)
event: user       data: [message]                                       (the user turn that started generation — echoed to every subscriber)
event: resync     data: {"reason":"regenerate"|"edit"}                  (history before this point changed — fetch GET /api/chats/{id})
event: meta       data: {"chat_id":"…","message_id":"…","title":"…"}   (first frame of the reply proper)
event: reasoning  data: {"delta":"…","seq":N}                          (thinking tokens)
event: token      data: {"delta":"…","seq":N}                           (answer tokens; seq monotonic per turn)
event: tool       data: {"phase":"call","id","name","arguments"}        (tool call started)
event: tool       data: {"phase":"result","id","ok","preview"}          (tool ran; ≤200 chars preview)
event: sources    data: [{"title":"…","score":0.35}, …]                 (kb chunks injected as context; kb_enabled chats with a match, after meta)
event: title      data: {"title":"…"}                                  (LLM chat title, first exchange only)
event: error      data: {"message":"…"}                                (only on failure)
event: done       data: {}                                              (answer complete; a late `title` may follow before close)
```
Clients apply deltas only when `seq > last_seq`; the `state` snapshot carries the
current `seq` as a floor, so a mid-turn reconnect never double-applies or loses
deltas. The server persists the user message on POST, creates the assistant row
at turn start, appends deltas as they are broadcast, and finalizes when the
upstream ends (or `/stop` fires — partial text is kept and `done` still
emitted). Idle `/events` streams close on their own the moment a turn starts
(or the chat is deleted), prompting the browser's EventSource to reconnect into
the live turn.

## Chat request flow
1. Frontend posts the **full user turn** (text + images as data URLs) + chat_id.
2. Server loads chat settings + last N (=32) messages, maps them to OpenAI `messages[]`
   with multimodal `content` parts (`text` + `image_url`), prepends system prompt.
    `role=tool` rows replay as one synthesized assistant message carrying `tool_calls`
    (group JSON stored on each tool row) followed by the matching `tool` message.
 2b. `kb_enabled` chats: the last user message is embedded (`/embeddings` on the user's
    `embed_provider_id`/`embed_model`), cosine-scored against all `kb_documents` vectors
    (top 4, score > 0.15) and injected as a second system message; the citations travel
    as the `sources` SSE event and persist on `messages.sources`.
3. `halo-providers::AnyClient::stream(kind, …)` picks the upstream client by the provider's
   `kind` row: OpenAI-compatible `/v1/chat/completions` (default), Anthropic `/v1/messages`
   (`anthropic-version` header, SSE events translated to OpenAI-shaped chunks), or Gemini
   `/v1beta/models/{model}:streamGenerateContent?alt=sse` (`x-goog-api-key`; ids for
   `functionCall` parts are synthesized as `call_{n}`). All call
   with `stream:true` and passthrough params: `model, temperature, top_p, max_tokens,
   reasoning_effort` (only when set); when the chat has `tools_enabled`, the built-in
   `tools` array (currently `web_search`) is sent too (mapped to each API's tool format).
3b. Anthropic: system messages hoist to top-level `system`; `tool_calls`→`tool_use` blocks;
   `role=tool`→`tool_result` (with `tool_use_id`); consecutive same-role contents are folded
   (Anthropic requires alternating roles). Gemini: `systemInstruction`, role `model`/`user`,
   `functionCall`/`functionResponse` parts, `generationConfig` + `functionDeclarations`.
4. Upstream SSE lines parsed → `delta.content`→`token`, `delta.reasoning_content` /
   `delta.reasoning`→`reasoning`, `delta.tool_calls` fragments merged by index. Forwarded
   to client; accumulated into DB. If the round ends with tool calls, the server runs each
   tool (`web_search` POSTs `{"query"}` to `HALO_WEB_SEARCH_URL`), persists a `role=tool`
   row, emits `tool` frames, and streams again — up to 4 rounds. Tool errors return as
   tool-result JSON so the model can recover; `tool` rows are not FTS-indexed.

## Errors
Upstream/downstream failures return the SSE `error` frame (HTTP stays 200) and mark the
assistant message `error` column; UI shows a red retry affordance.

## Config
`HALO_DB_PATH` (default `data/halotalk.db`), `HALO_UPLOAD_DIR` (default `data/uploads`),
`HALO_PORT` (default 8420), `HALO_WEB_SEARCH_URL` (endpoint the built-in web_search tool
POSTs `{"query"}` to; unset → tool returns an error result). API keys stored in DB for the prototype — single-user trust
boundary (see ADR-004).
