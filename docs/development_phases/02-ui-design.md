# HaloTalk — UI Design

## Principles
- **Professional, quiet, dense-but-airy.** Neutral grays, one accent (violet "halo").
  No heavy shadows, no rounded-everything. 1px borders + subtle surface elevation.
- Open WebUI information architecture, but calmer: thinner sidebar, settings in a
  right drawer instead of modal overlays.

## Layout (desktop ≥1024px)
```
┌──────────┬──────────────────────────────┬───────────────┐
│ Sidebar  │  Chat header (model pill, ⚙) │ Settings      │
│ ⌕ new    │  ─────────────────────────── │ drawer        │
│ chats…   │  message thread (scroll)     │ (slide-over,  │
│ ⚙ ⬤theme │  ─────────────────────────── │  right,       │
│          │  composer (+img, send)       │  optional)    │
└──────────┴──────────────────────────────┴───────────────┘
```
Mobile: sidebar becomes overlay; composer full-width.

## Components
- `Sidebar`: New chat button, search, grouped chat list (Today/Yesterday/Older),
  rename/delete on hover, theme toggle + settings footer.
- `ChatHeader`: editable title, model pill (provider · model) opens drawer.
- `MessageList`: alternating bubbles; user = accent-tinted, assistant = surface;
  markdown (GFM) + `highlight.js` code blocks w/ language label + copy button;
  image attachments in a thumbnail row w/ lightbox; **collapsible "Thinking…"**
  block for reasoning deltas; streaming caret; error state w/ Retry.
- `Composer`: auto-grow textarea (Enter send, Shift+Enter newline), image attachments
  via paste/drag/file → thumbnails w/ remove, disabled + spinner while streaming.
- `SettingsDrawer` (per chat): provider select (Add provider… inline), model search-select
  (fetched live), sliders: Temperature 0–2, Top-P 0–1, Max tokens; textarea system prompt;
  reasoning effort: off / low / medium / high; Save.

## Theming
CSS custom properties under `[data-theme="light"]` / `[data-theme="dark"]`, initialized from
`prefers-color-scheme`, choice persisted to `localStorage`. Tailwind maps to the vars.

```
--bg #fafafa / #0e1013   --surface #ffffff / #171a1f
--border #e5e7eb / #262b33  --text #1a1d21 / #e6e9ef
--muted #6b7280 / #98a2b3   --accent #6d5ce7 / #8b7cf2   --danger #dc2626 / #f87171
```
Font: Inter (UI), ui-monospace/JetBrains Mono (code). Radius 8px cards / 6px inputs. Motion:
120ms ease for theme + drawer; no gratuitous animation.
