# Phase 1 — Prototype (build now)

**Definition of done:** `cargo run -p halo-server` + `npm run dev` in `web/` → open
browser, add an Ollama/OpenRouter provider, stream a chat (text + image), tune model
settings per chat, toggle light/dark.

## Checklist
### Backend
- [x] Workspace scaffold, 3 crates, workspace deps (axum 0.8, sqlx 0.9 sqlite, reqwest SSE)
- [x] Migrations: providers / chats / messages
- [x] Provider CRUD + `GET /api/providers/{id}/models` proxy
- [x] Chat CRUD (`GET /api/chats`, create/get/patch/delete; get includes messages)
- [x] `POST /api/chats/{id}/messages` → SSE relay w/ persistence (meta/token/reasoning/done/error)
- [x] `POST /api/uploads` (images ≤ 15 MB, jpg/png/gif/webp) + static serving
- [x] CORS for vite dev, `HALO_PORT` env, seed nothing (clean slate)

### Frontend
- [x] Vite + React + TS + Tailwind v4; `/api` proxy → :8420
- [x] Shell: sidebar + header + thread + composer; responsive
- [x] Streaming renderer w/ SSE parser, markdown + code highlight + copy
- [x] Reasoning collapsible block
- [x] Image attach (paste/drag/pick) → data URLs on send
- [x] SettingsDrawer: provider add, model picker (live fetch), temp/top_p/max tokens,
      system prompt, reasoning effort
- [x] Light/dark theme w/ system default + persisted toggle
- [x] Chat list: create/select/rename/delete, grouped by recency

### Explicitly deferred to Phase 2
Auth, regenerate/edit message, search, prompt library, folders, stop-generation button
(cancel is UI-side in P1 if trivial), multiple concurrent streams, key encryption.

## Verification commands
```bash
cargo build --workspace && cargo test --workspace
cd web && npm run build       # typechecks + bundles
curl -s localhost:8420/api/health
```

Verified 2026-10-31 against a mock OpenAI SSE server (:9999): provider key redaction,
model proxy, chat settings persistence on create, full meta/reasoning/token/done stream,
assistant content + reasoning persisted, auto-title, image parts reach upstream and are
stored on the message row, uploads served + non-image 400 rejected.
Smoke with Ollama: add provider base_url `http://localhost:11434/v1`, key `ollama`,
pick `llava`/`qwen2.5vl` for image tests.
