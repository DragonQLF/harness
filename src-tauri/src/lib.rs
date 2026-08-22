use std::sync::Arc;

use harness_agent_claude::ClaudeCliAgent;
use harness_domain::{CardId, Command, Status};
use harness_engine::{Engine, EngineConfig, EngineHandle, Snapshot};
use harness_git_cli::{CliGit, ensure_workspace};
use harness_ports::{ClockPort, StorePort};
use harness_store_jsonl::JsonlStore;
use tauri::{Emitter, Manager, State};

struct EngineState(EngineHandle);

struct SystemClock;

impl ClockPort for SystemClock {
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

#[tauri::command]
async fn create_card(title: String, state: State<'_, EngineState>) -> Result<u64, String> {
    let cmd = Command::CreateCard {
        card_id: CardId::new(uuid::Uuid::new_v4().to_string()),
        title,
    };
    state.0.execute(cmd).await
}

#[tauri::command]
async fn move_card(
    card_id: String,
    to: Status,
    state: State<'_, EngineState>,
) -> Result<u64, String> {
    state
        .0
        .execute(Command::MoveCard {
            card_id: CardId::new(card_id),
            to,
        })
        .await
}

#[tauri::command]
async fn override_card(
    card_id: String,
    to: Status,
    reason: String,
    state: State<'_, EngineState>,
) -> Result<u64, String> {
    state
        .0
        .execute(Command::OverrideCard {
            card_id: CardId::new(card_id),
            to,
            reason,
        })
        .await
}

#[tauri::command]
async fn start_run(card_id: String, prompt: String, state: State<'_, EngineState>) -> Result<String, String> {
    state
        .0
        .start_run(CardId::new(card_id), prompt)
        .await
        .map(|run_id| run_id.0)
}

#[tauri::command]
async fn cancel_run(card_id: String, state: State<'_, EngineState>) -> Result<(), String> {
    state.0.cancel_run(CardId::new(card_id)).await
}

#[tauri::command]
async fn snapshot(state: State<'_, EngineState>) -> Result<Snapshot, String> {
    state.0.snapshot().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let workspace_dir = data_dir.join("workspace");
            let store = Arc::new(JsonlStore::open(data_dir.join("events.jsonl"))?);
            let git = Arc::new(CliGit::new(&workspace_dir));
            ensure_workspace(&workspace_dir)?;

            let history = store.read_all()?;
            let agent = Arc::new(ClaudeCliAgent::new("claude"));
            let config = EngineConfig {
                repo_root: workspace_dir.clone(),
                base_branch: "main".to_string(),
            };

            let (engine, mut logged_rx, mut runs_rx) =
                tauri::async_runtime::block_on(async {
                    Engine::spawn(store, Arc::new(SystemClock), agent, git, config, history)
                });
            app.manage(EngineState(engine));

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(envelope) = logged_rx.recv().await {
                    let _ = app_handle.emit("engine://event", &envelope);
                }
            });

            let app_handle2 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(update) = runs_rx.recv().await {
                    let _ = app_handle2.emit("engine://run", &update);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_card,
            move_card,
            override_card,
            start_run,
            cancel_run,
            snapshot
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
