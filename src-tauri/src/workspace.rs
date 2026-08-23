//! The workspace: the registry of projects the operator added, and one engine
//! per project. Everything the Tauri commands need hangs off here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use harness_agent_claude::ClaudeCliAgent;
use harness_agent_sidecar::SidecarAgent;
use harness_domain::{CardId, RunId};
use harness_engine::{Engine, EngineConfig, EngineDeps, EngineHandle, RunUpdate};
use harness_git_cli::{ensure_workspace, CliGit};
use harness_ports::{
    AgentPort, ClockPort, RunEvent, RunLogPort, RunOutcome, RunSpec, StorePort, ToolRunner,
};
use harness_store_jsonl::{JsonlRunLog, JsonlStore};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use harness_app::agents::{self, AgentProfile};
use harness_app::approvals::{ApprovalRouter, Notifier, PendingApproval};
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
struct SwitchingAgent {
    sidecar: Arc<dyn AgentPort>,
    cli: Arc<dyn AgentPort>,
    settings: Arc<Mutex<Settings>>,
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
        let sidecar_dir = sidecar::prepare(&app, &paths);

        let workspace = Arc::new(Self {
            app,
            paths,
            settings,
            router,
            sidecar_dir,
            agents: Mutex::new(agents),
            projects: Mutex::new(projects),
            runtimes: Mutex::new(HashMap::new()),
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

    pub fn agent(&self, id: &str) -> Option<AgentProfile> {
        let agents = self.agents.lock().unwrap();
        agents::find(&agents, id).cloned()
    }

    pub fn set_agents(&self, next: Vec<AgentProfile>) -> Result<Vec<AgentProfile>, String> {
        {
            let mut guard = self.agents.lock().unwrap();
            *guard = agents::normalise(next);
        }
        self.save_agents_file()?;
        Ok(self.agents())
    }

    fn save_agents_file(&self) -> Result<(), String> {
        paths::write_json(&self.paths.agents_file(), &self.agents())
    }

    // ---- projects ----

    pub fn projects(&self) -> Vec<Project> {
        self.projects.lock().unwrap().clone()
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
        };

        self.projects.lock().unwrap().push(project.clone());
        self.save_projects_file()?;
        Ok(project)
    }

    pub fn update_project(&self, project: Project) -> Result<Project, String> {
        {
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

    /// Ask the Director something with no project open — the conversation you
    /// need before there is a board: what to point Harness at, how to split the
    /// first piece of work. It runs read-only in the app-data directory, so it
    /// can answer without touching anything of yours.
    pub async fn ask_director(
        self: &Arc<Self>,
        text: String,
        project_id: Option<String>,
    ) -> Result<(), String> {
        let profile = self
            .agent(agents::DIRECTOR_ID)
            .ok_or_else(|| "no Director profile configured".to_string())?;

        // Every board it watches, with the open one marked: that is what makes
        // this one Director rather than one per project.
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
            // read what its worktree actually holds instead of leaving the
            // Director to guess.
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
                name: project.name.clone(),
                path: project.path.clone(),
                active: project_id.as_deref() == Some(project.id.as_str()),
                cards: lines,
            });
        }

        // Reading code only makes sense inside the project that is open.
        let cwd = project_id
            .as_deref()
            .and_then(|id| self.project(id))
            .map(|p| PathBuf::from(p.path))
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| self.paths.root().to_path_buf());

        let script = sidecar::script_in(&self.sidecar_dir);
        let agent: Arc<dyn AgentPort> = Arc::new(SwitchingAgent {
            sidecar: Arc::new(SidecarAgent::new("node", script)),
            cli: Arc::new(ClaudeCliAgent::new("claude")),
            settings: Arc::clone(&self.settings),
        });

        // Harness's own tools. They are not in `allowed_tools`, so the agent
        // SDK sends each call through the approver first: the operator sees
        // "the Director wants to move c_7b30" before anything moves.
        let tool_ws = Arc::clone(self);
        let tool_app = self.app.clone();
        let tool_project = project_id.clone();
        let tools: ToolRunner = Arc::new(move |call| {
            let ws = Arc::clone(&tool_ws);
            let app = tool_app.clone();
            let project = tool_project.clone();
            Box::pin(async move { crate::director_tools::run(&ws, &app, project, call).await })
        });

        let spec = RunSpec {
            prompt: harness_app::director::ask_prompt(&text, &briefs),
            cwd,
            model: profile.model.clone(),
            // Granted outright, because none of it changes anything: reading
            // the repository, reading a diff, and moving the operator's own
            // window. The SDK auto-approves bare entries, so showing someone a
            // screen never interrupts them for permission. Everything that
            // *changes* the board is absent here on purpose, so it reaches the
            // approver below. (`dontAsk` would deny those outright instead.)
            allowed_tools: Some(vec![
                "Read".into(),
                "Glob".into(),
                "Grep".into(),
                "mcp__harness__open_screen".into(),
                "mcp__harness__read_diff".into(),
            ]),
            max_budget_usd: profile.budget_usd,
            permission_mode: Some("manual".to_string()),
            approver: Some(self.router.approver_for(
                project_id.as_deref().unwrap_or("workspace"),
            )),
            resume_session: None,
            tools: Some(tools),
            // The operator watches it think while it works, so give it room to.
            thinking_tokens: Some(4000),
        };

        let (ev_tx, mut ev_rx) = mpsc::channel::<RunEvent>(64);
        let fut = agent.run(spec, ev_tx, CancellationToken::new());
        let app = self.app.clone();
        let project_id = project_id.unwrap_or_default();
        let run_id = RunId(uuid::Uuid::new_v4().to_string());

        tauri::async_runtime::spawn(async move {
            // Published on the reserved card id, in the same shape as any run,
            // so the UI has one typed listener for both.
            let publish = |event: RunEvent| {
                let _ = app.emit(
                    "engine://run",
                    RunUpdate {
                        project_id: project_id.clone(),
                        card_id: CardId::new(harness_app::director::CARD_ID),
                        run_id: run_id.clone(),
                        ts_ms: SystemClock.now_millis(),
                        event,
                    },
                );
            };
            let forward = async {
                while let Some(ev) = ev_rx.recv().await {
                    match ev {
                        RunEvent::Delta { text } => publish(RunEvent::Delta { text }),
                        RunEvent::Thinking { text } => publish(RunEvent::Thinking { text }),
                        RunEvent::Text { text } => publish(RunEvent::Text { text }),
                        RunEvent::ToolUse { tool, summary } => {
                            publish(RunEvent::ToolUse { tool, summary })
                        }
                        RunEvent::Failed { message } => publish(RunEvent::Failed { message }),
                        _ => {}
                    }
                }
            };
            let (res, _) = tokio::join!(fut, forward);
            // The UI clears its thinking state on this, so it always goes out.
            match res {
                Ok(RunOutcome::Failed(message)) | Err(message) => {
                    publish(RunEvent::Failed { message })
                }
                Ok(RunOutcome::Completed { cost_usd, turns, .. }) => publish(RunEvent::Done {
                    session_id: None,
                    cost_usd,
                    turns,
                    result: None,
                }),
                Ok(RunOutcome::Cancelled) => publish(RunEvent::Done {
                    session_id: None,
                    cost_usd: None,
                    turns: None,
                    result: None,
                }),
            }
        });

        Ok(())
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
