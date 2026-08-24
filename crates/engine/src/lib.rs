//! The single writer. Every state change in Harness goes through this actor:
//! a command is decided against the board, persisted, then broadcast. Runs and
//! reviews happen in spawned tasks that report back through the same queue, so
//! there is exactly one owner of the truth.

mod director;
mod runs;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use harness_domain::{Board, Card, CardId, Command, Event, RunId, RunOutcome};
use harness_ports::{
    AgentPort, Approver, ClockPort, GitPort, RunEvent, RunLogLine, RunLogPort, RunProfile,
    StorePort, StoredEvent, WorktreeMode, WorktreePath,
};
use serde::Serialize;
use ts_rs::TS;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Name of the one checkout shared by every agent configured for it.
pub(crate) const SHARED_WORKTREE: &str = "shared";

const QUEUE_CAPACITY: usize = 256;
const BROADCAST_CAPACITY: usize = 1024;
/// How long shutdown waits for a run task to wind down before giving up on
/// its wip commit. Generous: an agent that ignores cancellation should not
/// hold the window open forever, but a commit needs seconds, not millis.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    pub seq: u64,
    pub ts_ms: u64,
    /// Which project this event belongs to; the UI keeps one board per project.
    pub project_id: String,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct Snapshot {
    pub project_id: String,
    pub last_seq: u64,
    pub cards: Vec<Card>,
    pub sessions: Vec<SessionView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunUpdate {
    pub project_id: String,
    pub card_id: CardId,
    pub run_id: RunId,
    pub ts_ms: u64,
    #[serde(flatten)]
    pub event: RunEvent,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct SessionView {
    pub card_id: CardId,
    pub run_id: Option<RunId>,
    pub worktree: String,
    pub branch: Option<String>,
    pub session_id: Option<String>,
    pub agent_id: String,
    pub started_ms: u64,
    pub live: bool,
}

/// Static wiring for one project's engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub project_id: String,
    pub repo_root: PathBuf,
    pub base_branch: String,
    /// Fallbacks used when an agent profile leaves them unset.
    pub permission_mode: String,
    pub worker_allowed_tools: Vec<String>,
    pub director_allowed_tools: Vec<String>,
    pub director_model: Option<String>,
    /// When the stored log reaches this many events, startup folds it into a
    /// single `BoardSnapshot` so the *next* startup replays one event instead
    /// of thousands. Zero disables compaction.
    pub compact_at: usize,
    /// Mirror mode: after a successful commit, the engine itself builds the
    /// project and attaches the artefact. None disables it. Deliberately the
    /// engine's job and not the agent's: outside the model's budget, and the
    /// result is our fact rather than its account (#41).
    pub post_build: Option<BuildSpec>,
}

/// How to build this project, and where finished artefacts live. The build
/// runs with the card's worktree as cwd; `artifact` is the path inside that
/// worktree which must exist for the build to count as green.
#[derive(Debug, Clone)]
pub struct BuildSpec {
    pub program: String,
    pub args: Vec<String>,
    pub updates_dir: PathBuf,
    pub artifact: String,
}

impl EngineConfig {
    pub fn new(project_id: impl Into<String>, repo_root: PathBuf) -> Self {
        Self {
            project_id: project_id.into(),
            repo_root,
            base_branch: "main".to_string(),
            permission_mode: "acceptEdits".to_string(),
            worker_allowed_tools: vec![
                "Read".into(),
                "Edit".into(),
                "Write".into(),
                "Glob".into(),
                "Grep".into(),
                "Bash(git *)".into(),
            ],
            director_allowed_tools: vec!["Read".into(), "Glob".into(), "Grep".into()],
            director_model: None,
            compact_at: 1000,
            post_build: None,
        }
    }
}

/// Operator settings the engine re-reads on every run, so toggling them in the
/// UI takes effect without a restart.
#[derive(Debug, Clone)]
pub struct EnginePolicy {
    /// The Director reads every finished diff before it reaches the operator.
    pub director_reviews_first: bool,
    /// Cancel and commit work in progress when the app is closing.
    pub commit_wip_on_close: bool,
}

impl Default for EnginePolicy {
    fn default() -> Self {
        Self {
            director_reviews_first: true,
            commit_wip_on_close: true,
        }
    }
}

/// A run the engine is currently driving.
#[derive(Debug, Clone, Serialize, TS)]
pub struct ActiveRun {
    pub card_id: CardId,
    pub run_id: RunId,
    pub agent_id: String,
    pub worktree: String,
    pub started_ms: u64,
}

#[derive(Debug)]
enum Msg {
    Command {
        cmd: Command,
        reply: oneshot::Sender<Result<u64, String>>,
    },
    Snapshot {
        reply: oneshot::Sender<Snapshot>,
    },
    StartRun {
        card_id: CardId,
        prompt: String,
        profile: Box<RunProfile>,
        reply: oneshot::Sender<Result<RunId, String>>,
    },
    CancelRun {
        card_id: CardId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    ActiveRuns {
        reply: oneshot::Sender<Vec<ActiveRun>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
    SetPolicy {
        policy: EnginePolicy,
        reply: oneshot::Sender<()>,
    },
    RunDone {
        card_id: CardId,
        run_id: RunId,
        outcome: Box<harness_ports::RunOutcome>,
        profile: Box<RunProfile>,
        /// The work could not be committed, so there is no diff to review.
        commit_failed: bool,
        /// The SHA the run task just committed, when it did.
        commit_sha: Option<String>,
    },
    /// A worktree the start of a run asked for is ready — or failed. The
    /// creation runs off the actor, because `git worktree add` takes seconds
    /// on a large repository and nothing else should wait behind it. `created`
    /// says the checkout was built fresh for this attempt (so a failed start
    /// owns its removal); an adopted one is never ours to delete.
    WorktreeResolved {
        card_id: CardId,
        prompt: String,
        profile: Box<RunProfile>,
        reply: oneshot::Sender<Result<RunId, String>>,
        result: Result<WorktreePath, String>,
        created: bool,
    },
    AgentSession {
        card_id: CardId,
        session_id: String,
    },
    /// The agent called `report_work`: its own account of finished work. The
    /// event goes to the log; the summary waits in the run's slot until the
    /// commit reads it. A second call replaces the first — last wins. The
    /// ack closes only when the report is durable, so "reported" means
    /// recorded rather than merely queued.
    WorkReport {
        card_id: CardId,
        summary: String,
        notes: Vec<String>,
        #[allow(dead_code)]
        ack: oneshot::Sender<Result<(), String>>,
    },
    /// A mirror-mode build finished off the actor. Green writes the artefact
    /// manifest; red leaves none, whatever was there before (#63). Either way
    /// the card then proceeds to its reviewer.
    BuildDone {
        card_id: CardId,
        run_id: RunId,
        profile: Box<RunProfile>,
        commit_sha: String,
        ok: bool,
        tail: String,
    },
    DirectorDone {
        card_id: CardId,
        outcome: Box<harness_ports::RunOutcome>,
        verdict: Option<String>,
    },
}

#[derive(Clone)]
pub struct EngineHandle {
    tx: mpsc::Sender<Msg>,
}

/// Every handle method is a round trip to the actor; nothing mutates state here.
impl EngineHandle {
    async fn ask<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<T>) -> Msg,
    ) -> Result<T, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(make(reply_tx))
            .await
            .map_err(|_| "engine is not running".to_string())?;
        reply_rx.await.map_err(|_| "engine dropped the reply".to_string())
    }

    pub async fn execute(&self, cmd: Command) -> Result<u64, String> {
        self.ask(|reply| Msg::Command { cmd, reply }).await?
    }

    pub async fn snapshot(&self) -> Result<Snapshot, String> {
        self.ask(|reply| Msg::Snapshot { reply }).await
    }

    pub async fn start_run(
        &self,
        card_id: CardId,
        prompt: String,
        profile: RunProfile,
    ) -> Result<RunId, String> {
        self.ask(|reply| Msg::StartRun {
            card_id,
            prompt,
            profile: Box::new(profile),
            reply,
        })
        .await?
    }

    pub async fn cancel_run(&self, card_id: CardId) -> Result<(), String> {
        self.ask(|reply| Msg::CancelRun { card_id, reply }).await?
    }

    pub async fn active_runs(&self) -> Result<Vec<ActiveRun>, String> {
        self.ask(|reply| Msg::ActiveRuns { reply }).await
    }

    /// Cancel everything and let the worktrees commit their work in progress.
    pub async fn shutdown(&self) -> Result<(), String> {
        self.ask(|reply| Msg::Shutdown { reply }).await
    }

    pub async fn set_policy(&self, policy: EnginePolicy) -> Result<(), String> {
        self.ask(|reply| Msg::SetPolicy { policy, reply }).await
    }

}

pub fn rebuild(history: &[StoredEvent]) -> Board {
    let mut board = Board::default();
    for stored in history {
        board.apply(&stored.event);
    }
    board
}

/// Where each card last worked, replayed from the log.
///
/// The board carries the durable facts (worktree, branch, session id); what it
/// cannot carry is *when* a run started, because the domain has no clock. That
/// comes from the stored timestamp, which is why this is a second pass here
/// rather than a field on `Card`.
fn restore_sessions(history: &[StoredEvent]) -> HashMap<CardId, SessionEntry> {
    let mut out: HashMap<CardId, SessionEntry> = HashMap::new();
    for stored in history {
        match &stored.event {
            Event::RunStarted {
                card_id,
                run_id,
                worktree,
                branch,
            } => {
                // A run logged before worktrees were recorded has nothing to
                // restore; the card simply has no session until it runs again.
                let Some(worktree) = worktree else { continue };
                let carried = out.get(card_id).and_then(|s| s.session_id.clone());
                out.insert(
                    card_id.clone(),
                    SessionEntry {
                        run_id: Some(run_id.clone()),
                        worktree: PathBuf::from(worktree),
                        branch: branch.clone(),
                        session_id: carried,
                        agent_id: String::new(),
                        started_ms: stored.ts_ms,
                    },
                );
            }
            Event::AgentSession {
                card_id,
                session_id,
            } => {
                if let Some(entry) = out.get_mut(card_id) {
                    entry.session_id = Some(session_id.clone());
                }
            }
            // The card is gone, and so is its checkout.
            Event::CardDiscarded { card_id, .. } => {
                out.remove(card_id);
            }
            _ => {}
        }
    }
    out
}

/// What the agent reported about its own work, waiting in the run's slot for
/// the commit to read. Last report of a run wins; absence is normal.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkReport {
    pub summary: String,
    pub notes: Vec<String>,
}

#[derive(Debug)]
struct RunEntry {
    run_id: RunId,
    token: CancellationToken,
    /// Cleared by shutdown when the policy says closing must not commit. The
    /// run task reads it when its agent reports a cancellation: the task, not
    /// the actor, owns the wip commit — two `git commit`s racing on one
    /// worktree is how index.lock fights and half-written states happen.
    commit_on_cancel: Arc<AtomicBool>,
    /// The agent's `report_work`, as it stands. Shared with the run task so
    /// the commit — which happens off the actor — can read the latest one.
    work_report: Arc<Mutex<WorkReport>>,
    /// The task driving this run. Shutdown takes it to wait for that commit.
    handle: Option<JoinHandle<()>>,
    worktree: WorktreePath,
    agent_id: String,
    started_ms: u64,
}

struct SessionEntry {
    run_id: Option<RunId>,
    worktree: PathBuf,
    branch: Option<String>,
    session_id: Option<String>,
    agent_id: String,
    started_ms: u64,
}

pub struct Engine {
    rx: mpsc::Receiver<Msg>,
    self_tx: mpsc::Sender<Msg>,
    board: Board,
    last_seq: u64,
    store: Arc<dyn StorePort>,
    clock: Arc<dyn ClockPort>,
    agent: Arc<dyn AgentPort>,
    director: Arc<dyn AgentPort>,
    approver: Option<Approver>,
    git: Arc<dyn GitPort>,
    run_log: Option<Arc<dyn RunLogPort>>,
    config: EngineConfig,
    policy: EnginePolicy,
    runs: HashMap<CardId, RunEntry>,
    /// Starts that are between "dispatched to build a worktree" and
    /// "registered in `runs`". The two phases of a start straddle a message
    /// boundary, and without this the card is invisible in between: two
    /// dispatches would both pass the checks, both build a worktree — and for
    /// per-card mode the second one deletes the first's checkout out from
    /// under an agent. Maps card → agent, so the concurrency limit can count
    /// what is not running yet.
    starting: HashMap<CardId, String>,
    sessions: HashMap<CardId, SessionEntry>,
    /// Worktree reused by agents configured for a shared branch.
    shared_worktree: Option<WorktreePath>,
    logged_tx: broadcast::Sender<Envelope>,
    runs_tx: broadcast::Sender<RunUpdate>,
}

/// Everything the engine needs, grouped so `spawn` stays readable.
pub struct EngineDeps {
    pub store: Arc<dyn StorePort>,
    pub clock: Arc<dyn ClockPort>,
    pub agent: Arc<dyn AgentPort>,
    pub director: Arc<dyn AgentPort>,
    pub git: Arc<dyn GitPort>,
    pub approver: Option<Approver>,
    pub run_log: Option<Arc<dyn RunLogPort>>,
}

impl Engine {
    pub fn spawn(
        deps: EngineDeps,
        config: EngineConfig,
        policy: EnginePolicy,
        history: Vec<StoredEvent>,
    ) -> (
        EngineHandle,
        broadcast::Receiver<Envelope>,
        broadcast::Receiver<RunUpdate>,
    ) {
        let board = rebuild(&history);
        let last_seq = history.last().map(|s| s.seq).unwrap_or(0);
        // Sessions survive a restart: the agent id is the one the card carries
        // now, since a profile can be reassigned while the app is closed.
        let mut sessions = restore_sessions(&history);
        for (card_id, entry) in sessions.iter_mut() {
            if let Some(card) = board.get(card_id) {
                entry.agent_id = card.agent_id.clone();
            }
        }

        // Anything still marked Running belongs to a process that died with us.
        let interrupted: Vec<(CardId, RunId)> = board
            .cards()
            .into_iter()
            .filter_map(|c| c.current_run.clone().map(|r| (c.id.clone(), r)))
            .collect();

        let (logged_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let logged_rx = logged_tx.subscribe();
        let (runs_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let runs_rx = runs_tx.subscribe();
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);

        let mut engine = Self {
            rx,
            self_tx: tx.clone(),
            board,
            last_seq,
            store: deps.store,
            clock: deps.clock,
            agent: deps.agent,
            director: deps.director,
            approver: deps.approver,
            git: deps.git,
            run_log: deps.run_log,
            config,
            policy,
            runs: HashMap::new(),
            starting: HashMap::new(),
            sessions,
            shared_worktree: None,
            logged_tx,
            runs_tx,
        };

        for (card_id, run_id) in interrupted {
            let events = engine.board.decide(&Command::FinishRun {
                card_id: card_id.clone(),
                run_id: run_id.clone(),
                outcome: RunOutcome::Failed,
                cost_usd: None,
                turns: None,
            });
            if let Ok(events) = events {
                if let Err(e) = engine.persist_sync(events) {
                    eprintln!("recovery persist failed: {e}");
                }
            }
        }

        // Compaction. Everything the log has said is already folded into
        // `engine.board`; writing that board as one event and restarting the
        // file from it keeps startup flat no matter how long a project lives.
        // A failure leaves the long log in place, which is only the old cost.
        if engine.config.compact_at > 0 && history.len() >= engine.config.compact_at {
            let snapshot = Event::BoardSnapshot {
                cards: engine.board.cards().into_iter().cloned().collect(),
            };
            match engine.store.append_event(&snapshot, engine.now()) {
                Ok(stored) => {
                    engine.board.apply(&snapshot);
                    engine.last_seq = stored.seq;
                    if let Err(e) = engine.store.compact(&[stored]) {
                        eprintln!("could not compact the log; it stays as it was: {e}");
                    }
                }
                Err(e) => eprintln!("could not write the board snapshot: {e}"),
            }
        }

        tokio::spawn(async move {
            engine.run().await;
        });

        (EngineHandle { tx }, logged_rx, runs_rx)
    }

    fn now(&self) -> u64 {
        self.clock.now_millis()
    }

    fn persist_sync(&mut self, events: Vec<Event>) -> Result<u64, String> {
        let ts = self.now();
        for event in events {
            let stored = self
                .store
                .append_event(&event, ts)
                .map_err(|e| e.to_string())?;
            self.board.apply(&stored.event);
            self.last_seq = stored.seq;
            let _ = self.logged_tx.send(Envelope {
                seq: stored.seq,
                ts_ms: stored.ts_ms,
                project_id: self.config.project_id.clone(),
                event: stored.event,
            });
        }
        Ok(self.last_seq)
    }

    async fn persist(&mut self, events: Vec<Event>) -> Result<u64, String> {
        let ts = self.now();
        for event in events {
            let store = Arc::clone(&self.store);
            let ev = event.clone();
            let stored = tokio::task::block_in_place(move || store.append_event(&ev, ts))
                .map_err(|e| e.to_string())?;
            self.board.apply(&stored.event);
            self.last_seq = stored.seq;
            let _ = self.logged_tx.send(Envelope {
                seq: stored.seq,
                ts_ms: stored.ts_ms,
                project_id: self.config.project_id.clone(),
                event: stored.event,
            });
        }
        Ok(self.last_seq)
    }

    /// Broadcast a run event and, when a log is configured, keep it on disk.
    fn emit_run(&self, card_id: &CardId, run_id: &RunId, event: RunEvent) {
        let ts_ms = self.now();
        if let (Some(log), false) = (&self.run_log, event.is_ephemeral()) {
            let _ = log.append(
                run_id.0.as_str(),
                &RunLogLine {
                    ts_ms,
                    event: event.clone(),
                },
            );
        }
        let _ = self.runs_tx.send(RunUpdate {
            project_id: self.config.project_id.clone(),
            card_id: card_id.clone(),
            run_id: run_id.clone(),
            ts_ms,
            event,
        });
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            project_id: self.config.project_id.clone(),
            last_seq: self.last_seq,
            cards: self.board.cards().into_iter().cloned().collect(),
            sessions: self
                .sessions
                .iter()
                .map(|(card_id, s)| SessionView {
                    card_id: card_id.clone(),
                    run_id: s.run_id.clone(),
                    worktree: s.worktree.to_string_lossy().to_string(),
                    branch: s.branch.clone(),
                    session_id: s.session_id.clone(),
                    agent_id: s.agent_id.clone(),
                    started_ms: s.started_ms,
                    live: self.runs.contains_key(card_id),
                })
                .collect(),
        }
    }

    async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                Msg::Command { cmd, reply } => {
                    let outcome = self.execute(cmd).await;
                    let _ = reply.send(outcome);
                }
                Msg::Snapshot { reply } => {
                    let _ = reply.send(self.snapshot());
                }
                Msg::StartRun {
                    card_id,
                    prompt,
                    profile,
                    reply,
                } => {
                    self.start_run(card_id, prompt, *profile, reply).await;
                }
                Msg::CancelRun { card_id, reply } => {
                    let _ = reply.send(self.cancel_run(&card_id));
                }
                Msg::ActiveRuns { reply } => {
                    let _ = reply.send(self.active_runs());
                }
                Msg::Shutdown { reply } => {
                    self.shutdown().await;
                    let _ = reply.send(());
                }
                Msg::SetPolicy { policy, reply } => {
                    self.policy = policy;
                    let _ = reply.send(());
                }
                Msg::RunDone {
                    card_id,
                    run_id,
                    outcome,
                    profile,
                    commit_failed,
                    commit_sha,
                } => {
                    self.finish_run(
                        card_id,
                        run_id,
                        *outcome,
                        *profile,
                        commit_failed,
                        commit_sha,
                    )
                    .await;
                }
                Msg::WorktreeResolved {
                    card_id,
                    prompt,
                    profile,
                    reply,
                    result,
                    created,
                } => {
                    self.launch_run(card_id, prompt, *profile, reply, result, created)
                        .await;
                }
                Msg::AgentSession {
                    card_id,
                    session_id,
                } => {
                    self.record_session(card_id, session_id).await;
                }
                Msg::WorkReport {
                    card_id,
                    summary,
                    notes,
                    ack,
                } => {
                    let outcome = self.work_reported(card_id, summary, notes).await;
                    let _ = ack.send(outcome);
                }
                Msg::BuildDone {
                    card_id,
                    run_id,
                    profile,
                    commit_sha,
                    ok,
                    tail,
                } => {
                    self.build_done(card_id, run_id, *profile, commit_sha, ok, tail)
                        .await;
                }
                Msg::DirectorDone {
                    card_id,
                    outcome,
                    verdict,
                } => {
                    self.handle_director_done(card_id, *outcome, verdict).await;
                }
            }
        }
    }

    async fn execute(&mut self, cmd: Command) -> Result<u64, String> {
        let events = self.board.decide(&cmd).map_err(|e| e.to_string())?;
        let seq = self.persist(events).await?;
        // A discarded card leaves a branch and a checkout behind; the board no
        // longer knows about it, so nothing else would ever clean it up. The
        // removal is detached: `worktree remove --force` takes seconds and the
        // card is already off the board, so nobody should wait behind it.
        if let Command::DiscardCard { card_id, .. } = &cmd {
            if let Some(session) = self.sessions.remove(card_id) {
                let git = Arc::clone(&self.git);
                let worktree = WorktreePath(session.worktree);
                let cid = card_id.clone();
                tokio::spawn(async move {
                    let removed =
                        tokio::task::spawn_blocking(move || git.remove_worktree(&worktree)).await;
                    match removed {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => eprintln!("could not remove the worktree for {cid}: {e}"),
                        Err(e) => eprintln!("could not remove the worktree for {cid}: {e}"),
                    }
                });
            }
        }
        Ok(seq)
    }

    /// The agent called `report_work`: the event is durable (summary, notes),
    /// and the summary waits in the run's slot until its commit reads it. A
    /// second call replaces the first — an agent refining itself beats two
    /// summaries glued together, so last wins rather than accumulate.
    async fn work_reported(
        &mut self,
        card_id: CardId,
        summary: String,
        notes: Vec<String>,
    ) -> Result<(), String> {
        let outcome = async {
            match self.board.decide(&Command::ReportWork {
                card_id: card_id.clone(),
                summary: summary.clone(),
                notes: notes.clone(),
            }) {
                Ok(events) => {
                    self.persist(events)
                        .await
                        .map_err(|e| format!("could not record the work report: {e}"))?;
                }
                // A card discarded mid-run has nothing to report against.
                Err(e) => return Err(format!("work report rejected: {e}")),
            }
            let kept = WorkReport {
                summary: summary.trim().to_string(),
                notes: notes.into_iter().filter(|n| !n.trim().is_empty()).collect(),
            };
            let note_count = kept.notes.len();
            match self.runs.get_mut(&card_id) {
                Some(entry) => {
                    *entry.work_report.lock().unwrap() = kept;
                    let unit = if note_count == 1 { "note" } else { "notes" };
                    let run_id = entry.run_id.clone();
                    self.emit_run(
                        &card_id,
                        &run_id,
                        RunEvent::Notice {
                            text: format!(
                                "the agent reported its work ({note_count} memory {unit})"
                            ),
                        },
                    );
                    Ok(())
                }
                None => Err("late work report: the run is already over".to_string()),
            }
        }
        .await;
        if let Err(e) = &outcome {
            eprintln!("work report for {card_id}: {e}");
        }
        outcome
    }

    /// Remember the session an agent reported, on the card and in the log, so
    /// the next run can resume it after a restart. Written once: the same id
    /// arrives twice per run (on init and on the result).
    async fn record_session(&mut self, card_id: CardId, session_id: String) {        if let Some(entry) = self.sessions.get_mut(&card_id) {
            entry.session_id = Some(session_id.clone());
        }
        let already = self
            .board
            .get(&card_id)
            .and_then(|c| c.session_id.clone())
            .is_some_and(|known| known == session_id);
        if already {
            return;
        }
        match self.board.decide(&Command::RecordSession {
            card_id: card_id.clone(),
            session_id,
        }) {
            Ok(events) => {
                if let Err(e) = self.persist(events).await {
                    eprintln!("could not record the session for {card_id}: {e}");
                }
            }
            // A card discarded mid-run has nothing to record against.
            Err(e) => eprintln!("session not recorded for {card_id}: {e}"),
        }
    }

    fn active_runs(&self) -> Vec<ActiveRun> {
        self.runs
            .iter()
            .map(|(card_id, entry)| ActiveRun {
                card_id: card_id.clone(),
                run_id: entry.run_id.clone(),
                agent_id: entry.agent_id.clone(),
                worktree: entry.worktree.0.to_string_lossy().to_string(),
                started_ms: entry.started_ms,
            })
            .collect()
    }

    /// The mirror build came back. Green writes the artefact manifest — the
    /// record that says a reviewed commit produced a working binary; red
    /// removes any manifest, because there is never an artefact of a failed
    /// build. Either way the transcript carries the compiler's last words,
    /// and only then does anyone review.
    async fn build_done(
        &mut self,
        card_id: CardId,
        run_id: RunId,
        profile: RunProfile,
        commit_sha: String,
        ok: bool,
        tail: String,
    ) {
        let manifest_dir = self
            .config
            .post_build
            .as_ref()
            .map(|b| b.updates_dir.join(card_id.as_str()));
        if let Some(dir) = &manifest_dir {
            let _ = std::fs::create_dir_all(dir);
            let manifest = dir.join("manifest.json");
            if ok {
                let body = serde_json::json!({
                    "card_id": card_id.to_string(),
                    "run_id": run_id.to_string(),
                    "commit_sha": commit_sha,
                    "built_at_ms": self.now(),
                });
                if let Err(e) = std::fs::write(&manifest, body.to_string()) {
                    eprintln!("could not write the artefact manifest: {e}");
                }
            } else {
                // Never an artefact of a failed build — not even a stale one.
                let _ = std::fs::remove_file(&manifest);
            }
        }

        let unit = if ok { "holds" } else { "failed" };
        self.emit_run(
            &card_id,
            &run_id,
            RunEvent::Notice {
                text: format!("the orchestrator build {unit}"),
            },
        );
        if !ok && !tail.trim().is_empty() {
            for line in tail.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
                self.emit_run(
                    &card_id,
                    &run_id,
                    RunEvent::Notice {
                        text: format!("build: {line}"),
                    },
                );
            }
        }
        self.dispatch_review(card_id, run_id, profile).await;
    }

    /// Cancel every run and wait for the run tasks to wind down. Each task
    /// performs its own wip commit when its agent reports a cancellation, so
    /// committing here would race it: two `git commit`s on one worktree, the
    /// second failing on index.lock or catching a half-written file.
    async fn shutdown(&mut self) {
        if !self.policy.commit_wip_on_close {
            for entry in self.runs.values() {
                entry.commit_on_cancel.store(false, Ordering::SeqCst);
            }
        }
        let handles: Vec<JoinHandle<()>> = self
            .runs
            .values_mut()
            .filter_map(|entry| {
                entry.token.cancel();
                entry.handle.take()
            })
            .collect();
        for handle in handles {
            if tokio::time::timeout(SHUTDOWN_GRACE, handle).await.is_err() {
                eprintln!("a run did not stop within the grace period; its work may be uncommitted");
            }
        }
    }
}

/// Pull a JSON object out of a model reply that may be wrapped in prose.
pub(crate) fn extract_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end < start {
        return None;
    }
    Some(s[start..=end].to_string())
}

pub(crate) fn worktree_label(mode: WorktreeMode) -> &'static str {
    match mode {
        WorktreeMode::PerCard => "per card",
        WorktreeMode::Shared => "shared",
        WorktreeMode::None => "main checkout",
    }
}
