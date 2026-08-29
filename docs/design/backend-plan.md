# Backend plan — making screens 3a and 3b real

Scope: the two adapted screens in `Relay HUD.dc.html` — **3a Code (file tree + editor + Director panel)** and **3b Workflows (run-history heatmap)** — running on the real Relay stack (Tauri 2 + Rust crates + React, sidecar via `@anthropic-ai/claude-agent-sdk`). Everything below names existing modules from `DragonQLF/harness@master` and what has to be added.

Frontend libraries stay what the repo already uses: React 19, Tailwind 3 (tokens in `tailwind.config.js` — extend it with the light "Asphalt" ramp: `primary #3559E9`, `primary-soft #EEF1FE`, `neutral #16191E`, borders `#E4E7EC`), `lucide-react` for every icon in these screens (Search, GitBranch, SlidersHorizontal, Folder, FileText, History, Workflow, Play, Pause, Plus), `motion` for panel/row entrances (reuse `paneIn`, `rowIn` from `src/lib/motion.ts`). Heatmap, sparklines and diff view need no chart library — plain divs/SVG as mocked.

---

## Screen 3a — Code view

### Already exists
- Worktrees per card (`crates/adapters/git`, `src-tauri/src/workspace.rs`).
- Diff reading + review flow in the engine (`crates/engine/director.rs`, `read_diff` tool in `src-tauri/src/director_tools.rs`).
- Director chat with streaming deltas (`src-tauri/src/chat.rs`, `src/state/chat.ts`).
- Approve / send-back intents (`approve_card`, `send_back`) already routed through the engine actor.

### To build
1. **File-tree command** — `list_tree(project_id, card_id?) -> Vec<TreeEntry {path, kind, dirty}>` in `src-tauri/src/commands/`, backed by the git adapter (`git ls-files` inside the card's worktree, overlaid with `git status` for dirty markers). Cache per worktree; invalidate on `run.progress` events that carry a file write.
2. **File-read command** — `read_worktree_file(project_id, card_id, path, rev?) -> {text, lang, size}`. Cap at ~1 MB; binary detection returns a stub. `rev` lets the editor show HEAD vs. working copy.
3. **Hunk-level diff** — extend the git adapter with `diff_hunks(card_id, path?) -> Vec<Hunk {file, header, lines: Vec<(sign, text)>}>` (parse `git diff --unified=3`). The Director panel's "2 lines change" card is one `Hunk`; Approve/Reject at hunk level maps to `approve_card` / `send_back` with the hunk id in the intent payload so partial rejection can create a follow-up card.
4. **Review-queue projection** — the panel's IN PROGRESS / NEEDS YOUR REVIEW sections are a projection over existing events (`run.started`, `tool.call`, `permission.asked`, `card.moved -> Review`). Add a derived snapshot in `crates/app` (`review_queue(project) -> {in_progress: Vec<RunSummary>, needs_you: Vec<ReviewItem>}`) so the frontend keeps holding no truth; broadcast on the same engine queue.
5. **Syntax highlighting** — client-side only. Add `shiki` (lazy-loaded, WASM, no server) or reuse a tiny lexer for Rust/TS; keep tokens as Tailwind classes so themes stay in the config.
6. **Editor is read-only in v1.** Writing through the UI means write-locking against a running agent's worktree — defer; the Director applies changes, the human approves.

## Screen 3b — Workflows

### Already exists
- Append-only event log per project (`crates/adapters/store-jsonl`, `projects/<id>/events.jsonl`) and per-run transcripts (`runs/<run>.jsonl`) — the raw material for run history.
- Checks config + last result (`projects/<id>/checks.json`).
- Budget/settings (`crates/app` settings, daily spend already computed for the Home screen).

### To build
1. **Automation domain** — new `crates/domain` aggregate: `Automation {id, name, enabled, triggers: Vec<Trigger>, instructions, model, tool_allowlist}` with `Trigger = Schedule(cron) | CardMoved(column) | RunFailed | PermissionAsked | CheckFailed`. Persist as `projects/<id>/automations.json` via a new StorePort method (same torn-write-safe pattern as `checks.json`).
2. **Scheduler + event subscriber** — a task inside each project engine (single-writer rule holds: it only *sends intents*). Cron ticks (`tokio` interval + next-fire computation; no new heavy deps) and engine-event subscriptions both enqueue `start_automation_run(automation_id, trigger_ctx)`. Runs reuse the existing run lifecycle (`crates/engine/runs.rs`) so transcripts, spend and permissions come for free.
3. **Run-history aggregation** — `automation_stats(project_id, automation_id?, window) -> {total, heatmap: [[day_bucket; 7]; 38], last_1h, last_24h, last_7d, series_24h}`. Implement in `crates/app` (pure, unit-testable) over an index file `projects/<id>/run-index.jsonl` (one line per finished run: `{ts, automation_id, actor, status, duration_ms, cost}`) appended by the engine on `run.finished` — avoids re-parsing every transcript on screen open. Backfill command reads existing `events.jsonl` once.
4. **Heatmap filter tabs** — `All | Director | Agents` filters on the `actor` field of the index; same aggregation call with a filter param.
5. **Trigger table** — `recent_automation_runs(project_id, automation_id, limit) -> Vec<{trigger_label, triggered_at, first_tool, status, duration}>`; status = existing run states (Running / Succeeded / Failed / Cancelled).
6. **Usage card** — percentages are `spend / plan_allowance` split by actor class; allowance lives in operator settings (`settings.json`), spend already accumulates per run via commit trailers + transcripts. No plan/upgrade cards — Relay is local-first. The rail shows the daily budget (operator `settings.json`, runs pause at the cap) and live worktrees (`list_worktrees` already exists for the Worktrees screen; add a `stale` flag = no run in N days). Claude Code login usage folds into Total via the same transcript cost accounting.
7. **IPC surface** (`src/lib/ipc.ts` + `src-tauri/src/commands/`): `list_automations`, `save_automation`, `set_automation_enabled`, `stop_all_automation_runs`, `automation_stats`, `recent_automation_runs`. Regenerate TS types via the existing `pnpm codegen` (`export_types` test).

## Suggested order
1. Run-index file + backfill (unblocks all 3b numbers) — small, pure, testable.
2. `list_tree` / `read_worktree_file` / `diff_hunks` (3a becomes browsable against real worktrees).
3. Review-queue projection + hunk-level approve.
4. Automation domain + scheduler; wire the sidebar list and toggle.
5. Stats commands + heatmap UI; usage card last.

Tests follow the repo's rule: all logic in `crates/app`/`crates/domain`/`crates/engine` (cargo-testable), the Tauri crate stays a shell.
