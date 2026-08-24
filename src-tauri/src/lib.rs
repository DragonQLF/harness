//! Tauri shell: wires the app-data layout, the project engines and the IPC
//! surface together. All state lives in `Workspace`; this file only assembles.

mod chat;
mod commands;
mod director_tools;
mod sidecar;
mod update;
mod workspace;

use std::sync::Arc;

use harness_app::paths::AppPaths;
use tauri::{Manager, WindowEvent};
use workspace::Workspace;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = AppPaths::new(app.path().app_data_dir()?)?;
            // Rollback first, before anything else: if the last update never
            // got healthy, this process is running on the restored binary and
            // the operator must be told why.
            let rollback = update::recover_if_needed(
                &update::default_marker(paths.root()),
                &std::env::current_exe().map_err(|e| e.to_string())?,
                &update::previous_binary_path(),
            );
            if let Some(reason) = &rollback {
                eprintln!("{reason}");
            }
            let workspace = Workspace::load(app.handle().clone(), paths);
            // Engines spawn tokio tasks, so bring them up inside the runtime.
            // Starting them all now lets the overview count work across
            // projects without visiting each board first.
            let warming = workspace.clone();
            tauri::async_runtime::block_on(async move { warming.warm_all() });
            app.manage(workspace);
            // Setup made it to the end: this launch is healthy. The marker —
            // if this very boot was an update — can go.
            if let Ok(paths) = AppPaths::new(app.path().app_data_dir()?) {
                update::mark_healthy(&update::default_marker(paths.root()));
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let Some(workspace) = window.try_state::<Arc<Workspace>>() else {
                    return;
                };
                let workspace = Arc::clone(&workspace);
                if !workspace.settings().commit_wip_on_close {
                    return;
                }
                // Hold the window open just long enough for running agents to
                // leave a wip commit behind.
                api.prevent_close();
                let window = window.clone();
                tauri::async_runtime::spawn(async move {
                    workspace.shutdown().await;
                    let _ = window.destroy();
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            // board
            commands::board::snapshot,
            commands::board::create_card,
            commands::board::move_card,
            commands::board::override_card,
            commands::board::set_dependencies,
            commands::board::assign_agent,
            commands::board::approve_card,
            commands::board::reject_card,
            commands::board::discard_card,
            commands::board::start_run,
            commands::board::cancel_run,
            commands::board::active_runs,
            commands::board::run_log,
            commands::board::card_diff,
            commands::board::review_queue,
            commands::board::activity,
            commands::board::project_stats,
            // conversations
            commands::chat::conversations_list,
            commands::chat::conversation_new,
            commands::chat::conversation_open,
            commands::chat::conversation_select,
            commands::chat::conversation_rename,
            commands::chat::conversation_archive,
            commands::chat::conversation_delete,
            commands::chat::conversation_pin,
            commands::chat::conversation_transcript,
            commands::chat::chat_send,
            commands::chat::agent_templates,
            commands::chat::agent_create_from_template,
            commands::chat::agent_duplicate,
            commands::chat::agent_remove,
            commands::chat::analyst_ask,
            // conversations
            commands::chat::conversations_list,
            commands::chat::conversation_new,
            commands::chat::conversation_open,
            commands::chat::conversation_select,
            commands::chat::conversation_rename,
            commands::chat::conversation_archive,
            commands::chat::conversation_delete,
            commands::chat::conversation_pin,
            commands::chat::conversation_transcript,
            commands::chat::chat_send,
            commands::chat::agent_templates,
            commands::chat::agent_create_from_template,
            commands::chat::agent_duplicate,
            commands::chat::agent_remove,
            commands::chat::analyst_ask,
            // projects
            commands::project::projects_list,
            commands::project::project_pick_folder,
            commands::project::project_inspect,
            commands::project::project_add,
            commands::project::project_create,
            commands::project::project_update,
            commands::project::project_remove,
            commands::project::project_detail,
            commands::project::worktrees,
            commands::project::remove_worktree,
            commands::project::reveal_path,
            commands::project::project_checks,
            commands::project::project_set_checks,
            commands::project::project_run_checks,
            // system
            commands::system::bootstrap,
            commands::system::status,
            commands::system::settings_get,
            commands::system::settings_update,
            commands::system::agents_get,
            commands::system::agents_save,
            commands::system::agents_stats,
            commands::system::approvals_pending,
            commands::system::respond_approval,
            commands::system::sidecar_install,
            commands::system::open_claude_terminal,
            commands::system::open_agent_terminal,
            commands::system::prepare_shutdown,
            commands::system::updates_list,
            commands::system::update_install,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
