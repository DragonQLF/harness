//! Taking a project's run transcripts off the machine.
//!
//! The picker and the filesystem live here; which files an export is made of,
//! where the folder lands and what it is called are decided in
//! `harness_app::insights`, where the tests are.

use std::path::PathBuf;
use std::sync::Arc;

use harness_app::insights::{self, TranscriptExport};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

/// Copy run transcripts into a folder the operator picks.
///
/// `run_ids` names the runs to take; absent means every transcript this project
/// has on disk. Returns `None` when the picker was dismissed — a cancelled
/// export is a decision, not a failure, and the screen should not shout at one.
#[tauri::command]
pub async fn export_transcripts(
    project_id: String,
    run_ids: Option<Vec<String>>,
    app: tauri::AppHandle,
    ws: Shared<'_>,
) -> Result<Option<TranscriptExport>, String> {
    let runtime = ws.runtime(&project_id).await?;
    let run_log = Arc::clone(&runtime.run_log);
    let runs_dir = ws.paths.runs_dir(&project_id);

    // Naming a transcript file is the run log's rule; asked for, never rebuilt.
    let sources: Vec<(String, PathBuf)> = match run_ids {
        Some(ids) => ids
            .into_iter()
            .map(|id| {
                let path = run_log.path_of(&id);
                (id, path)
            })
            .collect(),
        None => on_disk(&runs_dir),
    };
    if sources.is_empty() {
        return Err("this project has no recorded transcript to export".to_string());
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Where should the transcripts go?")
        .pick_folder(move |picked| {
            let _ = tx.send(picked);
        });
    let Some(dest) = rx.await.map_err(|_| "the folder picker closed".to_string())? else {
        return Ok(None);
    };
    let dest = dest
        .into_path()
        .map_err(|e| format!("that folder cannot be written to: {e}"))?;

    let name = insights::export_folder_name(&project_id);
    tauri::async_runtime::spawn_blocking(move || {
        insights::export_transcripts(&dest, &name, &sources).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Every transcript in the runs directory, paired with the run id its file is
/// named for. A directory that is not there yet is simply no runs.
fn on_disk(runs_dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(runs_dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|p| {
            let id = p.file_stem()?.to_str()?.to_string();
            Some((id, p))
        })
        .collect();
    out.sort();
    out
}
