# Phase 2 — Core Product Features

Goal: make HaloTalk safe to share beyond localhost and pleasant for daily use.

## 2.1 Auth & multi-user
- [x] Sessions: argon2 password hashing, self-rolled cookie sessions (`halo_session`, sha256-hashed
      64-hex tokens, 30-day sliding), `users`/`sessions` tables.
- [x] First-run setup screen creates admin; open-signup toggle in settings (admin-only; default off).
- [x] Ownership columns (`user_id`) + row filtering in every query (cross-user access → 404;
      legacy rows assigned to the first admin at setup).
- [x] API keys encrypted at rest (ChaCha20-Poly1305 `xc1:` ciphertext — originally fernet, migrated
      in 3.3b to drop openssl; `HALO_SECRET` or generated `data/secret.key` 0600) — closes ADR-004 risk.

## 2.2 Chat quality-of-life
- [x] Stop-generation (AbortController + server-side upstream cancel — partial reply
      persisted and frozen on disconnect).
- [x] Regenerate (drop last assistant reply, redo) + edit user message (resend from the
      edited point; thread forking/branching deferred below).
- [x] Branching: one user message → several assistant replies, switchable (regenerate keeps
      siblings + `POST /api/chats/{id}/messages/{mid}/activate`; ‹i/N› pager under branched
      replies; provider context and chat rendering use the active reply only; editing a user
      message discards its old siblings).
- [x] Chat search (FTS5 over messages, debounced in sidebar w/ snippets).
- [x] Pin, archive, tags (pinned-first sections + hover actions + Archived section in sidebar;
      tags edited via UI and matched with `#tag` tokens in search, combinable with text terms).
- [x] Title auto-generation via cheap model call after first exchange.

## 2.3 Prompts & models UX
- [x] Prompt library (`/`- autocomplete), per-user system prompts.
- [x] Model registry: per-model capabilities (vision?, reasoning?, ctx len) auto-detected
      where advertised; vision gating in composer.
- [x] Presets: named parameter sets per chat/provider.

## 2.4 Platform
- [x] Serve `web/dist` from axum with SPA fallback → true single binary (rust-embed).
- [x] Config export/import (JSON). (`GET /api/config/export[?include_secrets=1]`,
      `POST /api/config/import`, Export/Import buttons in Settings drawer Account section.)
- [x] Provider connectivity test button (`POST /api/providers/{id}/test` → reachability,
      model count, latency; inline result in Settings drawer).
- [x] Rate-limit + body-size middleware; structured logging (tracing).

## Exit criteria
Multi-user deployment behind HTTPS with clean security review; 90 % of Open WebUI
day-one flow coverage.
