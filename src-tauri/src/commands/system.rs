//! Everything that is not a board or a project: settings, the crew, auth,
//! approvals, terminals and the one-shot bootstrap the UI opens with.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use harness_app::agents::AgentProfile;
use harness_app::approvals::PendingApproval;
use harness_app::insights::{self, AgentStats};
use harness_app::projects::Project;
use harness_app::settings::Settings;

use crate::sidecar::{self, ClaudeStatus, SidecarStatus};
use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

#[derive(Debug, Serialize)]
pub struct SystemStatus {
    pub claude: ClaudeStatus,
    pub sidecar: SidecarStatus,
    /// True when a run can actually start right now.
    pub ready: bool,
    pub blocker: Option<String>,
}

fn system_status(ws: &Workspace) -> SystemStatus {
    let claude = sidecar::claude_status();
    let side = sidecar::status(ws.sidecar_dir());
    let use_sidecar = ws.settings().sidecar;

    let blocker = if !claude.logged_in {
        Some("Claude is not logged in. Open a terminal and run /login.".to_string())
    } else if use_sidecar && !side.node_found {
        Some("Node was not found on PATH. Install Node 20 or newer, or turn the sidecar off in Settings.".to_string())
    } else if use_sidecar && !side.script_found {
        Some("The sidecar script is missing from this install.".to_string())
    } else if use_sidecar && !side.ready {
        Some("The sidecar needs its dependencies installed.".to_string())
    } else if !use_sidecar && !claude.cli_found {
        Some("The claude command line tool was not found on PATH.".to_string())
    } else {
        None
    };

    SystemStatus {
        ready: blocker.is_none(),
        blocker,
        claude,
        sidecar: side,
    }
}

/// One call the UI makes on open, so the first paint needs no waterfall.
#[derive(Debug, Serialize)]
pub struct Bootstrap {
    pub settings: Settings,
    pub agents: Vec<AgentProfile>,
    pub projects: Vec<Project>,
    pub status: SystemStatus,
    pub approvals: Vec<PendingApproval>,
    pub data_dir: String,
}

#[tauri::command]
pub async fn bootstrap(ws: Shared<'_>) -> Result<Bootstrap, String> {
    Ok(Bootstrap {
        settings: ws.settings(),
        agents: ws.agents(),
        projects: ws.projects(),
        status: system_status(&ws),
        approvals: ws.router.pending_list(),
        data_dir: ws.paths.root().to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub async fn status(ws: Shared<'_>) -> Result<SystemStatus, String> {
    Ok(system_status(&ws))
}

#[tauri::command]
pub async fn settings_get(ws: Shared<'_>) -> Result<Settings, String> {
    Ok(ws.settings())
}

#[tauri::command]
pub async fn settings_update(settings: Settings, ws: Shared<'_>) -> Result<Settings, String> {
    ws.set_settings(settings)
}

#[tauri::command]
pub async fn agents_get(ws: Shared<'_>) -> Result<Vec<AgentProfile>, String> {
    Ok(ws.agents())
}

#[tauri::command]
pub async fn agents_save(
    agents: Vec<AgentProfile>,
    ws: Shared<'_>,
) -> Result<Vec<AgentProfile>, String> {
    ws.set_agents(agents)
}

/// Per-agent numbers, summed across every project so an agent page reads the
/// whole workspace. Line counts come from the git history.
#[tauri::command]
pub async fn agents_stats(
    tz_offset_minutes: Option<i64>,
    ws: Shared<'_>,
) -> Result<Vec<AgentStats>, String> {
    let tz = tz_offset_minutes.unwrap_or(0);
    let mut merged: std::collections::BTreeMap<String, AgentStats> = Default::default();

    for project in ws.projects() {
        if !Path::new(&project.path).is_dir() {
            continue;
        }
        let Ok(runtime) = ws.runtime(&project.id) else {
            continue;
        };
        let cards = runtime.engine.snapshot().await?.cards;
        let store = Arc::clone(&runtime.store);
        let git = Arc::clone(&runtime.git);
        let (history, commits) = tauri::async_runtime::spawn_blocking(move || {
            (
                harness_ports::StorePort::read_all(store.as_ref()).unwrap_or_default(),
                git.recent_commits(400),
            )
        })
        .await
        .map_err(|e| e.to_string())?;

        for (id, stats) in insights::agent_stats(&history, &cards, tz) {
            let slot = merged.entry(id.clone()).or_insert_with(|| AgentStats {
                agent_id: id,
                week_runs: vec![0; 7],
                ..Default::default()
            });
            slot.runs += stats.runs;
            slot.cards += stats.cards;
            slot.cards_done += stats.cards_done;
            slot.spend += stats.spend;
            slot.turns += stats.turns;
            slot.reviews += stats.reviews;
            slot.sent_back += stats.sent_back;
            for (i, v) in stats.week_runs.iter().enumerate() {
                if let Some(cell) = slot.week_runs.get_mut(i) {
                    *cell += v;
                }
            }
        }

        for commit in commits {
            let Some(agent) = commit.agent else { continue };
            let slot = merged.entry(agent.clone()).or_insert_with(|| AgentStats {
                agent_id: agent,
                week_runs: vec![0; 7],
                ..Default::default()
            });
            slot.lines_added += commit.added;
            slot.lines_removed += commit.removed;
            slot.commits += 1;
        }
    }

    for stats in merged.values_mut() {
        if stats.runs > 0 {
            stats.avg_cost = stats.spend / stats.runs as f64;
        }
    }
    Ok(merged.into_values().collect())
}

// ---- approvals ----

#[tauri::command]
pub async fn approvals_pending(ws: Shared<'_>) -> Result<Vec<PendingApproval>, String> {
    Ok(ws.router.pending_list())
}

/// Answer a permission request. `always` records a standing allowance for the
/// tool so the operator stops being asked.
#[tauri::command]
pub async fn respond_approval(
    request_id: String,
    allow: bool,
    always: bool,
    ws: Shared<'_>,
) -> Result<(), String> {
    if allow && always {
        if let Some(pending) = ws
            .router
            .pending_list()
            .into_iter()
            .find(|p| p.request_id == request_id)
        {
            let mut settings = ws.settings();
            settings.allow_always(&pending.tool);
            ws.set_settings(settings)?;
        }
    }
    ws.router.resolve(&request_id, allow)
}

// ---- sidecar ----

#[tauri::command]
pub async fn sidecar_install(app: AppHandle, ws: Shared<'_>) -> Result<String, String> {
    let dir = ws.sidecar_dir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || sidecar::install(&app, &dir))
        .await
        .map_err(|e| e.to_string())?
}

// ---- terminals ----

fn open_terminal_in(dir: &Path, argv: &[&str]) -> Result<(), String> {
    let dir_str = dir.to_string_lossy().to_string();

    #[cfg(windows)]
    {
        let mut wt_args: Vec<&str> = vec!["-d", &dir_str];
        wt_args.extend_from_slice(argv);
        if std::process::Command::new("wt").args(&wt_args).spawn().is_ok() {
            return Ok(());
        }
        return std::process::Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("harness")
            .arg("/D")
            .arg(&dir_str)
            .args(argv)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open a terminal: {e}"));
    }

    #[cfg(not(windows))]
    {
        let joined = argv.join(" ");
        let program = if cfg!(target_os = "macos") {
            "open"
        } else {
            "x-terminal-emulator"
        };
        return std::process::Command::new(program)
            .current_dir(dir)
            .arg(if cfg!(target_os = "macos") { "-a" } else { "-e" })
            .arg(if cfg!(target_os = "macos") {
                "Terminal".to_string()
            } else {
                joined
            })
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not open a terminal: {e}"));
    }
}

/// A real terminal in the project, for `claude /login` and anything else.
#[tauri::command]
pub async fn open_claude_terminal(
    project_id: Option<String>,
    ws: Shared<'_>,
) -> Result<(), String> {
    let dir = project_id
        .and_then(|id| ws.project(&id))
        .map(|p| PathBuf::from(p.path))
        .unwrap_or_else(|| ws.paths.root().to_path_buf());
    open_terminal_in(&dir, &["cmd", "/K", "claude"])
}

/// Drop into the agent's own session inside its worktree.
#[tauri::command]
pub async fn open_agent_terminal(
    project_id: String,
    card_id: String,
    ws: Shared<'_>,
) -> Result<(), String> {
    let runtime = ws.runtime(&project_id)?;
    let snap = runtime.engine.snapshot().await?;
    let session = snap
        .sessions
        .iter()
        .find(|s| s.card_id.as_str() == card_id)
        .ok_or_else(|| "no agent session for this card yet".to_string())?;
    let dir = PathBuf::from(&session.worktree);
    match &session.session_id {
        Some(sid) => open_terminal_in(&dir, &["cmd", "/K", "claude", "--resume", sid]),
        None => open_terminal_in(&dir, &["cmd", "/K", "claude"]),
    }
}

/// Cancel everything and let the worktrees commit before the window closes.
#[tauri::command]
pub async fn prepare_shutdown(ws: Shared<'_>) -> Result<(), String> {
    ws.shutdown().await;
    Ok(())
}
