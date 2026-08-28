//! The workspace: the registry of projects the operator added, and one engine
//! per project. Everything the Tauri commands need hangs off here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use harness_agent_claude::ClaudeCliAgent;
use harness_agent_sidecar::SidecarAgent;
use harness_engine::{Engine, EngineConfig, EngineDeps, EngineHandle};
use harness_git_cli::{ensure_workspace, CliGit};
use harness_ports::{
    AgentPort, ClockPort, RunEvent, RunLogLine, RunLogPort, RunOutcome, RunSpec, StorePort,
};
use harness_store_jsonl::{JsonlRunLog, JsonlStore};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use harness_app::agents::{self, AgentProfile};
use harness_app::approvals::{ApprovalRouter, Notifier, PendingApproval};
use harness_app::conversations::{Conversation, ConversationIndex};
use harness_app::inbox::{InboxState, Proposal};
use harness_app::paths::{self, AppPaths};
use harness_app::director::{CardLine, DiffFacts, ProjectBrief};
use harness_app::projects::{self, FolderInfo, Project};
use harness_app::settings::Settings;

use crate::conversations::ConversationsHandle;
use crate::registry::RegistryHandle;
use crate::sidecar;

/// Bridges approval traffic to the window.
struct WindowNotifier(AppHandle);

impl Notifier for WindowNotifier {
    fn asked(&self, request: &PendingApproval) {
        let _ = self.0.emit("approvals://asked", request);
    }

    fn queue(&self, pending: &[PendingApproval]) {
        let _ = self.0.emit("approvals://pending", pending);
    }
}

pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

/// Picks the sidecar or the command line adapter per run, so the Settings
/// toggle applies immediately instead of at the next restart.
pub struct SwitchingAgent {
    pub sidecar: Arc<dyn AgentPort>,
    pub cli: Arc<dyn AgentPort>,
    pub settings: Arc<Mutex<Settings>>,
}

impl AgentPort for SwitchingAgent {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<RunOutcome, String>> + Send>> {
        let use_sidecar = self.settings.lock().map(|s| s.sidecar).unwrap_or(true);
        if use_sidecar {
            self.sidecar.run(spec, tx, cancel)
        } else {
            self.cli.run(spec, tx, cancel)
        }
    }
}

pub struct ProjectRuntime {
    pub project: Project,
    pub engine: EngineHandle,
    pub git: Arc<CliGit>,
    pub store: Arc<JsonlStore>,
    pub run_log: Arc<JsonlRunLog>,
}

/// The last look at Relay's own repository, kept whole.
///
/// Two readers want different halves of the same finding and neither should
/// rebuild the other's: the Director's prompt wants the sentence, and the
/// window wants the numbers. Keeping both together means the sentence the
/// operator is shown a piece of is the same sentence the Director received,
/// not one written again later against a different clock.
pub struct Workspace {
    app: AppHandle,
    pub paths: AppPaths,
    pub settings: Arc<Mutex<Settings>>,
    pub router: Arc<ApprovalRouter>,
    sidecar_dir: PathBuf,
    /// Quem são os agentes e quais são os projectos. Não está aqui: está dentro
    /// de um actor, e isto é só o telefone (`registry.rs`).
    registry: RegistryHandle,
    runtimes: Mutex<HashMap<String, Arc<ProjectRuntime>>>,
    /// Quais as conversas, que sessão Claude cada uma continua e qual delas tem
    /// um turno no ar. Também não está aqui: é outro actor (`conversations.rs`).
    /// As palavras continuam a viver no `chat_log`, nunca no índice.
    chats: ConversationsHandle,
    /// One transcript per conversation, through the same port every run
    /// transcript already uses.
    chat_log: Arc<JsonlRunLog>,
    /// Improvement proposals waiting on the operator, plus the mark of the
    /// last end-of-day look.
    inbox: Mutex<InboxState>,
    /// What the last look at Relay's own repository found: commits that never
    /// went through a card. Held here so the Director's next turn receives it
    /// rather than having to know to ask — and so the window can come and get
    /// it, which the startup emit cannot promise (see [`Finding`]).
    /// Guards against two shutdown paths starting the daily look twice.
    reflection_running: std::sync::atomic::AtomicBool,
    /// Set by whoever wins the race to close the window, so a second close
    /// press means "stop waiting" rather than a second shutdown sequence.
    closing: std::sync::atomic::AtomicBool,
    /// Cancelled when the operator refuses to wait for the close sequence.
    closing_token: CancellationToken,
}

impl Workspace {
    pub async fn load(app: AppHandle, paths: AppPaths) -> Arc<Self> {
        let mut settings: Settings = paths::read_json_or_default(&paths.settings_file());
        settings.forget_unchosen_accent();
        let settings = Arc::new(Mutex::new(settings));
        let router = Arc::new(ApprovalRouter::new(Arc::clone(&settings)));
        router.attach(Box::new(WindowNotifier(app.clone())));

        let stored_agents: Vec<AgentProfile> = paths::read_json_or_default(&paths.agents_file());
        let agents = agents::normalise(stored_agents);
        let projects: Vec<Project> = paths::read_json_or_default(&paths.projects_file());
        let registry = RegistryHandle::spawn(paths.clone(), agents, projects);
        let conversations: ConversationIndex =
            paths::read_json_or_default(&paths.conversations_file());
        let chats = ConversationsHandle::spawn(app.clone(), paths.clone(), conversations);
        let inbox: InboxState = paths::read_json_or_default(&paths.inbox_file());
        let sidecar_dir = sidecar::prepare(&app, &paths);
        // A missing transcript directory must not stop the app opening: an
        // in-memory conversation is still better than no window.
        let chat_log = Arc::new(
            JsonlRunLog::open(paths.conversations_dir()).unwrap_or_else(|e| {
                eprintln!("could not open the conversation transcripts: {e}");
                JsonlRunLog::open(std::env::temp_dir().join("harness-conversations"))
                    .expect("a writable transcript directory")
            }),
        );

        let expired_file = paths.approvals_expired_file();
        router.attach_expiry_sink(Arc::new(move |ts_ms, pending| {
            // One JSON line per expiry, shaped like `ExpiredApproval` so
            // self_report reads it back without a second format.
            let line = serde_json::json!({
                "ts_ms": ts_ms,
                "project_id": pending.project_id,
                "tool": pending.tool,
                "summary": pending.summary,
            });
            if let Err(e) = (|| -> std::io::Result<()> {
                if let Some(parent) = expired_file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&expired_file)?;
                writeln!(file, "{line}")
            })() {
                eprintln!("could not record the expired approval: {e}");
            }
        }));

        let workspace = Arc::new(Self {
            app,
            paths,
            settings,
            router,
            sidecar_dir,
            registry,
            runtimes: Mutex::new(HashMap::new()),
            chats,
            chat_log,
            inbox: Mutex::new(inbox),
            reflection_running: std::sync::atomic::AtomicBool::new(false),
            closing: std::sync::atomic::AtomicBool::new(false),
            closing_token: CancellationToken::new(),
        });
        // Persist the normalised crew and settings so the files on disk match
        // what we are actually running.
        let _ = workspace.registry.save_agents().await;
        let _ = paths::write_json(&workspace.paths.settings_file(), &workspace.settings());
        workspace.adopt_legacy_workspace().await;
        workspace
    }

    /// Earlier builds kept a single synthetic repository at `<data>/workspace`
    /// with its log at `<data>/events.jsonl`. Adopt it as a normal project so
    /// no history is stranded.
    async fn adopt_legacy_workspace(self: &Arc<Self>) {
        if !self.projects().await.is_empty() {
            return;
        }
        let legacy_repo = self.paths.root().join("workspace");
        let legacy_log = self.paths.root().join("events.jsonl");
        if !legacy_repo.is_dir() {
            return;
        }
        let adopted = self
            .add_project(&legacy_repo.to_string_lossy(), Some("Workspace".into()), true)
            .await;
        let Ok(project) = adopted else { return };
        let target = self.paths.events_file(&project.id);
        if legacy_log.is_file() && !target.exists() {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::rename(&legacy_log, &target).is_err() {
                let _ = std::fs::copy(&legacy_log, &target);
            }
        }
    }

    pub fn sidecar_dir(&self) -> &Path {
        &self.sidecar_dir
    }

    pub fn app_handle(&self) -> AppHandle {
        self.app.clone()
    }

    /// The agent used for conversations, chosen per run so the sidecar toggle
    /// applies without a restart.
    pub fn agent_port(&self) -> Arc<dyn AgentPort> {
        crate::chat::agent_for(self)
    }

    pub fn chat_log(&self) -> &JsonlRunLog {
        self.chat_log.as_ref()
    }

    // ---- settings ----
    //
    // Isto continua atrás de um `Mutex`, e continua de propósito (#87). São
    // leituras síncronas em sítios que não podem esperar: o
    // `AgentPort::run` do `SwitchingAgent` é um método de trait sem `async`
    // (#3), e o guardo do fecho da janela em `lib.rs` tem de decidir o
    // `prevent_close` antes de a função retornar. Um actor obrigaria a uma
    // cópia síncrona ao lado — que é exactamente a segunda fonte de verdade
    // que a migração veio remover.

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn set_settings(&self, next: Settings) -> Result<Settings, String> {
        {
            let mut guard = self.settings.lock().unwrap();
            *guard = next;
        }
        let settings = self.settings();
        paths::write_json(&self.paths.settings_file(), &settings)?;
        let policy = settings.policy();
        for runtime in self.runtimes() {
            let policy = policy.clone();
            let engine = runtime.engine.clone();
            tauri::async_runtime::spawn(async move {
                let _ = engine.set_policy(policy).await;
            });
        }
        Ok(settings)
    }

    // ---- agents ----

    pub async fn agents(&self) -> Vec<AgentProfile> {
        self.registry.agents().await
    }

    /// The profile for an id, falling back to a worker when it is unknown.
    /// Right for assigning work; wrong for a conversation, which must speak as
    /// the profile it says it does — see `agent_exact`.
    pub async fn agent(&self, id: &str) -> Option<AgentProfile> {
        self.registry.agent(id).await
    }

    /// The profile for an id, or nothing.
    pub async fn agent_exact(&self, id: &str) -> Option<AgentProfile> {
        self.registry.agent_exact(id).await
    }

    pub async fn set_agents(&self, next: Vec<AgentProfile>) -> Result<Vec<AgentProfile>, String> {
        self.registry.set_agents(next).await
    }

    /// Add a profile from a template. Templates are a menu: nothing is
    /// installed until this is called.
    pub async fn add_agent_from_template(
        &self,
        template_id: &str,
    ) -> Result<AgentProfile, String> {
        self.registry.add_agent_from_template(template_id).await
    }

    pub async fn duplicate_agent(&self, agent_id: &str) -> Result<AgentProfile, String> {
        self.registry.duplicate_agent(agent_id).await
    }

    /// Remove a profile. Every profile is optional except the Director, which
    /// the review loop needs.
    pub async fn remove_agent(&self, agent_id: &str) -> Result<Vec<AgentProfile>, String> {
        self.registry.remove_agent(agent_id).await
    }


    // ---- conversations ----

    pub async fn conversations(&self, include_archived: bool) -> Vec<Conversation> {
        self.chats.list(include_archived).await
    }

    pub async fn conversation(&self, id: &str) -> Option<Conversation> {
        self.chats.get(id).await
    }

    /// The conversation to reopen when the app starts.
    pub async fn last_conversation(&self) -> Option<Conversation> {
        self.chats.resume_target(agents::DIRECTOR_ID).await
    }

    /// Start a conversation. A fresh row means a fresh native session: there is
    /// nothing to resume, which is what makes New Chat actually new.
    pub async fn new_conversation(
        &self,
        profile_id: Option<String>,
        project_id: Option<String>,
    ) -> Result<Conversation, String> {
        let profile_id = profile_id.unwrap_or_else(|| agents::DIRECTOR_ID.to_string());
        let profile = self
            .agent_exact(&profile_id)
            .await
            .ok_or_else(|| format!("no agent profile called {profile_id}"))?;
        if !profile.chat_enabled {
            return Err(format!("{} is not set up for conversations", profile.name));
        }
        // A pin to a project that is gone would only mislead.
        let project_id = match project_id {
            Some(id) if self.project(&id).await.is_some() => Some(id),
            _ => None,
        };
        let id = format!("chat_{}", uuid::Uuid::new_v4().simple());
        self.chats
            .insert(Conversation::new(
                id,
                profile_id,
                project_id,
                SystemClock.now_millis(),
            ))
            .await
    }

    /// The conversation to talk in right now: the one asked for, the last one
    /// used, or a new one.
    pub async fn open_conversation(
        &self,
        profile_id: Option<String>,
        project_id: Option<String>,
    ) -> Result<Conversation, String> {
        let wanted = profile_id
            .clone()
            .unwrap_or_else(|| agents::DIRECTOR_ID.to_string());
        let existing = self
            .chats
            .resume_target(&wanted)
            .await
            .filter(|c| c.profile_id == wanted);
        match existing {
            Some(found) => {
                self.select_conversation(&found.id).await?;
                Ok(found)
            }
            None => self.new_conversation(profile_id, project_id).await,
        }
    }

    pub async fn select_conversation(&self, id: &str) -> Result<Conversation, String> {
        self.chats.select(id).await
    }

    pub async fn rename_conversation(&self, id: &str, title: &str) -> Result<Conversation, String> {
        self.chats.rename(id, title).await
    }

    pub async fn archive_conversation(
        &self,
        id: &str,
        archived: bool,
    ) -> Result<Conversation, String> {
        self.chats.set_archived(id, archived).await
    }

    /// Forget a conversation and its transcript. Destructive, so the UI asks
    /// first; by the time this runs the decision is made.
    pub async fn delete_conversation(&self, id: &str) -> Result<(), String> {
        let gone = self.chats.remove(id).await?;
        // Ask the log where it put it: the name is sanitised on the way in.
        let file = self.chat_log.path_of(&gone.id);
        if let Err(e) = std::fs::remove_file(&file) {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!("could not delete the transcript {}: {e}", file.display());
            }
        }
        Ok(())
    }

    /// Pin a conversation to a project, or unpin it. This is what decides which
    /// code it can read.
    pub async fn pin_conversation(
        &self,
        id: &str,
        project_id: Option<String>,
    ) -> Result<Conversation, String> {
        let project_id = match project_id {
            Some(p) if self.project(&p).await.is_some() => Some(p),
            _ => None,
        };
        self.chats.pin(id, project_id).await
    }

    pub async fn record_chat_message(
        &self,
        id: &str,
        message: &str,
    ) -> Result<Conversation, String> {
        self.chats.record_message(id, message).await
    }

    /// Save the session the SDK handed back, and tell the window, so the list
    /// stops saying a conversation has never been answered.
    pub async fn record_chat_version(&self, id: &str, version: &str) {
        self.chats.record_version(id, version).await
    }

    pub async fn record_chat_session(&self, id: &str, session_id: &str) {
        self.chats.record_session(id, session_id).await
    }

    pub async fn record_chat_cost(&self, id: &str, cost_usd: Option<f64>) {
        self.chats.record_cost(id, cost_usd).await
    }

    pub async fn record_chat_resume_failure(&self, id: &str) {
        self.chats.record_resume_failure(id).await
    }

    pub fn append_chat_line(&self, conversation_id: &str, line: RunLogLine) {
        if let Err(e) = RunLogPort::append(self.chat_log.as_ref(), conversation_id, &line) {
            eprintln!("could not write the conversation transcript: {e}");
        }
    }

    /// Every board, with the one this conversation can read marked.
    pub async fn project_briefs(
        self: &Arc<Self>,
        active: Option<&str>,
    ) -> Result<Vec<ProjectBrief>, String> {
        let mut briefs = Vec::new();
        for project in self.projects().await {
            if !Path::new(&project.path).is_dir() {
                continue;
            }
            let Ok(runtime) = self.runtime(&project.id).await else {
                continue;
            };
            let snap = runtime.engine.snapshot().await?;
            let base = project.base_branch.clone();
            let git = Arc::clone(&runtime.git);
            let sessions = snap.sessions.clone();

            // A card waiting on the operator is the one they will ask about, so
            // read what its worktree actually holds instead of leaving it to
            // guess.
            let lines = tauri::async_runtime::spawn_blocking(move || {
                snap.cards
                    .iter()
                    .map(|card| {
                        let line = CardLine::from_card(card);
                        // Only a card waiting on the operator: work in flight
                        // changes under us, and the rest is already committed.
                        let worth_reading = card.status == harness_domain::Status::Review;
                        let facts = if worth_reading {
                            sessions
                                .iter()
                                .find(|s| s.card_id == card.id)
                                .map(|s| {
                                    let (files, added, removed) =
                                        git.changed_files(Path::new(&s.worktree), &base);
                                    DiffFacts { files, added, removed }
                                })
                        } else {
                            None
                        };
                        line.with_diff(facts)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(|e| e.to_string())?;

            briefs.push(ProjectBrief {
                id: project.id.clone(),
                name: project.name.clone(),
                path: project.path.clone(),
                active: active == Some(project.id.as_str()),
                cards: lines,
                // Only the open project carries its charter: every board
                // carrying one would bloat every turn for no gain. Preferred
                // home is the project's memory directory; the repository root
                // still counts for hands that already wrote one there.
                charter: if active == Some(project.id.as_str()) {
                    harness_app::memory::charter_between(
                        &self.paths.project_memory_charter(&project.id),
                        &Path::new(&project.path).join("charter.md"),
                    )
                } else {
                    None
                },
            });
        }
        Ok(briefs)
    }

    // ---- projects ----

    pub async fn projects(&self) -> Vec<Project> {
        self.registry.projects().await
    }

    /// Every live runtime, for sweeps that must visit each project once.
    ///
    /// O mapa dos runtimes fica onde está, e não por preguiça (#87): um
    /// `ProjectRuntime` não é estado, é uma mesa de punhos para outros donos —
    /// o `EngineHandle` é ele próprio um actor, e o git, o store e o run log
    /// são portos com sincronização própria. Não há aqui nenhum facto sobre o
    /// quadro que possa divergir de outro, que é a classe de bug que a
    /// premissa protege.
    pub fn runtimes(&self) -> Vec<Arc<ProjectRuntime>> {
        self.runtimes.lock().unwrap().values().cloned().collect()
    }

    pub async fn project(&self, id: &str) -> Option<Project> {
        self.registry.project(id).await
    }

    /// What a folder looks like, so the UI can offer the right next step.
    pub async fn inspect_folder(&self, path: &str) -> FolderInfo {
        let root = PathBuf::from(path.trim());
        let exists = root.is_dir();
        let is_repo = exists && CliGit::is_repo(&root);
        let empty = exists
            && std::fs::read_dir(&root)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
        let canonical = Self::canonical(&root);
        let already = self
            .projects()
            .await
            .into_iter()
            .any(|p| p.path.eq_ignore_ascii_case(&canonical));
        FolderInfo::describe(&canonical, exists, is_repo, empty, already)
    }

    /// Absolute path in the shape an operator recognises: Windows hands back a
    /// verbatim `\\?\C:\...` prefix from `canonicalize`, which we drop.
    fn canonical(root: &Path) -> String {
        let full = root
            .canonicalize()
            .unwrap_or_else(|_| root.to_path_buf())
            .to_string_lossy()
            .to_string();
        full.strip_prefix(r"\\?\").unwrap_or(&full).to_string()
    }

    /// Create a repository from scratch: `<parent>/<name>`, initialised with a
    /// first commit, then registered.
    pub async fn create_project(&self, parent: &str, name: &str) -> Result<Project, String> {
        let clean = name.trim();
        if clean.is_empty() {
            return Err("give the project a name".to_string());
        }
        let parent = PathBuf::from(parent.trim());
        if !parent.is_dir() {
            return Err(format!("{} is not a directory", parent.display()));
        }
        let folder = harness_app::paths::sanitize(clean);
        let root = parent.join(&folder);
        if root.exists() {
            let occupied = std::fs::read_dir(&root)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false);
            if occupied && !CliGit::is_repo(&root) {
                return Err(format!("{} already exists and is not empty", root.display()));
            }
        }
        ensure_workspace(&root).map_err(|e| e.to_string())?;
        self.add_project(&root.to_string_lossy(), Some(clean.to_string()), false)
            .await
    }

    /// Register a git repository. `init` is the operator explicitly agreeing to
    /// run `git init` in a folder that is not a repository yet; without it a
    /// non-empty folder is refused, because turning someone's folder into a
    /// repo is not ours to decide.
    pub async fn add_project(
        &self,
        path: &str,
        name: Option<String>,
        init: bool,
    ) -> Result<Project, String> {
        let root = PathBuf::from(path.trim());
        if !root.is_dir() {
            return Err(format!("{} is not a directory", root.display()));
        }
        let canonical = Self::canonical(&root);
        let root = PathBuf::from(&canonical);

        if let Some(existing) = self
            .projects()
            .await
            .into_iter()
            .find(|p| p.path.eq_ignore_ascii_case(&canonical))
        {
            return Ok(existing);
        }

        if !CliGit::is_repo(&root) {
            let empty = std::fs::read_dir(&root)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
            if !empty && !init {
                return Err(format!(
                    "{} has files but is not a git repository. Initialise one there first, or confirm it here.",
                    root.display()
                ));
            }
            ensure_workspace(&root).map_err(|e| e.to_string())?;
        }

        let display = name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .or_else(|| {
                root.file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "Project".to_string());

        let taken: Vec<String> = self.projects().await.into_iter().map(|p| p.id).collect();
        let id = projects::unique_id(&display, &taken);
        let git = CliGit::new(&root, self.paths.project_worktrees(&id));

        // Without a committer identity every agent commit fails, and it fails
        // late — at the end of a run. Give the repository a local one only when
        // there is nothing to inherit; never touch global config.
        if !git.has_committer_identity() {
            if let Err(e) = git.set_local_identity() {
                eprintln!("could not set a committer identity in {canonical}: {e}");
            }
        }

        let project = Project {
            id,
            glyph: projects::glyph_for(&display),
            tone: projects::TONES[taken.len() % projects::TONES.len()].to_string(),
            base_branch: git.default_branch(),
            name: display,
            path: canonical,
            added_ms: SystemClock.now_millis(),
            paused: false,
            mirror: false,
        };

        let project = self.registry.add_project(project).await?;

        // A charter is written at creation, never invented later: an empty
        // template tells the operator the file exists and who reads it. Only
        // when there is nothing to inherit from either home.
        let charter = self.paths.project_memory_charter(&project.id);
        if !charter.exists()
            && harness_app::memory::charter_for(Path::new(&project.path)).is_none()
        {
            if let Some(parent) = charter.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let template = format!(
                "# {}\n\nWhat this project is, in your words. Rules and taste that every\n\
                 agent working on it should hold. Every run reads this; keep it short.\n",
                project.name
            );
            if let Err(e) = std::fs::write(&charter, template) {
                eprintln!("could not write the starter charter: {e}");
            }
        }

        Ok(project)
    }

    // ---- chat turns ----

    /// A conversation has a turn in flight: remember its cancellation token.
    pub async fn register_chat_turn(&self, conversation_id: &str, token: CancellationToken) {
        self.chats.register_turn(conversation_id, token).await
    }

    /// Take the turn's token out (None if the conversation had none). Taking
    /// it is both how stop finds it and how completion cleans up.
    pub async fn finish_chat_turn(&self, conversation_id: &str) -> Option<CancellationToken> {
        self.chats.finish_turn(conversation_id).await
    }

    pub async fn update_project(&self, project: Project) -> Result<Project, String> {
        self.registry.update_project(project).await
    }

    /// Forget a project. Its event log and worktrees are only deleted when the
    /// operator explicitly asks; the repository itself is never touched.
    pub async fn remove_project(&self, id: &str, delete_data: bool) -> Result<(), String> {
        self.runtimes.lock().unwrap().remove(id);
        self.registry.remove_project(id).await?;
        // A conversation pinned to a project that is gone would point nowhere.
        self.chats.unpin_project(id).await;
        if delete_data {
            let _ = std::fs::remove_dir_all(self.paths.project_dir(id));
            let _ = std::fs::remove_dir_all(self.paths.project_worktrees(id));
        }
        Ok(())
    }

    // ---- engines ----

    pub async fn runtime(&self, project_id: &str) -> Result<Arc<ProjectRuntime>, String> {
        if let Some(existing) = self.runtimes.lock().unwrap().get(project_id) {
            return Ok(Arc::clone(existing));
        }
        let project = self
            .project(project_id)
            .await
            .ok_or_else(|| format!("unknown project {project_id}"))?;
        let runtime = Arc::new(self.spawn_runtime(project).await?);
        // Quem chegar primeiro fica. Duas chamadas ao mesmo projecto frio
        // podem levantar dois engines; devolver sempre o que está no mapa
        // garante que só um deles é usado, e o outro morre com o handle.
        let mut live = self.runtimes.lock().unwrap();
        let slot = live.entry(project_id.to_string()).or_insert(runtime);
        Ok(Arc::clone(slot))
    }

    /// Bring up every registered project so the overview can count work
    /// without the operator visiting each one first.
    pub async fn warm_all(&self) {
        for project in self.projects().await {
            if let Err(e) = self.runtime(&project.id).await {
                eprintln!("could not start project {}: {e}", project.id);
            }
        }
    }

    async fn spawn_runtime(&self, project: Project) -> Result<ProjectRuntime, String> {
        let root = PathBuf::from(&project.path);
        if !root.is_dir() {
            return Err(format!(
                "{} is gone; remove the project or restore the folder",
                project.path
            ));
        }
        let store = Arc::new(
            JsonlStore::open(self.paths.events_file(&project.id)).map_err(|e| e.to_string())?,
        );
        let run_log = Arc::new(
            JsonlRunLog::open(self.paths.runs_dir(&project.id)).map_err(|e| e.to_string())?,
        );
        let git = Arc::new(CliGit::new(
            &root,
            self.paths.project_worktrees(&project.id),
        ));

        let script = sidecar::script_in(&self.sidecar_dir);
        let worker: Arc<dyn AgentPort> = Arc::new(SwitchingAgent {
            sidecar: Arc::new(SidecarAgent::new("node", script.clone())),
            cli: Arc::new(ClaudeCliAgent::new("claude")),
            settings: Arc::clone(&self.settings),
        });
        let director: Arc<dyn AgentPort> = Arc::new(SwitchingAgent {
            sidecar: Arc::new(SidecarAgent::new("node", script)),
            cli: Arc::new(ClaudeCliAgent::new("claude")),
            settings: Arc::clone(&self.settings),
        });

        let settings = self.settings();
        // Uma leitura só do perfil do Director: três idas ao registo dariam
        // três respostas que podiam já não concordar entre si.
        let director_profile = self.agent(agents::DIRECTOR_ID).await;
        let mut config = EngineConfig::new(&project.id, root);
        config.base_branch = project.base_branch.clone();
        config.permission_mode = settings.permission_mode.clone();
        config.director_model = director_profile
            .as_ref()
            .and_then(|d| d.model.clone());
        config.director_provider = director_profile
            .as_ref()
            .and_then(|d| {
                harness_app::providers::find(&settings.providers, &d.provider).cloned()
            })
            .and_then(|p| p.resolve());
        // Mirror mode: this project is the orchestrator itself, so a finished
        // run is followed by an engine-owned build. The artefact waits in
        // appdata; installing it is nobody's decision but the operator's.
        if project.mirror {
            config.post_build = Some(harness_engine::BuildSpec {
                program: "pnpm".into(),
                args: vec!["tauri".into(), "build".into(), "--no-bundle".into()],
                updates_dir: self.paths.updates_dir(),
                artifact: "target/release/relay.exe".into(),
            });
        }
        if let Some(director_profile) = director_profile.as_ref() {
            let tools = director_profile.allowed_tools();
            if !tools.is_empty() {
                config.director_allowed_tools = tools;
            }
        }

        let history = store.read_all().map_err(|e| e.to_string())?;
        let (engine, mut events_rx, mut runs_rx) = Engine::spawn(
            EngineDeps {
                store: store.clone() as Arc<dyn StorePort>,
                clock: Arc::new(SystemClock),
                agent: worker,
                director,
                git: git.clone(),
                approver: Some(self.router.approver_for(&project.id)),
                run_log: Some(run_log.clone() as Arc<dyn RunLogPort>),
            },
            config,
            settings.policy(),
            history,
        );

        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(envelope) = events_rx.recv().await {
                let _ = app.emit("engine://event", &envelope);
            }
        });

        let app = self.app.clone();
        let router = Arc::clone(&self.router);
        tauri::async_runtime::spawn(async move {
            while let Ok(update) = runs_rx.recv().await {
                // The run stream is the only place that knows which card a
                // permission request came from.
                if let RunEvent::ApprovalRequested { request_id, .. } = &update.event {
                    router.attach_card(request_id, update.card_id.as_str());
                }
                let _ = app.emit("engine://run", &update);
            }
        });

        Ok(ProjectRuntime {
            project,
            engine,
            git,
            store,
            run_log,
        })
    }

    // ---- inbox ----

    pub fn inbox(&self) -> InboxState {
        self.inbox.lock().unwrap().clone()
    }

    fn save_inbox(&self, state: &InboxState) {
        if let Err(e) = paths::write_json(&self.paths.inbox_file(), state) {
            eprintln!("could not save the inbox: {e}");
        }
    }

    fn publish_inbox(&self) {
        let _ = self
            .app
            .emit("inbox://proposals", self.inbox.lock().unwrap().proposals.clone());
    }

    /// A proposal from the Director: filed, never acted on. The operator
    /// decides whether it becomes work.
    pub fn propose_improvement(
        &self,
        title: &str,
        observation: &str,
        suggestion: &str,
    ) -> Result<Proposal, String> {
        let id = format!("prp_{}", uuid::Uuid::new_v4().simple());
        use harness_ports::ClockPort;
        let now_ms = SystemClock.now_millis();
        let proposal = {
            let mut guard = self.inbox.lock().unwrap();
            guard.propose(id, now_ms, title, observation, suggestion)
        };
        self.save_inbox(&self.inbox());
        self.publish_inbox();
        Ok(proposal)
    }

    /// Accept a proposal: the card is born in the harness's own project — the
    /// one mirror mode builds — never in whatever is open (#72).
    pub async fn accept_proposal(self: &Arc<Self>, proposal_id: &str) -> Result<Proposal, String> {
        let open = self.inbox().proposals.into_iter().find(|p| {
            p.id == proposal_id && p.status == harness_app::inbox::ProposalStatus::Open
        });
        let Some(proposal) = open else {
            return Err(format!("no open proposal {proposal_id}"));
        };
        let known = self.projects().await;
        let Some(mirror) = projects::mirror_project(&known).cloned() else {
            return Err(
                "the harness repository is not registered as a project (mirror mode), so there \
                 is nowhere to put this card — add it as a project first"
                    .to_string(),
            );
        };
        // The whole proposal, not only its title. The title is the prompt the
        // agent receives, so a card born from a title alone reaches the builder
        // with none of the reasons that motivated it.
        let created = crate::commands::board::create_card_inner(
            self,
            &mirror.id,
            &proposal.as_card_text(),
            agents::DEFAULT_WORKER,
            false,
            true,
        )
        .await?;
        let accepted = {
            let mut guard = self.inbox.lock().unwrap();
            match guard.accept(
                proposal_id,
                &mirror.id,
                created.card_id.as_str(),
            ) {
                Some(p) => p,
                None => return Err(format!("no open proposal {proposal_id}")),
            }
        };
        self.save_inbox(&self.inbox());
        self.publish_inbox();
        Ok(accepted)
    }

    /// The last thing the look at Relay's own repository found, if anything,
    /// as the Director's prompt wants it: one paragraph.
    pub async fn outside_work(&self) -> Option<String> {
        self.registry.outside_work().await.map(|(said, _)| said)
    }

    /// The same finding as the window wants it: numbers, not prose.
    ///
    /// This is what closes the hole the emit could not: `look_for_outside_work`
    /// is spawned in `setup()`, before the webview exists, so a fast git and a
    /// slow window mean the event is emitted to nobody — and a reload loses it
    /// the same way. The `bootstrap` call the window opens with reads this, so
    /// a warning that already existed is on screen either way.
    pub async fn outside_work_warning(&self) -> Option<harness_app::mirror::MirrorWarning> {
        self.registry.outside_work().await.map(|(_, warning)| warning)
    }

    /// Has Relay's own source moved without a card behind it?
    ///
    /// Every commit a card produces carries a `Harness-Card` trailer, so the
    /// whole detection is "commits since last time, minus ours". What they
    /// mean is nobody's to decide here: the finding is handed to the Director
    /// to flag and to the operator to judge (#79 — propose, never create).
    ///
    /// Runs at startup and at the end-of-day close, and **never holds either
    /// up**: git is asked on a blocking thread with a short deadline, and a
    /// slow or broken repository is given up on in silence. A window that
    /// takes longer to close because of a status check is a worse bug than a
    /// missed warning.
    ///
    /// The first run reports nothing — it records where the repository stands
    /// and stops. Dumping a whole history the first time Relay looks would be
    /// noise, not a signal.
    pub async fn look_for_outside_work(self: &Arc<Self>) -> Option<String> {
        /// Long enough for `git log` on a real repository, short enough that
        /// nobody notices it in a shutdown.
        const DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

        let known = self.projects().await;
        let mirror = projects::mirror_project(&known).cloned()?;
        let root = PathBuf::from(&mirror.path);
        if !root.is_dir() {
            return None;
        }
        let watch_file = self.paths.mirror_watch_file();
        let known: harness_app::mirror::Watch = paths::read_json_or_default(&watch_file);
        let worktrees = self.paths.project_worktrees(&mirror.id);

        let looked = tokio::time::timeout(
            DEADLINE,
            tauri::async_runtime::spawn_blocking(move || {
                let git = CliGit::new(&root, worktrees);
                // O que interessa é o que chegou ao remoto, não onde este clone
                // ficou. Sem o fetch, a vigia compara o repositório consigo
                // mesmo e nunca vê nada — e o Director continua a ler o código
                // da versão em que o clone nasceu.
                let head = git.refresh_from_remote().or_else(|| git.head_sha())?;
                // First look, or nothing moved: record and say nothing.
                let Some(base) = harness_app::mirror::base_to_compare(&known, &head) else {
                    return Some((head, Vec::new()));
                };
                // An unknown base (rebased away, garbage collected) is "I
                // cannot tell", not "nothing happened" — re-anchor on the
                // current head rather than reporting silence as calm.
                let commits = git.commits_without_a_card(&base).ok()?;
                Some((
                    head,
                    commits
                        .into_iter()
                        .map(|c| (c.ts_ms, c.files))
                        .collect::<Vec<_>>(),
                ))
            }),
        )
        .await;

        let Ok(Ok(Some((head, commits)))) = looked else {
            // Timed out, panicked, or git had nothing to say. Silence is the
            // whole point: the close does not wait on this.
            return None;
        };

        if let Err(e) = paths::write_json(
            &watch_file,
            &harness_app::mirror::Watch {
                sha: head,
                checked_ms: SystemClock.now_millis(),
            },
        ) {
            eprintln!("could not record where Relay's repository stands: {e}");
        }

        let work = harness_app::mirror::outside_work(&commits)?;
        let said = harness_app::mirror::describe(&work, SystemClock.now_millis());
        // The facts travel beside the sentence rather than inside it: the
        // window states them as numbers, and the sentence stays whole for the
        // Director, who is the one it is written for.
        let warning = harness_app::mirror::MirrorWarning {
            work,
            for_director: harness_app::mirror::FOR_DIRECTOR.to_string(),
        };
        // Recorded before it is announced, never after: a window that hears
        // the event and asks in the same breath must not be told there is
        // nothing to report.
        self.registry
            .set_outside_work(Some(crate::registry::Finding {
                said: said.clone(),
                warning: warning.clone(),
            }))
            .await;
        let _ = self.app.emit("mirror://outside-work", &warning);
        Some(said)
    }

    pub fn dismiss_proposal(&self, proposal_id: &str) -> Result<Proposal, String> {
        let dismissed = {
            let mut guard = self.inbox.lock().unwrap();
            guard
                .dismiss(proposal_id)
                .ok_or_else(|| format!("no open proposal {proposal_id}"))?
        };
        self.save_inbox(&self.inbox());
        self.publish_inbox();
        Ok(dismissed)
    }

    /// The docs of the harness's own repository, when it is registered here.
    pub async fn harness_docs_dir(&self) -> Option<PathBuf> {
        let known = self.projects().await;
        projects::mirror_project(&known)
            .map(|p| PathBuf::from(&p.path).join("docs"))
            .filter(|dir| dir.is_dir())
    }

    /// Everything that happened to every agent, merged and counted. Reads our
    /// own logs only — event logs, run transcripts, chat transcripts, expired
    /// approvals — so the model receives a finished table instead of raw files.
    pub fn collect_self_report(&self, window_days: u32) -> harness_app::selfreport::SelfReport {
        use harness_app::selfreport::ExpiredApproval;
        use harness_ports::{ClockPort, StorePort};

        let now = SystemClock.now_millis();

        // Board events across every live project.
        let mut events: Vec<harness_ports::StoredEvent> = Vec::new();
        for runtime in self.runtimes() {
            if let Ok(history) = StorePort::read_all(runtime.store.as_ref()) {
                events.extend(history);
            }
        }

        // Transcript lines: run logs per project, plus every conversation.
        let mut lines: Vec<harness_ports::RunLogLine> = Vec::new();
        let read_transcripts = |dir: &Path, into: &mut Vec<harness_ports::RunLogLine>| {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                for row in text.lines().filter(|l| !l.trim().is_empty()) {
                    if let Ok(parsed) = serde_json::from_str::<harness_ports::RunLogLine>(row) {
                        into.push(parsed);
                    }
                }
            }
        };
        for runtime in self.runtimes() {
            read_transcripts(&self.paths.runs_dir(&runtime.project.id), &mut lines);
        }
        read_transcripts(&self.paths.conversations_dir(), &mut lines);

        // Expired approvals, one JSON line each.
        let mut expired: Vec<ExpiredApproval> = Vec::new();
        if let Ok(text) = std::fs::read_to_string(self.paths.approvals_expired_file()) {
            for row in text.lines().filter(|l| !l.trim().is_empty()) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(row) {
                    expired.push(ExpiredApproval {
                        ts_ms: value.get("ts_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                        project_id: value
                            .get("project_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        tool: value
                            .get("tool")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        summary: value
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    });
                }
            }
        }

        harness_app::selfreport::aggregate(&events, &lines, &expired, now, window_days)
    }

    /// Mark that the end-of-day look ran (or was skipped) at this moment.
    pub fn mark_daily_look(&self) {
        use harness_ports::ClockPort;
        {
            let mut guard = self.inbox.lock().unwrap();
            guard.last_look_ms = SystemClock.now_millis();
        }
        self.save_inbox(&self.inbox());
    }

    pub fn daily_look_due(&self) -> bool {
        harness_app::inbox::look_due(self.inbox().last_look_ms, {
            use harness_ports::ClockPort;
            SystemClock.now_millis()
        })
    }

    /// One at a time: two shutdown paths can race to start the daily look.
    pub fn claim_daily_look(&self) -> bool {
        self.reflection_running
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
    }

    /// The token the close sequence watches. Cancelling it ends the wait.
    pub fn closing_token(&self) -> CancellationToken {
        self.closing_token.clone()
    }

    /// True for the first caller only: that one runs the close sequence.
    /// Everyone after it is a second press of the close button, which means
    /// the operator is done waiting.
    pub fn begin_closing(&self) -> bool {
        self.closing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
    }

    /// Stop waiting: the close sequence gives up whatever it was doing and the
    /// window goes. Nothing filed is lost — proposals are written when the
    /// tool runs, and a look that did not finish is due again, not marked done.
    pub fn stop_waiting(&self) {
        self.closing_token.cancel();
    }

    pub fn release_daily_look(&self) {
        self.reflection_running
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Cancel every run everywhere and let the worktrees commit.
    pub async fn shutdown(&self) {
        self.router.deny_all();
        for runtime in self.runtimes() {
            let _ = runtime.engine.shutdown().await;
        }
    }
}
