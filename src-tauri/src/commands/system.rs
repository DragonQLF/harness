//! Everything that is not a board or a project: settings, the crew, auth,
//! approvals, terminals and the one-shot bootstrap the UI opens with.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use harness_app::agents::AgentProfile;
use harness_app::approvals::PendingApproval;
use harness_app::conversations::Conversation;
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
    /// The chats that exist, and which one to reopen — so the window comes back
    /// where it was left instead of on an empty conversation.
    pub conversations: Vec<Conversation>,
    pub last_conversation: Option<String>,
    /// Unscoped shell allowances left by an older build. They authorise nothing
    /// now; the UI says so once.
    pub revoked_allowances: Vec<String>,
    /// Improvement proposals waiting on the operator, newest first.
    pub inbox: Vec<harness_app::inbox::Proposal>,
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
        conversations: ws.conversations(false),
        last_conversation: ws.last_conversation().map(|c| c.id),
        revoked_allowances: ws
            .settings()
            .revoked_allowances()
            .into_iter()
            .map(|r| r.label())
            .collect(),
        inbox: ws.inbox().proposals,
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

/// Answer a permission request. `always` records a standing allowance — scoped
/// to this call, not to the bare tool name: agreeing to one `git push` must
/// never authorise every shell command. Some calls cannot be scoped safely (a
/// chained shell command), and those are allowed once and asked about again.
#[tauri::command]
pub async fn respond_approval(
    request_id: String,
    allow: bool,
    always: bool,
    ws: Shared<'_>,
) -> Result<Option<String>, String> {
    let mut recorded = None;
    if allow && always {
        // The tool and input come from the pending request, never from the
        // caller, so the UI cannot widen what it is answering about.
        if let Some(pending) = ws
            .router
            .pending_list()
            .into_iter()
            .find(|p| p.request_id == request_id)
        {
            let mut settings = ws.settings();
            let rule = settings.allow_always(&pending.tool, &pending.input);
            match rule {
                Some(rule) => {
                    ws.set_settings(settings)?;
                    recorded = Some(rule.label());
                }
                // Nothing safe to remember: allow this one and keep asking.
                None => recorded = None,
            }
        }
    }
    ws.router.resolve(&request_id, allow)?;
    Ok(recorded)
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
    crate::reflection::maybe_run_daily_look(&ws).await;
    ws.shutdown().await;
    Ok(())
}

// ---- inbox ----

#[tauri::command]
pub async fn inbox_list(ws: Shared<'_>) -> Result<Vec<harness_app::inbox::Proposal>, String> {
    Ok(ws.inbox().proposals)
}

/// Accept a proposal: the card is born in the harness's own project, never in
/// whatever is open (#72). Creating the card is ours; deciding was theirs.
#[tauri::command]
pub async fn inbox_accept(
    proposal_id: String,
    ws: State<'_, Arc<Workspace>>,
) -> Result<harness_app::inbox::Proposal, String> {
    let ws = Arc::clone(&ws);
    ws.accept_proposal(&proposal_id).await
}

#[tauri::command]
pub async fn inbox_dismiss(
    proposal_id: String,
    ws: Shared<'_>,
) -> Result<harness_app::inbox::Proposal, String> {
    ws.dismiss_proposal(&proposal_id)
}



/// Mirror builds that finished and are waiting for a decision. Newest first;
/// a manifest without its binary is a broken promise and is not shown.
#[tauri::command]
pub async fn updates_list(ws: Shared<'_>) -> Result<Vec<crate::update::PendingUpdate>, String> {
    Ok(crate::update::list_pending(&ws.paths.updates_dir()))
}

/// Install a parked build. The running exe moves aside (legal even while it
/// runs), the new one takes its place, the startup marker goes down, and the
/// app relaunches itself — with the old binary kept for the rollback that
/// fires if the new one never gets healthy. Refused while any agent runs.
#[tauri::command]
pub async fn update_install(card_id: String, ws: Shared<'_>) -> Result<(), String> {
    for runtime in ws.runtimes() {
        let active = runtime.engine.active_runs().await?;
        if !active.is_empty() {
            return Err(format!(
                "{} has an agent working; stop it before installing",
                runtime.project.name
            ));
        }
    }

    let chosen = crate::update::list_pending(&ws.paths.updates_dir())
        .into_iter()
        .find(|p| p.card_id == card_id)
        .ok_or_else(|| format!("no pending update for {card_id}"))?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let marker = crate::update::default_marker(ws.paths.root());
    use harness_ports::ClockPort;
    let installed_at_ms = {
        let clock = crate::workspace::SystemClock;
        clock.now_millis()
    };
    let info = serde_json::json!({
        "card_id": card_id,
        "commit_sha": chosen.commit_sha,
        "installed_at_ms": installed_at_ms,
    });
    crate::update::swap(
        &exe,
        &chosen.binary,
        &crate::update::previous_binary_path(),
        &marker,
        &info,
    )?;

    // The marker is down; from here the next launch either proves itself or
    // rolls back. Nothing after this line should be allowed to fail loudly.
    let _ = ws.shutdown().await;
    crate::update::relaunch(&exe)?;
    std::process::exit(0);
}



/// The Curator pass, on demand: promote report_work notes from Done cards
/// into the project's memory areas and regenerate the index from those files.
/// Idempotent — the watermark in curator-state.json means nothing is promoted
/// twice.
#[tauri::command]
pub async fn curator_run(
    project_id: String,
    ws: Shared<'_>,
) -> Result<String, String> {
    let runtime = ws.runtime(&project_id)?;
    let dir = ws.paths.project_dir(&project_id).join("memory");
    std::fs::create_dir_all(dir.join("areas")).map_err(|e| e.to_string())?;

    let store = std::sync::Arc::clone(&runtime.store);
    let history = tauri::async_runtime::spawn_blocking(move || {
        harness_ports::StorePort::read_all(store.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    let watermark_path = dir.join("curator-state.json");
    let since: u64 = std::fs::read_to_string(&watermark_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("last_seq").and_then(|s| s.as_u64()))
        .unwrap_or(0);

    let cards = runtime.engine.snapshot().await?.cards;
    let promotions = harness_app::curator::plan_promotions(&history, since, &cards);
    if promotions.is_empty() {
        return Ok("nothing new to curate".to_string());
    }

    for p in &promotions {
        let path = dir.join("areas").join(&p.file_name);
        if let Err(e) = std::fs::write(&path, &p.markdown) {
            return Err(format!("could not write {}: {e}", path.display()));
        }
    }

    // Index regenerated from what actually exists in areas/.
    let mut listing: Vec<(String, Vec<String>)> = Vec::new();
    for entry in std::fs::read_dir(dir.join("areas")).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
        let headers: Vec<String> = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("## "))
            .map(|l| l[3..].trim().to_string())
            .collect();
        listing.push((name, headers));
    }
    let index = harness_app::curator::render_index(&listing);
    std::fs::write(dir.join("index.md"), index).map_err(|e| e.to_string())?;

    let last_seq = history.last().map(|h| h.seq).unwrap_or(since);
    std::fs::write(
        &watermark_path,
        serde_json::json!({ "last_seq": last_seq }).to_string(),
    )
    .map_err(|e| e.to_string())?;

    Ok(format!(
        "promoted {} note file(s) from {} to {}",
        promotions.len(),
        since,
        last_seq
    ))
}
