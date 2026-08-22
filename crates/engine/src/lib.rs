use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use harness_domain::{Board, Card, CardId, Command, Event, RunId, RunOutcome};
use harness_ports::{
    AgentPort, ClockPort, GitPort, RunEvent, RunSpec, StoredEvent, WorktreePath,
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
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub last_seq: u64,
    pub cards: Vec<Card>,
    pub sessions: Vec<SessionView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunUpdate {
    pub card_id: CardId,
    pub run_id: RunId,
    #[serde(flatten)]
    pub event: RunEvent,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub card_id: CardId,
    pub worktree: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub repo_root: PathBuf,
    pub base_branch: String,
    pub permission_mode: String,
    pub worker_allowed_tools: Vec<String>,
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
        reply: oneshot::Sender<Result<RunId, String>>,
    },
    CancelRun {
        card_id: CardId,
        reply: oneshot::Sender<Result<(), String>>,
    },
    RunDone {
        card_id: CardId,
        run_id: RunId,
        outcome: harness_ports::RunOutcome,
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

impl EngineHandle {
    pub async fn execute(&self, cmd: Command) -> Result<u64, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Msg::Command { cmd, reply: reply_tx })
            .await
            .map_err(|_| "engine is not running".to_string())?;
        match reply_rx.await {
            Ok(outcome) => outcome,
            Err(_) => Err("engine dropped the reply".to_string()),
        }
    }

    pub async fn snapshot(&self) -> Result<Snapshot, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Msg::Snapshot { reply: reply_tx })
            .await
            .map_err(|_| "engine is not running".to_string())?;
        match reply_rx.await {
            Ok(snap) => Ok(snap),
            Err(_) => Err("engine dropped the reply".to_string()),
        }
    }

    pub async fn start_run(&self, card_id: CardId, prompt: String) -> Result<RunId, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Msg::StartRun {
                card_id,
                prompt,
                reply: reply_tx,
            })
            .await
            .map_err(|_| "engine is not running".to_string())?;
        match reply_rx.await {
            Ok(outcome) => outcome,
            Err(_) => Err("engine dropped the reply".to_string()),
        }
    }

    pub async fn cancel_run(&self, card_id: CardId) -> Result<(), String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Msg::CancelRun { card_id, reply: reply_tx })
            .await
            .map_err(|_| "engine is not running".to_string())?;
        match reply_rx.await {
            Ok(outcome) => outcome,
            Err(_) => Err("engine dropped the reply".to_string()),
        }
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
    token: CancellationToken,
    _worktree: WorktreePath,
}

pub struct Engine<S, C, A, D, G>
where
    S: StorePort + 'static,
    C: ClockPort + 'static,
    A: AgentPort + 'static,
    D: AgentPort + 'static,
    G: GitPort + 'static,
{
    rx: mpsc::Receiver<Msg>,
    self_tx: mpsc::Sender<Msg>,
    board: Board,
    last_seq: u64,
    store: Arc<S>,
    clock: Arc<C>,
    agent: Arc<A>,
    director: Arc<D>,
    git: Arc<G>,
    config: EngineConfig,
    runs: HashMap<CardId, RunEntry>,
    sessions: HashMap<CardId, (PathBuf, Option<String>)>,
    logged_tx: broadcast::Sender<Envelope>,
    runs_tx: broadcast::Sender<RunUpdate>,
}

use harness_ports::StorePort;

impl<S, C, A, D, G> Engine<S, C, A, D, G>
where
    S: StorePort + 'static,
    C: ClockPort + 'static,
    A: AgentPort + 'static,
    D: AgentPort + 'static,
    G: GitPort + 'static,
{
    #[allow(clippy::type_complexity)]
    pub fn spawn(
        store: Arc<S>,
        clock: Arc<C>,
        agent: Arc<A>,
        director: Arc<D>,
        git: Arc<G>,
        config: EngineConfig,
        history: Vec<StoredEvent>,
    ) -> (
        EngineHandle,
        broadcast::Receiver<Envelope>,
        broadcast::Receiver<RunUpdate>,
    ) {
        let board = rebuild(&history);
        let last_seq = history.last().map(|s| s.seq).unwrap_or(0);

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
            store,
            clock,
            agent,
            director,
            git,
            config,
            runs: HashMap::new(),
            sessions: HashMap::new(),
            logged_tx,
            runs_tx,
        };

        for (card_id, run_id) in interrupted {
            let events = engine.board.decide(&Command::FinishRun {
                card_id: card_id.clone(),
                run_id: run_id.clone(),
                outcome: RunOutcome::Failed,
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

    fn persist_sync(&mut self, events: Vec<Event>) -> Result<u64, String> {
        for event in events {
            let stored = self.store.append_event(&event).map_err(|e| e.to_string())?;
            self.board.apply(&stored.event);
            self.last_seq = stored.seq;
            let _ = self.logged_tx.send(Envelope {
                seq: stored.seq,
                ts_ms: 0,
                event: stored.event,
            });
        }
        Ok(self.last_seq)
    }

    async fn persist(&mut self, events: Vec<Event>) -> Result<u64, String> {
        for event in events {
            let store = Arc::clone(&self.store);
            let ev = event.clone();
            let stored = tokio::task::block_in_place(move || store.append_event(&ev))
                .map_err(|e| e.to_string())?;
            self.board.apply(&stored.event);
            self.last_seq = stored.seq;
            let envelope = Envelope {
                seq: stored.seq,
                ts_ms: self.clock.now_millis(),
                event: stored.event,
            };
            let _ = self.logged_tx.send(envelope);
        }
        Ok(self.last_seq)
    }

    async fn run(mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                Msg::Command { cmd, reply } => {
                    let outcome = self.execute(cmd).await;
                    let _ = reply.send(outcome);
                }
                Msg::Snapshot { reply } => {
                    let snap = Snapshot {
                        last_seq: self.last_seq,
                        cards: self.board.cards().into_iter().cloned().collect(),
                        sessions: self
                            .sessions
                            .iter()
                            .map(|(card_id, (worktree, sid))| SessionView {
                                card_id: card_id.clone(),
                                worktree: worktree.to_string_lossy().to_string(),
                                session_id: sid.clone(),
                            })
                            .collect(),
                    };
                    #[cfg(test)]
                    eprintln!(
                        "[probe] snapshot seq={} statuses={:?}",
                        snap.last_seq,
                        snap.cards.iter().map(|c| (c.id.to_string(), c.status)).collect::<Vec<_>>()
                    );
                    let _ = reply.send(snap);
                }
                Msg::StartRun {
                    card_id,
                    prompt,
                    reply,
                } => {
                    let outcome = self.start_run(card_id, prompt).await;
                    let _ = reply.send(outcome);
                }
                Msg::CancelRun { card_id, reply } => {
                    let outcome = self.cancel_run(&card_id);
                    let _ = reply.send(outcome);
                }
                Msg::RunDone {
                    card_id,
                    run_id,
                    outcome,
                } => {
                    self.finish_run(card_id, run_id, outcome).await;
                }
                Msg::AgentSession {
                    card_id,
                    session_id,
                } => {
                    if let Some((_, sid)) = self.sessions.get_mut(&card_id) {
                        *sid = Some(session_id);
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
        let events = match self.board.decide(&cmd) {
            Ok(events) => events,
            Err(e) => return Err(e.to_string()),
        };
        self.persist(events).await
    }

    async fn start_run(&mut self, card_id: CardId, prompt: String) -> Result<RunId, String> {
        if self.runs.contains_key(&card_id) {
            return Err("card already has an active run".to_string());
        }
        let run_id = RunId(uuid::Uuid::new_v4().to_string());
        let events = self
            .board
            .decide(&Command::StartRun {
                card_id: card_id.clone(),
                run_id: run_id.clone(),
            })
            .map_err(|e| e.to_string())?;
        self.persist(events).await?;
        #[cfg(test)]
        eprintln!(
            "[probe] start_run persisted ok card={card_id} status={:?}",
            self.board.get(&card_id).map(|c| c.status)
        );

        let worktree = {
            let git = Arc::clone(&self.git);
            let cid = card_id.to_string();
            let base = self.config.base_branch.clone();
            tokio::task::block_in_place(move || git.create_worktree(&cid, &base))
                .map_err(|e| e.to_string())?
        };

        let token = CancellationToken::new();
        self.runs.insert(
            card_id.clone(),
            RunEntry {
                token: token.clone(),
                _worktree: worktree.clone(),
            },
        );
        self.sessions
            .insert(card_id.clone(), (worktree.0.clone(), None));

        let spec = RunSpec {
            prompt,
            cwd: worktree.0.clone(),
            model: None,
            allowed_tools: Some(self.config.worker_allowed_tools.clone()),
            max_budget_usd: None,
            permission_mode: Some(self.config.permission_mode.clone()),
        };

        let (ev_tx, mut ev_rx) = mpsc::channel::<RunEvent>(256);
        let fut = self.agent.run(spec, ev_tx, token);

        let upd_tx = self.runs_tx.clone();
        let self_tx = self.self_tx.clone();
        let upd_card = card_id.clone();
        let upd_run = run_id.clone();
        let done_card = card_id;
        let done_run = run_id.clone();
        let git = Arc::clone(&self.git);
        let wt_for_wip = worktree;

        tokio::spawn(async move {
            let forward = async {
                while let Some(ev) = ev_rx.recv().await {
                    if let RunEvent::Started { session_id } = &ev {
                        let _ = self_tx.send(Msg::AgentSession {
                            card_id: upd_card.clone(),
                            session_id: session_id.clone(),
                        })
                        .await;
                    }
                    let _ = upd_tx.send(RunUpdate {
                        card_id: upd_card.clone(),
                        run_id: upd_run.clone(),
                        event: ev,
                    });
                }
            };
            let (result, _) = tokio::join!(fut, forward);
            let outcome = result.unwrap_or_else(harness_ports::RunOutcome::Failed);
            match &outcome {
                harness_ports::RunOutcome::Completed { .. } => {
                    let trailers = harness_ports::Trailers(vec![
                        ("Harness-Card".to_string(), done_card.to_string()),
                        ("Harness-Run".to_string(), done_run.to_string()),
                    ]);
                    let _ = tokio::task::block_in_place(|| {
                        git.commit(
                            &wt_for_wip,
                            &format!("work for card {done_card}"),
                            &trailers,
                        )
                    });
                }
                harness_ports::RunOutcome::Cancelled | harness_ports::RunOutcome::Failed(_) => {
                    let _ = tokio::task::block_in_place(|| git.commit_wip(&wt_for_wip));
                }
            }
            let _ = self_tx
                .send(Msg::RunDone {
                    card_id: done_card,
                    run_id: done_run,
                    outcome,
                })
                .await;
        });

        Ok(run_id)
    }

    fn cancel_run(&mut self, card_id: &CardId) -> Result<(), String> {
        match self.runs.get(card_id) {
            Some(entry) => {
                entry.token.cancel();
                Ok(())
            }
            None => Err("no active run for this card".to_string()),
        }
    }

    async fn finish_run(
        &mut self,
        card_id: CardId,
        run_id: RunId,
        outcome: harness_ports::RunOutcome,
    ) {
        self.runs.remove(&card_id);
        let domain_outcome = match outcome {
            harness_ports::RunOutcome::Completed { .. } => RunOutcome::Completed,
            harness_ports::RunOutcome::Cancelled => RunOutcome::Cancelled,
            harness_ports::RunOutcome::Failed(ref msg) => {
                eprintln!("run on card {card_id} failed: {msg}");
                RunOutcome::Failed
            }
        };
        let events = match self.board.decide(&Command::FinishRun {
            card_id: card_id.clone(),
            run_id: run_id.clone(),
            outcome: domain_outcome,
        }) {
            Ok(events) => events,
            Err(e) => {
                eprintln!("finish_run rejected for {card_id}/{run_id}: {e}");
                return;
            }
        };
        if let Err(e) = self.persist(events).await {
            eprintln!("finish_run persist failed for {card_id}: {e}");
            return;
        }
        if matches!(outcome, harness_ports::RunOutcome::Completed { .. }) {
            self.run_director(card_id).await;
        }
    }

    async fn run_director(&mut self, card_id: CardId) {
        let (worktree, _) = match self.sessions.get(&card_id) {
            Some(v) => v.clone(),
            None => return,
        };
        let git = Arc::clone(&self.git);
        let wt = WorktreePath(worktree.clone());
        let base = self.config.base_branch.clone();
        let diff = tokio::task::block_in_place(move || git.diff_summary(&wt, &base))
            .unwrap_or_else(|_| "(diff unavailable)".to_string());

        let prompt = format!(
            "You are the Director reviewing work done for kanban card '{card_id}'.\n\n\
             Review the following git diff summary produced by the agent in its worktree.\n\
             Judge whether the work fulfills the task reasonably.\n\n\
             Respond with ONLY a JSON object, no other text:\n\
             {{\"decision\": \"approve\"|\"reject\", \"reason\": \"<short explanation>\"}}\n\n\
             DIFF:\n{diff}"
        );

        let run_id = RunId(uuid::Uuid::new_v4().to_string());
        let spec = RunSpec {
            prompt,
            cwd: worktree,
            model: None,
            allowed_tools: Some(vec!["Read".into(), "Glob".into(), "Grep".into()]),
            max_budget_usd: None,
            permission_mode: Some("dontAsk".to_string()),
        };

        let (ev_tx, mut ev_rx) = mpsc::channel::<RunEvent>(64);
        let token = CancellationToken::new();
        let fut = self.director.run(spec, ev_tx, token);

        let upd_tx = self.runs_tx.clone();
        let self_tx = self.self_tx.clone();
        let upd_card = card_id;
        let director_run = run_id;
        let last_result = Arc::new(std::sync::Mutex::new(None::<String>));
        let lr = Arc::clone(&last_result);

        tokio::spawn(async move {
            let forward = async {
                while let Some(ev) = ev_rx.recv().await {
                    match &ev {
                        RunEvent::Done { result: Some(r), .. } => {
                            *lr.lock().unwrap() = Some(r.clone());
                        }
                        RunEvent::Text { text } => {
                            let _ = upd_tx.send(RunUpdate {
                                card_id: upd_card.clone(),
                                run_id: director_run.clone(),
                                event: RunEvent::Text {
                                    text: format!("[director] {text}"),
                                },
                            });
                        }
                        _ => {}
                    }
                }
            };
            let (res, _) = tokio::join!(fut, forward);
            let outcome = res.unwrap_or_else(harness_ports::RunOutcome::Failed);
            let verdict = last_result.lock().unwrap().clone();
            let _ = self_tx
                .send(Msg::DirectorDone {
                    card_id: upd_card,
                    outcome: Box::new(outcome),
                    verdict,
                })
                .await;
        });
    }

    async fn handle_director_done(
        &mut self,
        card_id: CardId,
        outcome: harness_ports::RunOutcome,
        verdict: Option<String>,
    ) {
        if !matches!(outcome, harness_ports::RunOutcome::Completed { .. }) {
            eprintln!("director run failed for {card_id}; leaving card in Review");
            return;
        }
        let raw = verdict.unwrap_or_default();
        let parsed = extract_json_object(&raw)
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                let decision = v.get("decision")?.as_str()?.to_string();
                let reason = v
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();
                Some((decision, reason))
            });

        let cmd = match parsed {
            Some((d, reason)) if d == "approve" => Command::ApproveCard {
                card_id: card_id.clone(),
            },
            Some((d, reason)) if d == "reject" => Command::RejectCard {
                card_id: card_id.clone(),
                reason: format!("director: {reason}"),
            },
            _ => {
                eprintln!("director verdict unparsable for {card_id}; leaving card in Review");
                return;
            }
        };
        let events = match self.board.decide(&cmd) {
            Ok(events) => events,
            Err(e) => {
                eprintln!("director decision rejected for {card_id}: {e}");
                return;
            }
        };
        if let Err(e) = self.persist(events).await {
            eprintln!("director persist failed for {card_id}: {e}");
        }
    }
}

fn extract_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end < start {
        return None;
    }
    Some(s[start..=end].to_string())
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use harness_domain::{CardId, Status};
    use harness_ports::{GitError, RunOutcome, StoreError, StorePort, Trailers};

    use super::*;

    struct MemStore {
        records: Mutex<Vec<StoredEvent>>,
        next: AtomicU64,
    }

    impl Default for MemStore {
        fn default() -> Self {
            Self {
                records: Mutex::new(Vec::new()),
                next: AtomicU64::new(1),
            }
        }
    }

    impl MemStore {
        fn count(&self) -> usize {
            self.records.lock().unwrap().len()
        }
    }

    impl StorePort for MemStore {
        fn append_event(&self, e: &Event) -> Result<StoredEvent, StoreError> {
            let seq = self.next.fetch_add(1, Ordering::SeqCst);
            self.records.lock().unwrap().push(StoredEvent {
                seq,
                event: e.clone(),
            });
            Ok(StoredEvent { seq, event: e.clone() })
        }

        fn read_all(&self) -> Result<Vec<StoredEvent>, StoreError> {
            Ok(self.records.lock().unwrap().clone())
        }
    }

    struct FixedClock;

    impl ClockPort for FixedClock {
        fn now_millis(&self) -> u64 {
            42
        }
    }

    enum FakeMode {
        Complete,
        WaitCancelled,
        DirectApprove,
        DirectReject,
    }

    struct FakeAgent(FakeMode);

    type PinBox<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

    impl AgentPort for FakeAgent {
        fn run(
            &self,
            _spec: RunSpec,
            tx: mpsc::Sender<RunEvent>,
            cancel: CancellationToken,
        ) -> PinBox<Result<RunOutcome, String>> {
            match self.0 {
                FakeMode::Complete => Box::pin(async move {
                    let _ = tx.send(RunEvent::Started { session_id: "sess-42".into() }).await;
                    let _ = tx.send(RunEvent::Text { text: "working".into() }).await;
                    drop(tx);
                    Ok(RunOutcome::Completed {
                        session_id: Some("s1".into()),
                        cost_usd: Some(0.01),
                    })
                }),
                FakeMode::WaitCancelled => Box::pin(async move {
                    cancel.cancelled().await;
                    drop(tx);
                    Ok(RunOutcome::Cancelled)
                }),
                FakeMode::DirectApprove => Box::pin(async move {
                    let _ = tx
                        .send(RunEvent::Done {
                            session_id: None,
                            cost_usd: Some(0.0),
                            result: Some(
                                "{\"decision\":\"approve\",\"reason\":\"fine\"}".into(),
                            ),
                        })
                        .await;
                    drop(tx);
                    Ok(RunOutcome::Completed {
                        session_id: None,
                        cost_usd: Some(0.0),
                    })
                }),
                FakeMode::DirectReject => Box::pin(async move {
                    let _ = tx
                        .send(RunEvent::Done {
                            session_id: None,
                            cost_usd: Some(0.0),
                            result: Some(
                                "{\"decision\":\"reject\",\"reason\":\"not good enough\"}"
                                    .into(),
                            ),
                        })
                        .await;
                    drop(tx);
                    Ok(RunOutcome::Completed {
                        session_id: None,
                        cost_usd: Some(0.0),
                    })
                }),
            }
        }
    }

    struct FakeGit {
        calls: Mutex<Vec<String>>,
    }

    impl FakeGit {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl GitPort for FakeGit {
        fn create_worktree(&self, card_id: &str, _base: &str) -> Result<WorktreePath, GitError> {
            self.calls.lock().unwrap().push(format!("create:{card_id}"));
            Ok(WorktreePath(std::env::temp_dir()))
        }

        fn commit(&self, _wt: &WorktreePath, msg: &str, _t: &Trailers) -> Result<String, GitError> {
            self.calls.lock().unwrap().push(format!("commit:{msg}"));
            Ok("deadbeef".into())
        }

        fn commit_wip(&self, _wt: &WorktreePath) -> Result<Option<String>, GitError> {
            self.calls.lock().unwrap().push("wip".into());
            Ok(Some("wipbeef".into()))
        }

        fn remove_worktree(&self, _wt: &WorktreePath) -> Result<(), GitError> {
            Ok(())
        }

        fn diff_summary(
            &self,
            _wt: &WorktreePath,
            _base: &str,
        ) -> Result<String, GitError> {
            Ok("diff --git a/eggs.md b/eggs.md\n+egg content".to_string())
        }
    }

    async fn wait_for(label: &str, mut check: impl AsyncFnMut() -> bool) {
        let mut check = check;
        for _ in 0..300 {
            if check().await {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timeout: {label}");
    }

    fn test_config() -> EngineConfig {
        EngineConfig {
            repo_root: std::env::temp_dir(),
            base_branch: "main".into(),
            permission_mode: "acceptEdits".into(),
            worker_allowed_tools: vec![
                "Read".into(),
                "Edit".into(),
                "Write".into(),
                "Glob".into(),
                "Grep".into(),
                "Bash(git *)".into(),
            ],
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn accepted_commands_persist_and_update_state() {
        let store = Arc::new(MemStore::default());
        let (handle, mut sub, _runs) = Engine::spawn(
            store.clone(),
            Arc::new(FixedClock),
            Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeGit::new()),
            test_config(),
            vec![],
        );

        let id = CardId::new("c1");
        let seq = handle
            .execute(Command::CreateCard { card_id: id.clone(), title: "t".into() })
            .await
            .unwrap();
        assert_eq!(seq, 1);

        handle
            .execute(Command::MoveCard { card_id: id.clone(), to: Status::Ready })
            .await
            .unwrap();

        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.last_seq, 2);
        assert_eq!(snap.cards[0].status, Status::Ready);
        assert_eq!(store.count(), 2);
        assert_eq!(sub.try_recv().unwrap().seq, 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn start_run_streams_events_creates_worktree_and_finishes_in_review() {
        let store = Arc::new(MemStore::default());
        let git = Arc::new(FakeGit::new());
        let (handle, _logged, mut runs) = Engine::spawn(
            store.clone(),
            Arc::new(FixedClock),
            Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeAgent(FakeMode::Complete)),
            git.clone(),
            test_config(),
            vec![],
        );
        let id = CardId::new("c1");

        handle
            .execute(Command::CreateCard { card_id: id.clone(), title: "t".into() })
            .await
            .unwrap();

        let err = handle.start_run(id.clone(), "do it".into()).await.unwrap_err();
        assert!(err.contains("must be Ready"), "got: {err}");

        handle
            .execute(Command::OverrideCard {
                card_id: id.clone(),
                to: Status::Ready,
                reason: "test".into(),
            })
            .await
            .unwrap();

        handle.start_run(id.clone(), "do it".into()).await.unwrap();

        wait_for(
            "card reaches Running",
            async || handle.snapshot().await.unwrap().cards[0].status == Status::Running,
        )
        .await;

        wait_for(
            "session captured",
            async || {
                handle
                    .snapshot()
                    .await
                    .unwrap()
                    .sessions
                    .iter()
                    .any(|s| s.session_id.as_deref() == Some("sess-42"))
            },
        )
        .await;

        let upd = runs.recv().await.unwrap();
        assert_eq!(upd.card_id, id);
        assert!(matches!(upd.event, RunEvent::Started { .. }));
        let upd = runs.recv().await.unwrap();
        assert!(matches!(upd.event, RunEvent::Text { .. }));

        wait_for("card reaches Review", async || handle.snapshot().await.unwrap().cards[0].status == Status::Review).await;

        assert!(git.calls().iter().any(|c| c.starts_with("create:")));
        assert_eq!(store.count(), 4);
        assert!(handle.cancel_run(id.clone()).await.is_err());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancelled_run_returns_card_to_ready_and_commits_wip() {
        let git = Arc::new(FakeGit::new());
        let (handle, _logged, _runs) = Engine::spawn(
            Arc::new(MemStore::default()),
            Arc::new(FixedClock),
            Arc::new(FakeAgent(FakeMode::WaitCancelled)),
            Arc::new(FakeAgent(FakeMode::WaitCancelled)),
            git.clone(),
            test_config(),
            vec![],
        );
        let id = CardId::new("c2");

        handle
            .execute(Command::CreateCard { card_id: id.clone(), title: "t".into() })
            .await
            .unwrap();
        handle
            .execute(Command::OverrideCard {
                card_id: id.clone(),
                to: Status::Ready,
                reason: "test".into(),
            })
            .await
            .unwrap();
        handle.start_run(id.clone(), "go".into()).await.unwrap();

        handle.cancel_run(id.clone()).await.unwrap();

        wait_for("card returns to Ready", async || handle.snapshot().await.unwrap().cards[0].status == Status::Ready).await;
        assert!(git.calls().contains(&"wip".to_string()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn interrupted_run_recovered_as_failed_on_restart() {
        let store = Arc::new(MemStore::default());
        let id = CardId::new("seed");

        {
            let (handle, _, _) = Engine::spawn(
                store.clone(),
                Arc::new(FixedClock),
                Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeAgent(FakeMode::Complete)),
                Arc::new(FakeGit::new()),
                test_config(),
                vec![],
            );
            handle
                .execute(Command::CreateCard { card_id: id.clone(), title: "t".into() })
                .await
                .unwrap();
            handle
                .execute(Command::MoveCard { card_id: id.clone(), to: Status::Ready })
                .await
                .unwrap();
        }

        let mut history = store.read_all().unwrap();
        history.push(StoredEvent {
            seq: 3,
            event: Event::RunStarted {
                card_id: id.clone(),
                run_id: RunId("ghost".into()),
            },
        });

        let (handle, _, _) = Engine::spawn(
            store,
            Arc::new(FixedClock),
            Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeGit::new()),
            test_config(),
            history,
        );
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.cards[0].status, Status::Ready);
        assert_eq!(snap.cards[0].current_run, None);

        let seq = handle
            .execute(Command::MoveCard { card_id: id, to: Status::Running })
            .await
            .unwrap();
        assert_eq!(seq, 4);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejected_commands_leave_no_trace() {
        let store = Arc::new(MemStore::default());
        let (handle, _sub, _runs) = Engine::spawn(
            store.clone(),
            Arc::new(FixedClock),
            Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeGit::new()),
            test_config(),
            vec![],
        );
        let id = CardId::new("c1");
        handle
            .execute(Command::CreateCard { card_id: id.clone(), title: "t".into() })
            .await
            .unwrap();

        let err = handle
            .execute(Command::MoveCard { card_id: id, to: Status::Done })
            .await
            .unwrap_err();
        assert!(err.contains("illegal"), "got: {err}");
        assert_eq!(store.count(), 1);
    }

    async fn driven_to_review(mode: FakeMode, director: FakeMode) -> (EngineHandle, Arc<MemStore>, Arc<FakeGit>) {
        let store = Arc::new(MemStore::default());
        let git = Arc::new(FakeGit::new());
        let (handle, _logged, _runs) = Engine::spawn(
            store.clone(),
            Arc::new(FixedClock),
            Arc::new(FakeAgent(FakeMode::Complete)),
            Arc::new(FakeAgent(director)),
            git.clone(),
            test_config(),
            vec![],
        );
        let id = CardId::new("d1");
        handle
            .execute(Command::CreateCard { card_id: id.clone(), title: "t".into() })
            .await
            .unwrap();
        handle
            .execute(Command::OverrideCard {
                card_id: id.clone(),
                to: Status::Ready,
                reason: "setup".into(),
            })
            .await
            .unwrap();
        let _ = (mode,);
        handle.start_run(id.clone(), "work".into()).await.unwrap();
        (handle, store, git)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn director_approval_moves_card_to_done() {
        let (handle, store, git) =
            driven_to_review(FakeMode::Complete, FakeMode::DirectApprove).await;
        wait_for(
            "card reaches Done via director",
            async || handle.snapshot().await.unwrap().cards[0].status == Status::Done,
        )
        .await;
        assert_eq!(store.count(), 5);
        assert!(git.calls().iter().any(|c| c.starts_with("commit:")));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn director_rejection_sends_card_back_to_ready() {
        let (handle, store, _git) =
            driven_to_review(FakeMode::Complete, FakeMode::DirectReject).await;
        wait_for(
            "card returns to Ready via director rejection",
            async || handle.snapshot().await.unwrap().cards[0].status == Status::Ready,
        )
        .await;
        assert_eq!(store.count(), 5);
        let history = store.read_all().unwrap();
        assert!(matches!(
            history.last().unwrap().event,
            Event::CardRejected { .. }
        ));
    }
}
