use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use harness_domain::{Actor, Board, CardId, Command, Event, RunOutcome as DomainOutcome, Status};
use harness_ports::{
    AgentPort, ApprovalRequest, ClockPort, GitError, GitPort, Reviewer, RunEvent, RunLogLine,
    RunLogPort, RunOutcome, RunProfile, RunSpec, StoreError, StorePort, StoredEvent, Trailers,
    WorktreeMode, WorktreePath,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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

    fn events(&self) -> Vec<Event> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.event.clone())
            .collect()
    }
}

impl StorePort for MemStore {
    fn append_event(&self, e: &Event, ts_ms: u64) -> Result<StoredEvent, StoreError> {
        let seq = self.next.fetch_add(1, Ordering::SeqCst);
        let stored = StoredEvent {
            seq,
            ts_ms,
            event: e.clone(),
        };
        self.records.lock().unwrap().push(stored.clone());
        Ok(stored)
    }

    fn read_all(&self) -> Result<Vec<StoredEvent>, StoreError> {
        Ok(self.records.lock().unwrap().clone())
    }

    fn compact(&self, keep: &[StoredEvent]) -> Result<(), StoreError> {
        let mut records = self.records.lock().unwrap();
        records.clear();
        records.extend(keep.iter().cloned());
        Ok(())
    }
}

#[derive(Default)]
struct MemRunLog {
    lines: Mutex<Vec<(String, RunLogLine)>>,
}

impl RunLogPort for MemRunLog {
    fn append(&self, run_id: &str, line: &RunLogLine) -> Result<(), StoreError> {
        self.lines
            .lock()
            .unwrap()
            .push((run_id.to_string(), line.clone()));
        Ok(())
    }

    fn read(&self, run_id: &str) -> Result<Vec<RunLogLine>, StoreError> {
        Ok(self
            .lines
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _)| id == run_id)
            .map(|(_, l)| l.clone())
            .collect())
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
    NeedsApproval,
    DirectApprove,
    DirectReject,
    Garbage,
}

struct FakeAgent(FakeMode);

type PinBox<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send>>;

impl AgentPort for FakeAgent {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        cancel: CancellationToken,
    ) -> PinBox<Result<RunOutcome, String>> {
        match self.0 {
            FakeMode::Complete => Box::pin(async move {
                let _ = tx
                    .send(RunEvent::Started {
                        session_id: "sess-42".into(),
                    })
                    .await;
                let _ = tx.send(RunEvent::Text { text: "working".into() }).await;
                drop(tx);
                Ok(RunOutcome::Completed {
                    session_id: Some("s1".into()),
                    cost_usd: Some(0.01),
                    turns: Some(7),
                })
            }),
            FakeMode::WaitCancelled => Box::pin(async move {
                cancel.cancelled().await;
                drop(tx);
                Ok(RunOutcome::Cancelled)
            }),
            FakeMode::NeedsApproval => Box::pin(async move {
                let allowed = match &spec.approver {
                    Some(approve) => {
                        approve(ApprovalRequest {
                            request_id: "req-1".into(),
                            tool: "Bash".into(),
                            summary: "git push".into(),
                            input: serde_json::json!({ "command": "git push" }),
                        })
                        .await
                    }
                    None => false,
                };
                let _ = tx
                    .send(RunEvent::Text {
                        text: format!("allowed={allowed}"),
                    })
                    .await;
                drop(tx);
                Ok(RunOutcome::completed(None, Some(0.0)))
            }),
            FakeMode::DirectApprove => Box::pin(async move {
                let _ = tx
                    .send(RunEvent::Done {
                        session_id: None,
                        cost_usd: Some(0.0),
                        turns: None,
                        result: Some("{\"decision\":\"approve\",\"reason\":\"fine\"}".into()),
                        error: None,
                    })
                    .await;
                drop(tx);
                Ok(RunOutcome::completed(None, Some(0.0)))
            }),
            FakeMode::DirectReject => Box::pin(async move {
                let _ = tx
                    .send(RunEvent::Done {
                        session_id: None,
                        cost_usd: Some(0.0),
                        turns: None,
                        result: Some(
                            "sure thing: {\"decision\":\"reject\",\"reason\":\"not good enough\"}"
                                .into(),
                        ),
                        error: None,
                    })
                    .await;
                drop(tx);
                Ok(RunOutcome::completed(None, Some(0.0)))
            }),
            FakeMode::Garbage => Box::pin(async move {
                let _ = tx
                    .send(RunEvent::Done {
                        session_id: None,
                        cost_usd: None,
                        turns: None,
                        result: Some("I could not decide".into()),
                        error: None,
                    })
                    .await;
                drop(tx);
                Ok(RunOutcome::completed(None, None))
            }),
        }
    }
}

struct FakeGit {
    /// Its own directory per instance: two tests sharing one would see each
    /// other's checkouts, and "does it already exist" is now a real question.
    root: std::path::PathBuf,
    calls: Mutex<Vec<String>>,
    /// When set, every commit fails with this reason.
    commit_fails: Option<String>,
    /// A checkout that cannot be created, the way a locked worktree is.
    fail_worktree: bool,
    /// When set, wip commits are also recorded here, so a test can prove an
    /// ordering against events the agent recorded in the same vec.
    order: Option<Arc<Mutex<Vec<&'static str>>>>,
}

impl FakeGit {
    fn new() -> Self {
        Self {
            root: unique_worktree_root(),
            calls: Mutex::new(Vec::new()),
            commit_fails: None,
            fail_worktree: false,
            order: None,
        }
    }

    /// A repository that refuses commits, the way one with no committer
    /// identity or a rejecting hook does.
    fn refusing_commits(reason: &str) -> Self {
        Self {
            root: unique_worktree_root(),
            calls: Mutex::new(Vec::new()),
            commit_fails: Some(reason.to_string()),
            fail_worktree: false,
            order: None,
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

fn unique_worktree_root() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "harness-engine-tests-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}

impl Default for FakeGit {
    fn default() -> Self {
        Self::new()
    }
}

impl GitPort for FakeGit {
    fn worktree_path(&self, name: &str) -> std::path::PathBuf {
        self.root.join(name)
    }

    fn create_worktree(&self, card_id: &str, _base: &str) -> Result<WorktreePath, GitError> {
        self.calls.lock().unwrap().push(format!("create:{card_id}"));
        if self.fail_worktree {
            return Err(GitError::Git("worktree is locked".into()));
        }
        let path = self.root.join(card_id);
        std::fs::create_dir_all(&path).map_err(|e| GitError::Io(e.to_string()))?;
        Ok(WorktreePath(path))
    }

    fn commit(&self, _wt: &WorktreePath, msg: &str, t: &Trailers) -> Result<String, GitError> {
        let trailers: Vec<String> = t.0.iter().map(|(k, v)| format!("{k}={v}")).collect();
        self.calls
            .lock()
            .unwrap()
            .push(format!("commit:{msg}|{}", trailers.join(",")));
        match &self.commit_fails {
            Some(reason) => Err(GitError::Git(reason.clone())),
            None => Ok("deadbeef".into()),
        }
    }

    fn commit_wip(&self, _wt: &WorktreePath) -> Result<Option<String>, GitError> {
        self.calls.lock().unwrap().push("wip".into());
        if let Some(order) = &self.order {
            order.lock().unwrap().push("wip");
        }
        match &self.commit_fails {
            Some(reason) => Err(GitError::Git(reason.clone())),
            None => Ok(Some("wipbeef".into())),
        }
    }

    fn remove_worktree(&self, _wt: &WorktreePath) -> Result<(), GitError> {
        Ok(())
    }

    fn diff_summary(&self, _wt: &WorktreePath, _base: &str) -> Result<String, GitError> {
        Ok("diff --git a/eggs.md b/eggs.md\n+egg content".to_string())
    }

    fn diff_numstat(&self, _wt: &WorktreePath, _base: &str) -> Result<(u64, u64), GitError> {
        Ok((1, 0))
    }
}

async fn wait_for(label: &str, mut check: impl AsyncFnMut() -> bool) {
    for _ in 0..300 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timeout: {label}");
}

/// Records the `resume_session` each run was handed, so a test can prove the
/// engine offered it rather than trusting that it did.
#[derive(Default)]
struct ResumeSpy {
    seen: Arc<Mutex<Vec<Option<String>>>>,
}

impl ResumeSpy {
    fn seen(&self) -> Vec<Option<String>> {
        self.seen.lock().unwrap().clone()
    }
}

impl AgentPort for ResumeSpy {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        _cancel: CancellationToken,
    ) -> PinBox<Result<RunOutcome, String>> {
        self.seen.lock().unwrap().push(spec.resume_session.clone());
        Box::pin(async move {
            drop(tx);
            Ok(RunOutcome::Completed {
                session_id: Some("s2".into()),
                cost_usd: Some(0.0),
                turns: Some(1),
            })
        })
    }
}

/// An agent that keeps working for a while *after* being cancelled — what a
/// sidecar mid-write looks like. It records when it actually stopped, so a
/// test can prove the wip commit waited for it.
struct SlowCancelAgent {
    order: Arc<Mutex<Vec<&'static str>>>,
}

impl AgentPort for SlowCancelAgent {
    fn run(
        &self,
        _spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        cancel: CancellationToken,
    ) -> PinBox<Result<RunOutcome, String>> {
        let order = Arc::clone(&self.order);
        Box::pin(async move {
            cancel.cancelled().await;
            tokio::time::sleep(Duration::from_millis(200)).await;
            drop(tx);
            order.lock().unwrap().push("agent-stopped");
            Ok(RunOutcome::Cancelled)
        })
    }
}

fn test_config() -> EngineConfig {
    EngineConfig::new("proj-test", std::env::temp_dir())
}

fn profile() -> RunProfile {
    RunProfile {
        agent_id: "builder".into(),
        model: None,
        allowed_tools: None,
        permission_mode: None,
        max_budget_usd: None,
        worktree: WorktreeMode::PerCard,
        reviewer: Reviewer::Director,
        max_concurrent: 1,
    }
}

struct Rig {
    handle: EngineHandle,
    store: Arc<MemStore>,
    git: Arc<FakeGit>,
    events: broadcast::Receiver<Envelope>,
    runs: broadcast::Receiver<RunUpdate>,
    log: Arc<MemRunLog>,
}

fn rig(worker: FakeMode, director: FakeMode) -> Rig {
    rig_with(worker, director, None, EnginePolicy::default())
}

fn rig_with(
    worker: FakeMode,
    director: FakeMode,
    approver: Option<harness_ports::Approver>,
    policy: EnginePolicy,
) -> Rig {
    rig_full(worker, director, approver, policy, FakeGit::new())
}

fn rig_full(
    worker: FakeMode,
    director: FakeMode,
    approver: Option<harness_ports::Approver>,
    policy: EnginePolicy,
    fake_git: FakeGit,
) -> Rig {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(fake_git);
    let log = Arc::new(MemRunLog::default());
    let (handle, events, runs) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(worker)),
            director: Arc::new(FakeAgent(director)),
            git: git.clone(),
            approver,
            run_log: Some(log.clone()),
        },
        test_config(),
        policy,
        vec![],
    );
    Rig {
        handle,
        store,
        git,
        events,
        runs,
        log,
    }
}

async fn card_ready(handle: &EngineHandle, id: &CardId) {
    handle
        .execute(Command::CreateCard {
            card_id: id.clone(),
            title: "t".into(),
        })
        .await
        .unwrap();
    handle
        .execute(Command::MoveCard {
            card_id: id.clone(),
            to: Status::Ready,
        })
        .await
        .unwrap();
}

async fn status_of(handle: &EngineHandle, id: &CardId) -> Option<Status> {
    handle
        .snapshot()
        .await
        .unwrap()
        .cards
        .into_iter()
        .find(|c| &c.id == id)
        .map(|c| c.status)
}

#[tokio::test(flavor = "multi_thread")]
async fn accepted_commands_persist_and_update_state() {
    let mut r = rig(FakeMode::Complete, FakeMode::Complete);
    let id = CardId::new("c1");

    let seq = r
        .handle
        .execute(Command::CreateCard {
            card_id: id.clone(),
            title: "t".into(),
        })
        .await
        .unwrap();
    assert_eq!(seq, 1);

    r.handle
        .execute(Command::MoveCard {
            card_id: id.clone(),
            to: Status::Ready,
        })
        .await
        .unwrap();

    let snap = r.handle.snapshot().await.unwrap();
    assert_eq!(snap.project_id, "proj-test");
    assert_eq!(snap.last_seq, 2);
    assert_eq!(snap.cards[0].status, Status::Ready);
    assert_eq!(snap.cards[0].agent_id, "builder");
    assert_eq!(r.store.count(), 2);

    let first = r.events.try_recv().unwrap();
    assert_eq!(first.seq, 1);
    assert_eq!(first.ts_ms, 42);
    assert_eq!(first.project_id, "proj-test");
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_commands_leave_no_trace() {
    let r = rig(FakeMode::Complete, FakeMode::Complete);
    let id = CardId::new("nope");

    let err = r
        .handle
        .execute(Command::MoveCard {
            card_id: id.clone(),
            to: Status::Ready,
        })
        .await
        .unwrap_err();
    assert!(err.contains("not found"), "unexpected error: {err}");
    assert_eq!(r.store.count(), 0);

    r.handle
        .execute(Command::CreateCard {
            card_id: id.clone(),
            title: "t".into(),
        })
        .await
        .unwrap();
    let err = r
        .handle
        .execute(Command::MoveCard {
            card_id: id,
            to: Status::Done,
        })
        .await
        .unwrap_err();
    assert!(err.contains("illegal move"), "unexpected error: {err}");
    assert_eq!(r.store.count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_run_commits_with_trailers_and_records_cost() {
    let mut r = rig_with(
        FakeMode::Complete,
        FakeMode::Complete,
        None,
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
    );
    let id = CardId::new("c2");
    card_ready(&r.handle, &id).await;

    let run_id = r
        .handle
        .start_run(id.clone(), "do the thing".into(), profile())
        .await
        .unwrap();

    wait_for("card reaches review", async || {
        status_of(&r.handle, &id).await == Some(Status::Review)
    })
    .await;

    let card = r
        .handle
        .snapshot()
        .await
        .unwrap()
        .cards
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    assert_eq!(card.cost_usd, 0.01);
    assert_eq!(card.turns, 7);
    assert_eq!(card.runs, 1);

    let calls = r.git.calls();
    assert!(calls.iter().any(|c| c == "create:c2"), "calls: {calls:?}");
    let commit = calls
        .iter()
        .find(|c| c.starts_with("commit:"))
        .expect("a commit");
    assert!(commit.contains("Harness-Card=c2"), "commit was {commit}");
    assert!(commit.contains("Harness-Run="), "commit was {commit}");
    assert!(commit.contains("Harness-Agent=builder"), "commit was {commit}");

    // The session survives for a resume, with the id the agent reported.
    let sessions = r.handle.snapshot().await.unwrap().sessions;
    let session = sessions.iter().find(|s| s.card_id == id).unwrap();
    assert_eq!(session.session_id.as_deref(), Some("s1"));
    assert!(!session.live);

    // Streamed events reached the broadcast channel and the durable log.
    let mut saw_text = false;
    while let Ok(update) = r.runs.try_recv() {
        if matches!(update.event, RunEvent::Text { .. }) {
            saw_text = true;
        }
    }
    assert!(saw_text, "expected the streamed text on the run channel");
    let logged = r.log.read(&run_id.0).unwrap();
    assert!(
        logged.iter().any(|l| matches!(l.event, RunEvent::Text { .. })),
        "expected the run log to hold the transcript"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_run_returns_card_to_ready_and_commits_wip() {
    let r = rig(FakeMode::WaitCancelled, FakeMode::Complete);
    let id = CardId::new("c3");
    card_ready(&r.handle, &id).await;

    r.handle
        .start_run(id.clone(), "long job".into(), profile())
        .await
        .unwrap();
    wait_for("run registers as active", async || {
        !r.handle.active_runs().await.unwrap().is_empty()
    })
    .await;

    let active = r.handle.active_runs().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].agent_id, "builder");

    r.handle.cancel_run(id.clone()).await.unwrap();
    wait_for("card returns to ready", async || {
        status_of(&r.handle, &id).await == Some(Status::Ready)
    })
    .await;

    assert!(r.git.calls().iter().any(|c| c == "wip"));
    assert!(r.handle.active_runs().await.unwrap().is_empty());
    assert!(r.handle.cancel_run(id).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_cancels_running_work_and_leaves_a_wip_commit() {
    let r = rig(FakeMode::WaitCancelled, FakeMode::Complete);
    let id = CardId::new("c4");
    card_ready(&r.handle, &id).await;
    r.handle
        .start_run(id.clone(), "long job".into(), profile())
        .await
        .unwrap();
    wait_for("run registers as active", async || {
        !r.handle.active_runs().await.unwrap().is_empty()
    })
    .await;

    r.handle.shutdown().await.unwrap();
    assert_eq!(
        r.git.calls().iter().filter(|c| c == &&"wip".to_string()).count(),
        1,
        "exactly one commit owns the worktree: two racing is how index.lock fights start"
    );
}

/// The old shutdown cancelled the token and committed immediately, while the
/// run task — seeing its cancellation — committed too. One wip commit per
/// worktree, made by the run task, *after* the agent has stopped.
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_waits_for_the_agent_before_the_single_wip_commit() {
    let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let mut git = FakeGit::new();
    git.order = Some(Arc::clone(&order));
    let git = Arc::new(git);
    let store = Arc::new(MemStore::default());

    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(SlowCancelAgent {
                order: Arc::clone(&order),
            }),
            director: Arc::new(FakeAgent(FakeMode::Complete)),
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: true,
            commit_wip_on_close: true,
        },
        vec![],
    );

    let id = CardId::new("c_slow");
    card_ready(&handle, &id).await;
    handle
        .start_run(id.clone(), "long job".into(), profile())
        .await
        .unwrap();
    wait_for("run registers as active", async || {
        !handle.active_runs().await.unwrap().is_empty()
    })
    .await;

    handle.shutdown().await.unwrap();

    assert_eq!(
        *order.lock().unwrap(),
        vec!["agent-stopped", "wip"],
        "the commit happened once, and only after the agent had actually stopped"
    );
}

/// With "commit on close" off, a close-cancellation must not leave commits
/// behind — that setting means the operator takes the risk, not that we
/// pretend to honour it while a second code path commits anyway. An in-app
/// cancel still commits (the flag is only cleared for shutdown).
#[tokio::test(flavor = "multi_thread")]
async fn shutdown_without_commit_policy_leaves_nothing_behind() {
    let mut git = FakeGit::new();
    git.order = Some(Arc::new(Mutex::new(Vec::new())));
    let git = Arc::new(git);
    let store = Arc::new(MemStore::default());

    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::WaitCancelled)),
            director: Arc::new(FakeAgent(FakeMode::Complete)),
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: true,
            commit_wip_on_close: false,
        },
        vec![],
    );

    let id = CardId::new("c_nocommit");
    card_ready(&handle, &id).await;
    handle.start_run(id.clone(), "work".into(), profile()).await.unwrap();
    wait_for("run registers as active", async || {
        !handle.active_runs().await.unwrap().is_empty()
    })
    .await;

    handle.shutdown().await.unwrap();
    assert!(
        !git.calls().iter().any(|c| c == "wip"),
        "no commit was asked for on close"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_run_recovered_as_failed_on_restart() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::new());
    let id = CardId::new("c5");

    {
        let (handle, _e, _r) = Engine::spawn(
            EngineDeps {
                store: store.clone(),
                clock: Arc::new(FixedClock),
                agent: Arc::new(FakeAgent(FakeMode::WaitCancelled)),
                director: Arc::new(FakeAgent(FakeMode::Complete)),
                git: git.clone(),
                approver: None,
                run_log: None,
            },
            test_config(),
            EnginePolicy::default(),
            vec![],
        );
        card_ready(&handle, &id).await;
        handle
            .start_run(id.clone(), "work".into(), profile())
            .await
            .unwrap();
        wait_for("card is running", async || {
            status_of(&handle, &id).await == Some(Status::Running)
        })
        .await;
    }

    let history = store.read_all().unwrap();
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            director: Arc::new(FakeAgent(FakeMode::Complete)),
            git,
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy::default(),
        history,
    );

    wait_for("recovery marks the run failed", async || {
        status_of(&handle, &id).await == Some(Status::Ready)
    })
    .await;
    assert!(matches!(
        store.events().last(),
        Some(Event::RunFinished {
            outcome: DomainOutcome::Failed,
            ..
        })
    ));
}

async fn driven_to_review(worker: FakeMode, director: FakeMode) -> (Rig, CardId) {
    let r = rig(worker, director);
    let id = CardId::new("c6");
    card_ready(&r.handle, &id).await;
    r.handle
        .start_run(id.clone(), "work".into(), profile())
        .await
        .unwrap();
    (r, id)
}

#[tokio::test(flavor = "multi_thread")]
async fn director_approval_moves_card_to_done() {
    let (r, id) = driven_to_review(FakeMode::Complete, FakeMode::DirectApprove).await;
    wait_for("director approves", async || {
        status_of(&r.handle, &id).await == Some(Status::Done)
    })
    .await;
    let card = r
        .handle
        .snapshot()
        .await
        .unwrap()
        .cards
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    let review = card.last_review.unwrap();
    assert_eq!(review.by, Actor::Director);
    assert!(review.approved);
    assert_eq!(review.reason, "fine");
}

#[tokio::test(flavor = "multi_thread")]
async fn director_rejection_sends_card_back_to_ready_with_a_reason() {
    let (r, id) = driven_to_review(FakeMode::Complete, FakeMode::DirectReject).await;
    wait_for("director rejects", async || {
        status_of(&r.handle, &id).await == Some(Status::Ready)
    })
    .await;
    let card = r
        .handle
        .snapshot()
        .await
        .unwrap()
        .cards
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    let review = card.last_review.unwrap();
    assert!(!review.approved);
    assert_eq!(review.reason, "not good enough");
}

#[tokio::test(flavor = "multi_thread")]
async fn unreadable_verdict_leaves_the_card_in_review() {
    let (r, id) = driven_to_review(FakeMode::Complete, FakeMode::Garbage).await;
    wait_for("card reaches review", async || {
        status_of(&r.handle, &id).await == Some(Status::Review)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(status_of(&r.handle, &id).await, Some(Status::Review));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reviewerless_agent_closes_its_own_card() {
    let r = rig(FakeMode::Complete, FakeMode::Complete);
    let id = CardId::new("c7");
    card_ready(&r.handle, &id).await;
    let mut p = profile();
    p.reviewer = Reviewer::Nobody;
    r.handle.start_run(id.clone(), "work".into(), p).await.unwrap();

    wait_for("card closes without a review", async || {
        status_of(&r.handle, &id).await == Some(Status::Done)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_human_reviewer_keeps_the_card_in_review() {
    let r = rig(FakeMode::Complete, FakeMode::DirectApprove);
    let id = CardId::new("c8");
    card_ready(&r.handle, &id).await;
    let mut p = profile();
    p.reviewer = Reviewer::Human;
    r.handle.start_run(id.clone(), "work".into(), p).await.unwrap();

    wait_for("card reaches review", async || {
        status_of(&r.handle, &id).await == Some(Status::Review)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(status_of(&r.handle, &id).await, Some(Status::Review));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_agent_runs_in_the_checkout_and_never_commits() {
    let r = rig(FakeMode::Complete, FakeMode::Complete);
    let id = CardId::new("c9");
    card_ready(&r.handle, &id).await;
    let mut p = profile();
    p.worktree = WorktreeMode::None;
    p.reviewer = Reviewer::Human;
    r.handle.start_run(id.clone(), "read".into(), p).await.unwrap();

    wait_for("card reaches review", async || {
        status_of(&r.handle, &id).await == Some(Status::Review)
    })
    .await;
    let calls = r.git.calls();
    assert!(calls.is_empty(), "expected no git writes, got {calls:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_worktree_is_created_once_and_reused() {
    let r = rig_with(
        FakeMode::Complete,
        FakeMode::Complete,
        None,
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
    );
    for name in ["s1", "s2"] {
        let id = CardId::new(name);
        card_ready(&r.handle, &id).await;
        let mut p = profile();
        p.worktree = WorktreeMode::Shared;
        r.handle.start_run(id.clone(), "work".into(), p).await.unwrap();
        wait_for("card reaches review", async || {
            status_of(&r.handle, &id).await == Some(Status::Review)
        })
        .await;
    }
    let creates: Vec<String> = r
        .git
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("create:"))
        .collect();
    assert_eq!(creates, vec!["create:shared".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_approver_sees_the_request_id_the_adapter_minted() {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured = Arc::clone(&seen);
    let approver: harness_ports::Approver = Arc::new(move |req: ApprovalRequest| {
        let captured = Arc::clone(&captured);
        Box::pin(async move {
            captured.lock().unwrap().push(req.request_id.clone());
            req.tool == "Bash"
        })
    });

    let r = rig_with(
        FakeMode::NeedsApproval,
        FakeMode::Complete,
        Some(approver),
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
    );
    let id = CardId::new("c10");
    card_ready(&r.handle, &id).await;
    r.handle
        .start_run(id.clone(), "push".into(), profile())
        .await
        .unwrap();

    wait_for("card reaches review", async || {
        status_of(&r.handle, &id).await == Some(Status::Review)
    })
    .await;
    assert_eq!(seen.lock().unwrap().clone(), vec!["req-1".to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_run_on_the_same_card_is_refused() {
    let r = rig(FakeMode::WaitCancelled, FakeMode::Complete);
    let id = CardId::new("c11");
    card_ready(&r.handle, &id).await;
    r.handle
        .start_run(id.clone(), "one".into(), profile())
        .await
        .unwrap();
    let err = r
        .handle
        .start_run(id, "two".into(), profile())
        .await
        .unwrap_err();
    assert!(err.contains("active run"), "unexpected error: {err}");
}

/// `max_concurrent` used to be stored and displayed but enforced nowhere. It
/// counts active runs per agent across cards, and the engine refuses a start
/// that would exceed it.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_at_its_limit_refuses_another_card() {
    let r = rig(FakeMode::WaitCancelled, FakeMode::Complete);
    let first = CardId::new("c13a");
    let second = CardId::new("c13b");
    card_ready(&r.handle, &first).await;
    card_ready(&r.handle, &second).await;

    r.handle
        .start_run(first.clone(), "one".into(), profile())
        .await
        .unwrap();
    wait_for("first run is active", async || {
        !r.handle.active_runs().await.unwrap().is_empty()
    })
    .await;

    // A different card, same agent: over the limit of 1.
    let err = r
        .handle
        .start_run(second.clone(), "two".into(), profile())
        .await
        .unwrap_err();
    assert!(
        err.contains("its limit is 1"),
        "unexpected error: {err}"
    );
    assert_eq!(status_of(&r.handle, &second).await, Some(Status::Ready));

    // Raising the limit lets it through.
    let mut wider = profile();
    wider.max_concurrent = 2;
    r.handle.start_run(second, "two".into(), wider).await.unwrap();
    assert_eq!(r.handle.active_runs().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_that_fails_is_reported_and_skips_review() {
    let mut r = rig_full(
        FakeMode::Complete,
        // The director would approve if it were ever asked; it must not be.
        FakeMode::DirectApprove,
        None,
        EnginePolicy::default(),
        FakeGit::refusing_commits("Author identity unknown"),
    );
    let id = CardId::new("c12");
    card_ready(&r.handle, &id).await;
    let run_id = r
        .handle
        .start_run(id.clone(), "work".into(), profile())
        .await
        .unwrap();

    wait_for("card reaches review", async || {
        status_of(&r.handle, &id).await == Some(Status::Review)
    })
    .await;

    // The Director never ran, so the card is still waiting for the operator.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(status_of(&r.handle, &id).await, Some(Status::Review));
    let card = r
        .handle
        .snapshot()
        .await
        .unwrap()
        .cards
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    assert!(
        card.last_review.is_none(),
        "nothing was committed, so nothing can have been reviewed"
    );

    // And the reason is on the run, both live and in the durable log.
    let mut live = Vec::new();
    while let Ok(update) = r.runs.try_recv() {
        if let RunEvent::Notice { text } = update.event {
            live.push(text);
        }
    }
    assert!(
        live.iter().any(|t| t.contains("Author identity unknown")),
        "the failure must reach the operator, got {live:?}"
    );
    let logged = r.log.read(&run_id.0).unwrap();
    assert!(logged.iter().any(|l| matches!(
        &l.event,
        RunEvent::Notice { text } if text.contains("could not commit")
    )));
}

/// A log past the threshold folds into one `BoardSnapshot` at startup: the
/// file shrinks to a single event, the board comes back exactly as it was,
/// and sequence numbers carry on from there.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_log_is_compacted_into_a_snapshot_on_startup() {
    let store = Arc::new(MemStore::default());
    let board = Board::default();
    for name in ["k1", "k2", "k3"] {
        let events = board
            .decide(&Command::CreateCard {
                card_id: CardId::new(name),
                title: name.into(),
            })
            .unwrap();
        for e in &events {
            store.append_event(e, 1).unwrap();
        }
    }

    let mut config = test_config();
    config.compact_at = 2;
    let git = Arc::new(FakeGit::default());
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            director: Arc::new(FakeAgent(FakeMode::Complete)),
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        config,
        EnginePolicy::default(),
        store.read_all().unwrap(),
    );

    assert_eq!(store.count(), 1, "the log is one snapshot now");
    let snap = handle.snapshot().await.unwrap();
    assert_eq!(snap.cards.len(), 3, "the board survived the fold");
    assert_eq!(snap.last_seq, 4);

    // And life goes on top of it: new events append after the snapshot.
    handle
        .execute(Command::MoveCard {
            card_id: CardId::new("k1"),
            to: Status::Ready,
        })
        .await
        .unwrap();
    assert_eq!(store.count(), 2);
}

/// The wip commit on close keeps the *work*. This keeps the agent's *memory* of
/// it: without the session id, the next run starts a stranger on a card it has
/// already worked on.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_session_survives_a_restart() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let id = CardId::new("c_resume");

    {
        let (handle, _e, _r) = Engine::spawn(
            EngineDeps {
                store: store.clone(),
                clock: Arc::new(FixedClock),
                agent: Arc::new(FakeAgent(FakeMode::Complete)),
                director: Arc::new(FakeAgent(FakeMode::Complete)),
                git: git.clone(),
                approver: None,
                run_log: None,
            },
            test_config(),
            EnginePolicy::default(),
            vec![],
        );
        card_ready(&handle, &id).await;
        let mut human = profile();
        human.reviewer = Reviewer::Human;
        handle
            .start_run(id.clone(), "work".into(), human)
            .await
            .unwrap();
        wait_for("the run finished", async || {
            status_of(&handle, &id).await == Some(Status::Review)
        })
        .await;

        let live = handle.snapshot().await.unwrap();
        let session = live
            .sessions
            .iter()
            .find(|s| s.card_id == id)
            .expect("a session while the engine is up");
        // The result carries the session, and it wins over the init event.
        assert_eq!(session.session_id.as_deref(), Some("s1"));
        assert!(!session.worktree.is_empty());
    }

    // A new engine over the same log: this is what a restart is.
    let history = store.read_all().unwrap();
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            director: Arc::new(FakeAgent(FakeMode::Complete)),
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy::default(),
        history,
    );

    let snap = handle.snapshot().await.unwrap();
    let session = snap
        .sessions
        .iter()
        .find(|s| s.card_id == id)
        .expect("the session came back after the restart");
    assert_eq!(session.session_id.as_deref(), Some("s1"));
    assert_eq!(session.agent_id, "builder");
    assert!(!session.worktree.is_empty(), "and it remembers where it worked");
    assert!(!session.live, "but nothing is running any more");

    // The card carries it too, so anything reading cards can offer a resume.
    let card = snap.cards.iter().find(|c| c.id == id).unwrap();
    assert_eq!(card.session_id.as_deref(), Some("s1"));
    assert!(card.worktree.is_some());
}

/// And the point of keeping it: the next run continues that conversation.
#[tokio::test(flavor = "multi_thread")]
async fn the_run_after_a_restart_resumes_the_session_from_before_it() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let id = CardId::new("c_again");

    {
        let (handle, _e, _r) = Engine::spawn(
            EngineDeps {
                store: store.clone(),
                clock: Arc::new(FixedClock),
                agent: Arc::new(FakeAgent(FakeMode::Complete)),
                director: Arc::new(FakeAgent(FakeMode::Complete)),
                git: git.clone(),
                approver: None,
                run_log: None,
            },
            test_config(),
            EnginePolicy::default(),
            vec![],
        );
        card_ready(&handle, &id).await;
        let mut human = profile();
        human.reviewer = Reviewer::Human;
        handle.start_run(id.clone(), "work".into(), human).await.unwrap();
        wait_for("the first run finished", async || {
            status_of(&handle, &id).await == Some(Status::Review)
        })
        .await;
    }

    let spy = ResumeSpy::default();
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(ResumeSpy { seen: Arc::clone(&spy.seen) }),
            director: Arc::new(FakeAgent(FakeMode::Complete)),
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy::default(),
        store.read_all().unwrap(),
    );

    // Send it back so it can be worked again, then run it.
    handle
        .execute(Command::RejectCard {
            card_id: id.clone(),
            reason: "another pass".into(),
            by: Actor::Human,
        })
        .await
        .unwrap();
    let mut human = profile();
    human.reviewer = Reviewer::Human;
    handle.start_run(id.clone(), "again".into(), human).await.unwrap();
    wait_for("the second run finished", async || {
        status_of(&handle, &id).await == Some(Status::Review)
    })
    .await;

    let seen = spy.seen();
    assert_eq!(
        seen,
        vec![Some("s1".to_string())],
        "the run after the restart was handed the session from before it"
    );

    // And the newer session replaces the old one on the card.
    let snap = handle.snapshot().await.unwrap();
    let card = snap.cards.iter().find(|c| c.id == id).unwrap();
    assert_eq!(card.session_id.as_deref(), Some("s2"));
}

/// A worktree that cannot be created must not leave a card marked Running with
/// no run behind it: the checkout is resolved before the run is recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_worktree_leaves_the_card_alone() {
    let mut fake_git = FakeGit::default();
    fake_git.fail_worktree = true;
    let store = Arc::new(MemStore::default());
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            director: Arc::new(FakeAgent(FakeMode::Complete)),
            git: Arc::new(fake_git),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy::default(),
        vec![],
    );

    let id = CardId::new("c_nowhere");
    card_ready(&handle, &id).await;
    let before = store.count();
    assert!(handle.start_run(id.clone(), "work".into(), profile()).await.is_err());

    assert_eq!(status_of(&handle, &id).await, Some(Status::Ready));
    assert_eq!(store.count(), before, "nothing was written for a run that never began");
}

/// `create_worktree` removes and recreates, which takes the branch with it. A
/// shared checkout has commits on that branch, so after a restart it has to be
/// adopted rather than rebuilt.
#[tokio::test(flavor = "multi_thread")]
async fn a_shared_worktree_is_adopted_after_a_restart_not_rebuilt() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let mut profile_shared = profile();
    profile_shared.worktree = WorktreeMode::Shared;
    profile_shared.reviewer = Reviewer::Human;

    {
        let (handle, _e, _r) = Engine::spawn(
            EngineDeps {
                store: store.clone(),
                clock: Arc::new(FixedClock),
                agent: Arc::new(FakeAgent(FakeMode::Complete)),
                director: Arc::new(FakeAgent(FakeMode::Complete)),
                git: git.clone(),
                approver: None,
                run_log: None,
            },
            test_config(),
            EnginePolicy::default(),
            vec![],
        );
        let first = CardId::new("c_shared_1");
        card_ready(&handle, &first).await;
        handle
            .start_run(first.clone(), "work".into(), profile_shared.clone())
            .await
            .unwrap();
        wait_for("the first shared run finished", async || {
            status_of(&handle, &first).await == Some(Status::Review)
        })
        .await;
    }
    assert_eq!(
        git.calls().iter().filter(|c| c.starts_with("create:")).count(),
        1,
        "the checkout was created once"
    );

    // A new engine over the same log, then another shared run.
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            director: Arc::new(FakeAgent(FakeMode::Complete)),
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy::default(),
        store.read_all().unwrap(),
    );
    let second = CardId::new("c_shared_2");
    card_ready(&handle, &second).await;
    handle
        .start_run(second.clone(), "more".into(), profile_shared)
        .await
        .unwrap();
    wait_for("the second shared run finished", async || {
        status_of(&handle, &second).await == Some(Status::Review)
    })
    .await;

    assert_eq!(
        git.calls().iter().filter(|c| c.starts_with("create:")).count(),
        1,
        "and never again: rebuilding it would delete the branch its commits are on"
    );
}
