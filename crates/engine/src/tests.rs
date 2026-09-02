use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use harness_domain::{Actor, Board, CardId, Command, Event, RunOutcome as DomainOutcome, Status};
use harness_ports::{
    AgentPort, ApprovalRequest, ClockPort, GitError, GitPort, Grants, Reviewer, RunEvent, RunLogLine,
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

/// Um relógio que o teste move à mão. Para o que precisa de ver tempo a passar
/// sem esperar por ele.
#[derive(Default)]
struct SettableClock(std::sync::atomic::AtomicU64);

impl SettableClock {
    fn set(&self, ms: u64) {
        self.0.store(ms, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ClockPort for SettableClock {
    fn now_millis(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

enum FakeMode {
    Complete,
    WaitCancelled,
    NeedsApproval,
}

/// O revisor, agora do lado de fora.
///
/// O engine deixou de rever: pede, e quem responde é o shell — na app, uma
/// vez na conversa do Director. Aqui é um gancho que aprova, recusa ou não
/// pega, e que precisa do `EngineHandle` para responder. Daí o `OnceLock`: o
/// gancho entra na construção do engine e a resposta só é possível depois
/// dela, que é exactamente a ordem que a app também tem.
#[derive(Clone, Copy, PartialEq)]
enum ReviewMode {
    Approve,
    Reject,
    /// Ninguém pegou. O cartão fica em Review à espera do operador.
    Refuse,
    /// Não devia ser chamado de todo; se for, o teste vê-o em `seen`.
    Unused,
}

#[derive(Clone, Default)]
struct Reviews(Arc<Mutex<Vec<harness_ports::ReviewRequest>>>);

impl Reviews {
    fn seen(&self) -> Vec<harness_ports::ReviewRequest> {
        self.0.lock().unwrap().clone()
    }
}

fn fake_reviewer(
    mode: ReviewMode,
    handle: Arc<std::sync::OnceLock<EngineHandle>>,
    seen: Reviews,
) -> harness_ports::ReviewHook {
    Arc::new(move |request: harness_ports::ReviewRequest| {
        seen.0.lock().unwrap().push(request.clone());
        let handle = Arc::clone(&handle);
        Box::pin(async move {
            let approving = match mode {
                ReviewMode::Approve => true,
                ReviewMode::Reject => false,
                ReviewMode::Refuse | ReviewMode::Unused => return false,
            };
            let Some(engine) = handle.get().cloned() else {
                return false;
            };
            let card_id = CardId::new(request.card_id);
            let cmd = if approving {
                Command::ApproveCard {
                    card_id,
                    by: Actor::Director,
                    reason: "fine".into(),
                    hunks: Vec::new(),
                }
            } else {
                Command::RejectCard {
                    card_id,
                    reason: "not good enough".into(),
                    by: Actor::Director,
                    hunks: Vec::new(),
                }
            };
            engine.execute(cmd).await.is_ok()
        })
    })
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
                let _ = tx.send(RunEvent::Text { text: "working".into(), parent_tool_use_id: None }).await;
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
                    None => harness_ports::ApprovalOutcome::Unanswered,
                };
                let allowed = allowed.allowed();
                let _ = tx
                    .send(RunEvent::Text {
                        text: format!("allowed={allowed}"),
                        parent_tool_use_id: None,
                    })
                    .await;
                drop(tx);
                Ok(RunOutcome::completed(None, Some(0.0)))
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
    /// When set, create_worktree takes this long — long enough that another
    /// message lands mid-start, which is the window these tests are about.
    create_delay_ms: u64,
    /// Uma integração que recusa, como a de um ramo que conflitua com a base.
    merge_fails: Option<String>,
}

impl FakeGit {
    fn new() -> Self {
        Self {
            root: unique_worktree_root(),
            calls: Mutex::new(Vec::new()),
            commit_fails: None,
            fail_worktree: false,
            order: None,
            create_delay_ms: 0,
            merge_fails: None,
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
            create_delay_ms: 0,
            merge_fails: None,
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

    fn merge_card(&self, card_id: &str, base: &str) -> Result<Option<String>, GitError> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("merge:{card_id}->{base}"));
        if let Some(reason) = &self.merge_fails {
            return Err(GitError::Git(reason.clone()));
        }
        Ok(Some(format!("merge-sha-{card_id}")))
    }

    fn create_worktree(&self, card_id: &str, _base: &str) -> Result<WorktreePath, GitError> {
        self.calls.lock().unwrap().push(format!("create:{card_id}"));
        if self.fail_worktree {
            return Err(GitError::Git("worktree is locked".into()));
        }
        if self.create_delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(self.create_delay_ms));
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

    fn remove_worktree(&self, wt: &WorktreePath) -> Result<(), GitError> {
        let name = wt
            .0
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        self.calls.lock().unwrap().push(format!("remove:{name}"));
        Ok(())
    }

    fn diff_summary(&self, _wt: &WorktreePath, _base: &str) -> Result<String, GitError> {
        Ok("diff --git a/eggs.md b/eggs.md\n+egg content".to_string())
    }

    fn diff_numstat(&self, _wt: &WorktreePath, _base: &str) -> Result<(u64, u64), GitError> {
        Ok((1, 0))
    }
}

/// Poll until the engine has caught up, or give up loudly.
///
/// The budget is deliberately far longer than any of these tests needs. It is
/// only ever spent when something is actually broken: a passing check returns
/// on the first poll, so a generous ceiling costs a healthy suite nothing. The
/// previous six seconds were enough on a developer machine and not enough on a
/// loaded CI runner, where thirty-odd async tests share whatever cores the
/// hosted runner feels like giving them — which turns a real failure signal
/// into a coin flip.
/// O prazo é de relógio de parede, portanto mede a máquina e não o trabalho —
/// e já custou várias falhas, sempre neste ficheiro.
///
/// **O reprodutor.** Acrescentar `worker_threads = 2` aos atributos
/// `#[tokio::test(flavor = "multi_thread")]` deste ficheiro fazia o
/// `a_failed_run_leaves_work_and_the_next_run_finds_it` falhar aos 30s. Fica
/// documentado, não ligado: um `worker_threads = 2` comitado põe o CI a falhar
/// em todos os releases.
///
/// **A leitura antiga estava errada.** Dizia-se aqui que havia duas worker
/// threads presas em trabalho bloqueante e que faltava uma terceira para haver
/// progresso. Não havia. A prova é o próprio `wait_for`: ele só entra em
/// `panic` depois de `check().await` **voltar** — se o actor estivesse preso, a
/// sondagem nunca voltava e o teste pendurava para sempre em vez de estourar
/// aos 30,0s. Instrumentado, o actor respondeu a 93 sondagens em 2s enquanto
/// "não progredia".
///
/// **O que era.** O teste esperava por `active_runs()` não vazio — um estado
/// **transitório**. O `WritesThenFailsAgent` morre no primeiro turno, portanto
/// com poucas workers o segundo run nascia e acabava antes da primeira
/// sondagem; a partir daí a lista nunca mais enchia e o prazo era gasto à
/// espera de algo que já tinha acontecido. Mais threads não curavam nada:
/// só faziam a sondagem chegar a tempo de apanhar a janela.
///
/// A regra que fica: **sondar factos que só acumulam** (um evento no log, um
/// estado terminal), nunca uma janela que fecha sozinha. Subir o prazo continua
/// a ser a correcção errada — só faria a falha demorar mais a aparecer.
const WAIT_BUDGET: Duration = Duration::from_secs(30);
const WAIT_POLL: Duration = Duration::from_millis(20);

async fn wait_for(label: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = std::time::Instant::now() + WAIT_BUDGET;
    loop {
        if check().await {
            return;
        }
        if std::time::Instant::now() >= deadline {
            panic!("timeout after {WAIT_BUDGET:?}: {label}");
        }
        tokio::time::sleep(WAIT_POLL).await;
    }
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

/// An agent that reports its work through the harness tool Relay handed the
/// run — twice, with different summaries. Proves the tool reaches the engine,
/// that both land in the event log, and that the *last* wins at commit time.
struct ReportingAgent;

impl AgentPort for ReportingAgent {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        _cancel: CancellationToken,
    ) -> PinBox<Result<RunOutcome, String>> {
        Box::pin(async move {
            if let Some(report) = &spec.tools {
                let call = |summary: &str, notes: serde_json::Value| harness_ports::ToolCall {
                    name: "report_work".into(),
                    input: serde_json::json!({ "summary": summary, "memory_notes": notes }),
                };
                let _ = report(call("first draft", serde_json::json!(["note A"]))).await;
                let _ = report(
                    call(
                        "final account of the retry fix",
                        serde_json::json!(["note A", "note B"]),
                    ),
                )
                .await;
            }
            let _ = tx.send(RunEvent::Text { text: "done".into(), parent_tool_use_id: None }).await;
            drop(tx);
            Ok(RunOutcome::Completed {
                session_id: Some("s9".into()),
                cost_usd: Some(0.01),
                turns: Some(2),
            })
        })
    }
}

fn test_config() -> EngineConfig {
    EngineConfig::new("proj-test", std::env::temp_dir())
}

fn profile() -> RunProfile {
    RunProfile {
        provider: None,
        backend: harness_ports::Backend::Claude,
        agent_id: "builder".into(),
        model: None,
        allowed_tools: None,
        permission_mode: None,
        max_budget_usd: None,
        worktree: WorktreeMode::PerCard,
        reviewer: Reviewer::Director,
        max_concurrent: 1,
        grants: Grants::default(),
        output_style: None,
    }
}

/// Um agente que regista as concessões com que cada run o chamou, para um
/// teste poder provar que chegaram — e que chegaram as certas.
#[derive(Default)]
struct GrantSpy {
    seen: Arc<Mutex<Vec<Grants>>>,
}

impl AgentPort for GrantSpy {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        _cancel: CancellationToken,
    ) -> PinBox<Result<RunOutcome, String>> {
        self.seen.lock().unwrap().push(spec.grants.clone());
        Box::pin(async move {
            drop(tx);
            Ok(RunOutcome::Completed {
                session_id: None,
                cost_usd: Some(0.0),
                turns: Some(1),
            })
        })
    }
}

struct Rig {
    handle: EngineHandle,
    store: Arc<MemStore>,
    git: Arc<FakeGit>,
    events: broadcast::Receiver<Envelope>,
    runs: broadcast::Receiver<RunUpdate>,
    log: Arc<MemRunLog>,
    /// Os pedidos de revisão que o engine fez. O que antes se verificava
    /// olhando para um segundo agente verifica-se agora aqui: o engine pede,
    /// e quem responde está do lado de fora.
    reviews: Reviews,
}

fn rig(worker: FakeMode, review: ReviewMode) -> Rig {
    rig_with(worker, review, None, EnginePolicy::default())
}

fn rig_with(
    worker: FakeMode,
    review: ReviewMode,
    approver: Option<harness_ports::Approver>,
    policy: EnginePolicy,
) -> Rig {
    rig_full(worker, review, approver, policy, FakeGit::new())
}

fn rig_full(
    worker: FakeMode,
    review: ReviewMode,
    approver: Option<harness_ports::Approver>,
    policy: EnginePolicy,
    fake_git: FakeGit,
) -> Rig {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(fake_git);
    let log = Arc::new(MemRunLog::default());
    // O gancho precisa do handle para responder, e o handle só existe depois
    // de o engine arrancar. Mesma ordem que a app tem.
    let later = Arc::new(std::sync::OnceLock::<EngineHandle>::new());
    let reviews = Reviews::default();
    let (handle, events, runs) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(worker)),
            git: git.clone(),
            approver,
            review: Some(fake_reviewer(review, Arc::clone(&later), reviews.clone())),
            message: None,
            run_log: Some(log.clone()),
        },
        test_config(),
        policy,
        vec![],
    );
    let _ = later.set(handle.clone());
    Rig {
        handle,
        store,
        git,
        events,
        runs,
        log,
        reviews,
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
    let mut r = rig(FakeMode::Complete, ReviewMode::Unused);
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
    let r = rig(FakeMode::Complete, ReviewMode::Unused);
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
        ReviewMode::Unused,
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
    let r = rig(FakeMode::WaitCancelled, ReviewMode::Unused);
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
    let r = rig(FakeMode::WaitCancelled, ReviewMode::Unused);
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
            review: None,
            message: None,
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
            review: None,
            message: None,
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
                review: None,
                message: None,
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
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy::default(),
        history,
    );

    // The crashed run left a dirty worktree behind. Recovery commits it the
    // way `shutdown` commits a cancelled run, and because there is now a diff
    // nobody has read, the card lands in Review instead of a Ready that would
    // hide it forever.
    wait_for("recovery commits the wip and sends the card to review", async || {
        status_of(&handle, &id).await == Some(Status::Review)
    })
    .await;
    assert!(
        git.calls().iter().any(|c| c == "wip"),
        "recovery should commit whatever the crashed run left behind"
    );
    let events = store.events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::RunFinished {
                outcome: DomainOutcome::Failed,
                ..
            }
        )),
        "the run itself is still recorded as failed"
    );
    assert!(
        matches!(
            events.last(),
            Some(Event::CardOverridden {
                to: Status::Review,
                ..
            })
        ),
        "the override to review is the last thing recovery does"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_run_recovery_honours_commit_wip_on_close() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::new());
    let id = CardId::new("c5b");
    let policy = EnginePolicy {
        director_reviews_first: true,
        commit_wip_on_close: false,
    };

    {
        let (handle, _e, _r) = Engine::spawn(
            EngineDeps {
                store: store.clone(),
                clock: Arc::new(FixedClock),
                agent: Arc::new(FakeAgent(FakeMode::WaitCancelled)),
                review: None,
                message: None,
                git: git.clone(),
                approver: None,
                run_log: None,
            },
            test_config(),
            policy.clone(),
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
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        policy,
        history,
    );

    // With the policy off, recovery must not commit anything — the same
    // promise `commit_wip_on_close = false` already makes for a graceful
    // shutdown.
    wait_for("recovery marks the run failed without committing", async || {
        status_of(&handle, &id).await == Some(Status::Ready)
    })
    .await;
    assert!(
        !git.calls().iter().any(|c| c == "wip"),
        "commit_wip_on_close = false must skip the recovery commit too"
    );
}

async fn driven_to_review(worker: FakeMode, review: ReviewMode) -> (Rig, CardId) {
    let r = rig(worker, review);
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
    let (r, id) = driven_to_review(FakeMode::Complete, ReviewMode::Approve).await;
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

/// The whole cycle, in one test, because the pieces of it were only ever
/// checked apart: the commit here, the cost there, the verdict somewhere else.
///
/// Three failures have been found by looking at the screen rather than by a
/// test — work that evaporates, cost that does not add up, state that does not
/// transition — and each one is a step of this cycle. Asserting the steps
/// separately cannot catch them, because each was a seam between two steps that
/// were individually fine. So this walks one card the whole way and checks what
/// it left behind at every stage.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_goes_from_ready_to_done_and_leaves_its_work_behind() {
    let r = rig(FakeMode::Complete, ReviewMode::Approve);
    let id = CardId::new("c_cycle");
    card_ready(&r.handle, &id).await;

    let run_id = r
        .handle
        .start_run(id.clone(), "build the thing".into(), profile())
        .await
        .unwrap();

    wait_for("card reaches done", async || {
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

    // The state transitioned, and through the right places. Asserting only that
    // it ended in Done would pass on a card that teleported there.
    //
    // Most of the walk is not a `CardMoved`: after Ready the status is implied
    // by what happened — a run started, a run finished, a verdict landed — and
    // the board is derived from those. So the chain to check is the events the
    // status is read from, in the order they were written.
    let walk: Vec<&str> = r
        .store
        .events()
        .iter()
        .filter_map(|e| match e {
            Event::CardMoved { card_id, .. } if *card_id == id => Some("moved"),
            Event::RunStarted { card_id, .. } if *card_id == id => Some("run started"),
            Event::RunFinished { card_id, .. } if *card_id == id => Some("run finished"),
            Event::CardApproved { card_id, .. } if *card_id == id => Some("approved"),
            _ => None,
        })
        .collect();
    assert_eq!(
        walk,
        vec!["moved", "run started", "run finished", "approved"],
        "the card did not walk the board in order"
    );
    assert_eq!(card.status, Status::Done);

    // The work exists somewhere it can be found: a checkout was made for this
    // card, and the run was committed into it under the trailers that are how
    // per-card history is read back out of git.
    let calls = r.git.calls();
    assert!(
        calls.iter().any(|c| c == "create:c_cycle"),
        "no checkout was made; calls: {calls:?}"
    );
    let commit = calls
        .iter()
        .find(|c| c.starts_with("commit:"))
        .unwrap_or_else(|| panic!("the run was never committed; calls: {calls:?}"));
    assert!(commit.contains("Harness-Card=c_cycle"), "commit was {commit}");
    assert!(commit.contains("Harness-Run="), "commit was {commit}");
    assert!(commit.contains("Harness-Agent=builder"), "commit was {commit}");

    // The cost landed on the card. A run that bills and does not add up is how
    // a budget ceiling gets passed without anyone being able to see it coming.
    assert_eq!(card.cost_usd, 0.01, "the run's cost never reached the card");
    assert_eq!(card.turns, 7, "the run's turns never reached the card");
    assert_eq!(card.runs, 1);

    // The verdict is on the card, attributed and reasoned — the approval is
    // the point of the cycle, not a status change that happens to be last.
    let review = card.last_review.expect("an approved card carries its verdict");
    assert_eq!(review.by, Actor::Director);
    assert!(review.approved);
    assert_eq!(review.reason, "fine");

    // The transcript survives the run that produced it, which is what makes an
    // unattended run believable afterwards.
    let logged = r.log.read(&run_id.0).unwrap();
    assert!(
        logged.iter().any(|l| matches!(l.event, RunEvent::Text { .. })),
        "the run log kept nothing of what the agent said"
    );
}

/// As concessões chegam aos runs de cartão, e chegam separadas.
///
/// O motor tem um porto só para todos os runs, portanto pendurar as concessões
/// no porto daria a toda a gente as mesmas. É por isso que elas viajam no
/// `RunSpec`: dois cartões entregues a perfis diferentes têm de ver cada um o
/// seu, e nenhum o do outro. Um teste que só verificasse que *alguma coisa*
/// chegou passaria com os dois a receberem tudo, que é exactamente o defeito.
#[tokio::test(flavor = "multi_thread")]
async fn two_cards_with_different_profiles_each_get_only_their_own_grants() {
    let seen: Arc<Mutex<Vec<Grants>>> = Arc::default();
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::new());
    let (handle, _events, _runs) = Engine::spawn(
        EngineDeps {
            store,
            clock: Arc::new(FixedClock),
            agent: Arc::new(GrantSpy { seen: seen.clone() }),
            review: None,
            message: None,
            git,
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
        vec![],
    );

    let designer = RunProfile {
        agent_id: "designer".into(),
        grants: Grants {
            skills_dir: Some(std::path::PathBuf::from("/relay/skills/designer")),
            mcp_servers: Vec::new(),
        },
        max_concurrent: 4,
        ..profile()
    };
    let builder = RunProfile {
        agent_id: "builder".into(),
        grants: Grants {
            skills_dir: Some(std::path::PathBuf::from("/relay/skills/builder")),
            mcp_servers: Vec::new(),
        },
        max_concurrent: 4,
        ..profile()
    };

    let one = CardId::new("c_grant_a");
    card_ready(&handle, &one).await;
    handle
        .start_run(one, "desenhar".into(), designer)
        .await
        .unwrap();

    let two = CardId::new("c_grant_b");
    card_ready(&handle, &two).await;
    handle
        .start_run(two, "construir".into(), builder)
        .await
        .unwrap();

    wait_for("os dois runs foram chamados", async || {
        seen.lock().unwrap().len() == 2
    })
    .await;

    let calls = seen.lock().unwrap().clone();
    let dirs: Vec<String> = calls
        .iter()
        .map(|g| {
            g.skills_dir
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "(nenhuma)".into())
        })
        .collect();

    assert!(
        dirs.contains(&"/relay/skills/designer".to_string()),
        "o designer não recebeu as suas: {dirs:?}"
    );
    assert!(
        dirs.contains(&"/relay/skills/builder".to_string()),
        "o builder não recebeu as suas: {dirs:?}"
    );
    // O que prova a separação: cada chamada leva uma só, não as duas.
    assert_eq!(dirs.len(), 2, "esperavam-se dois runs: {dirs:?}");
    assert_ne!(dirs[0], dirs[1], "os dois runs levaram a mesma concessão");
}

#[tokio::test(flavor = "multi_thread")]
async fn director_rejection_sends_card_back_to_ready_with_a_reason() {
    let (r, id) = driven_to_review(FakeMode::Complete, ReviewMode::Reject).await;
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

/// Ninguém pegou na revisão — não há Director com quem falar, ou a conversa
/// recusou-a. O cartão fica em Review à espera do operador, que é a única
/// saída honesta: melhor um cartão parado do que um cartão movido por algo
/// que o operador não viu acontecer.
#[tokio::test(flavor = "multi_thread")]
async fn a_review_nobody_takes_leaves_the_card_in_review() {
    let (r, id) = driven_to_review(FakeMode::Complete, ReviewMode::Refuse).await;
    wait_for("card reaches review", async || {
        status_of(&r.handle, &id).await == Some(Status::Review)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(status_of(&r.handle, &id).await, Some(Status::Review));
}

/// Dizer alguma coisa a um agente que já está a trabalhar.
///
/// Não interrompe e não começa um segundo run: entra na fila daquele run, e o
/// modelo lê-a na leitura seguinte. O que se prende aqui são as duas recusas,
/// porque são elas que impedem uma mensagem de desaparecer em silêncio — sem
/// run não há caixa onde a pôr, e um texto vazio não é uma mensagem.
#[tokio::test(flavor = "multi_thread")]
async fn a_working_agent_can_be_told_something_midway() {
    let r = rig(FakeMode::WaitCancelled, ReviewMode::Unused);
    let id = CardId::new("c_msg");
    card_ready(&r.handle, &id).await;
    r.handle
        .start_run(id.clone(), "work".into(), profile())
        .await
        .unwrap();
    wait_for("run registers as active", async || {
        !r.handle.active_runs().await.unwrap().is_empty()
    })
    .await;

    let queued = r
        .handle
        .message_run(id.clone(), "stop — the card said the wrong thing".into())
        .await;
    assert!(queued.is_ok(), "a live run takes a message: {queued:?}");

    assert!(
        r.handle
            .message_run(id.clone(), "   ".into())
            .await
            .is_err(),
        "an empty message is not a message"
    );
    assert!(
        r.handle
            .message_run(CardId::new("c_nothing"), "hello".into())
            .await
            .is_err(),
        "a card with nothing running has no inbox to put it in"
    );
}

/// A run is not what `self.runs` says; it is what the board says.
///
/// `execute()` moves a card off `Running` for every command that touches
/// it — `OverrideCard` among them — without ever touching `self.runs`. So a
/// card forced out of Running by hand, the way an operator overriding a
/// stuck card actually does it, left its run entry exactly as it was:
/// present, and a naive guard would answer as if a live run were still
/// listening. This is that guard's regression test: the agent is still
/// genuinely running (`WaitCancelled` never returns on its own), the board
/// has already moved the card to Ready, and the message must be refused —
/// naming the state, not the generic "nothing is running".
#[tokio::test(flavor = "multi_thread")]
async fn a_message_to_a_card_the_board_already_left_is_refused() {
    let r = rig(FakeMode::WaitCancelled, ReviewMode::Unused);
    let id = CardId::new("c_stale");
    card_ready(&r.handle, &id).await;
    r.handle
        .start_run(id.clone(), "work".into(), profile())
        .await
        .unwrap();
    wait_for("run registers as active", async || {
        !r.handle.active_runs().await.unwrap().is_empty()
    })
    .await;

    // Not a cancel: an override, the same move the operator makes from the
    // board when a card looks stuck. The task behind it is untouched — still
    // genuinely running — and only the board's status changes.
    r.handle
        .execute(Command::OverrideCard {
            card_id: id.clone(),
            to: Status::Ready,
            reason: "operator moved it by hand".into(),
        })
        .await
        .unwrap();
    assert_eq!(status_of(&r.handle, &id).await, Some(Status::Ready));

    let sent = r.handle.message_run(id.clone(), "hello?".into()).await;
    let err = sent.expect_err("the board says nothing is running; a message must not be accepted");
    assert!(err.contains(&id.to_string()), "names the card: {err}");
    assert!(err.contains("Ready"), "names the state it actually found: {err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reviewerless_agent_closes_its_own_card() {
    let r = rig(FakeMode::Complete, ReviewMode::Unused);
    let id = CardId::new("c7");
    card_ready(&r.handle, &id).await;
    let mut p = profile();
    p.reviewer = Reviewer::Nobody;
    r.handle.start_run(id.clone(), "work".into(), p).await.unwrap();

    wait_for("card closes without a review", async || {
        status_of(&r.handle, &id).await == Some(Status::Done)
    })
    .await;

    // Who closed it is load-bearing outside the engine: the Director is told
    // about verdicts his own reviewer reached, and `nobody` is the operator's
    // brake — it must not read as a judgement of his. The actor is what makes
    // that distinction, so it is asserted rather than assumed.
    let card = r
        .handle
        .snapshot()
        .await
        .unwrap()
        .cards
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    let review = card.last_review.expect("closing a card leaves its record");
    assert_eq!(review.by, Actor::Human, "no reviewer is not the Director");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_human_reviewer_keeps_the_card_in_review() {
    let r = rig(FakeMode::Complete, ReviewMode::Approve);
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
    // O revisor deste rig aprovaria; o que prende a regra é ele nunca ter sido
    // chamado. Sem isto, um `dispatch_review` que pedisse na mesma e ignorasse
    // a resposta passava o teste — e o espião existia sem ninguém o ler.
    assert!(
        r.reviews.seen().is_empty(),
        "reviewer: you não pede revisão a ninguém"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_agent_runs_in_the_checkout_and_never_commits() {
    let r = rig(FakeMode::Complete, ReviewMode::Unused);
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
        ReviewMode::Unused,
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
            if req.tool == "Bash" {
                harness_ports::ApprovalOutcome::Allowed
            } else {
                harness_ports::ApprovalOutcome::Denied
            }
        })
    });

    let r = rig_with(
        FakeMode::NeedsApproval,
        ReviewMode::Unused,
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
    let r = rig(FakeMode::WaitCancelled, ReviewMode::Unused);
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
    let r = rig(FakeMode::WaitCancelled, ReviewMode::Unused);
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
        // The reviewer would approve if it were ever asked; it must not be.
        ReviewMode::Approve,
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
            review: None,
            message: None,
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
                review: None,
                message: None,
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
            review: None,
            message: None,
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
                review: None,
                message: None,
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
            review: None,
            message: None,
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
            hunks: Vec::new(),
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

/// `git log` is history: the subject of a finished run's commit is the card's
/// title, not an id. The ids ride in the body and the trailers, where machines
/// — and the Code screen — look.
#[tokio::test(flavor = "multi_thread")]
async fn the_commit_subject_is_the_card_title() {
    let r = rig_with(
        FakeMode::Complete,
        ReviewMode::Unused,
        None,
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
    );
    let id = CardId::new("c_msg");
    r.handle
        .execute(Command::CreateCard {
            card_id: id.clone(),
            title: "Fix the retry loop".into(),
        })
        .await
        .unwrap();
    r.handle
        .execute(Command::MoveCard {
            card_id: id.clone(),
            to: Status::Ready,
        })
        .await
        .unwrap();

    r.handle.start_run(id.clone(), "work".into(), profile()).await.unwrap();
    wait_for("card reaches review", async || {
        status_of(&r.handle, &id).await == Some(Status::Review)
    })
    .await;

    let commit = r
        .git
        .calls()
        .into_iter()
        .find(|c| c.starts_with("commit:"))
        .expect("a commit was made");
    assert!(
        commit.contains("commit:harness: Fix the retry loop"),
        "subject reads like history, got {commit}"
    );
    assert!(commit.contains("Harness-Card=c_msg"), "trailers survive: {commit}");
}

/// The happy path of report_work: both calls reach the log, the commit body
/// carries the agent's *last* summary (refined beats accumulated), and the
/// transcript says a report arrived.
#[tokio::test(flavor = "multi_thread")]
async fn a_reported_summary_becomes_the_commit_body_and_the_last_one_wins() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let (handle, _e, mut runs) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(ReportingAgent),
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
        vec![],
    );

    let id = CardId::new("c_report");
    card_ready(&handle, &id).await;
    handle
        .start_run(id.clone(), "work".into(), profile())
        .await
        .unwrap();
    wait_for("card reaches review", async || {
        status_of(&handle, &id).await == Some(Status::Review)
    })
    .await;

    // The commit body is the agent's final account, under the title subject.
    let commit = git
        .calls()
        .into_iter()
        .find(|c| c.starts_with("commit:"))
        .expect("a commit was made");
    assert!(commit.contains("harness: t"), "subject from the board: {commit}");
    assert!(
        commit.contains("final account of the retry fix"),
        "summary became the body: {commit}"
    );
    assert!(
        !commit.contains("first draft"),
        "the last report wins, nothing accumulates: {commit}"
    );
    assert!(commit.contains("Harness-Card=c_report"), "trailers intact");

    // Both reports are durable events; notes ride along, trimmed.
    let events = store.events();
    let reported: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::WorkReported { .. }))
        .collect();
    assert_eq!(reported.len(), 2, "both calls are in the log");
    assert!(matches!(
        reported[1],
        Event::WorkReported { summary, notes, .. }
            if summary == "final account of the retry fix"
                && notes == &vec!["note A".to_string(), "note B".to_string()]
    ));

    // And the transcript said so, once per call.
    let mut reported_notices = 0;
    while let Ok(update) = runs.try_recv() {
        if let RunEvent::Notice { text } = update.event {
            if text.contains("reported its work") {
                reported_notices += 1;
            }
        }
    }
    assert_eq!(reported_notices, 2, "each report was named on the stream");
}

/// Silence is normal and safe: no call, generic body, explicit Notice — never
/// a parsed-from-prose pseudo-summary (#41's shape).
#[tokio::test(flavor = "multi_thread")]
async fn silence_still_commits_with_a_generic_body_and_a_notice() {
    let r = rig_with(
        FakeMode::Complete,
        ReviewMode::Unused,
        None,
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
    );
    let id = CardId::new("c_silent");
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

    let commit = r
        .git
        .calls()
        .into_iter()
        .find(|c| c.starts_with("commit:"))
        .expect("the commit happened anyway");
    assert!(commit.contains("harness card c_silent"), "{commit}");
    assert!(
        !commit.contains("Harness-Card=c_silent\n"),
        "no invented body"
    );

    let logged = r.log.read(&run_id.0).unwrap();
    assert!(
        logged
            .iter()
            .any(|l| matches!(&l.event, RunEvent::Notice { text }
                if text.contains("did not report its work"))),
        "the silence is named: {:?}",
        logged.iter().map(|l| &l.event).collect::<Vec<_>>()
    );
    assert!(
        !store_has_reports(&r),
        "nothing was recorded for a run that never reported"
    );
}

fn store_has_reports(r: &Rig) -> bool {
    r.store
        .events()
        .iter()
        .any(|e| matches!(e, Event::WorkReported { .. }))
}

/// A rejected card keeps its report: the log records what the agent said,
/// promotion to memory is somebody else's decision — and it will only ever
/// read cards that reached Done.
#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_card_keeps_its_work_report_in_the_log() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(ReportingAgent),
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
        vec![],
    );

    let id = CardId::new("c_reject");
    card_ready(&handle, &id).await;
    handle
        .start_run(id.clone(), "work".into(), profile())
        .await
        .unwrap();
    wait_for("card reaches review", async || {
        status_of(&handle, &id).await == Some(Status::Review)
    })
    .await;

    handle
        .execute(Command::RejectCard {
            card_id: id.clone(),
            reason: "not good enough".into(),
            by: Actor::Human,
            hunks: Vec::new(),
        })
        .await
        .unwrap();

    assert_eq!(
        store
            .events()
            .iter()
            .filter(|e| matches!(e, Event::WorkReported { .. }))
            .count(),
        2,
        "the reports survive the rejection"
    );
}

/// The two phases of a start straddle a message boundary. Two dispatches for
/// the same card in that window — double-click, or the Director's tool racing
/// your click — must not both build a worktree: per-card `create_worktree`
/// destroys first, so the second would delete the first's checkout out from
/// under an agent that is about to start.
#[tokio::test(flavor = "multi_thread")]
async fn two_starts_for_one_card_build_exactly_one_worktree() {
    let r = rig(FakeMode::WaitCancelled, ReviewMode::Unused);
    let id = CardId::new("c_race");
    card_ready(&r.handle, &id).await;

    // Both dispatched before either resolves: the actor takes message 1,
    // marks the card as starting and spawns the build; message 2 then hits
    // the marker while the worktree is still being made.
    let first = r.handle.start_run(id.clone(), "one".into(), profile());
    let second = r.handle.start_run(id.clone(), "two".into(), profile());
    let (ra, rb) = tokio::join!(first, second);

    let ok = ra.is_ok() || rb.is_ok();
    assert!(ok, "one of them starts: {ra:?} / {rb:?}");
    let refused = [ra.err(), rb.err()]
        .into_iter()
        .flatten()
        .find(|e| e.contains("under way"))
        .expect("the second dispatch is named as already under way");
    assert!(refused.contains("under way"));

    let creates = r
        .git
        .calls()
        .into_iter()
        .filter(|c| c == &format!("create:{id}"))
        .count();
    assert_eq!(creates, 1, "one checkout built, never two: {creates}");

    // And the winner is genuinely running.
    wait_for("run registers as active", async || {
        !r.handle.active_runs().await.unwrap().is_empty()
    })
    .await;
}

/// The same window over the agent limit: the second dispatch is refused while
/// the first is still building — the limit counts what is starting, not only
/// what runs — so the loser never builds a checkout at all. Nothing to orphan.
#[tokio::test(flavor = "multi_thread")]
async fn the_loser_of_the_agent_limit_never_builds() {
    let r = rig(FakeMode::WaitCancelled, ReviewMode::Unused);
    let a = CardId::new("c_lim_a");
    let b = CardId::new("c_lim_b");
    card_ready(&r.handle, &a).await;
    card_ready(&r.handle, &b).await;

    let ra = r.handle.start_run(a.clone(), "one".into(), profile());
    let rb = r.handle.start_run(b.clone(), "two".into(), profile());
    let (ra, rb) = tokio::join!(ra, rb);

    let (winner, err) = match (ra, rb) {
        (Ok(_), Err(e)) => (&a, e),
        (Err(e), Ok(_)) => (&b, e),
        other => panic!("exactly one wins: {other:?}"),
    };
    assert!(
        err.contains("its limit is 1"),
        "the refusal names the limit: {err}"
    );
    wait_for("winner registers as active", async || {
        r.handle
            .active_runs()
            .await
            .unwrap()
            .iter()
            .any(|run| run.card_id == *winner)
    })
    .await;

    let calls = r.git.calls();
    assert_eq!(
        calls.iter().filter(|c| c.starts_with("create:")).count(),
        1,
        "the loser never built anything: {calls:?}"
    );
}

/// The cleanup path itself: a card discarded *while* its worktree is being
/// built. When the resolution lands, StartRun is refused — and the fresh
/// checkout is removed instead of sitting on disk with no owner.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_discarded_mid_start_leaves_no_checkout_behind() {
    let mut fake = FakeGit::new();
    fake.create_delay_ms = 400; // hold the window open
    let git = Arc::new(fake);
    let store = Arc::new(MemStore::default());
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy::default(),
        vec![],
    );

    let id = CardId::new("c_vanish");
    card_ready(&handle, &id).await;

    // The start takes the slow road; the discard lands inside the window.
    let start = handle.start_run(id.clone(), "work".into(), profile());
    let discard = async {
        tokio::time::sleep(Duration::from_millis(120)).await;
        handle
            .execute(Command::DiscardCard {
                card_id: id.clone(),
                reason: "changed my mind mid-flight".into(),
            })
            .await
    };
    let (started, _) = tokio::join!(start, discard);
    assert!(
        started.is_err(),
        "StartRun is refused for a card that no longer exists"
    );

    let calls = git.calls();
    assert_eq!(
        calls.iter().filter(|c| c.starts_with("create:")).count(),
        1,
        "one checkout was built: {calls:?}"
    );
    // The cleanup is detached, so give it a beat before judging it.
    wait_for("the abandoned checkout is removed", async || {
        git.calls().iter().any(|c| c == "remove:c_vanish")
    })
    .await;
    assert!(git.calls().iter().any(|c| c == &"remove:c_vanish".to_string()));
    assert!(
        !store.events().iter().any(|e| matches!(e, Event::RunStarted { .. })),
        "nothing was recorded for the run that never began"
    );
}

/// Mirror mode, green path: the engine builds after the commit (not the
/// agent), the manifest records the exact SHA that produced it, and only
/// then does anyone review.
#[tokio::test(flavor = "multi_thread")]
async fn a_green_build_writes_a_manifest_with_the_commit_sha() {
    let updates = unique_worktree_root();
    let mut config = test_config();
    config.post_build = Some(crate::BuildSpec {
        program: "node".into(),
        args: vec![
            "-e".into(),
            "require('fs').writeFileSync('built.bin', 'ok')".into(),
        ],
        updates_dir: updates.clone(),
        artifact: "built.bin".into(),
    });
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let later = Arc::new(std::sync::OnceLock::<EngineHandle>::new());
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            git: git.clone(),
            approver: None,
            review: Some(fake_reviewer(
                ReviewMode::Approve,
                Arc::clone(&later),
                Reviews::default(),
            )),
            message: None,
            run_log: None,
        },
        config,
        EnginePolicy::default(),
        vec![],
    );
    let _ = later.set(handle.clone());

    let id = CardId::new("c_build_ok");
    card_ready(&handle, &id).await;
    handle.start_run(id.clone(), "work".into(), profile()).await.unwrap();

    wait_for("build holds and the card closes", async || {
        status_of(&handle, &id).await == Some(Status::Done)
    })
    .await;

    // FakeGit commits answer "deadbeef": the manifest names that SHA.
    let manifest = std::fs::read_to_string(updates.join(id.as_str()).join("manifest.json"))
        .expect("a manifest exists for a green build");
    assert!(manifest.contains("\"commit_sha\":\"deadbeef\""), "{manifest}");
    assert!(manifest.contains("c_build_ok"));

    let _ = std::fs::remove_dir_all(&updates);
}

/// Red path: the compiler's verdict reaches the transcript, the Director is
/// still asked (it reviews the diff, not the binary), and there is **no**
/// artefact — not even a stale one from an earlier green build.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_build_leaves_no_artifact_behind() {
    let updates = unique_worktree_root();
    // A stale manifest sits there from some earlier green build.
    std::fs::create_dir_all(updates.join("c_build_bad")).unwrap();
    std::fs::write(
        updates.join("c_build_bad").join("manifest.json"),
        "{\"stale\":true}",
    )
    .unwrap();

    let mut config = test_config();
    config.post_build = Some(crate::BuildSpec {
        program: "node".into(),
        args: vec!["-e".into(), "process.exit(3)".into()],
        updates_dir: updates.clone(),
        artifact: "built.bin".into(),
    });
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let (handle, _e, mut runs) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        config,
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
        vec![],
    );

    let id = CardId::new("c_build_bad");
    card_ready(&handle, &id).await;
    handle.start_run(id.clone(), "work".into(), profile()).await.unwrap();

    wait_for("card reaches review", async || {
        status_of(&handle, &id).await == Some(Status::Review)
    })
    .await;

    wait_for("the failure is named on the stream", async || {
        let mut saw = false;
        while let Ok(update) = runs.try_recv() {
            if let RunEvent::Notice { text } = update.event {
                if text.contains("build failed") || text.contains("process exited") {
                    saw = true;
                }
            }
        }
        saw
    })
    .await;

    assert!(
        !updates.join(id.as_str()).join("manifest.json").exists(),
        "never an artefact of a failed build"
    );
    let _ = std::fs::remove_dir_all(&updates);
}

/// The c_19a1 incident: run fails on budget with the site written in the
/// worktree; the next run must find that work, not a fresh empty checkout.
/// Adoption replaces destroy-and-recreate when there is anything to lose.
struct WritesThenFailsAgent;

impl AgentPort for WritesThenFailsAgent {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        _cancel: CancellationToken,
    ) -> PinBox<Result<RunOutcome, String>> {
        Box::pin(async move {
            std::fs::create_dir_all(spec.cwd.join("site")).map_err(|e| e.to_string())?;
            std::fs::write(spec.cwd.join("site/feed.xml"), "rss").map_err(|e| e.to_string())?;
            let _ = tx.send(RunEvent::Text { text: "wrote the feed".into(), parent_tool_use_id: None }).await;
            drop(tx);
            Ok(RunOutcome::Failed {
                message: "Reached maximum budget ($0.75)".into(),
                cost_usd: Some(0.766),
                turns: Some(17),
            })
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_run_leaves_work_and_the_next_run_finds_it() {
    let updates_placeholder = ();
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let mut config = test_config();
    let _ = &mut config;
    let _ = updates_placeholder;
    let (handle, _e, runs, id, git2) = {
        let git2 = git.clone();
        let (handle, e, r) = Engine::spawn(
            EngineDeps {
                store: store.clone(),
                clock: Arc::new(FixedClock),
                agent: Arc::new(WritesThenFailsAgent),
                review: None,
                message: None,
                git: git.clone(),
                approver: None,
                run_log: None,
            },
            test_config(),
            EnginePolicy {
                director_reviews_first: false,
                commit_wip_on_close: true,
            },
            vec![],
        );
        (handle, e, r, CardId::new("c_keep"), git2)
    };
    drop(runs);

    card_ready(&handle, &id).await;

    // Run 1: writes the work, then dies on budget.
    let mut failing = profile();
    failing.reviewer = Reviewer::Human;
    handle
        .start_run(id.clone(), "one".into(), failing)
        .await
        .unwrap();
    wait_for("failed run returns the card to ready", async || {
        status_of(&handle, &id).await == Some(Status::Ready)
    })
    .await;

    // The wip commit kept the files on disk; the card kept the spend.
    let card = handle
        .snapshot()
        .await
        .unwrap()
        .cards
        .into_iter()
        .find(|c| c.id == id)
        .unwrap();
    assert_eq!(card.cost_usd, 0.766, "a failed run still spent it");
    assert_eq!(card.turns, 17);
    assert!(card.budget_paused, "the pause flag is up");

    // Pins the exact wording Claude Code uses today: if they rephrase the
    // error, this fails loudly instead of the pause regressing silently.
    assert!(
        store
            .events()
            .iter()
            .any(|e| matches!(e, Event::BudgetPauseSet { paused: true, .. })),
        "the cut was recognised and paused"
    );
    let worktree = card.worktree.clone().expect("a worktree was recorded");
    assert!(
        std::path::Path::new(&worktree).join("site/feed.xml").is_file(),
        "run 1's work survived the budget cut"
    );

    // Run 2: still under the old ceiling → refused as paused. The operator
    // raises the budget; the next Start clears the flag and proceeds.
    let mut human = profile();
    human.reviewer = Reviewer::Human;
    let refused = handle
        .start_run(id.clone(), "two".into(), human.clone())
        .await
        .unwrap_err();
    assert!(
        refused.contains("budget ceiling"),
        "the refusal explains the pause: {refused}"
    );

    human.max_budget_usd = Some(1.0); // clears what run 1 spent
    let second = handle.start_run(id.clone(), "two".into(), human).await.unwrap();

    // Esperar por `active_runs()` era esperar por um estado **transitório**: o
    // `WritesThenFailsAgent` morre no primeiro turno, portanto o segundo run
    // podia nascer e acabar antes da primeira sondagem, e a partir daí a lista
    // de activos nunca mais voltava a encher. O `wait_for` gastava então os 30s
    // inteiros à espera de algo que já tinha acontecido.
    //
    // O que este teste quer saber é que o segundo run **ficou registado**, e
    // isso é durável: o `RunStarted` dele fica no log para sempre. Espera-se
    // por um facto que só pode acumular, nunca por uma janela que fecha.
    wait_for("o segundo run fica registado", async || {
        store.events().iter().any(
            |e| matches!(e, Event::RunStarted { run_id, .. } if run_id == &second),
        )
    })
    .await;

    assert_eq!(
        git2
            .calls()
            .iter()
            .filter(|c| c.starts_with(&format!("create:{id}")))
            .count(),
        1,
        "the checkout was adopted, never rebuilt"
    );
    assert!(
        std::path::Path::new(&worktree).join("site/feed.xml").is_file(),
        "and it is still there once run 2 is under way"
    );
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
            review: None,
            message: None,
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
                review: None,
                message: None,
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
            review: None,
            message: None,
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

/// A partial rejection through the single writer: the approved work lands, the
/// rejected work is carried on a card of its own, and a restart that replays
/// the stored log alone reaches exactly the same board.
///
/// The rule itself lives in `harness_domain` and is tested there. What this
/// proves is that the writer persists the whole decision as one run of events,
/// so nothing about it depends on the process that took it.
#[tokio::test(flavor = "multi_thread")]
async fn a_partial_rejection_lands_the_rest_and_survives_a_restart() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let (handle, _e, _r) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
        vec![],
    );

    let id = CardId::new("c_split");
    card_ready(&handle, &id).await;
    let mut human = profile();
    human.reviewer = Reviewer::Human;
    handle.start_run(id.clone(), "work".into(), human).await.unwrap();
    wait_for("card reaches review", async || {
        status_of(&handle, &id).await == Some(Status::Review)
    })
    .await;

    // The two blocks the shell would have read out of the worktree.
    let diff = vec![
        harness_domain::HunkRef::new("src/policy.rs", "@@ -14,6 +14,8 @@"),
        harness_domain::HunkRef::new("src/policy.rs", "@@ -40,3 +42,5 @@"),
    ];
    let rest = CardId::new("c_rest");
    for (hunk, approved, reason) in [
        (&diff[0], true, ""),
        (&diff[1], false, "the guard is inverted"),
    ] {
        handle
            .execute(Command::ReviewHunk {
                card_id: id.clone(),
                hunk: hunk.clone(),
                approved,
                by: Actor::Human,
                reason: reason.into(),
                diff: diff.clone(),
                follow_up: rest.clone(),
            })
            .await
            .unwrap();
    }

    let live = handle.snapshot().await.unwrap();
    assert_eq!(
        live.cards.iter().find(|c| c.id == id).map(|c| c.status),
        Some(Status::Done),
    );
    let carried = live
        .cards
        .iter()
        .find(|c| c.id == rest)
        .expect("the rejected block is on a card of its own");
    assert_eq!(carried.status, Status::Ready);
    assert!(carried.title.contains("the guard is inverted"));

    // Nothing but the log: the same board, from a cold start.
    let mut replayed = Board::default();
    for stored in store.read_all().unwrap() {
        replayed.apply_at(&stored.event, stored.ts_ms);
    }
    let from_log: Vec<Card> = replayed.cards().into_iter().cloned().collect();
    assert_eq!(from_log, live.cards);
}

/// Um agente cuja tarefa acaba sem que ninguém a ouça: entra em pânico dentro
/// do `tokio::spawn` que a conduz, portanto o `RunDone` nunca é enviado.
///
/// É a forma real do defeito. As três causas vistas em produção — um pânico, a
/// mensagem perdida, o sidecar morto por baixo — chegam todas aqui: a tarefa
/// terminou e a entrada em `self.runs` ficou.
struct PanicsAgent;

impl AgentPort for PanicsAgent {
    fn run(
        &self,
        _spec: RunSpec,
        _tx: mpsc::Sender<RunEvent>,
        _cancel: CancellationToken,
    ) -> PinBox<Result<RunOutcome, String>> {
        Box::pin(async move { panic!("a tarefa morreu sem reportar nada") })
    }
}

/// Regressão: um cartão ficava com uma execução para sempre.
///
/// O `self.runs` só era limpo pelo `finish_run`, portanto uma tarefa que
/// morresse sem entregar o resultado deixava lá a entrada. A partir daí todo o
/// `start` era recusado com "card already has an active run", os controlos do
/// próprio quadro não a tiravam, e a única saída encontrada na prática foi
/// achar o processo pelo `lsof` do directório de trabalho e mandar-lhe um
/// sinal. O cartão ficava morto sem que nada o dissesse.
#[tokio::test(flavor = "multi_thread")]
async fn a_run_whose_task_died_without_reporting_stops_owning_the_card() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let clock = Arc::new(SettableClock::default());
    clock.set(1_000);

    let (handle, _e, runs) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: clock.clone(),
            agent: Arc::new(PanicsAgent),
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
        vec![],
    );
    drop(runs);

    let id = CardId::new("c_stuck");
    card_ready(&handle, &id).await;

    handle
        .start_run(id.clone(), "one".into(), profile())
        .await
        .expect("o primeiro arranque é aceite");

    // A tarefa morre sem entregar nada. Passa-se o tempo de graça: uma
    // execução que ainda pode estar a reportar-se não é um resto.
    clock.set(1_000 + 60_000);

    // Sonda-se o facto que só acumula: uma vez ceifado, o cartão fica
    // arrancável. Antes desta correcção respondia "card already has an active
    // run" para sempre, e este `wait_for` estouraria aos 30s.
    let started = Arc::new(Mutex::new(false));
    let seen = Arc::clone(&started);
    wait_for("o cartão volta a poder arrancar", async || {
        if *seen.lock().unwrap() {
            return true;
        }
        match handle.start_run(id.clone(), "two".into(), profile()).await {
            Ok(_) => {
                *seen.lock().unwrap() = true;
                true
            }
            Err(_) => false,
        }
    })
    .await;

    // E o cartão não ficou a mentir que estava a correr: a execução perdida foi
    // fechada como falhada, porque falhar em silêncio continua a ser falhar.
    assert!(
        store
            .events()
            .iter()
            .any(|e| matches!(
                e,
                Event::RunFinished { outcome: harness_domain::RunOutcome::Failed, .. }
            )),
        "a execução perdida é registada como falhada, não esquecida",
    );
}

/// Aprovar passou a integrar, e a ordem entre as duas coisas é a correcção.
///
/// Cada worktree é cortada do `base_branch` e nada voltava a fundir, portanto
/// cada cartão começava numa árvore sem o trabalho do anterior. Não é arrumação:
/// um cartão mandado crescer um directório que já existia não o encontrou e
/// reconstruiu o projecto inteiro ao lado. Trabalho bom, inutilizável.
#[tokio::test(flavor = "multi_thread")]
async fn approving_a_card_puts_its_branch_on_the_base_branch() {
    let store = Arc::new(MemStore::default());
    let git = Arc::new(FakeGit::default());
    let (handle, _e, runs) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
        vec![],
    );
    drop(runs);

    let id = CardId::new("c_int");
    card_ready(&handle, &id).await;
    handle
        .start_run(id.clone(), "work".into(), profile())
        .await
        .unwrap();
    wait_for("o cartão chega a revisão", async || {
        status_of(&handle, &id).await == Some(Status::Review)
    })
    .await;

    handle
        .execute(Command::ApproveCard {
            card_id: id.clone(),
            by: Actor::Director,
            reason: "fine".into(),
            hunks: Vec::new(),
        })
        .await
        .expect("aprovar corre");

    assert!(
        git.calls().iter().any(|c| c == "merge:c_int->main"),
        "aprovar tem de integrar; chamadas: {:?}",
        git.calls(),
    );
    assert_eq!(status_of(&handle, &id).await, Some(Status::Done));
}

/// A outra metade, e a que faz a primeira valer alguma coisa: se a integração
/// não corre, a aprovação não fica escrita. Um cartão marcado como feito por
/// cima de trabalho que não aterrou é precisamente a mentira que isto veio
/// tirar do sistema.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_that_cannot_be_merged_is_not_approved() {
    let store = Arc::new(MemStore::default());
    let mut failing = FakeGit::default();
    failing.merge_fails = Some("conflicts with main in:\nsrc/main.rs".into());
    let git = Arc::new(failing);

    let (handle, _e, runs) = Engine::spawn(
        EngineDeps {
            store: store.clone(),
            clock: Arc::new(FixedClock),
            agent: Arc::new(FakeAgent(FakeMode::Complete)),
            review: None,
            message: None,
            git: git.clone(),
            approver: None,
            run_log: None,
        },
        test_config(),
        EnginePolicy {
            director_reviews_first: false,
            commit_wip_on_close: true,
        },
        vec![],
    );
    drop(runs);

    let id = CardId::new("c_conflict");
    card_ready(&handle, &id).await;
    handle
        .start_run(id.clone(), "work".into(), profile())
        .await
        .unwrap();
    wait_for("o cartão chega a revisão", async || {
        status_of(&handle, &id).await == Some(Status::Review)
    })
    .await;

    let refused = handle
        .execute(Command::ApproveCard {
            card_id: id.clone(),
            by: Actor::Director,
            reason: "fine".into(),
            hunks: Vec::new(),
        })
        .await;

    assert!(refused.is_err(), "uma integração que falha recusa a aprovação");
    let said = refused.unwrap_err();
    assert!(
        said.contains("src/main.rs"),
        "e diz onde está o conflito, para haver o que fazer a seguir: {said}",
    );
    assert_eq!(
        status_of(&handle, &id).await,
        Some(Status::Review),
        "o cartão fica onde o operador ainda lhe pega",
    );
}
