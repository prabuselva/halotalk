# HaloTalk — Live Status Ledger

> Permanent tracking file. Survives reboots. Update at every milestone.
> Started: 2026-10-31. See `AGENTS.md` (repo root) for the working playbook.

## Resume / restart commands (after reboot)
```bash
cd /home/praburaja/projects/llm_playground/test_llm_basic_code/halotalk
# mock provider (e2e):
cd /tmp/opencode && setsid python3 mock_llm.py </dev/null > mock.log 2>&1 &
# server (MUST run from repo root — relative data/, web/dist):
cd /home/praburaja/projects/llm_playground/test_llm_basic_code/halotalk
setsid ./target/debug/halotalk </dev/null > /tmp/opencode/halotalk.log 2>&1 &
# ports: mock :9999 (DB provider "Mock" base_url http://127.0.0.1:9999/v1), server :8420
# GOTCHA: kill only via explicit PID or bracket-pattern pkill (e.g. 'hal[o]talk');
#         a plain -f pattern matches the killing shell itself.
```

## Environment probe (2026-10-31)
- Network to crates.io + npm: OK. cargo 1.98.1, node v26.10.0.
- podman 6.1.3 works (containers run). Host has NO cross gccs; rustup targets present:
  x86_64-unknown-linux-gnu, armv7-unknown-linux-gnueabihf (no aarch64 target yet).
- Multi-arch plan: podman container (rust:*-bookworm) with apt cross gccs:
  gcc-aarch64-linux-gnu + gcc-arm-linux-gnueabihf; web built in node:22 container.

## Master TODO (full list, persistent)

### Done
- [x] Phase 1 prototype (backend crates, SSE chat, uploads, React UI, docs) — verified e2e
- [x] 2.2 Stop-generation, Regenerate, Edit-resend, FTS5 search — verified e2e
- [x] 2.2 LLM chat title auto-generation (parallel SSE `title` event) — verified e2e
- [x] 2.4 Provider connectivity test button (`POST /api/providers/{id}/test`) — verified e2e
- [x] status.md + AGENTS.md bootstrapped

### 2.1 Auth & multi-user — DONE 2026-10-31
- [x] 2.1a users + sessions tables; argon2 hashing; cookie session middleware; /api/auth/* (setup, signup, login, logout, me, settings) — e2e verified
- [x] 2.1b first-run setup screen; open-signup toggle; AuthGate UI; logout; admin flag — `web/src/components/AuthGate.tsx`, sidebar user footer; npm build clean
- [x] 2.1c ownership: user_id on chats/providers; row filtering + 404 cross-user; backfill legacy rows — e2e: bob GET/PATCH/DELETE admin chat → 404; legacy chats visible to admin
- [x] 2.1d API keys encrypted at rest (ChaCha20-Poly1305 `xc1:` since 3.3b, originally fernet; HALO_SECRET or generated data/secret.key 0600); plaintext→cipher migration (startup `migrate_plaintext_keys`)
  - Design: cookie `halo_session` = raw 64-hex token; DB stores sha256 hex; 30-day sliding expiry;
    middleware resolves session on EVERY path (optional insert of AuthUser ext) and enforces 401
    only on `/api/*` minus `/api/auth/*` + `/api/health`. open_signup default off (admin toggle).
    Decrypt failure on api keys → empty string (re-enter key; no crash/leak).

### 2.2 remaining
- [x] 2.2d Branching — DONE 2026-10-31: regenerate keeps siblings (`truncate_after_keeping_branch`),
      /activate endpoint (404 cross-user), ‹i/N› pager UI (MessageList buildView), provider context
      verified active-only via mock request dump; edit still truncates (discards siblings). e2e green.
- [x] 2.2e Pin / archive / tags — DONE 2026-10-31: store.search `#tag` tokens (tag-only scan +
      AND-filter on FTS hits, archived excluded, `split_search_tokens` unit-tested); sidebar
      Pinned section, pin/tag/archive hover actions, collapsible Archived section (unarchive/delete),
      `#tag` title filter. e2e: tag/case/mixed/wrong-tag, archive list exclusion + ?archived=1,
      pinned-first order, bob isolation, 401 unauth. Phase 2.2 now fully complete.

### 2.3 Prompts & models UX — DONE 2026-10-31
- [x] 2.3a prompts table + CRUD + `/`-autocomplete in Composer; per-user default system prompt (used when chat has none)
- [x] 2.3b model registry capabilities (vision/reasoning/ctx), heuristic auto-detect on provider list, badges + toggles, vision gating in Composer
- [x] 2.3c presets table + CRUD + apply-from-settings UI

### 2.4 Platform
- [x] 2.4b config export/import JSON (`GET /api/config/export[?include_secrets=1]`, `POST /api/config/import`) — DONE 2026-10-31
- [x] 2.4c global DefaultBodyLimit(10MB) + per-IP rate limiter (600/60s) + tracing + TraceLayer
- [x] 2.4a rust-embed `web/dist` in release builds; debug keeps disk/HMR path; verified single-binary e2e

### 3.1 Experience
- [x] 3.1a command palette Ctrl+K (new chat, go to chat, toggle theme, open settings) — DONE 2026-10-31
- [x] 3.1b KaTeX + mermaid (dynamic import ≈ code-split) + table styling; rebuild bundle size check — DONE 2026-10-31
- [x] 3.1c export thread: Markdown + JSON (browser download from messages state) — DONE 2026-10-31
- [x] 3.1d PWA: vite-plugin-pwa app-shell + NetworkFirst chat caching (build-verified) — DONE 2026-10-31
- [x] 3.1e scroll pinning review; bundle manualChunks — DONE 2026-10-31 (entry 70 kB; initial-load < 575 kB; mermaid/elk lazy)
- NOTE virtualized long threads: NOT DONE (scope-out — threads performant without it; revisit
  with react-window only if profiling shows need)
- Export PDF: SKIPPED (needs headless print CSS; parked)

### 3.2 Capabilities
- [x] 3.2a tool/function-calling pass-through (tools in request, stream `tool_call` events, exec loop, `tool` role messages) + built-in web_search tool (env-configurable endpoint, off by default)
      — chats.tools_enabled toggle (SettingsDrawer); tools.rs web_search POSTs `{"query"}` to
      HALO_WEB_SEARCH_URL, errors → JSON tool-result so model recovers; ≤4 rounds in spawned task;
      SSE `tool` events `phase:call|result`; role=tool rows persisted (group JSON on each row,
      tool_call_id, not FTS-indexed), replayed on regenerate as synthesized assistant(tool_calls)+tool;
      e2e vs mock /search: arg fragments merged, result preview, "used web search" echo in answer,
      GET chat returns tool rows, regenerate rebuilds pair, tools-off flow unchanged. Note:
      regenerate/edit prune drops tool rows of the re-generated turn (by design — tool re-runs if asked).
- [x] 3.2b native Anthropic + Gemini adapters (AnyClient kind dispatch; Anthropic `/messages` +
      Gemini `generateContent` translate to/from OpenAI shapes incl. tools/vision/thinking;
      mock fixtures for all 3 families; e2e per kind: text, tool loop w/ id linkage, image,
      title (non-stream), model list (Gemini ids normalized); SettingsDrawer API-type select)
- [x] 3.2c RAG v1 lite: .txt/.md upload → embeddings (any provider's OpenAI-compatible
      /embeddings, incl. Ollama) → JSON-blob vectors in SQLite + cosine top-k in Rust
      (no sqlite-vec = cross-compile-safe) → per-chat `kb_enabled` toggle, `sources` SSE
      event + persisted citation chips. PDF text extraction NOT done (honest scope-out:
      client reads .txt/.md; PDF needs a parser dep — revisit later). Browser check → 3.3e.
- [x] 3.2d voice: TTS + STT passthrough endpoints (server `/api/audio/{speech,transcribe}` →
      upstream OpenAI-compatible `/audio/*`) + mic (MediaRecorder) / Speak UI, config-gated
      via tts_provider_id/tts_model + stt_provider_id/stt_model. Browser check → 3.3e.
- If time/verification forbids any of 3.2c/3.2d: mark NOT DONE honestly here rather than ship unverified code.

### 3.3 Ops & quality
- [x] 3.3a GitHub Actions CI (.github/workflows/ci.yml): rust job (fmt --check, clippy -D
      warnings, test) + web job (npm ci + build; tsc --noEmit serves as the linter — no
      eslint config in repo). Every step executed locally to green; GH-side run pending
      (directory is not a git repo with remote yet — workflow is inert until pushed).
- [x] 3.3b Docker multi-stage Dockerfile (node→rust→debian:trixie-slim runtime, rust-embed web,
      VOLUME /data, EXPOSE 8420) + container verification: fresh volume → first-run setup →
      provider (key encrypted at rest) → chat SSE "Hello from mock!" with the host mock reachable
      via --network=pasta (mock now honors HALO_MOCK_HOST, default stays 127.0.0.1); / serves
      embedded assets; podman reports "(healthy)". Gotchas recorded: HEALTHCHECK needs
      `--format docker` (OCI silently drops it); runtime base MUST match rust:1-slim's Debian
      (bookworm runtime → GLIBC_2.38 error); crypto fernet→ChaCha20-Poly1305 `xc1:` to drop
      openssl-sys (legacy `fer1:` rows decrypt empty → key re-entry, by design)
 - [x] 3.3c multi-arch linux binaries x86_64 / aarch64 / armv7: scripts/build-multiarch.sh via podman
       (cross image: rust:1-slim + gcc/binutils-aarch64 + gcc-arm-linux-gnueabihf +
       libc6-dev-{arm64,armhf}-cross [needed by aws-lc-sys C build] + rustup targets); bundled
       sqlite + rustls already satisfied (no openssl-sys since ChaCha swap). dist/release/ =
       halotalk-linux-{amd64,arm64,armv7} + SHA256SUMS, `file`-verified (x86-64 PIE / aarch64 /
       ARM EABI5, glibc-dynamic — built against trixie's libc, so host baseline is glibc ≥ 2.38
       i.e. Debian 13 / Ubuntu 24.04+, same tradeoff as the container runtime), x86_64 release e2e:
       health {"ok":true} + embedded UI ok. Arm run-smoke SKIPPED honestly: no qemu binfmt
       registered and sudo needs a password; determinism check (rm -rf armv7 target → rebuild
       reproduced byte-identical BuildID) is the substitute guarantee.
 - [x] 3.3d tokens/s + first-token latency badge in header while streaming
       (perfBadge in App measured in streamInto, header shows "first X.XXs · N tok/s"; FIRST real
       browser verification via the 3.3e test — badge assertion green)
 - [x] 3.3e Playwright smoke vs mock: `npm run test:e2e --prefix web` (needs dev server :8420 +
       mock :9999; E2E_USER/E2E_PASS, defaults bob). smoke.spec.ts = login → chat seeded via API →
       send in UI → perf badge visible → streamed "Hello from mock!". Message must contain "SLOW"
       so the mock's 1.2 s/chunk pacing keeps the badge (unmounts at stream end) assertable.
       The first-ever run of this test FOUND + we fixed a P0: `GET /api/chats` serialized
       `tags` as a JSON *string* while Sidebar does `c.tags.map(...)` → blank page after
       login (models.rs now serialize_with → real array). chromium install:
       `npx --prefix web playwright install chromium` (one-time, system deps already present)

### Wrap-up
- [x] Final regression matrix — 2026-11-01: fmt/clippy -D/5 test suites/npm build green; API
      sweep script `scripts/regression_sweep.py` ALL GREEN (auth+me, chat create/pin/tags/archive
       /#tag-search/SSE round-trip (provider+model were bound at CREATION in the sweep; PATCH
       does update them too — the earlier "PATCH ignores them" note was a wrong curl method),
       prompts+presets+registry, config export→import, unauth 401); prompts body key is
      `body`, presets body needs `params` object, creates return 201, updates/deletes 204 — sweep
      encodes all this. Playwright suite 3/3 green (round-trip+badge, Ctrl+K palette→new chat,
      settings drawer open/close). Rate limit: 600/min/IP limiter live via state.limiter + unit
      test (NOT stress-tested live — would lock bob out for a window). Release single-binary +
      multi-arch artifacts + SHA256SUMS verified (3.3c). README fully rewritten (auth first-run,
      provider kinds, 8 env vars incl. HALO_SECRET/HALO_WEB_DIST/HALO_WEB_SEARCH_URL, Docker,
      multi-arch, dev/e2e commands). All phase-doc checkboxes now closed or honestly scoped.

## Verification gates (definition of done per step)
1. `cargo test --workspace` green (from repo root).
2. `npm run build` clean (web/) after frontend changes.
3. E2E via mock (:9999) with curl/python client for every new endpoint: happy path + one failure path.
4. Playwright suite green (run from web/ with ./node_modules/.bin/playwright — see AGENTS.md
   Commands) and `python3 scripts/regression_sweep.py` ALL GREEN for chat-pipeline changes.
5. Phase doc checkbox + this ledger updated in the same change.

## Incident notes
- 2026-11-01: recurrence of the pkill self-match — a `pgrep -f` pattern still matches the
  wrapper bash when its text appears in the same compound command line (kill hit own shell; the
  hang masqueraded as a cargo stall). Rule: resolve PIDs in one command, kill in a SEPARATE
  command, or use `pgrep -x`.
- 2026-10-31: `pkill -f target/debug/halotalk` killed its own shell → restart lost. Use PIDs.
- 2026-10-31: relaunched server with setsid from the WRONG cwd (workspace parent) → it opened a
  throwaway empty `data/halotalk.db`; logins "failed" on the real creds and signup said
  "run setup first". Always restart with workdir=halotalk repo root AND verify
  `readlink /proc/$(pgrep -f 'debug/hal[o]talk')/cwd` == repo root before trusting e2e results.
- 2026-10-31: mock single-threaded → title event may land AFTER `done`; client reads to close
  and clears "generating" on `done` (onDone), so UX unaffected. Docs reflect this.
- 2026-10-31: axum `middleware::from_fn_with_state` rejects handlers whose future comes from an
  inferred `async {}`-block chain (Service bound unsatisfied, cryptic E0277) — use plain
  if-let/let-chains in `require_auth`. Also `/api/auth/*` must NOT skip the middleware (me needs
  the extension), it is only exempt from the 401 enforcement.
- 2026-10-31: shell background jobs die when a compound command hits its timeout — run server
  restarts as short standalone commands; verify with a curl probe rather than trusting `&` output.
- 2026-11-01: containerized cargo cross-builds share the repo `target/` via mount; the armv7 leg
  reported "Finished in 0.16s" (all fresh) right after a source edit that HAD already been
  compiled for armv7 in the previous run — fingerprint mtimes from the container are not beyond
  doubt. Trust `file` BuildIDs + a from-scratch rebuild determinism check over "it said fresh".

## Ledger update log
- 2026-10-31: ledger created; Phase 1 + 2.2 (stop/regenerate/edit/search/titles) + test button done;
  env probed; starting 2.1a auth backend.
- 2026-10-31: 2.1 complete (a-d): auth backend (users/sessions/argon2/fernet crypto.rs v2 schema),
  all handlers user-scoped (404 cross-user), AuthGate + signup + admin open-signup toggle +
  logout in UI, e2e green (setup/login/signup gating, isolation, logout→401, SSE chat +
  regenerate vs mock, static index served). fmt+clippy zero, 10 unit tests pass, npm build clean.
  Next: 2.2d branching (pager UI) per plan. (Note the test DB now holds users admin/password1,
  bob/bobpassword — dev data under data/, harmless.)
- 2026-10-31: 2.2d done — regenerate-as-branch (store.truncate_after_keeping_branch), activate
  endpoint + ‹i/N› pager in MessageList, regenerate/reload flow in App; e2e: 3 siblings kept,
  activated first sibling excluded later actives correctly, provider payload active-only
  (mock now dumps last request to /tmp/opencode/last_req.json), edit clears siblings.
  clippy 0 / tests 5 suites ok / npm build clean. Next: 2.2e pin/archive/tags.
- 2026-10-31: 2.2e done — #tag search (tag-only chat scan + FTS AND-filter; split_search_tokens
  unit test), sidebar Pinned section + pin/tag/archive hover actions + Archived section
  (lazy ?archived=1 fetch, unarchive/delete), #tag title filter, list?archived=1 supported.
  e2e matrix green incl. case-folding, mixed term+tag, archived exclusion, cross-user 0 hits,
  401 unauth, pinned-first order. clippy 0 / 5 test suites ok / npm build clean. Phase 2.2 whole
  item complete. Next: 2.3a prompts CRUD + `/` autocomplete + per-user default system prompt.
- 2026-10-31: 2.3 complete (a-c). Backend: /api/prompts CRUD (404 cross-user, 400 invalid),
  /api/presets CRUD with server-side param allowlist (model, temperature, top_p, max_tokens,
  system_prompt, reasoning_effort), /api/registry GET/PUT (provider ownership enforced),
  default_system_prompt user setting applied in create_chat when chat has none.
  Auto-detect: list_models now parses advertised fields (vision, architecture.input_modalities,
  reasoning, supported_parameters, context_length) and seeds registry ONLY when no row exists,
  so manual edits always win (detect_caps unit-tested x4). Verified e2e vs mock: mock-vision
  seeded {ctx:128000, vision:true}, plain mock-gpt not seeded, manual PUT survived a re-list.
  Frontend: Composer `/`-prompt autocomplete (arrows/Tab/Esc, no send while menu open) + vision
  gating (attach hidden + blocked when registry says no vision); SettingsDrawer gained Presets
  (save-current/apply/delete), Prompts (CRUD w/ inline editor), Account (default system prompt),
  and model capability chips + per-model Vision/Reasoning/Context editor (PUT registry).
  Incidental: discovered server restart in wrong cwd silently opened an empty DB — incident note
  + verify step added; throwaway outer data/ dir removed. clippy 0 / 5 test suites ok
  (halo-server now 6) / npm build clean (bundle 1.35MB — manualChunks parked at 3.1e).
  NOTE dev state: app_settings open_signup=1 (turned on during e2e, left on); dev DB has users
  admin/password1 + bob/bobpassword only (failed signup attempts hit the throwaway DB, nothing
  extra landed); registry rows: gpt-test-v (seed) + mock-vision (manual edit ctx=999 left from
  verification). Next: 2.4b config export/import JSON.
- 2026-10-31: 2.4b done — /api/config/export (providers/prompts/presets/registry-by-provider-name/
  settings; api_key ONLY with include_secrets=1) + /api/config/import (skips same name+URL
  providers, dedupes identical prompts/presets, remaps registry rows by provider name into newly
  created providers, merges settings; foreign json → 400). Drawer > Account gained Export JSON
  (blob download + optional-keys checkbox) and Import JSON (file picker, refreshes local data).
  e2e: export both modes, bob import {providers:1,prompts:1,presets:1,registry:2,settings:1},
  idempotent re-import, registry landed on bob's NEW provider id, 400 foreign file.
  clippy 0 / suites ok / npm build clean. Dev data: bob now also has Mock provider + exp1 +
  exp-preset (harmless). Next: 2.4c middleware hardening.
- 2026-10-31: 2.4c done — middleware hardening. Router layers (inbound order): RateLimiter →
  TraceLayer → DefaultBodyLimit(10MB) → cors → auth. Custom dependency-free RateLimiter
  (routes.rs, fixed-window 600 req/60s per IP, Arc<Mutex<HashMap>>, 4194-entry sweep, 429
  {error:"rate limit exceeded, retry shortly"}); needs ConnectInfo<SocketAddr> (serve switched
  to into_make_service_with_connect_info). NOTE: in-memory, resets on restart; put a real
  limiter at the reverse proxy for multi-instance deployments. EnvFilter default FIXED to
  "halotalk=info,tower_http=debug" (old "halo_server=…" never matched — crate target is
  halotalk; tower_http traces log at DEBUG). Per-route uploads keep DefaultBodyLimit 20MB
  (inner-replace semantics). Unit test window_allows_up_to_max_then_blocks. e2e: burst 700 →
  first 429 exactly at #600 then 101×429, other IP unaffected; 12MB JSON POST /api/prompts →
  413; 12MB PNG POST /api/uploads → 200 (override proven); structured request/response log
  lines (method/uri/latency/status) confirmed. clippy 0, all suites ok. Next: 2.4a rust-embed
  single binary (release), then 3.1 web features.
- 2026-10-31: 2.4a done — rust-embed single binary. New workspace deps rust-embed 8 + mime_guess.
  crates/halo-server/build.rs creates web/dist/index.html placeholder on clean checkouts so
  cargo builds before npm. routes.rs: EmbeddedWeb Embed struct + embedded_web fallback handler
  (disk dist still wins when present → debug live-reload unchanged; placeholder/misses →
  spa_hint). Cache: index.html no-store, hashed assets immutable + sniffed content-type.
  e2e on RELEASE binary copied to empty /tmp dir (:8430, fresh data/): embedded index served,
  SPA route /chats 200, JS/CSS 200 with correct types + cache headers, embedded JS byte-
  identical (sha256) to web/dist, setup→login→/api/chats on fresh DB OK. Release bin 18MB.
  clippy 0, suites ok. PHASE 2 COMPLETE. Next: 3.1a command palette.
- 2026-10-31: 3.1a+3.1b implemented. 3.1a CommandPalette.tsx (Ctrl/Cmd+K global toggle):
  fuzzy-substring over Actions (new chat, theme toggle, settings) / Chats / Models (registry per
  provider, lazy-fetched on open); ↑↓/↵/esc, scroll-into-view, model pick = updateChat on active
  chat or setPendingDraft for new chat. 3.1b: deps remark-math rehype-katex katex mermaid (lazy:
  code-split via dynamic import behind maybeMath() detection incl. code-stripping regex; mermaid
  only for ```mermaid fences, theme-aware re-render, error → fallback code block; strict security
  level). CSS: zebra table rows, katex-display overflow-x, .mermaid-box. vite-env.d.ts added.
  Build clean; entry 1,356 kB (grew ~7 kB); mermaid.core/cytoscape/elk/katex now separate lazy
  chunks. NOTE: a11y/keyboard interaction e2e deferred to 3.3e Playwright (no browser harness in
  dev env yet); bundle strings + served-asset checks pass. Next: 3.1c export MD/JSON.
- 2026-10-31: 3.1c done — web/src/export.ts (downloadChat md/json, active-path-only messages,
  slug filenames); MD + JSON pill buttons in header (visible sm+ when a chat has messages).
  Build clean (entry +1.5 kB). Interaction e2e again deferred to 3.3e harness. Next: 3.1d PWA.
- 2026-10-31: 3.1d+3.1e done. PWA: vite-plugin-pwa (autoUpdate SW, manifest w/ PIL-generated
  halo icons 192/512 on dark bg, navigateFallback index.html w/ /api+/uploads denylist,
  NetworkFirst same-origin /api GETs excl. /chat SSE, 4s timeout, 1-day TTL) → sw.js +
  manifest.webmanifest served 200 w/ correct content-types (offline behaviour to be exercised
  by 3.3e). 3.1e: hljs full → lib/common; manualChunks react|markdown; build now entry 70 kB,
  react 135, markdown 338, css 29 (initial ≈ 570 kB raw / 177 kB gz); katex/mermaid/cytoscape/
  elk all lazy (elk 1.5 MB gz 453 kB loads only for elk-layout diagrams). Scroll pinning:
  existing MessageList auto-scroll kept (reviewed, no regression). rust gates re-run green.
  Next: 3.2a tools pass-through.
- 2026-11-01: 3.2a done — schema: chats.tools_enabled + messages.tool_calls/tool_call_id
  (3 additive ALTERs; models Chat/Message plain fields). store: update_chat accepts
  tools_enabled; insert_tool_message() (role=tool, stores group JSON on every row, no FTS).
  providers/openai: lenient delta.tool_calls fragment capture → Chunk.tool_calls; OutMessage
  gains tool_call_id/tool_calls (skip_none) + text()/with_tool_call_id()/with_tool_calls();
  ChatCompletionRequest(+Default) tools/tool_choice passthrough; 4 unit tests. NEW
  halo-server/tools.rs: web_search definition + executor (POST {"query"} →
  HALO_WEB_SEARCH_URL, 20s, 6000-char truncate; every failure path → {"error":…} tool-result
  so the model recovers; 4 unit tests). routes: history rebuild replays tool rows as
  assistant(content:"",tool_calls)+tool pair; completion_stream runs ≤4-round tool loop in
  the spawned task (merge fragments by index → execute → persist → SSE tool call/result
  frames → re-stream; error/disconnect/empty-calls break). Frontend: Role "tool", synthetic
  tool chips during stream (App onTool) + centered pill for saved tool rows (MessageList),
  SettingsDrawer web-search toggle. mock_llm.py: /search endpoint + split-fragment tool_calls
  sim + tool-aware replies; rebuilt debug server WITH HALO_WEB_SEARCH_URL. E2E (bob, mock):
  tool fragments merged to valid args, real search executed, final answer echoes search JSON,
  DB role=tool row + GET returns it, regenerate rebuilds assistant+tool pair from DB and
  answers without redundant round, tools-off chat unchanged, workspace gates + npm build green.
  Decisions: tool_choice left to provider default; no complete_message() helper (unused
  speculative API avoided); regenerate prune intentionally discards the re-generated turn's
  tool rows (model re-runs search if needed). KNOWN PRE-EXISTING (not fixed): a send that
  400s during pre-stream validation still persists its user row → orphan duplicate user
   messages in that chat. Next: 3.2b Anthropic/Gemini adapters.
- 2026-11-01: 3.2b done — NEW halo-providers/anthropic.rs: AnthropicClient ({base}/messages,
  x-api-key + anthropic-version 2023-06-01); to_body hoists system → top-level, assistant
  tool_calls → tool_use blocks (args parsed), role=tool → user tool_result (tool_use_id kept),
  consecutive same-role messages folded (alternating-role rule), tools → input_schema,
  tool_choice required→{"type":"any"}, max_tokens default 8192; SSE → OpenAI-shaped chunks
  (content_block_start tool_use → empty-args fragment, input_json_delta → args fragments,
  thinking_delta → reasoning, stop_reason tool_use→tool_calls mapping). NEW gemini.rs:
  GeminiClient ({base}/models/{m}:streamGenerateContent?alt=sse / :generateContent, x-goog
  -api-key); systemInstruction, role user/model, functionCall parts (ids synthesized call_{n}
  — Gemini omits ids), role=tool → functionResponse (name resolved by scanning assistant
  tool_calls), generationConfig/functionDeclarations/toolConfig ANY; list_models normalizes
  {"name":"models/x"} → adds id. lib.rs: AnyClient facade {openai,anthropic,gemini},
  list_models/complete/stream(kind,…), unknown kind → OpenAI fallback; openai.rs upstream_error
  → pub(crate). routes.rs: AppState.provider = AnyClient; all 4 call sites pass provider.kind;
  title task uses non-stream complete per kind. web: SettingsDrawer provider form API-type
  select (kind) + kind badge on non-openai rows. mock_llm.py: anthropic + gemini fixture
  endpoints (thinking block, split input_json_delta, functionCall round, image/tool_result
  echoes, non-stream title detect). E2E (bob): provider create/list kind persisted; models
  lists ok (gemini id "mock-gemini" from models/x); provider test ok both; anthropic: reasoning
  + "Hello from mock-anthropic!" tokens, tool loop (tool_mock1 call→result→echo round; verified
  tool_use↔tool_result id linkage + alternating roles in captured request), image → anthropic
  image block echo, title "Mock Title: …"; gemini: same matrix with synthesized call_0 +
  functionResponse name "web_search"; openai regression send green; workspace gates zero-
  warning + npm build green; debug server + 3-family mock restarted. Decisions: adapters emit
  OpenAI Chunk shape so routes merge loop unchanged; per-message role=tool DB shape stays
  family-agnostic. Next: 3.2c RAG-lite.
- 2026-11-01: 3.2c done — RAG v1 lite. halo-core NEW rag.rs (chunk_text: paragraph→word-boundary
  slices ≤1200 chars, 3 unit tests; cosine returns 0 on length mismatch → safe across embed-model
  changes). Schema: chats.kb_enabled, messages.sources, kb_documents(id,user_id,source,title,text,
  vector JSON NULL-until-embedded,embed_model,created_at) via CREATE-IF-NOT-EXISTS + additive
  ALTERs. store: insert_kb_docs/list_kb/kb_search_pool/delete_kb_doc/delete_kb_source/
  set_message_sources; update_chat kb_enabled; create_chat now persists tools_enabled AND
  kb_enabled at creation (was PATCH-only). providers: OpenAIClient::embed (+ AnyClient::embed
  always via OpenAI path — works vs Ollama /v1). routes: /api/kb GET/POST {source,text}+DELETE
  ?source=, /api/kb/{id} DELETE; embed_provider_for (user_settings embed_provider_id/embed_model);
  kb_retrieve (embed query, brute-force cosine, score>0.15, top-4) → completion_stream injects a
  SECOND system message ("Knowledge base excerpts — … cite by title", adapters hoist/merge system
  for Anthropic/Gemini) + persists sources + SSE `sources` frame after meta; message_out exposes
  sources. web: api.ts onSources + "sources" event; MessageList citation chips under replies;
  SettingsDrawer per-chat KB toggle + Knowledge base section (embed provider/model save, .txt/.md
  file upload read client-side, docs grouped by source w/ delete, not-indexed badge). mock:
  /v1/embeddings FNV bag-of-words unit vectors + last_embed.json. E2E (bob): uploads embedded;
  5-file-style long doc → 3 chunks "solar.txt · 1..3"; per-topic ranking quantum vs banana
  (each query cites only its doc); kb-off + below-threshold → no sources frame; DTO sources
  persisted; DELETE chunk stops citations, DELETE ?source= removes all; regenerate re-cites;
  Anthropic-kind chat cites + merged top-level system verified in captured request; empty text
  → 400; embeddings unconfigured → stored embedded:false + "not indexed" badge path. FIXES along
  the way: create_chat kb_enabled (was silently dropped) and the KNOWN PRE-EXISTING orphan bug
  noted in 3.2a ledger — send now validates provider/model (resolve_provider/require_model
  helpers) BEFORE persisting the user turn; verified 400 leaves 0 messages. Mock embed dims
  64→256 (64 caused bag-of-words hash collisions citing unrelated docs — mock artifact, real
  embedders unaffected). Gates: fmt/clippy zero-warn, tests 9+8+12, npm build green. Docs synced:
  01-architecture (schema, /api/kb rows, sources SSE, flow 2b, settings keys), 05-phase3 tick,
  AGENTS.md SSE list. Next: 3.2d voice passthrough.
- 2026-11-01: 3.2d done — voice. SERVER routes.rs: audio_provider_for(user_settings provider_key+
  model_key, 400 "…not configured…") + POST /api/audio/speech {text ≤4000} → upstream
  {base}/audio/speech {model,input,response_format:"mp3"} (bearer iff api_key) forwarding upstream
  Content-Type + cache-control:no-store, BAD_GATEWAY w/ 300-char upstream body on error; POST
  /api/audio/transcribe multipart `file` (DefaultBodyLimit 25MB on that route) → whisper-style
  {base}/audio/transcriptions → {text}. Settings keys: tts_provider_id/tts_model +
  stt_provider_id/stt_model (generic user_settings, no schema change; family-agnostic as specced).
  /tmp/opencode was WIPED mid-feature (mock, cookies, logs — DB is repo-local, survived): mock
  RELOCATED into repo at scripts/mock_llm.py (3 families + /search + embeddings + NEW
  /v1/audio/speech silent-WAV + /v1/audio/transcriptions multipart) — keep it there; AGENTS.md
  updated. Two mock bugs fixed during rewrite: module-level end_headers() bypassed
  _headers_buffer (responses arrived headerless → curl "HTTP/0.9"; use self.end_headers()), and
  log_message signature must be (self, fmt, *args). FRONTEND: rest.speech(text)→Blob +
  rest.transcribe(blob)→string (shared errorMessage()); Icons +IconMic/+IconSpeaker; MessageList
  "Speak/Stop" per assistant reply (one Audio at a time, blob URL revoke on end, toast fail) —
  needs new ttsReady prop + onToast; Composer mic MediaRecorder → textarea append (sttReady prop,
  pulsing red stop state, getUserMedia denial toast); SettingsDrawer "Voice (TTS/STT)" 2×2
  provider/model grid + save; App voice state {tts,stt} loaded from userSettings per user +
  refreshed on drawer close. E2E (bob + fresh nil user, curl against server): settings PUT/GET
  all four keys incl. preserved embed config (PUT merges); speech 200 audio/wav 6444 B;
  transcribe t.wav → "mock transcript of t.wav (6444 bytes)"; {} text → 400 "text is required";
  no file part → 400; unconfigured user → 400 "TTS is not configured" / "STT is not configured".
  UI deferred to browser smoke (3.3e): Speak button, mic flow, voice settings grid.
  NOTE auth path is /api/auth/signup (not /register) — e2e'ed wrong path 405 earlier. Gates:
  fmt/clippy zero-warn, tests 9+8+12, npm build green (tsc). Docs synced: 05-phase3 voice tick,
  01-architecture audio rows + settings keys. Next: 3.3a CI.
- 2026-11-01: 3.3a done — .github/workflows/ci.yml: on push(main)/PR; rust job =
  checkout + dtolnay/rust-toolchain@stable(rustfmt,clippy) + Swatinem/rust-cache +
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`; web job = setup-node@v4 (node 22, npm cache on
  web/package-lock.json) + `npm ci --prefix web` + `npm run build --prefix web`. All four
  commands executed here and green (fmt no-diff, clippy zero-warn under -D, 9+8+12 tests,
  npm ci lock-consistent + full tsc+vite build incl. PWA). No eslint config exists — the
  honest "lint" is tsc --noEmit (in build); adding eslint is a possible 3.3e-era follow-up,
  noted not silently claimed. YAML parses (pyyaml). Actions themselves (rust-cache etc.) can't
  be exercised without a GitHub remote — recorded honestly: workflow inert until repo pushed.
  Dep audit for later steps: reqwest already rustls+webpki-roots, sqlx sqlite-bundled ✓ (3.3c).
  Next: 3.3b Dockerfile.
- 2026-11-01: 3.3d (code half) — PerfBadge: streamInto resets perfRef{startedAt,firstAt,chars};
  first token → first-token ms; tokens ≈ chars/4 → tok/s after 200ms window; header badge
  "first 0.42s · 37 tok/s" while streamingId active, self-clears 2.5 s after done; npm build
  green. Browser confirmation folded into 3.3e.
- 2026-11-01: 3.3b done — Dockerfile (node:22 web build → rust:1-slim `cargo build --release -p
  halo-server` with web/dist copied in for rust-embed → debian:trixie-slim runtime, non-root
  halotalk user, VOLUME /data, ENV HALO_HOST/DB_PATH/UPLOAD_DIR, HEALTHCHECK curl, EXPOSE 8420)
  + .dockerignore. CRYPTO MIGRATION (was blocked by openssl-sys build failure): fernet →
  chacha20poly1305 0.10 + getrandom (pure Rust); crypto.rs now ChaCha20Poly1305, `xc1:` prefix,
  12-B nonce; legacy `fer1:` + tampered decrypt → "" (re-enter key; dev DB had 4 fer1 provider
  rows, mocks ignore auth); existing base64-32B secret.key reused unchanged. `#![allow
  (deprecated)]` scoped in crypto.rs (aead 0.5 ships generic-array 0.14). cargo tree: fernet and
  openssl-sys GONE. Gates: fmt/clippy -D/test (10+8+12) green; host e2e restarted server: bob
  login, providers list, chat SSE stream fine. CONTAINER VERIFY: `podman build --format docker`
  (OCI default silently DROPS HEALTHCHECK — use docker format), image 118 MB; run with
  --network=pasta (slirp4netns REMOVED in podman 6; plain run gave NO network at all —
  NetworkSettings empty; also `-p` alone insufficient); first-run /api/auth/setup (signup 403 on
  fresh DB by design), provider add (api key stored encrypted), chat SSE → "Hello from mock!"
  via host mock at host.containers.internal (mock script +HALO_MOCK_HOST opt, default 127.0.0.1
  unchanged); GET / serves embedded hashed JS; podman status "(healthy)". Runtime base MUST
  match rust:1-slim's Debian: bookworm → "GLIBC_2.38 not found" → trixie-slim. Note: container
  ignores SIGTERM 10 s (SIGKILL fallback) — graceful-shutdown wiring is a possible later polish,
  not claimed. Docs synced: AGENTS.md + 01-architecture + 04-phase2 crypto wording, 05-phase3
  Docker tick, status 3.3b/3.3d notes. Next: 3.3c scripts/build-multiarch.sh.- 2026-11-01 ~10:00: 3.3c + 3.3d + 3.3e DONE. 3.3c: scripts/build-multiarch.sh (podman cross
  image rust:1-slim + gcc/binutils-aarch64 + gcc-arm-linux-gnueabihf + libc6-dev-{arm64,armhf}
  -cross + cmake/perl/nasm; the cross libc headers are REQUIRED because aws-lc-sys' C build
  aborted on missing sys/types.h — a short-lived "switch to ring provider" experiment was tried,
  found impossible on reqwest 0.13 (rustls-no-provider panics "No provider set", no process-
  provider hook) and FULLY REVERTED). dist/release/: amd64/arm64/armv7 + SHA256SUMS, file-
  verified, x86_64 release e2e (health + embedded UI) green. Glibc note: built on trixie →
  host baseline glibc ≥ 2.38 (Debian 13 / Ubuntu 24.04+). qemu binfmt absent + no sudo → arm
  run smoke skipped honestly; instead did a determinism proof: rm -rf target/armv7 → rebuild
  reproduced byte-identical hash/BuildID, confirming dist armv7 == fresh build of current
  source. 3.3e/3.3d: Playwright 1.64 + chromium installed, web/e2e/smoke.spec.ts green (9.2 s):
  real login → API-seeded chat → UI send ("SLOW" so badge stays assertable; mock 1.2 s/chunk)
  → perf badge "first X.XXs · N tok/s" → streamed "Hello from mock!". FIRST-EVER BROWSER RUN
  IMMEDIATELY CAUGHT P0: GET /api/chats serialized Chat.tags as a JSON string while Sidebar
  does c.tags.map → blank page after login since tags feature landed; fixed server-side
  (models.rs serialize_with → real array; export/import doesn't touch chats → no format
  compat issue). Gates: fmt/clippy -D (1 allow for serde's &String contract)/5-test-suites/
  npm build/e2e all green. 05-phase3 + status ticks done; README refresh + final regression
  matrix = remaining wrap-up.
- 2026-11-01 ~10:15: WRAP-UP DONE = all phases complete. Playwright suite extended 1→3 green
  (+ Ctrl+K palette, settings drawer — the two most testable previously-"deferred to 3.3e"
  surfaces; voice-mic/PWA-offline/tool-chips remain honest scope-outs noted in 05-phase3).
  New scripts/regression_sweep.py (API matrix, exits non-zero on fail) ALL GREEN; found NO new
  bugs — remaining sweep "failures" were my script's wrong expectations (201/204 creates,
  prompt body key `body`, preset `params`, provider/model bind at chat CREATION only, SSE
  deltas split so join before matching, [DONE] is `event: done`). README rewritten to current
  reality (was pre-Phase-2). 05-phase3 stale "deferred to 3.3e" notes replaced with honest
   per-feature status. PHASES 1-3 CLOSED.
 - 2026-11-01 ~11:00: user report "can't select model after configuring provider" (user
   admin123 + llama.cpp on LAN). Model relay itself was FINE (GET /api/providers/{id}/models
   200 + correct list). Two real bugs found + fixed: (1) web App.saveSettings bailed with
   `if (!activeChat) return` → drawer Save on a not-yet-created chat did nothing silently;
   now stores the draft via setPendingDraft ("Will apply to your next chat" toast, consumed
   by send's createChat like the Ctrl+K picker path). (2) POST /api/chats had no onboarding
   fallback → new chat could end up with no provider/model ("no model selected" on send);
   create_chat now inherits the default provider and, if the registry has one, its first
   model. Gates: fmt/clippy -D/5 suites/npm build; e2e: chat create auto-fills provider+model
   after PUT /api/registry (the correct endpoint — it is PUT /api/registry w/ provider_id in
   body, NOT /providers/{id}/registry); Playwright 3/3 + sweep ALL GREEN. GOTCHA recorded:
   clippy/test builds do not refresh target/debug/halotalk as restartable bin — run
   `cargo build -p halo-server` before restarting or you ship a stale binary (hit exactly this).
   Earlier ledger note "provider/model bind at chat CREATION only" corrected — update_chat
   does patch them (wrong curl method in the original probe). Embedded-model question: NOT
   required for chat/model-selection; embeddings only power the KB/RAG path.
 - 2026-11-01 ~11:40: follow-up report — banner STILL shown for admin123, who has 0 chats:
   modelMissing was `!activeChat?.model || !activeChat?.provider_id` → with no active chat it
   is ALWAYS true, disabling send forever regardless of drafts/defaults (also a latent bug for
   the palette pendingDraft path). App.tsx now mirrors the pending draft into React state
   (next to both setPendingDraft sites, cleared on take in send) and modelMissing relaxes when
   no chat exists but a complete pendingDraft or ANY default provider exists (server fills
   provider+model at creation; missing-model backend error surfaces as toast). Gates: npm
   build + Playwright 3/3. No service worker in the PWA — stale UI = plain browser cache,
   tell users to hard-reload. No server change in this step (no restart needed, dist from disk).
 - 2026-11-01 ~12:15: user asked "where is the provider edit button" — none existed: PATCH
   /api/providers/{id} (+rest.updateProvider) were implemented since Phase 1 but the drawer
   UI was add/delete-only. Added edit mode in SettingsDrawer: Edit button per provider row →
   pre-filled form (API key blank = keep existing), "Save provider" → PATCH, model list +
   draft model reset via modelsNonce when the edited provider is the selected one. Verified:
   npm build, PATCH e2e (create→patch base_url→delete as bob, trailing-slash normalized),
   Playwright 3/3. Context: admin123's 404 was llama.cpp lacking /chat/completions at root —
   fix by config = base URL http://192.168.0.88:8083/v1 (openai.rs posts {base}/chat/completions;
   /v1-fallback on 404 was offered to the user and explicitly deferred).
 - 2026-11-01 ~12:45: UX fix — streaming auto-scroll no longer fights the user (MessageList):
   stick-to-bottom pattern — scroller ref + onScroll sets pinned when within 80px of bottom
   (ref+state); the messages/streamingId effect only scrolls while pinned (instant now, old
   per-token "smooth" removed — also less jank); "↓ Jump to latest" pill (sticky bottom) when
   unpinned; opening/switching a chat (firstMsgId change) re-pins + jumps to bottom. data-testid
   "message-scroll". New smoke spec #4 verifies unlock + no-yank + jump-back. GOTCHA for e2e:
   fillers need \n\n (markdown collapses single newlines → 1-paragraph message → nothing to
    scroll). Gates: npm build + Playwright 4/4.
  - 2026-11-01 ~13:20: CHAT PIPELINE REWORK (user request) — generation is now decoupled from
    HTTP. crates/halo-server/src/hub.rs: per-chat Turn holds producer task + broadcast of named
    frames with per-turn seq; routes.rs: send/regenerate/edit = insert + spawn producer + 202
    {ok:true}, 409 while a turn runs on that chat; new GET /api/chats/{id}/events (SSE hub
    channel — first frame `state` = live snapshot incl. seq floor or {"streaming":false}; idle
    streams self-close the moment a turn starts so EventSource reconnects into it) and POST
    /api/chats/{id}/stop (cancels, partial persisted, `done` still emitted). SSE chosen over
    WebSockets (user asked): flow is server→client one-way, EventSource gives reconnect +
    Last-Event free, sends stay plain 202 POSTs. Front-end: api.ts subscribeChatEvents
    (EventSource, manual reconnect, `seq > last` delta guard); App.tsx subscription effect per
    active chat, user echo from `user` frame (no optimistic bubbles), `resync` frame
    (regenerate/edit) refetches; per-chat in-memory queue — Enter while streaming queues, chip
    "N queued", flushes FIFO on done; Composer stop button → POST stop. Docs synced same change:
    01-architecture.md API rows + full "Chat event protocol" section rewrite, AGENTS.md repo map.
    scripts/regression_sweep.py rewritten for hub era (first state frame, detached finalize after
    disconnect, live snapshot join, stop→partial, concurrent 409) — ALL GREEN. e2e extended 4→6:
    queueing test + multi-device live sync test — Playwright 6/6. Debugging saga: sync test failed
    because selectChat awaits getChat before setActiveId and the spec typed instantly after
    `.click()` → send() raced with activeId=null → turn created a NEW chat (sidebar rows don't
    prove the chat is open; human-speed typing can't hit it). Spec now waits
    getByRole("heading",{name:title}) after every nav-row click; sidebar rows clicked via
    nav div[title="…"] (exact, Sidebar.tsx title attr) not getByText; message-text assertions
    scoped to getByTestId("message-scroll") (auto-titled "Mock Title: …" sidebar rows caused
    strict-mode violations). 6 stale race-bug chats purged from bob. GOTCHA: run Playwright from
    web/ with ./node_modules/.bin/playwright (root npx → "did not expect test()"/"No tests
    found"). Gates: cargo fmt/clippy/test all green, npm build, sweep ALL GREEN, Playwright 6/6.

- 2026-11-01 ~13:40: INCIDENT + RECOVERY — web/src destroyed by a stray `rm -rf` appended to an
  unrelated icons-append command (all 17 source files under web/src lost; web config, e2e, dist
  survived). No git (halotalk not a repo) — recovered by replaying the opencode session DB
  (~/.local/share/opencode/opencode-dev.db `part` rows) into halotalk_recovery/: strict
  /halotalk/web/src/ path filter, full-file anchors = write events + complete 1..N reads, then
  chronological replay of every edit event AND the embedded bash/python patch scripts (assert-count
  guarded; one needed a manual normalization edit for a replace-first literal drift). Verified:
  App.tsx 825 lines, 0 dbg remnants; every line-numbered read taken today matches byte-for-byte;
  npm build clean; Playwright 6/6; sweep ALL GREEN. LESSON: never chain destructive commands; the
  DB replay is a one-time salvage path, not a backup strategy — init a real git repo next time we
  touch halotalk.

- 2026-11-01 ~15:10: User-requested UI batch (theme button top-right, viewport-aware textarea,
  send-status ticks, collapsible structured code blocks) shipped + verified; details in
  05-phase3-polish 3.1. Two latent bugs found by the new e2e specs (both fixed, not papered over):
  (1) HUB RECONNECT GAP — hub closes idle-held event streams at run start (by design) but the
  browser EventSource retries only after ~3 s; replies faster than that finish inside the dead
  window and the chat rendered empty (existing tests survived only because SLOW streams made the
  reconnect land mid-run → snapshot recovery). Front-end fix: onState `streaming:false` join +
  resync + mid-run replay now refetch the transcript; pending send-bubbles (App.tsx pendingRef +
  correlatePendings) reconcile against server user messages on every snapshot so ticks can't
  strand (also: "sent" accepted by echo matching since the 202 can beat the `user` frame).
  (2) REACT-MARKDOWN 9 — the sanitized `node` handed to a `pre` component override has stripped
  children (name/properties gone → fence lang always undefined). Lang/mermaid detection moved to
  the `code` component props (`pre` just unwraps); dead Hast/extractText helpers removed.
  mock_llm.py: JSONBLOCK trigger (28-line fenced json reply) for code-block e2e — mock restarted.
  e2e 6→9 (theme persistence, tick lifecycle, collapse + download); Playwright 9/9, build clean,
  sweep ALL GREEN. 6 stale pw-* chats purged from bob.
  FOLLOW-UP: halotalk is now a git repo — baseline commit 10657fa on `main` (64 files;
  .gitignore extended with /dist/, web/test-results/, web/playwright-report/). The placeholder
  commit identity must be replaced by the user's real git config for future commits.
