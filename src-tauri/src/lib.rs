//! Tauri shell: wires the app-data layout, the project engines and the IPC
//! surface together. All state lives in `Workspace`; this file only assembles.

mod chat;
mod closing;
mod commands;
mod conversations;
mod director_tools;
mod events;
mod menu;
mod reflection;
mod registry;
mod review;
#[cfg(unix)]
mod shellpath;
mod sidecar;
mod update;
mod workspace;

use std::sync::Arc;

use harness_app::paths::AppPaths;
use tauri::{Manager, WindowEvent};
use workspace::Workspace;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before anything goes looking for node, npm or claude: a window opened
    // from Finder starts with launchd's PATH, which has none of them on it.
    #[cfg(unix)]
    shellpath::adopt();

    let builder = tauri::Builder::default();

    // One Relay at a time. Two of them share `com.harness.app`, which means the
    // same settings, the same event log and the same worktrees — two writers on
    // files whose whole design assumes a single one. A second launch hands its
    // arguments to the window that is already open and raises it, which is what
    // the operator meant by opening the app anyway.
    //
    // This has to be the first plugin registered: it is what decides whether
    // this process is going to live at all.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));

    // Only macOS gets a menu bar of its own: it has a place to put one that is
    // not the window. Elsewhere the window still draws its own, and adding a
    // second copy here would say everything twice.
    #[cfg(target_os = "macos")]
    let builder = builder
        .menu(menu::build)
        .on_menu_event(|app, event| menu::on_event(app, event.id().as_ref()));

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // The updater relaunches into the version it just installed.
        .plugin(tauri_plugin_process::init())
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
            // Nada nosso devia ter sobrevivido à Relay anterior, mas sobrevive:
            // uma que morra a meio de um turno — force quit, crash, o
            // instalador a reiniciá-la — deixa o sidecar e o CLI dele de pé, e
            // de pé continuam a segurar a sessão. Aqui não há execução viva
            // nenhuma ainda, portanto qualquer um deles é, por definição, um
            // resto: ninguém lhe lê o que escreve. Antes do workspace, para que
            // a primeira conversa aberta já encontre as sessões livres.
            let swept = harness_app::strays::reap_all_on_start();
            if swept > 0 {
                eprintln!("swept {swept} stray agent process(es) left by an earlier Relay");
            }
            // O registo é um actor: levantá-lo e falar com ele acontece dentro
            // do runtime. Os engines também lá nascem, e levantá-los todos
            // agora deixa a Overview contar trabalho sem visitar cada quadro.
            let handle = app.handle().clone();
            let workspace = tauri::async_runtime::block_on(async move {
                let workspace = Workspace::load(handle, paths).await;
                workspace.warm_all().await;
                workspace
            });
            // Did Relay's own source move while nobody on the board was
            // looking? Spawned, never awaited: a git call must not stand
            // between the operator and their window.
            let watching = workspace.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(said) = watching.look_for_outside_work().await {
                    eprintln!("outside the board: {said}");
                }
            });
            // Algum turno continuou sem nós? Lançado e não esperado: ligar-se a
            // um sidecar sobrevivente não pode ficar entre a operadora e a
            // janela dela.
            {
                let reopening = workspace.clone();
                tauri::async_runtime::spawn(async move {
                    crate::chat::reattach_all(&reopening).await;
                });
            }
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
                // Only the main window closing means Relay is closing. The
                // splash closes itself the moment it hands over, and without
                // this it began the whole shutdown sequence — wip commits, the
                // end-of-day look, the three-minute wait — over a window that
                // had simply finished its job.
                if window.label() != "main" {
                    return;
                }
                let Some(workspace) = window.try_state::<Arc<Workspace>>() else {
                    return;
                };
                let workspace = Arc::clone(&workspace);
                // The end-of-day look holds the window too: it runs against
                // closing on purpose (that is when the day is over), bounded
                // in time and budget. Without work and without a look due, the
                // window closes as fast as it ever did.
                let look_due = workspace.daily_look_due();
                if !workspace.settings().commit_wip_on_close && !look_due {
                    return;
                }
                // Pressing close while already closing means the operator is
                // done waiting — not that a second shutdown should start.
                if !workspace.begin_closing() {
                    api.prevent_close();
                    workspace.stop_waiting();
                    return;
                }
                // Hold the window open just long enough for running agents to
                // leave a wip commit behind — and, once a day, for the Director
                // to file what he noticed. `closing::run` narrates the wait and
                // destroys the window on every path out.
                api.prevent_close();
                let window = window.clone();
                tauri::async_runtime::spawn(closing::run(window, workspace));
            }
        })
        .invoke_handler(tauri::generate_handler![
            // board
            commands::board::snapshot,
            commands::board::create_card,
            commands::board::move_card,
            commands::board::override_card,
            commands::board::set_dependencies,
            commands::board::edit_card,
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
            // code
            commands::code::list_tree,
            commands::code::read_worktree_file,
            commands::code::diff_hunks,
            commands::code::review_hunk,
            // run history
            commands::stats::run_stats,
            // sessions
            commands::sessions::export_transcripts,
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
            commands::chat::chat_queue,
            commands::chat::conversation_totals,
            commands::chat::chat_pick_files,
            commands::chat::chat_save_attachment,
            commands::chat::chat_attachment_preview,
            commands::chat::agent_templates,
            commands::chat::agent_create_from_template,
            commands::chat::agent_duplicate,
            commands::chat::agent_remove,
            commands::chat::analyst_ask,
            commands::chat::chat_stop,
            // projects
            commands::project::projects_list,
            commands::project::project_pick_folder,
            commands::project::project_inspect,
            commands::project::project_add,
            commands::project::project_create,
            commands::project::mirror_setup,
            commands::project::project_update,
            commands::project::project_remove,
            commands::project::project_detail,
            commands::project::worktrees,
            commands::project::remove_worktree,
            commands::project::reveal_path,
            commands::project::project_checks,
            commands::project::project_set_checks,
            commands::project::project_run_checks,
            commands::project::card_checks,
            commands::project::card_run_checks,
            // system
            commands::system::bootstrap,
            commands::system::status,
            commands::system::settings_get,
            commands::system::settings_update,
            commands::crew::agents_get,
            commands::crew::agents_save,
            commands::crew::agents_stats,
            commands::approvals::approvals_pending,
            commands::approvals::respond_approval,
            commands::system::sidecar_install,
            commands::system::open_claude_terminal,
            commands::system::open_agent_terminal,
            commands::system::prepare_shutdown,
            commands::system::close_now,
            commands::project::curator_run,
            commands::crew::model_catalog,
            commands::crew::skill_offers,
            commands::crew::skill_grant,
            commands::crew::browser_offers,
            commands::crew::browser_grant,
            commands::updates::updates_list,
            commands::updates::update_install,
            // inbox
            commands::inbox::inbox_list,
            commands::inbox::inbox_accept,
            commands::inbox::inbox_dismiss,
            // menu
            menu::sync_menu,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
