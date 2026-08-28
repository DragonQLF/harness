# Product

<!-- impeccable:product-schema 1 -->

> **How to read this file.** Sections state what is *confirmed*, and every
> forward-looking claim is labelled. Relay is early: much of it is built but has
> barely been run. Nothing here should be read as "this works today" unless it
> says so under **Exercised**.

## Platform

web

## Users

One user: the operator, a developer running Relay on their own machine. Single
operator is a **product decision, not a stage** — no accounts, no sharing, no
multi-user affordances, no second identity to design around. The only other
actors on screen are the agents, which the operator addresses and which never
operate the app on anyone else's behalf.

## Product Purpose

A desktop orchestrator for Claude Code agents. The operator points Relay at git
repositories, describes work, and a crew of agents does it — each card in its
own checkout, with a Director that reads finished diffs before they reach the
operator.

**Goals, not current state.** In priority order:

1. **Agents ship work the operator trusts without watching.** Queue cards, walk
   away, and the returned diffs are good enough to approve without re-reading
   everything. Review speed and trust signals are the design priority. *Not yet
   demonstrated — see Evidence.*
2. **Relay improves itself when the operator complains.** Friction the operator
   notices and names becomes a proposal, then a card, then a build. Triggered by
   the operator's reaction, not run as a background ambition. *The machinery
   exists; the loop has never completed once.*

## Positioning

The intended difference: the Director is a resident reviewer, not a chat window
bolted onto a task list. It holds Relay's own tools, watches every board at
once, reads finished diffs before the operator does, and files proposals about
Relay itself — with the whole loop closing on one machine, no server, no
account.

The reviewer half is real and has fired once. The self-improvement half is
built and unproven.

## Operating Context

- **Boards per project.** Later, Ready, Working, Review, Done. One single-writer
  engine per project owns its board.
- **A run gets a checkout.** Agent profiles choose `per_card`, `shared`, or
  `none`. `per_card` is the default: a git worktree on branch `harness/<card>`
  under app data — so the operator can keep working while agents work, a bad run
  cannot damage the open checkout, and several agents can run at once. Working
  in place is a setting, not a forbidden act.
- **Local git.** No remote required, no account. Nothing leaves the machine
  except calls to the model API.
- **Relay's own files stay out of the operator's repos** — settings, event logs,
  run transcripts and worktrees all live under `%APPDATA%\com.harness.app`.

## Capabilities and Constraints

### Exercised

Confirmed working by real use, however thin:

- Registering a project, cards, the board, and moving between columns.
- Starting a run: worktree created on `harness/<card>`, agent works, run
  finishes, cost and turn count recorded.
- Cancelling a run, and runs failing — both recorded as outcomes rather than
  silence.
- Transcripts and the append-only event log, replayable from disk.
- The Director reviewing a finished card and approving it with a written reason.
- Conversations with the Director, persisted across restarts.

### Built, never exercised

Implemented and tested in unit tests, but never once run end to end by the
operator. Treat as unproven, not as working:

- **Mirror mode** and its whole chain: the post-run build, parking the artefact
  in `updates/`, the update banner, install-and-relaunch, and the rollback. No
  project has ever carried the flag; the `updates/` directory has never existed.
- **The end-of-day look.** Has been triggered, and has never produced a single
  event. No `inbox.json` has ever been written.
- **The proposal inbox.** No proposal has ever been filed.
- **The Curator** pass over `report_work` notes.

### Technical constraints

- Tauri 2 (Rust) + React frontend; Claude Code through a Node sidecar running
  the Agent SDK, with the `claude` CLI as fallback.
- The frontend holds no truth: it sends intents and renders backend snapshots
  rather than replaying domain rules in TypeScript.
- Agent profiles are policy — model, budget, capabilities, checkout mode and
  reviewer resolve into a `RunProfile` when a run starts. The engine carries no
  policy of its own.
- Commits carry `Harness-Card` / `Harness-Run` / `Harness-Agent` trailers, which
  is how per-card history and per-agent line counts are read back out of git.
  They keep the old name deliberately: a persisted format inside commits that
  already exist.
- The identifier stays `com.harness.app` after the rename to Relay, because it
  picks the app-data folder and changing it would hide the operator's data.

### Intended, not built

- **Approvals should stop being unconditional.** Today every board mutation
  reaches the permission sheet by design (`chat.rs`, decisions #29/#76). The
  intent: the Director notices a pattern ("you have approved this twelve
  times"), proposes the standing rule, the operator accepts once, and it acts
  freely inside it — learned, then confirmed, the same shape as the proposal
  inbox. No mechanism designed. Destructive actions (`delete_card`,
  `reject_card`) stay button-only regardless (#70).

### Not law

Local-only, agents-work-in-a-copy, and data-outside-repos are current choices
with reasons — **not permanent commitments**. Any may be revisited with a good
argument rather than treated as sacred.

## Brand Commitments

- The product is **Relay**. It was called Harness until 2026-08-26; the old name
  survives on purpose in the app-data identifier, the git trailers, the
  `harness/<card>` branch prefix, and the internal Rust crate names.
- Icon: a white R on a teal-to-blue signal ramp
  (`src-tauri/icons/relay-icon.svg`), generated into every size from that file.
- The UI accent is still the Harness purple (`--accent: #8b7cff` dark,
  `#5b53d8` light) and has not been reconciled with the new icon.

## Evidence on Hand

**Real usage, in full.** One card (`c_19a1`) in a scratch project, five runs:
failed, failed, cancelled, failed ($0.78, 17 turns), completed ($1.04, 33
turns), then a Director approval with a written verdict. Roughly $1.82 spent in
total. The only registered project points at a scratch directory inside app
data, not at a real codebase.

That is the whole basis for any claim about whether agents produce trustworthy
work. It is one success out of five attempts, on one card, reviewed once.

**Documents:**

- `docs/DECISIONS.md` — append-only log of every decision and deviation,
  numbered and stable, over 90KB. The authority for why anything is as it is.
- `docs/DEBT.md` — rewritten each pass; current state of what is unfinished.
- `docs/SPEC-ORIGINAL.md` — the founding architecture document.
- `docs/screenshots/` — the shipped screens, light and dark.
- The UI is a transcription of a design file, `Relay v4.dc.html`. It was
  written with the design's tokens in `src/styles/theme.css` and every other
  style inline, so a screen could be read beside the design. Since decision #80
  it is Tailwind: the tokens are literal values in `tailwind.config.js`, and
  what a screen says about itself is in its class names. `theme.css` is gone.

No customers, testimonials, benchmarks, pricing, or deployment claims exist.
Future work must not invent any.

## Product Principles

Normative — how future work should behave, not descriptions of what it does.

1. **The operator is one person, and the design knows it.** No accounts, no
   sharing, no seats. Density and shortcuts beat onboarding for a stranger.
2. **Trust is earned by showing the work.** Transcripts, diffs, event logs and
   verdicts exist so an unattended run can be believed afterwards. Anything the
   operator must take on faith is a design failure.
3. **Ask once, then remember.** Repeating a question the operator has answered
   twelve times is exactly the friction the product should remove from itself.
4. **Nothing is silently lost.** Cancelled runs leave a `wip:` commit, filed
   proposals survive a skipped shutdown, and a step that produced nothing says
   so rather than showing an empty screen.
5. **Don't dress up the unproven.** Most of Relay has run once or never. Surfaces
   should not imply confidence the evidence does not support.
