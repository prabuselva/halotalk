# Risks & Architecture Decisions (ADRs)

## ADR-001 — Frontend: React + Vite, not Rust/WASM (accepted, 2026-10)
Options: React+Vite+Tailwind · Leptos 0.8 · Dioxus 0.7.
**Decision:** React. A polished Open-WebUI-style chat (markdown, code highlight, image
uploads, SSE UI) has off-the-shelf, battle-tested components; Leptos/Dioxus would cost
prototype speed for purity. Backend stays 100 % Rust.
**Risk/mitigation:** two toolchains → mitigated by one `npm run dev` proxy to cargo for the
dev loop, and rust-embed static serving later makes release a single Rust binary. Revisit if
FE complexity grows or the team wants Rust-only.

## ADR-002 — Providers: single OpenAI-compatible client first (accepted)
OpenAI-compatible `/v1/chat/completions` covers OpenRouter, Ollama, LM Studio, vLLM,
OpenAI, plus proxies for Anthropic. One client, runtime-configurable base URL + key =
maximum reach for least code. Native Anthropic/Gemini go behind the same trait in P3.
**Risk:** non-standard deltas (e.g. `reasoning_content` from DeepSeek-ish servers) —
mitigated by lenient parsing (`reasoning` ∪ `reasoning_content` ∪ `reasoning_details`).

## ADR-003 — SSE for streaming, not WebSockets (accepted)
Chat is unidirectional token flow over one request; SSE is HTTP/1.1-friendly, survives
proxies, trivial for browsers (`fetch` + ReadableStream). Uploads stay plain multipart.

## ADR-004 — Prototype stores API keys plaintext in SQLite (accepted, tracked)
Single-user localhost trust boundary. **Must** be replaced in Phase 2 (fernet + `HALO_SECRET`)
and the bind host must default-check when non-loopback. Risk accepted deliberately for speed.

## ADR-005 — Chat-scoped settings row (accepted)
Model params live on the `chats` row (not a separate settings blob) → zero extra state sync;
PATCH /chats/{id} updates them. If presets arrive (P2), presets materialize into the row.

## ADR-006 — Images as data URLs in the request path (accepted for P1)
Sending images as base64 data URLs keeps the chat request atomic; /uploads exists for
persistence & display. **Risk:** request bloat with many images → cap 4 images / 15 MB per
message in v1; switch to fetch-resolve-by-id for vision-capable servers later.

## Top risks (tracked across phases)
| Risk | Impact | Mitigation |
|---|---|---|
| Upstream format drift (SSE quirks) | broken streaming | lenient parser + fixture-based tests per provider |
| Key leakage on shared deploys | security | ADR-004 expiry; auth gate in P2 before any LAN exposure |
| Scope creep toward Open WebUI parity | never ship | phase checklists are the contract; park items in P3 |
| SQLite write contention with streams | data loss | WAL mode; message finalize on done/error is idempotent |
