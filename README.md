# Harness

Desktop orchestrator for Claude Code agents: a Kanban board where each card drives a
real agent run inside its own git worktree, with an AI Director that reviews finished
work and can be chatted with directly.

Stack: **Tauri 2 (Rust) + React** · Claude Code via **Node sidecar + Agent SDK**.

## Layout

```
crates/
  domain/               card state machine, events, board. Zero IO.
  ports/                GitPort, AgentPort, StorePort, ClockPort traits
  engine/               single-writer loop: intents -> decide -> persist -> broadcast
  adapters/
    store-jsonl/        append-only event log with replay (seq numbers, torn-write safe)
    git/                worktrees per card, commits w/ trailers, wip commits, diffs
    model-claude/       AgentPort over the claude CLI subprocess (option A)
    agent-sidecar/      AgentPort over the Node sidecar + Agent SDK (option B, default)
  src-tauri/            Tauri app shell: IPC commands, event bridges, approval router
sidecar/                Node process hosting @anthropic-ai/claude-agent-sdk
src/                    React frontend: pure projection of backend events + snapshots
docs/                   original architecture spec + decision/deviation log
```

## Run it

Prereqs: Rust toolchain, Node >= 20, pnpm, git, Claude Code logged in (`claude /login`).

```
pnpm install
cd sidecar && pnpm install && cd ..
pnpm tauri dev
```

Data lives in `%APPDATA%\com.harness.app` (event log + workspace repo).

## How it works

- Frontend holds no truth: it sends intents (`create_card`, `move_card`, `start_run`, ...)
  and renders snapshots plus sequenced broadcast events; gaps trigger re-snapshot.
- One engine task owns all state. Agent runs happen in spawned tasks that report back
  through the same queue.
- Every run gets its own worktree (`harness/<card>` branch). Completion auto-commits with
  `Harness-Card`/`Harness-Run` trailers; cancel/failure leaves a `wip:` commit.
- The Director reviews the diff of completed runs and approves (-> Done) or rejects with
  a reason (-> Ready). Humans can approve/reject from the Review column anytime.
- Tool use that needs permission pauses the agent and pops an approval dialog in the UI.

See `docs/SPEC-ORIGINAL.md` for the founding architecture document and
`docs/DECISIONS.md` for every deviation and decision made since.

## Tests

```
cargo test --workspace
node sidecar/smoke.mjs   # live end-to-end against your Claude login
```
