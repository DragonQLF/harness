//! Conversation commands. Each one is a thin translation from the UI's intent
//! to the workspace; the frontend renders what comes back and holds no truth of
//! its own.

use std::sync::Arc;

use harness_app::agents::AgentProfile;
use harness_app::conversations::Conversation;
use harness_ports::RunLogLine;
use tauri::State;

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
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    let ws = Arc::clone(&ws);
    crate::chat::send(&ws, conversation_id, text).await
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
