//! A casca em si: o que o arranque precisa de saber, as definições, o sidecar,
//! um terminal a sério, e o fecho da janela.
//!
//! O que era "tudo o que não é quadro nem projecto" está agora repartido por
//! dono — a tripulação, as aprovações, a caixa de entrada e as actualizações
//! têm ficheiro cada um. O que sobra aqui é a shell a falar de si própria.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use harness_app::agents::AgentProfile;
use harness_app::approvals::PendingApproval;
use harness_app::conversations::Conversation;
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
    /// The chats that exist, and which one to reopen — so the window comes back
    /// where it was left instead of on an empty conversation.
    pub conversations: Vec<Conversation>,
    pub last_conversation: Option<String>,
    /// Unscoped shell allowances left by an older build. They authorise nothing
    /// now; the UI says so once.
    pub revoked_allowances: Vec<String>,
    /// What `/` can reach, as the last session described it. Without this the
    /// composer's menu is empty after every restart — the event that carries it
    /// is ephemeral, so nothing on disk remembered it.
    pub commands: Vec<harness_ports::SlashCommand>,
    /// Improvement proposals waiting on the operator, newest first.
    pub inbox: Vec<harness_app::inbox::Proposal>,
    /// The last finding of the look at Relay's own repository, if it found
    /// anything this session.
    ///
    /// It is here because the event cannot be trusted to have been heard: the
    /// startup look is spawned before the webview exists, so it may be
    /// emitted to nobody, and reloading the window loses it the same way. This
    /// is the one call the UI always makes, so a warning that already exists
    /// is on screen on open and on reload — `"nothing is silently lost"`.
    /// The window drops the duplicate when both arrive.
    pub outside_work: Option<harness_app::mirror::MirrorWarning>,
}

#[tauri::command]
pub async fn bootstrap(ws: Shared<'_>) -> Result<Bootstrap, String> {
    Ok(Bootstrap {
        settings: ws.settings(),
        agents: ws.agents().await,
        projects: ws.projects().await,
        status: system_status(&ws),
        approvals: ws.router.pending_list(),
        data_dir: ws.paths.root().to_string_lossy().to_string(),
        conversations: ws.conversations(false).await,
        last_conversation: ws.last_conversation().await.map(|c| c.id),
        revoked_allowances: ws
            .settings()
            .revoked_allowances()
            .into_iter()
            .map(|r| r.label())
            .collect(),
        commands: ws.slash_commands(),
        inbox: ws.inbox().proposals,
        outside_work: ws.outside_work_warning().await,
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
    #[cfg(windows)]
    {
        let dir_str = dir.to_string_lossy().to_string();
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
        // The argv handed in is Windows-shaped (`cmd /K claude ...`). Joining it
        // into one string for `-e` would let anything in it — a session id, say
        // — be split or interpreted by whichever shell the terminal starts.
        // Keep it as separate arguments instead, and drop the `cmd /K` wrapper
        // that means nothing here.
        let mut args: Vec<&str> = argv
            .iter()
            .copied()
            .skip_while(|a| matches!(*a, "cmd" | "/K" | "/C"))
            .collect();
        if args.is_empty() {
            args.push("claude");
        }
        if cfg!(target_os = "macos") {
            // `open -a Terminal` cannot carry a command, so it opens in the
            // directory and the operator types it. Saying so beats pretending.
            return std::process::Command::new("open")
                .current_dir(dir)
                .arg("-a")
                .arg("Terminal")
                .arg(dir)
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("could not open a terminal: {e}"));
        }
        return std::process::Command::new("x-terminal-emulator")
            .current_dir(dir)
            .arg("-e")
            .args(&args)
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
    let dir = match project_id {
        Some(id) => ws.project(&id).await,
        None => None,
    }
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
    let runtime = ws.runtime(&project_id).await?;
    let snap = runtime.engine.snapshot().await?;
    let session = snap.sessions.iter().find(|s| s.card_id.as_str() == card_id);
    let card = snap.cards.iter().find(|c| c.id.as_str() == card_id);

    // Both survive a restart now. The card is the fallback for a run logged
    // before worktrees were recorded, which has a session but no directory.
    let dir = session
        .map(|s| PathBuf::from(&s.worktree))
        .or_else(|| card.and_then(|c| c.worktree.clone()).map(PathBuf::from))
        .filter(|p| p.is_dir())
        .ok_or_else(|| "this card has no worktree to open".to_string())?;
    let resume = session
        .and_then(|s| s.session_id.clone())
        .or_else(|| card.and_then(|c| c.session_id.clone()));

    match resume {
        Some(sid) => open_terminal_in(&dir, &["cmd", "/K", "claude", "--resume", &sid]),
        None => open_terminal_in(&dir, &["cmd", "/K", "claude"]),
    }
}

/// Cancel everything and let the worktrees commit before the window closes.
/// The end-of-day look runs first when it is due: shutdown is the one moment
/// a day is actually over, and it is bounded (budget + wall clock).
#[tauri::command]
pub async fn prepare_shutdown(ws: Shared<'_>) -> Result<(), String> {
    let ws = Arc::clone(&ws);
    let skip = ws.closing_token();
    crate::reflection::maybe_run_daily_look(&ws, skip).await;
    ws.shutdown().await;
    Ok(())
}

/// Stop waiting for the close sequence. The window goes as soon as the
/// in-flight step notices; nothing filed is lost, and a look that did not
/// finish is due again rather than marked done.
#[tauri::command]
pub async fn close_now(ws: Shared<'_>) -> Result<(), String> {
    ws.stop_waiting();
    Ok(())
}
