# HaloTalk — Overview

**HaloTalk** is a self-hosted, professional-looking LLM chat web app (in the spirit of Open
WebUI) built on a fully Rust backend. It supports text + image chat, streaming, per-chat
model settings (model, temperature, top_p, max tokens, system prompt, reasoning effort),
and light/dark themes.

## Goals
1. Single-binary-ish quick prototype: `cargo run` + `npm run dev`.
2. Connect to **any OpenAI-compatible endpoint** (OpenAI, OpenRouter, Ollama, LM Studio, vLLM).
3. Polish over feature-count: clean neutral UI, violet "halo" accent, dark/light themes.

## Non-goals (Phase 1)
- Auth / multi-user, RAG, tool calling, agents, voice, prompt marketplace.

## Iteration philosophy
- Phase 1 gets one chat working end-to-end with streaming + images. Ship it, then iterate.
- Each phase is a doc in this folder with an explicit checklist; a phase is done when its
  checklist is green. ADRs live in `06-risks-decisions.md`.

## Phases
| Phase | Doc | Theme | Est. |
|---|---|---|---|
| 1 | `03-phase1-prototype.md` | Working prototype: streaming chat, images, model settings, themes | now |
| 2 | `04-phase2-core.md` | Auth, multi-user, prompt library, folders/tags, regenerate/edit | 1–2 wks |
| 3 | `05-phase3-polish.md` | PWA, exports, tool calling, RAG hook, benchmarks | ongoing |

## Tech stack (verified latest stable, Oct 2026)
- Backend: Rust 2024, axum 0.8, tokio 1.54, sqlx 0.9 (SQLite), reqwest 0.13 (SSE), serde.
- Frontend: React 18 + Vite + TypeScript + Tailwind CSS (choice rationale: ADR-001).
- Providers: OpenAI-compatible client only (ADR-002).
