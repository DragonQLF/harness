# Handoff: Relay UI (Tauri 2 + React) — 1:1 rebuild on real data

## Overview

This bundle is the new UI for **Relay** (`DragonQLF/harness@master`): the app shell, five screens, the identity, and the install/launch/update surfaces. The goal is a **1:1 rebuild inside the existing app** — same layout, same spacing, same type, same colours — with **every value coming from the real engine**. No mock arrays, no sample JSON, no hardcoded numbers.

## About the design files

The files here are **design references written in HTML**. They are not production code and must not be dropped into `src/` or wrapped in an iframe. Read them as the specification and rebuild each screen as React components in the repo's existing environment:

- React 19 + TypeScript, Vite, Tailwind 3 (`tailwind.config.js`)
- `src/components/ui.tsx` primitives (`Card`, `Pill`, `Button`, `Segmented`, `Icon`)
- `lucide-react` icons, `motion` for entrances (`paneIn`, `rowIn` in `src/lib/motion.ts`)
- `src/lib/format.ts` for every number, duration, cost and relative time
- `src/views/views.ts` for the view union and titles

Where the design shows a value the repo already formats (spend, duration, "4m ago"), call the existing formatter rather than re-implementing it.

## Fidelity

**High fidelity.** Colours, type, radii and spacing in the HTML are final. Match them exactly. Two intentional deviations to carry over:

1. The whole app renders at **`zoom: .86`** on the root element (`width:100%; height:calc(100vh / .86)`). That is a deliberate density choice, not a bug — keep it, or bake the equivalent scale into the Tailwind type/spacing scale if you prefer a zoom-free implementation. Do not ship it at 1.0; the layouts were tuned at .86.
2. Each screen pane is `overflow:auto` with an inner canvas floor (`min-width: 880–960px`); everything inside is fluid (`minmax(0,Nfr)` grid tracks, no px column floors, no private card scrolls). **Chat is the exception:** its pane does not scroll — the thread scrolls inside its own card and the composer is pinned.

## The one hard rule: no fake data

Every number, row, label, badge, timestamp, avatar and status in these designs is a projection of real state. When you build a component:

- **Never** commit a placeholder array, sample object, `Math.random()`, lorem text, or a "demo mode".
- The frontend holds no truth: send intents, render snapshots, and re-read the snapshot when the engine broadcasts an event. Do not replay domain rules in TypeScript.
- If the data does not exist yet, **build the command** in the right crate (see `DATA-MAP.md` for exactly which ones are missing and where they belong) — do not stub the UI to make it look finished.
- Until a command lands, the screen shows its **empty or loading state**, which is part of this spec:
  - *loading* — the card's own skeleton at final dimensions (no layout shift, no spinner overlay on the whole screen)
  - *empty* — one line of plain copy stating what will appear here and, where there is one, the action that creates it
  - *error* — the real error string from the command plus a retry affordance; never a friendly paraphrase that hides the cause
- Counts in headers ("6 of 12,439 runs", "2 agents running") are derived from the same response as the rows, never typed in.

## Screens in this bundle

Source file: `Relay.dc.html`. Nav order is Home · Chat · Board · Code · Sessions.

| Screen | Purpose | Backed by |
| --- | --- | --- |
| **Home** | What is running, what needs you, today's spend, one field to add work | `project_stats`, `active_runs`, `review_queue`, `approvals_pending`, `create_card` |
| **Chat** | The Director thread, its tool receipts, and the permission sheet inline | `conversations_list`, `conversation_transcript`, `chat_send`, `chat_stop`, `approvals_pending`, `respond_approval` |
| **Board** | Later / Ready / Working / Review / Done with drag and drop | `snapshot`, `move_card`, `assign_agent`, `approve_card`, `reject_card`, `start_run` |
| **Code** | File tree, read-only source, and the Director's review panel | `card_diff` + three commands to build (see DATA-MAP) |
| **Sessions** | Every recorded run and its transcript, replayed from disk | `activity`, `run_log` |

**Excluded: the Automations screen.** It is in the design file but is not a harness view (`src/views/views.ts` has no such member) and nothing in the backend produces it. Do not build it. That nav slot is reserved for **Agents** (`agents_get`, `agents_stats`, profile drawer), whose design is not in this bundle yet.

## Layout system

```
root                zoom .86 · display:flex · column · bg #F6F7F9 · color #16191E
├─ title bar        height 52 · flex:none · bg #fff · border-bottom 1px #E4E7EC · padding 0 18px
│  ├─ wordmark      RELAY · Space Grotesk 700 15px · letter-spacing .075em
│  │                color #16191E · text-shadow 1.6px 1.6px 0 #C3C9D3
│  ├─ nav           centred · gap 2 · Inter 500 12.5px · color #5A6472
│  │                item: padding 7px 13px · radius 8
│  │                active: bg #F1F3F7 · color #16191E · weight 600
│  └─ actions       three 15px lucide icons · stroke 2.4 · color #5A6472 · gap 16
└─ body             flex:1 · min-height 0 · display:flex
   ├─ sidebar       width 258 · flex:none · bg #fff · border-right 1px #E4E7EC · padding 14px 12px
   └─ pane          flex:1 · min-width 0 · overflow auto · padding 20px 22px
      └─ canvas     min-width 880–960 · flex column · gap 13–14
```

Sidebar sections: the **project picker** at the top (this is the project switcher — `projects_list` / `project_add`, it is *not* the Projects registry screen), then CREW (`agents_get`), then RECORDS, then the user card pinned to the bottom.

## Design tokens

**Colour**

| Token | Hex | Use |
| --- | --- | --- |
| canvas | `#F6F7F9` | app background |
| surface | `#FFFFFF` | cards, bars, sidebar |
| border | `#E4E7EC` | every card and control border |
| divider-strong | `#EEF0F4` | header rules inside cards |
| divider | `#F4F5F8` | table row rules |
| divider-soft | `#F1F3F7` | list rules, active nav fill |
| ink | `#16191E` | primary text, primary button fill |
| ink-2 | `#3A4353` | secondary text, chip labels |
| muted | `#5A6472` | nav, labels, body secondary |
| faint | `#98A1B2` | meta, mono captions, placeholders |
| primary | `#3559E9` | selection, links, live accents |
| primary-soft | `#EEF1FE` | primary chip fill |
| primary-border | `#DCE4FC` | composer border, primary chip border |
| success | `#1B7F4D` on `#E8F5EE` | passed checks, Done |
| warn | `#C2410C` on `#FEF9F4`, border `#F0C9A8` | permission requests |
| danger | `#B3243B` on `#FDECEF` | deny, failure |

**Type** — Inter (400/500/600/700) for UI, IBM Plex Mono (400/500/600) for every id, path, sha, count and duration, Space Grotesk (700) for the wordmark only. Sizes actually used: 20px/700 screen titles · 14.5px/700 sheet titles · 12.5–13px/600 card titles · 12–12.5px/400 body · 11.5px/500 chips · 10.5–11px mono meta · 9.5–10px section labels (letter-spacing .08em, uppercase, `#98A1B2`).

**Radius** 16 cards · 14 sheets · 12 tables and inner cards · 9–10 inputs and chips · 999 pills.
**Shadow** used sparingly: `0 5px 16px -12px rgba(53,89,233,.4)` on the focused composer only.
**Spacing** 13–16px between cards, 15–18px card padding, 10–12px inside rows.

## Interactions

- **Nav** — click switches view; keep it in the router (`src/views/views.ts`), and honour the Director's `show_screen` tool so the agent can drive it.
- **Ctrl+K** — palette over every screen, project, agent and card. Already a product rule; the new shell must not swallow the shortcut.
- **Chat** — thread auto-scrolls to the newest message only when already at the bottom; streaming deltas append into the last bubble (the blue caret in the design is the live cursor: 6×13px, `#3559E9`, `steps(1)` blink at 1.05s). Tool receipts render as they arrive: a check for finished, a spinning dashed circle (`1.2s linear infinite`) for in-flight.
- **Permission sheet** — the amber card is `respond_approval` with the request id minted by the adapter. Three actions: Allow once, Always allow <verb>, Deny. "Always allow" writes a standing allowance into `settings.json`.
- **Board** — drag and drop issues `move_card`; the card returns to its origin if the intent is rejected. Never move it optimistically past a rule.
- **Tables** — full-row hover `#FAFBFC`; the whole row is the click target.
- **Motion** — reuse `paneIn` for a screen entering and `rowIn` for list items. Nothing else animates. Durations already in `src/lib/motion.ts`; do not invent new easings.

## State

Per screen: `loading | ready | empty | error`, plus the engine's own broadcast. Subscribe once to the engine event stream and re-read the snapshot on `card.*`, `run.*`, `permission.*` — no polling. Chat additionally holds `streaming: boolean` and the id of the conversation being read.

## Identity

`Relay Logo.dc.html` holds the exploration; the decisions are:

- **Installer / taskbar / dock icon:** the tile mark in `public/relay.svg` (extruded R: flat terminals, letter lifted off a `#7E8798` shadow offset 3.4/64 units, ink gradient `#FFFFFF → #B9C0CC`, tile `#22262E → #0D0F13`, radius 16/64).
- **In-app title bar:** the **wordmark only** — `RELAY`, Space Grotesk 700, `letter-spacing .075em`, `text-shadow 1.6px 1.6px 0 #C3C9D3`. No tile, no monogram: the letter next to the word was the redundancy we removed.
- Colour is deliberately withheld from the mark. If an accent is ever needed it goes on the tile, never on the letter.

## Install, launch, update

`Relay Lifecycle.dc.html`. What is configuration versus what you build:

- **NSIS** — `sidebarImage` (164×314) and `headerImage` (150×57) BMPs, `installerIcon`, strings, optional `template`. The dialog itself is MUI2 and stays as it ships. Design art and copy are in the file.
- **WiX** — banner 493×58, dialog 493×312 if you also ship an MSI.
- **DMG** — background 660×400 plus `windowSize`, `appPosition`, `applicationFolderPosition`.
- **Splash** — a second Tauri window, 420×264, undecorated and transparent, closed when the engine reports ready. Skip it entirely if ready lands under 400ms. Beats: mark draws in 480ms (stem → bowl → leg), shadow slides out at 0.48s, wordmark rises 6px at 0.72s, status line and hairline from 0.90s, 160ms crossfade into the main window.
- **First run** — three steps in the main window at 820px: Claude Code checks (`status`, `bootstrap`, `sidecar_install`), add a repository (`project_pick_folder`, `project_inspect`, `project_add`), crew defaults (`agents_get`, `agents_save`). Every check in step 1 is a real probe; none of the ticks are decorative.
- **Updater** — `tauri-plugin-updater` emits events and no UI, so all four sheets (available, downloading, ready, failed) are yours. Two decisions to keep: the default action is **Install on quit**, not Restart now, because agents may be mid-run; and the failure sheet shows the raw updater log, not a paraphrase.

## Assets

- `public/relay.svg` — the tile mark, 1024×1024, already the repo's icon path. Regenerate the platform icons from it (`pnpm tauri icon public/relay.svg`).
- Fonts: Inter, IBM Plex Mono, Space Grotesk. Self-host them (`src/assets/fonts`) rather than fetching from Google — the app runs offline from `tauri://localhost`.
- Icons: `lucide-react` only. The designs use Search, GitBranch, SlidersHorizontal, Folder, FileText, History, Play, Plus, Check, ChevronsUpDown, ArrowUp, Upload.

## Files

| File | What it is |
| --- | --- |
| `Relay.dc.html` | app shell + five screens (plus the excluded Automations screen) |
| `Relay Logo.dc.html` | identity exploration, chosen mark, title-bar lockups |
| `Relay Lifecycle.dc.html` | installer, DMG, splash (live animation), first run, updater |
| `public/relay.svg` | the mark itself |
| `DATA-MAP.md` | every UI element → its real source, and the commands still missing |
| `backend-plan.md` | earlier plan for the Code screen and run-history aggregation |

Open the HTML files in a browser to read them; they render standalone.

## Order of work

1. Shell first — title bar, nav, sidebar, project picker — on `projects_list` and `bootstrap`. Nothing below lands cleanly without it.
2. Home on `project_stats` + `active_runs` + `review_queue`.
3. Chat on the existing conversation commands, including the permission sheet.
4. Board on `snapshot` and the move/approve intents.
5. Sessions on `activity` + `run_log`.
6. Code last — it needs the three new commands in `DATA-MAP.md`.
7. Lifecycle surfaces once the app is real: splash, first run, updater.
