//! Conversation commands. Each one is a thin translation from the UI's intent
//! to the workspace; the frontend renders what comes back and holds no truth of
//! its own.

use std::sync::Arc;

use harness_app::agents::AgentProfile;
use harness_app::conversations::Conversation;
use harness_ports::RunLogLine;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

#[tauri::command]
pub async fn conversations_list(
    include_archived: Option<bool>,
    ws: Shared<'_>,
) -> Result<Vec<Conversation>, String> {
    Ok(ws.conversations(include_archived.unwrap_or(false)))
}

/// Start a new conversation. A new row means a new native Claude session, which
/// is what keeps New Chat from continuing the last one.
#[tauri::command]
pub async fn conversation_new(
    profile_id: Option<String>,
    project_id: Option<String>,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.new_conversation(profile_id, project_id)
}

/// The conversation to talk in: the last one for this profile, or a new one.
#[tauri::command]
pub async fn conversation_open(
    profile_id: Option<String>,
    project_id: Option<String>,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.open_conversation(profile_id, project_id)
}

#[tauri::command]
pub async fn conversation_select(
    conversation_id: String,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.select_conversation(&conversation_id)
}

#[tauri::command]
pub async fn conversation_rename(
    conversation_id: String,
    title: String,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.rename_conversation(&conversation_id, &title)
}

#[tauri::command]
pub async fn conversation_archive(
    conversation_id: String,
    archived: bool,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.archive_conversation(&conversation_id, archived)
}

/// Forget a conversation and its transcript. The UI confirms first.
#[tauri::command]
pub async fn conversation_delete(conversation_id: String, ws: Shared<'_>) -> Result<(), String> {
    ws.delete_conversation(&conversation_id)
}

/// Pin a conversation to a project, or unpin it. This decides which code it can
/// read while answering.
#[tauri::command]
pub async fn conversation_pin(
    conversation_id: String,
    project_id: Option<String>,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.pin_conversation(&conversation_id, project_id)
}

/// The stored transcript, so reopening a conversation shows what was said —
/// whether or not the native session can still be resumed.
#[tauri::command]
pub async fn conversation_transcript(
    conversation_id: String,
    ws: Shared<'_>,
) -> Result<Vec<RunLogLine>, String> {
    let ws = Arc::clone(&ws);
    tauri::async_runtime::spawn_blocking(move || crate::chat::transcript(&ws, &conversation_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Send a message. The answer streams back on the run channel, keyed by the
/// conversation id.
#[tauri::command]
pub async fn chat_send(
    text: String,
    conversation_id: Option<String>,
    attachments: Option<Vec<String>>,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    let ws = Arc::clone(&ws);
    crate::chat::send(&ws, conversation_id, text, attachments.unwrap_or_default()).await
}

/// Files to attach to the next message. The picker is native, so the operator
/// chooses with the same dialog they know; Harness only learns the paths.
#[tauri::command]
pub async fn chat_pick_files(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Attach files to this message")
        .pick_files(move |picked| {
            let _ = tx.send(picked.unwrap_or_default());
        });
    let picked = rx
        .await
        .map_err(|_| "the file picker closed".to_string())?;
    Ok(picked.into_iter().map(|p| p.to_string()).collect())
}

/// Profiles the operator can create from. Returned on request only: a template
/// is a menu entry, never something Harness installs by itself.
#[tauri::command]
pub async fn agent_templates() -> Result<Vec<AgentProfile>, String> {
    Ok(harness_app::agents::templates())
}

/// Add a profile from a template, under an id nobody is using.
#[tauri::command]
pub async fn agent_create_from_template(
    template_id: String,
    ws: Shared<'_>,
) -> Result<AgentProfile, String> {
    ws.add_agent_from_template(&template_id)
}

/// Copy an existing profile.
#[tauri::command]
pub async fn agent_duplicate(agent_id: String, ws: Shared<'_>) -> Result<AgentProfile, String> {
    ws.duplicate_agent(&agent_id)
}

/// Remove a profile. The Director cannot be removed: the review loop needs it.
#[tauri::command]
pub async fn agent_remove(agent_id: String, ws: Shared<'_>) -> Result<Vec<AgentProfile>, String> {
    ws.remove_agent(&agent_id)
}

/// The tables the Analyst reads: per project, the stats and recent activity
/// Harness already derived. JSON because it is exact, and the model reads it.
async fn analyst_tables(ws: &Arc<Workspace>, only: Option<&str>) -> Result<String, String> {
    let mut out = String::new();
    for project in ws.projects() {
        if let Some(wanted) = only {
            if &wanted != &project.id {
                continue;
            }
        }
        let Ok(runtime) = ws.runtime(&project.id) else {
            continue;
        };
        let cards = runtime.engine.snapshot().await?.cards;
        let store = std::sync::Arc::clone(&runtime.store);
        let history = tauri::async_runtime::spawn_blocking(move || {
            harness_ports::StorePort::read_all(store.as_ref()).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;

        let stats = harness_app::insights::project_stats(&history, &cards, 0);
        let rows = harness_app::insights::activity(&history, &cards, 60);
        out.push_str(&format!(
            "### {} ({})\nstats: {}\nrecent: {}\n\n",
            project.name,
            project.id,
            serde_json::to_string(&stats).unwrap_or_default(),
            serde_json::to_string(&rows).unwrap_or_default(),
        ));
    }
    Ok(out)
}

/// Ask the Analyst: opens (or reuses) a Director conversation, hands it the
/// precomputed tables, and the answer streams back like any other chat. The
/// model interprets numbers it was given — it never computes its own.
#[tauri::command]
pub async fn analyst_ask(
    project_id: Option<String>,
    ws: Shared<'_>,
) -> Result<String, String> {
    let tables = analyst_tables(&ws, project_id.as_deref()).await?;
    if tables.trim().is_empty() {
        return Err("no projects to analyse yet".to_string());
    }
    let conversation = ws.open_conversation(
        Some(harness_app::agents::DIRECTOR_ID.to_string()),
        project_id,
    )?;
    crate::chat::send(
        &ws,
        Some(conversation.id.clone()),
        harness_app::director::analyst_prompt(&tables),
        Vec::new(),
    )
        .await?;
    Ok(conversation.id)
}



/// Stop the turn a conversation has in flight. The transcript keeps whatever
/// was already said; the busy indicator falls on the cancelled event.
#[tauri::command]
pub async fn chat_stop(conversation_id: String, ws: Shared<'_>) -> Result<(), String> {
    crate::chat::stop_turn(&ws, &conversation_id);
    Ok(())
}
