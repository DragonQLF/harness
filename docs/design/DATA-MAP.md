# DATA-MAP — every element to its real source

Read with `README.md`. `✔` = the command exists on `master`; `+` = must be built. Command names are the Rust fns in `src-tauri/src/commands/`; call them through `src/lib/ipc.ts` and regenerate types with `pnpm codegen`.

## Shell

| Element | Source | |
| --- | --- | --- |
| Project picker (name, slug, switch) | `projects_list`, `project_detail` | ✔ |
| Add project (+) | `project_pick_folder` → `project_inspect` → `project_add` | ✔ |
| CREW list, per-agent dot | `agents_get`, `active_runs` for the live dot | ✔ |
| RECORDS · worktrees | `worktrees` | ✔ |
| RECORDS · activity | `activity` | ✔ |
| User card | `settings_get` (operator name), `status` | ✔ |
| Title-bar search icon | opens the Ctrl+K palette (existing) | ✔ |

## Home

| Element | Source | |
| --- | --- | --- |
| "N running / M need you" | `active_runs`, `review_queue` | ✔ |
| Today's spend | `project_stats` (already computed for Home) | ✔ |
| Daily budget cap and pause | `settings_get` → operator budget | ✔ |
| Weekly heatmap cells | `run_stats(project_id, actor, tz_offset_minutes)` in `crates/app/src/runstats.rs`, projected straight from `events.jsonl` | ✔ |
| Heatmap filter tabs (All / Director / Agents) | same call, `actor` filter. A run's actor is its card's `agent_id`, backfilled from `CardAssigned` so a discarded card keeps its owner | ✔ |
| Recent runs table | `activity`, paired start↔end by `run_id` | ✔ |
| Composer ("add work") | `create_card` then optionally `start_run` | ✔ |

`run_stats` reads the whole event log rather than a `run-index.jsonl`: measured, 50,000 events is a 102 ms read and the aggregation itself is sub-millisecond, and Home already reads that same log twice. An index would be a second copy of the truth, able to drift, needing a backfill, to save 100 ms at a log size no project has. `runstats.rs` keeps a test that fails if that stops being true — the index in `backend-plan.md` is the fallback if it ever does.

## Chat

| Element | Source | |
| --- | --- | --- |
| Conversation list / new / rename / pin | `conversations_list`, `conversation_new`, `conversation_rename`, `conversation_pin` | ✔ |
| Thread messages | `conversation_transcript` | ✔ |
| Send, stop | `chat_send`, `chat_stop` | ✔ |
| Streaming deltas + live caret | existing sidecar partial-message events | ✔ |
| Tool receipts (create_card, read_diff…) | `tool.call` events on the same stream | ✔ |
| Permission sheet | `approvals_pending`, `respond_approval` | ✔ |
| "Always allow git push" | `respond_approval` + standing allowance in `settings_update` | ✔ |
| Model / agent chips | `model_catalog`, `agent_templates` | ✔ |
| "What it touched" (cards this thread created) | `snapshot` filtered by the ids in the transcript's tool calls | ✔ |
| Thread tokens / spend / tool calls / context | `conversation_totals`, off `RunEvent::Usage` lines now written by both the sidecar and the CLI adapter. Threads recorded before that show `—` for tokens and context | ✔ |
| File attach | `chat_pick_files` | ✔ |

## Board

| Element | Source | |
| --- | --- | --- |
| Columns and cards | `snapshot` | ✔ |
| Column counts | derived from the same snapshot | ✔ |
| Drag to another column | `move_card` | ✔ |
| Assign agent | `assign_agent` | ✔ |
| Approve / send back | `approve_card`, `reject_card` | ✔ |
| Start / cancel a run | `start_run`, `cancel_run` | ✔ |
| Dependencies | `set_dependencies` | ✔ |
| Discard | `discard_card` | ✔ |
| Live per-card progress | `active_runs` + `run.progress` events | ✔ |

## Code

| Element | Source | |
| --- | --- | --- |
| File tree, dirty markers | `list_tree(project_id, card_id?) -> Vec<TreeEntry{path,kind,dirty}>` — `git ls-files` overlaid with `git status --untracked-files=all`, cached per worktree on HEAD | ✔ |
| Source pane | `read_worktree_file(project_id, card_id?, path, rev?) -> FileText` — capped at 1 MB, binary returns a stub. `card_id` is optional so the project's own checkout is browsable | ✔ |
| Syntax highlighting | client-side only, in `src/lib/highlight.ts`: `shiki` behind dynamic `import()`, so the oniguruma WASM and each grammar are their own chunks and the main bundle grows by the two theme tables alone. Nine grammars bundled (rust, typescript, tsx, javascript, jsx, json, toml, markdown, css, html) off `FileText.lang` / `Hunk.lang`; two themes written in-repo over the design's four roles, light and dark. Nothing is fetched at runtime. An unknown grammar, a file past 200 kB or any failure renders plain source, never an error | ✔ |
| Diff hunks | `diff_hunks(project_id, card_id, path?) -> Vec<Hunk>`. `card_diff` still returns the raw patch and is still what Review reads | ✔ |
| Review panel · IN PROGRESS / NEEDS YOUR REVIEW | `review_queue` | ✔ |
| Hunk-level approve / reject | `review_hunk(project_id, card_id, file, header, approved, reason?)` → `Command::ReviewHunk`. The block is typed (`HunkRef`: file, `@@` header, line range), never prose in a reason. Verdicts pile up on the card until the diff is fully read, then it resolves: all approved → approved, all rejected → sent back, **partial → the card is approved and the rejected blocks are carried onto a follow-up card in Ready**, whose id is minted by the shell so replay is deterministic. `approve_card` / `reject_card` still exist and are unchanged with no selection | ✔ |
| Checks strip | `project_checks`, `project_run_checks` | ✔ |

The editor is **read-only in v1**. Writing through the UI means write-locking a running agent's worktree.

A partial rejection lands the card. That is the rule because a branch merges whole: nothing in the engine can put three quarters of a diff on the base and leave the fourth behind, so a decision that approves some blocks and rejects others has to say where each half goes. The approved half goes in as the card; the rejected half goes onto a new card in Ready, assigned to the same agent, whose title names every block sent back and the reason given for it. Rejecting *every* block is not partial and makes no follow-up — the card itself goes back to Ready and is the carrier, exactly as `reject_card` has always worked. The rule, the follow-up's text and the outcome of every mix live in `crates/domain/src/lib.rs` (`resolve_review`, `follow_up_title`) with the tests; `crates/engine` has the one that proves a partial decision replays from the stored log to the same two cards.

## Sessions

| Element | Source | |
| --- | --- | --- |
| Run rows (agent, card, status, duration, cost) | `activity` — rows now carry `run_id`, `outcome` and `tools` | ✔ |
| "Showing 6 of 12,439 runs" | count from the same response — never a literal | ✔ |
| Transcript replay | `run_log` | ✔ |
| Drop a worktree from a row | `remove_worktree` | ✔ |
| Reveal in file manager | `reveal_path` | ✔ |

## Lifecycle

| Element | Source | |
| --- | --- | --- |
| Splash status line ("starting engine", "reading 3 boards") | real bootstrap phases from `bootstrap`; if a phase has no event, show nothing rather than a fake step | ✔ |
| First run · claude found / logged in / sidecar | `status`, `bootstrap`, `sidecar_install` | ✔ |
| First run · repository step | `project_pick_folder`, `project_inspect`, `project_add` | ✔ |
| First run · crew | `agents_get`, `agent_templates`, `agents_save` | ✔ |
| First run · "ask before anything leaves this machine" | `settings_update` | ✔ |
| Updater sheets | `tauri-plugin-updater` events, in `src/components/Updater.tsx`. All four sheets | ✔ |
| "N agents running" in the update sheet | `active_runs` | ✔ |
| Settings · version, last checked, auto-install toggle | plugin state + `Settings.auto_install_updates` | ✔ |

Note: the existing `updates_list` / `update_install` commands are about proposal/card updates, not the application updater. Do not reuse them for the update sheets.

## Not to be built

- **Automations** — no such view in `src/views/views.ts`, no domain aggregate, no persisted file. The screen in `Relay.dc.html` is a design artefact; skip it. If automations are wanted later, `backend-plan.md` has the domain sketch.
