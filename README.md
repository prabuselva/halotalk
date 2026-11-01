# HaloTalk

A lightweight, self-hosted chat UI for LLM backends — single Rust binary (axum +
sqlx/SQLite, web embedded) serving a React + Vite + Tailwind SPA. Streaming
replies with reasoning display, image attachments (vision), per-chat model
settings, branching/regeneration, chat tags + search, prompt library, presets,
tool calls with built-in web search, RAG over uploaded .md/.txt via any
`/embeddings` endpoint, voice (TTS/STT), PWA offline shell, multi-user auth
with encrypted API keys, light/dark themes.

## Quick start

Prereqs: Rust (stable), Node 20+ (only needed to rebuild the web assets;
release binaries ship them embedded).

```bash
# 1. Build the frontend (server serves web/dist)
cd web && npm install && npm run build && cd ..

# 2. Run the server (creates ./data/halotalk.db on first start)
cargo run -p halo-server          # http://127.0.0.1:8420

# 3. Open http://127.0.0.1:8420 → first-run creates the admin account
#    → Settings → add a provider (keys are stored ChaCha20-Poly1305-encrypted)
```

Dev mode (hot reload for both sides):

```bash
cargo run -p halo-server          # terminal 1 (API on :8420)
cd web && npm run dev             # terminal 2 (UI on :5173, proxies /api + /uploads)
```

### Providers

OpenAI-compatible by default; native Anthropic and Gemini endpoints are
supported per-provider (provider `kind`).

| Backend    | Base URL example                | API key        |
| ---------- | ------------------------------- | -------------- |
| OpenRouter | `https://openrouter.ai/api/v1`  | your key       |
| Ollama     | `http://localhost:11434/v1`     | `ollama`       |
| LM Studio  | `http://localhost:1234/v1`      | `lm-studio`    |
| vLLM       | `http://localhost:8000/v1`      | as configured  |
| Anthropic  | `https://api.anthropic.com`     | `kind=anthropic` |
| Gemini     | `https://generativelanguage.googleapis.com` | `kind=gemini` |

## Configuration (env vars)

| Var                 | Default            | Purpose                          |
| ------------------- | ------------------ | -------------------------------- |
| `HALO_PORT`         | `8420`             | HTTP port                        |
| `HALO_HOST`         | `127.0.0.1`        | Bind address                     |
| `HALO_DB_PATH`     | `data/halotalk.db` | SQLite file                      |
| `HALO_UPLOAD_DIR`   | `data/uploads`     | Upload storage                   |
| `HALO_SECRET`       | auto (`data/secret.key`, 0600) | key-encryption master key |
| `HALO_WEB_DIST`     | `web/dist`         | static assets (embedded in release) |
| `HALO_WEB_SEARCH_URL` | —                | endpoint for the built-in search tool |

Run the server from the repo root so the default relative paths resolve.

## Docker

Multi-stage `Dockerfile` (node web build → rust release w/ rust-embed →
debian:trixie-slim runtime), single image, healthcheck included:

```bash
podman build --format docker -t halotalk .   # HEALTHCHECK needs docker format
podman run -d -p 8420:8420 -v halotalk-data:/data halotalk
```

The image targets glibc ≥ 2.38 hosts (Debian 13 / Ubuntu 24.04+).

## Release binaries

`scripts/build-multiarch.sh` (podman-driven cross builds) produces
`dist/release/halotalk-linux-{amd64,arm64,armv7}` + `SHA256SUMS` — web
embedded, TLS via rustls (no OpenSSL). Same glibc baseline as the container.

## Development

```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
npm run build --prefix web
npm run test:e2e --prefix web     # Playwright smoke vs scripts/mock_llm.py (:9999)
```

`scripts/mock_llm.py` is a full mock backend for e2e (OpenAI/Anthropic/Gemini
SSE, tools, embeddings, speech). Status and honest scope-outs live in
`docs/development_phases/status.md`.

## Layout

```
crates/halo-core       domain models, SQLite schema, store, key crypto
crates/halo-providers  AnyClient: OpenAI-compatible / Anthropic / Gemini streaming
crates/halo-server     axum API + SSE relay + static hosting (binary: halotalk)
web/                   React + Vite + Tailwind v4 SPA (Playwright e2e in web/e2e)
scripts/               mock_llm.py, build-multiarch.sh
docs/development_phases/  plans, architecture, phase checklists, ADRs, status
```

## Status

Phases 1–3 complete (auth, branching, tools/search, RAG v1, voice, native
providers, PWA, palette, LaTeX/mermaid, export, Docker, multi-arch, e2e).
Honest scope-outs (PDF export/extraction, arm run-smoke without qemu, …) are
listed in `docs/development_phases/status.md`.
