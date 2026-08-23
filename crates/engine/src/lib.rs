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
use std::sync::Arc;

use harness_domain::{Board, Card, CardId, Command, Event, RunId, RunOutcome};
use harness_ports::{
    AgentPort, Approver, ClockPort, GitPort, RunEvent, RunLogLine, RunLogPort, RunProfile,
    StorePort, StoredEvent, WorktreeMode, WorktreePath,
};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

const QUEUE_CAPACITY: usize = 256;
const BROADCAST_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    pub seq: u64,
    pub ts_ms: u64,
    /// Which project this event belongs to; the UI keeps one board per project.
    pub project_id: String,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
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
    },
    AgentSession {
        card_id: CardId,
        session_id: String,
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

struct RunEntry {
    run_id: RunId,
    token: CancellationToken,
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
            sessions: HashMap::new(),
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
                    let outcome = self.start_run(card_id, prompt, *profile).await;
                    let _ = reply.send(outcome);
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
                } => {
                    self.finish_run(card_id, run_id, *outcome, *profile, commit_failed)
                        .await;
                }
                Msg::AgentSession {
                    card_id,
                    session_id,
                } => {
                    if let Some(entry) = self.sessions.get_mut(&card_id) {
                        entry.session_id = Some(session_id);
                    }
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
        // longer knows about it, so nothing else would ever clean it up.
        if let Command::DiscardCard { card_id, .. } = &cmd {
            if let Some(session) = self.sessions.remove(card_id) {
                let git = Arc::clone(&self.git);
                let worktree = WorktreePath(session.worktree);
                let removed =
                    tokio::task::block_in_place(move || git.remove_worktree(&worktree));
                if let Err(e) = removed {
                    eprintln!("could not remove the worktree for {card_id}: {e}");
                }
            }
        }
        Ok(seq)
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

    /// Cancel every run, then (when configured) leave a wip commit behind so
    /// closing the window never loses work.
    async fn shutdown(&mut self) {
        let entries: Vec<(CancellationToken, WorktreePath)> = self
            .runs
            .values()
            .map(|e| (e.token.clone(), e.worktree.clone()))
            .collect();
        for (token, _) in &entries {
            token.cancel();
        }
        if !self.policy.commit_wip_on_close {
            return;
        }
        for (_, worktree) in entries {
            let git = Arc::clone(&self.git);
            let _ = tokio::task::block_in_place(move || git.commit_wip(&worktree));
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
