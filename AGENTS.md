# AGENTS.md — HaloTalk autonomous development playbook

You are working unattended to drive HaloTalk through its remaining phases.
`docs/development_phases/status.md` is the single source of truth for progress —
read it first, keep it updated at every milestone (it is reboot-proof continuity).

## Ground rules (from the user)
- Complete the phases in `docs/development_phases/04-phase2-core.md` and
  `05-phase3-polish.md`; scope-outs must be recorded honestly in status.md, never silently dropped.
- DOUBLE-CHECK every change: unit tests + build + endpoint-level e2e against the mock provider
  before ticking anything. Prefer verification over ambition; never ship unverified code paths.
- Never `git commit` unless explicitly asked. No gratuitous comments (match existing density).
- Keep `docs/development_phases/*.md` (API tables, phase checkboxes, architecture notes) in sync
  inside the same change as the code.
- If unsure between two reasonable designs, pick the one the docs already hint at; note the
  decision in status.md "Ledger update log".

## Repo map
- `crates/halo-core` — SQLite store (`store.rs`), schema + `init_pool` migrations (`db.rs`).
  Schema changes: additive only; `db.rs` runs statements split on `;`; ALTER ADD COLUMN must
  tolerate "duplicate column" (migrations runner). No in-statement semicolons.
- `crates/halo-providers` — `AnyClient` facade: `stream()`/`complete()`/`list_models(kind, …)`
  dispatch to `openai` (lenient SSE), `anthropic`, or `gemini` client by provider `kind`
  (adapters emit OpenAI-shaped chunks).
- `crates/halo-server` — single binary `halotalk`: axum routes (`routes.rs`), hub chat event
  streams (`hub.rs`): generation is detached from HTTP, clients subscribe via
  `GET /api/chats/{id}/events`; send/regenerate/edit return 202 (409 while busy), `POST
  .../stop` cancels (partial kept). Frames: `state | user | resync | meta | reasoning |
  token(seq) | tool | sources | title | error | done`; `done` = answer complete, frames may
  follow it (title) until close; run from repo root (relative `data/`, `web/dist`).
- `web/` — React+Vite+TS+Tailwind(v4) src → `web/dist` (served from disk in debug; rust-embed in
  release once 2.4a lands). `rest.ts` = API client, `api.ts` = SSE client, components/.

## Commands
```bash
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace   # from repo root
npm run build --prefix web
# debug server:   setsid ./target/debug/halotalk </dev/null > /tmp/opencode/halotalk.log 2>&1 &  (:8420)
# mock provider:  scripts/mock_llm.py (:9999, run `python3 -u scripts/mock_llm.py`) — OpenAI +
#                 Anthropic (/anthropic/v1) + Gemini (/gemini/v1beta) mocks: stream+non-stream,
#                 images, tool rounds, "SLOW" → slow chunks; also /search, /v1/embeddings
#                 (256-dim FNV), /v1/audio/speech+/transcriptions; keep it in sync when the
#                 request shape changes. Dumps last request bodies to /tmp/opencode/last_req*.json
#                 (non-stream chat only). /tmp/opencode is EPHEMERAL — repo is truth.
# restart servers: kill by PID only (see status.md gotcha). web/dist edits need NO restart.
# e2e suite: workdir web/ + local binary — `./node_modules/.bin/playwright test
#            --config playwright.config.ts --reporter=list`. Running via root npx fails
#            ("did not expect test()" / "No tests found" — module-resolution artifact).
```

## Conventions
- IDs: uuid v7 strings (`uuid` crate, v7 feature). Timestamps: RFC3339 UTC strings.
- API errors: `{ "error": "message" }` with proper status via `ApiError`.
- SSE chat bodies never trust client history — rebuild from DB.
- Frontend: keep bundle lean; new heavy deps (mermaid etc.) dynamic-imported.
- Auth (2.1): cookie session; ALL /api/* except `/api/auth/*` and `/api/health` require a session;
  providers/chats carry `user_id`, cross-user access = 404 (not 403). API keys stored as
  ChaCha20-Poly1305 ciphertext (`xc1:` prefix; legacy fernet `fer1:` rows decrypt empty → re-enter);
  plaintext legacy rows migrated on startup.

## Per-feature loop (follow exactly)
1. Read the phase-doc item + related code paths.
2. Implement (schema → store → routes → api client → UI).
3. Update mock if the request/response shapes changed.
4. Verify per the "Verification gates" section in status.md.
5. Tick phase doc + status.md (+ ledger log line). Move to next item immediately.

## Multi-arch deliverable (3.3c, hard requirement)
`scripts/build-multiarch.sh` (podman-driven): linux x86_64 / aarch64 / armv7 (arm32) release
binaries, web embedded. Needs pure/cross-compilable deps: bundled SQLite and rustls (audit
Cargo.toml features; switch away from system-sqlite/native-tls if needed and re-verify FTS5).
Artifacts + SHA256SUMS under `dist/release/`.
