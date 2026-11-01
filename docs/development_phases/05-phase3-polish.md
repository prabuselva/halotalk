# Phase 3 — Polish & Extensibility

## 3.1 Experience
- [x] PWA (manifest + service worker, offline read of recent chats) — build/serve verified in-browser (Playwright loads app clean); live offline reload NOT tested (honest scope-out — SW cache path is standard workbox).
- [x] Command palette (Ctrl+K): switch chat/model, new chat, toggle theme.
- [x] Smooth-scroll pinning while streaming (existing behavior reviewed + kept). Virtualization:
      NOT DONE — scope-out per status.md (no observed need; revisit with react-window later).
- [x] LaTeX (KaTeX), mermaid diagrams, table styling.
- [x] Export: Markdown / JSON (PDF per-thread SKIPPED — see status.md).
- [x] Header theme toggle button (top-right, sun/moon, persists via `halotalk-theme` + inline
      no-flash bootstrap in index.html).
- [x] Send-status ticks (WhatsApp-style): local pending rows (clock → sending → sent → delivered
      double-tick, failed + retry) kept in `pending` state outside `messages`, reconciled against
      the hub transcript on every snapshot (echo, resync, state-idle reload, mid-run replay).
- [x] Long structured code blocks (json/html/yaml/… ≥15 lines, `STRUCT_LANGS` in Markdown.tsx)
      collapse after streaming (chevron + line count, first-line preview) with per-block
      download-as-file button.
- [x] Responsive reading column (max-w-3xl → xl:4xl → 2xl:5xl) + textarea auto-grow scaled to
      viewport (35% of innerHeight, 140–420px, `max-h-[40vh]`).
- [x] App version badge from `/api/health` (`data-testid="app-version"`) on the AuthGate logo card
      (works pre-login) and the sidebar user footer; empty on fetch failure (no broken layout).

## 3.2 Capabilities
- [x] Tool/function calling pass-through + built-in web search tool (opt-in per chat
      `tools_enabled`; `web_search` runs via `HALO_WEB_SEARCH_URL`; ≤4 tool rounds,
      `role=tool` rows replayed). Tool chips verified at SSE/DB level; in-browser chip render NOT re-tested (scope-out).
- [x] RAG v1: upload md/txt → OpenAI-compatible `/embeddings` (any provider incl. Ollama
      `/v1`) → `kb_documents` chunks with JSON-blob vectors + Rust cosine scan (no
      sqlite-vec — cross-compile safety; fine at personal scale); per-chat
      `kb_enabled` toggle, injected as a second system message, citation chips from the
      `sources` SSE event + `messages.sources` (e2e: per-topic ranking, chunked uploads,
      kb-off/below-threshold silence, chunk+source delete, regenerate, Anthropic kind).
      PDF text extraction scoped out (client reads .txt/.md); browser KB flow NOT re-tested beyond login/shell (scope-out); API-level verified in 2.x.
- [x] Voice: TTS read-aloud (provider-agnostic) + STT input (whisper endpoint).
- [x] Native Anthropic + Gemini providers behind `AnyClient` kind dispatch (ADR-002 revisit;
      adapters translate request/response + SSE to OpenAI shapes; provider form has API-type
      select; e2e: text, tool loop, vision, title, model list per kind vs mock). Browser-side
      SettingsDrawer open/close now covered by Playwright smoke.

## 3.3 Ops & quality
- [x] CI: cargo fmt/clippy/test + web typecheck/build (GitHub Actions, `.github/workflows/ci.yml`).
- [x] Docker multi-stage → single image (`Dockerfile`: node web build → rust release with
      rust-embed → debian:trixie-slim runtime; verified: setup→provider→chat vs host mock via
      pasta, /api/health, embedded UI, `(healthy)`; HEALTHCHECK requires `--format docker`).
- [x] Multi-arch linux release binaries (x86_64/aarch64/armv7, rustls + bundled sqlite,
      `scripts/build-multiarch.sh` via podman, SHA256SUMS under dist/release/; file-verified,
      x86_64 release e2e green; glibc ≥ 2.38 hosts; no qemu → arm run-smoke documented skip).
- [x] GitHub release workflow (`.github/workflows/release.yml`, on v* tag / manual dispatch):
      6-target matrix — linux amd64/arm64/armv7, macOS arm64/amd64, windows amd64 — embedding
      the web build like memos-rs; native + smoke verified locally, runs on push to GitHub.
- [x] Benchmarks: first-token latency & tokens/s shown in header while streaming (browser-verified).
- [x] Playwright e2e smoke (chat round-trip against mock SSE server): `npm run test:e2e
      --prefix web` — login → send → streamed reply + perf badge; caught+fixed a P0
      (`Chat.tags` serialized as string → blank page after login).

## Exit criteria
HaloTalk feels faster and sharper than Open WebUI on the same hardware; extensibility
points (providers, tools, RAG) stable enough for community contributions.
