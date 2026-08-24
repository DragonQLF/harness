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
use harness_app::paths::{self, AppPaths};
use harness_app::director::{CardLine, DiffFacts, ProjectBrief};
use harness_app::projects::{self, FolderInfo, Project};
use harness_app::settings::Settings;

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

pub struct Workspace {
    app: AppHandle,
    pub paths: AppPaths,
    pub settings: Arc<Mutex<Settings>>,
    pub router: Arc<ApprovalRouter>,
    sidecar_dir: PathBuf,
    agents: Mutex<Vec<AgentProfile>>,
    projects: Mutex<Vec<Project>>,
    runtimes: Mutex<HashMap<String, Arc<ProjectRuntime>>>,
    /// Which chats exist and which Claude session each continues. The words
    /// themselves live in `chat_log`, never here.
    conversations: Mutex<ConversationIndex>,
    /// One transcript per conversation, through the same port every run
    /// transcript already uses.
    chat_log: Arc<JsonlRunLog>,
    /// The cancellation token of the turn each conversation has in flight.
    /// Without this a chat turn that never emits `done` leaves the operator
    /// without a stop.
    chat_turns: Mutex<HashMap<String, CancellationToken>>,
}

impl Workspace {
    pub fn load(app: AppHandle, paths: AppPaths) -> Arc<Self> {
        let settings: Settings = paths::read_json_or_default(&paths.settings_file());
        let settings = Arc::new(Mutex::new(settings));
        let router = Arc::new(ApprovalRouter::new(Arc::clone(&settings)));
        router.attach(Box::new(WindowNotifier(app.clone())));

        let stored_agents: Vec<AgentProfile> = paths::read_json_or_default(&paths.agents_file());
        let agents = agents::normalise(stored_agents);
        let projects: Vec<Project> = paths::read_json_or_default(&paths.projects_file());
        let conversations: ConversationIndex =
            paths::read_json_or_default(&paths.conversations_file());
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

        let workspace = Arc::new(Self {
            app,
            paths,
            settings,
            router,
            sidecar_dir,
            agents: Mutex::new(agents),
            projects: Mutex::new(projects),
            runtimes: Mutex::new(HashMap::new()),
            conversations: Mutex::new(conversations),
            chat_log,
            chat_turns: Mutex::new(HashMap::new()),
        });
        // Persist the normalised crew and settings so the files on disk match
        // what we are actually running.
        let _ = workspace.save_agents_file();
        let _ = paths::write_json(&workspace.paths.settings_file(), &workspace.settings());
        workspace.adopt_legacy_workspace();
        workspace
    }

    /// Earlier builds kept a single synthetic repository at `<data>/workspace`
    /// with its log at `<data>/events.jsonl`. Adopt it as a normal project so
    /// no history is stranded.
    fn adopt_legacy_workspace(self: &Arc<Self>) {
        if !self.projects().is_empty() {
            return;
        }
        let legacy_repo = self.paths.root().join("workspace");
        let legacy_log = self.paths.root().join("events.jsonl");
        if !legacy_repo.is_dir() {
            return;
        }
        let adopted = self.add_project(&legacy_repo.to_string_lossy(), Some("Workspace".into()), true);
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
        let runtimes: Vec<Arc<ProjectRuntime>> =
            self.runtimes.lock().unwrap().values().cloned().collect();
        for runtime in runtimes {
            let policy = policy.clone();
            let engine = runtime.engine.clone();
            tauri::async_runtime::spawn(async move {
                let _ = engine.set_policy(policy).await;
            });
        }
        Ok(settings)
    }

    // ---- agents ----

    pub fn agents(&self) -> Vec<AgentProfile> {
        self.agents.lock().unwrap().clone()
    }

    /// The profile for an id, falling back to a worker when it is unknown.
    /// Right for assigning work; wrong for a conversation, which must speak as
    /// the profile it says it does — see `agent_exact`.
    pub fn agent(&self, id: &str) -> Option<AgentProfile> {
        let agents = self.agents.lock().unwrap();
        agents::find(&agents, id).cloned()
    }

    /// The profile for an id, or nothing.
    pub fn agent_exact(&self, id: &str) -> Option<AgentProfile> {
        self.agents.lock().unwrap().iter().find(|a| a.id == id).cloned()
    }

    pub fn set_agents(&self, next: Vec<AgentProfile>) -> Result<Vec<AgentProfile>, String> {
        {
            let mut guard = self.agents.lock().unwrap();
            *guard = agents::normalise(next);
        }
        self.save_agents_file()?;
        Ok(self.agents())
    }

    /// Add a profile from a template. Templates are a menu: nothing is
    /// installed until this is called.
    pub fn add_agent_from_template(&self, template_id: &str) -> Result<AgentProfile, String> {
        let taken: Vec<String> = self.agents().into_iter().map(|a| a.id).collect();
        let created = agents::from_template(template_id, &taken)
            .ok_or_else(|| format!("there is no template called {template_id}"))?;
        self.agents.lock().unwrap().push(created.clone());
        self.save_agents_file()?;
        Ok(created)
    }

    pub fn duplicate_agent(&self, agent_id: &str) -> Result<AgentProfile, String> {
        let original = self
            .agent_exact(agent_id)
            .ok_or_else(|| format!("no agent profile called {agent_id}"))?;
        let taken: Vec<String> = self.agents().into_iter().map(|a| a.id).collect();
        let copy = agents::duplicate(&original, &taken);
        self.agents.lock().unwrap().push(copy.clone());
        self.save_agents_file()?;
        Ok(copy)
    }

    /// Remove a profile. Every profile is optional except the Director, which
    /// the review loop needs.
    pub fn remove_agent(&self, agent_id: &str) -> Result<Vec<AgentProfile>, String> {
        if agent_id == agents::DIRECTOR_ID {
            return Err("the Director cannot be removed: the review loop needs it".to_string());
        }
        if self.agent_exact(agent_id).is_none() {
            return Err(format!("no agent profile called {agent_id}"));
        }
        {
            let mut guard = self.agents.lock().unwrap();
            guard.retain(|a| a.id != agent_id);
        }
        self.save_agents_file()?;
        Ok(self.agents())
    }

    fn save_agents_file(&self) -> Result<(), String> {
        paths::write_json(&self.paths.agents_file(), &self.agents())
    }


    // ---- conversations ----

    pub fn conversations(&self, include_archived: bool) -> Vec<Conversation> {
        self.conversations.lock().unwrap().list(include_archived)
    }

    pub fn conversation(&self, id: &str) -> Option<Conversation> {
        self.conversations.lock().unwrap().get(id).cloned()
    }

    /// The conversation to reopen when the app starts.
    pub fn last_conversation(&self) -> Option<Conversation> {
        self.conversations
            .lock()
            .unwrap()
            .resume_target(agents::DIRECTOR_ID)
            .cloned()
    }

    fn save_conversations(&self) -> Result<(), String> {
        let index = self.conversations.lock().unwrap();
        paths::write_json(&self.paths.conversations_file(), &*index)
    }

    /// Start a conversation. A fresh row means a fresh native session: there is
    /// nothing to resume, which is what makes New Chat actually new.
    pub fn new_conversation(
        &self,
        profile_id: Option<String>,
        project_id: Option<String>,
    ) -> Result<Conversation, String> {
        let profile_id = profile_id.unwrap_or_else(|| agents::DIRECTOR_ID.to_string());
        let profile = self
            .agent_exact(&profile_id)
            .ok_or_else(|| format!("no agent profile called {profile_id}"))?;
        if !profile.chat_enabled {
            return Err(format!("{} is not set up for conversations", profile.name));
        }
        // A pin to a project that is gone would only mislead.
        let project_id = project_id.filter(|id| self.project(id).is_some());
        let id = format!("chat_{}", uuid::Uuid::new_v4().simple());
        let created = self.conversations.lock().unwrap().insert(Conversation::new(
            id,
            profile_id,
            project_id,
            SystemClock.now_millis(),
        ));
        self.save_conversations()?;
        Ok(created)
    }

    /// The conversation to talk in right now: the one asked for, the last one
    /// used, or a new one.
    pub fn open_conversation(
        &self,
        profile_id: Option<String>,
        project_id: Option<String>,
    ) -> Result<Conversation, String> {
        let wanted = profile_id
            .clone()
            .unwrap_or_else(|| agents::DIRECTOR_ID.to_string());
        let existing = {
            let index = self.conversations.lock().unwrap();
            index
                .resume_target(&wanted)
                .filter(|c| c.profile_id == wanted)
                .cloned()
        };
        match existing {
            Some(found) => {
                self.select_conversation(&found.id)?;
                Ok(found)
            }
            None => self.new_conversation(profile_id, project_id),
        }
    }

    pub fn select_conversation(&self, id: &str) -> Result<Conversation, String> {
        {
            let mut index = self.conversations.lock().unwrap();
            index.select(id)?;
        }
        self.save_conversations()?;
        self.conversation(id).ok_or_else(|| format!("no conversation {id}"))
    }

    pub fn rename_conversation(&self, id: &str, title: &str) -> Result<Conversation, String> {
        let updated = {
            let mut index = self.conversations.lock().unwrap();
            index.rename(id, title, SystemClock.now_millis())?
        };
        self.save_conversations()?;
        Ok(updated)
    }

    pub fn archive_conversation(&self, id: &str, archived: bool) -> Result<Conversation, String> {
        let updated = {
            let mut index = self.conversations.lock().unwrap();
            index.set_archived(id, archived, SystemClock.now_millis())?
        };
        self.save_conversations()?;
        Ok(updated)
    }

    /// Forget a conversation and its transcript. Destructive, so the UI asks
    /// first; by the time this runs the decision is made.
    pub fn delete_conversation(&self, id: &str) -> Result<(), String> {
        let gone = {
            let mut index = self.conversations.lock().unwrap();
            index.remove(id)?
        };
        self.save_conversations()?;
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
    pub fn pin_conversation(
        &self,
        id: &str,
        project_id: Option<String>,
    ) -> Result<Conversation, String> {
        let project_id = project_id.filter(|p| self.project(p).is_some());
        {
            let mut index = self.conversations.lock().unwrap();
            let entry = index
                .conversations
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| format!("no conversation {id}"))?;
            entry.project_id = project_id;
            entry.updated_ms = SystemClock.now_millis();
        }
        self.save_conversations()?;
        self.conversation(id).ok_or_else(|| format!("no conversation {id}"))
    }

    pub fn record_chat_message(&self, id: &str, message: &str) -> Result<Conversation, String> {
        let updated = {
            let mut index = self.conversations.lock().unwrap();
            index.record_message(id, message, SystemClock.now_millis())?
        };
        self.save_conversations()?;
        Ok(updated)
    }

    /// Save the session the SDK handed back, and tell the window, so the list
    /// stops saying a conversation has never been answered.
    pub fn record_chat_session(&self, id: &str, session_id: &str) {
        let changed = {
            let mut index = self.conversations.lock().unwrap();
            index
                .record_session(id, session_id, SystemClock.now_millis())
                .unwrap_or(false)
        };
        if changed {
            let _ = self.save_conversations();
            self.publish_conversations();
        }
    }

    pub fn record_chat_cost(&self, id: &str, cost_usd: Option<f64>) {
        {
            let mut index = self.conversations.lock().unwrap();
            index.record_cost(id, cost_usd, SystemClock.now_millis());
        }
        let _ = self.save_conversations();
        self.publish_conversations();
    }

    pub fn record_chat_resume_failure(&self, id: &str) {
        {
            let mut index = self.conversations.lock().unwrap();
            index.record_resume_failure(id, SystemClock.now_millis());
        }
        let _ = self.save_conversations();
        self.publish_conversations();
    }

    pub fn append_chat_line(&self, conversation_id: &str, line: RunLogLine) {
        if let Err(e) = RunLogPort::append(self.chat_log.as_ref(), conversation_id, &line) {
            eprintln!("could not write the conversation transcript: {e}");
        }
    }

    /// The list changed without the UI asking; it renders backend state, so the
    /// backend says when it moved.
    fn publish_conversations(&self) {
        let _ = self
            .app
            .emit("chat://conversations", self.conversations(false));
    }

    /// Every board, with the one this conversation can read marked.
    pub async fn project_briefs(
        self: &Arc<Self>,
        active: Option<&str>,
    ) -> Result<Vec<ProjectBrief>, String> {
        let mut briefs = Vec::new();
        for project in self.projects() {
            if !Path::new(&project.path).is_dir() {
                continue;
            }
            let Ok(runtime) = self.runtime(&project.id) else {
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

    pub fn projects(&self) -> Vec<Project> {
        self.projects.lock().unwrap().clone()
    }

    /// Every live runtime, for sweeps that must visit each project once.
    pub fn runtimes(&self) -> Vec<Arc<ProjectRuntime>> {
        self.runtimes.lock().unwrap().values().cloned().collect()
    }

    pub fn project(&self, id: &str) -> Option<Project> {
        self.projects.lock().unwrap().iter().find(|p| p.id == id).cloned()
    }

    fn save_projects_file(&self) -> Result<(), String> {
        paths::write_json(&self.paths.projects_file(), &self.projects())
    }

    /// What a folder looks like, so the UI can offer the right next step.
    pub fn inspect_folder(&self, path: &str) -> FolderInfo {
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
    pub fn create_project(&self, parent: &str, name: &str) -> Result<Project, String> {
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
    }

    /// Register a git repository. `init` is the operator explicitly agreeing to
    /// run `git init` in a folder that is not a repository yet; without it a
    /// non-empty folder is refused, because turning someone's folder into a
    /// repo is not ours to decide.
    pub fn add_project(
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

        let taken: Vec<String> = self.projects().into_iter().map(|p| p.id).collect();
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

        self.projects.lock().unwrap().push(project.clone());
        self.save_projects_file()?;

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
    pub fn register_chat_turn(&self, conversation_id: &str, token: CancellationToken) {
        self.chat_turns
            .lock()
            .unwrap()
            .insert(conversation_id.to_string(), token);
    }

    /// Take the turn's token out (None if the conversation had none). Taking
    /// it is both how stop finds it and how completion cleans up.
    pub fn finish_chat_turn(
        &self,
        conversation_id: &str,
    ) -> Option<CancellationToken> {
        self.chat_turns
            .lock()
            .unwrap()
            .remove(conversation_id)
    }

    pub fn update_project(&self, project: Project) -> Result<Project, String> {        {
            let mut guard = self.projects.lock().unwrap();
            let slot = guard
                .iter_mut()
                .find(|p| p.id == project.id)
                .ok_or_else(|| format!("unknown project {}", project.id))?;
            *slot = project.clone();
        }
        self.save_projects_file()?;
        Ok(project)
    }

    /// Forget a project. Its event log and worktrees are only deleted when the
    /// operator explicitly asks; the repository itself is never touched.
    pub fn remove_project(&self, id: &str, delete_data: bool) -> Result<(), String> {
        self.projects.lock().unwrap().retain(|p| p.id != id);
        self.runtimes.lock().unwrap().remove(id);
        self.save_projects_file()?;
        // A conversation pinned to a project that is gone would point nowhere.
        {
            let mut index = self.conversations.lock().unwrap();
            index.unpin_project(id);
        }
        let _ = self.save_conversations();
        if delete_data {
            let _ = std::fs::remove_dir_all(self.paths.project_dir(id));
            let _ = std::fs::remove_dir_all(self.paths.project_worktrees(id));
        }
        Ok(())
    }

    // ---- engines ----

    pub fn runtime(&self, project_id: &str) -> Result<Arc<ProjectRuntime>, String> {
        if let Some(existing) = self.runtimes.lock().unwrap().get(project_id) {
            return Ok(Arc::clone(existing));
        }
        let project = self
            .project(project_id)
            .ok_or_else(|| format!("unknown project {project_id}"))?;
        let runtime = Arc::new(self.spawn_runtime(project)?);
        self.runtimes
            .lock()
            .unwrap()
            .insert(project_id.to_string(), Arc::clone(&runtime));
        Ok(runtime)
    }

    /// Bring up every registered project so the overview can count work
    /// without the operator visiting each one first.
    pub fn warm_all(&self) {
        for project in self.projects() {
            if let Err(e) = self.runtime(&project.id) {
                eprintln!("could not start project {}: {e}", project.id);
            }
        }
    }

    fn spawn_runtime(&self, project: Project) -> Result<ProjectRuntime, String> {
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
        let mut config = EngineConfig::new(&project.id, root);
        config.base_branch = project.base_branch.clone();
        config.permission_mode = settings.permission_mode.clone();
        config.director_model = self
            .agent(agents::DIRECTOR_ID)
            .and_then(|d| d.model.clone());
        // Mirror mode: this project is the orchestrator itself, so a finished
        // run is followed by an engine-owned build. The artefact waits in
        // appdata; installing it is nobody's decision but the operator's.
        if project.mirror {
            config.post_build = Some(harness_engine::BuildSpec {
                program: "pnpm".into(),
                args: vec!["tauri".into(), "build".into(), "--no-bundle".into()],
                updates_dir: self.paths.updates_dir(),
                artifact: "target/release/harness.exe".into(),
            });
        }
        if let Some(director_profile) = self.agent(agents::DIRECTOR_ID) {
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

    /// Cancel every run everywhere and let the worktrees commit.
    pub async fn shutdown(&self) {
        self.router.deny_all();
        let runtimes: Vec<Arc<ProjectRuntime>> =
            self.runtimes.lock().unwrap().values().cloned().collect();
        for runtime in runtimes {
            let _ = runtime.engine.shutdown().await;
        }
    }
}
