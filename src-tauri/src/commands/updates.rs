//! Builds parqueados e a troca do binário.
//!
//! Instalar é a única coisa nesta app que substitui o processo que a está a
//! correr, e por isso é a única que se recusa enquanto houver um agente a
//! trabalhar: o que ele tem a meio não sobrevive a um relaunch.

use std::sync::Arc;

use tauri::State;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

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


