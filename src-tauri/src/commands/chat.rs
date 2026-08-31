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
    Ok(ws.conversations(include_archived.unwrap_or(false)).await)
}

/// Start a new conversation. A new row means a new native Claude session, which
/// is what keeps New Chat from continuing the last one.
#[tauri::command]
pub async fn conversation_new(
    profile_id: Option<String>,
    project_id: Option<String>,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.new_conversation(profile_id, project_id).await
}

/// The conversation to talk in: the last one for this profile, or a new one.
#[tauri::command]
pub async fn conversation_open(
    profile_id: Option<String>,
    project_id: Option<String>,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.open_conversation(profile_id, project_id).await
}

#[tauri::command]
pub async fn conversation_select(
    conversation_id: String,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.select_conversation(&conversation_id).await
}

#[tauri::command]
pub async fn conversation_rename(
    conversation_id: String,
    title: String,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.rename_conversation(&conversation_id, &title).await
}

#[tauri::command]
pub async fn conversation_archive(
    conversation_id: String,
    archived: bool,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.archive_conversation(&conversation_id, archived).await
}

/// Forget a conversation and its transcript. The UI confirms first.
#[tauri::command]
pub async fn conversation_delete(conversation_id: String, ws: Shared<'_>) -> Result<(), String> {
    ws.delete_conversation(&conversation_id).await
}

/// Pin a conversation to a project, or unpin it. This decides which code it can
/// read while answering.
#[tauri::command]
pub async fn conversation_pin(
    conversation_id: String,
    project_id: Option<String>,
    ws: Shared<'_>,
) -> Result<Conversation, String> {
    ws.pin_conversation(&conversation_id, project_id).await
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

/// The thread's accounting: tokens, spend, tool calls and how full the model's
/// context is. A sibling of `conversation_transcript` rather than a field on
/// it, so the rail can refresh when a turn ends without re-reading every line
/// the thread already has on screen.
#[tauri::command]
pub async fn conversation_totals(
    conversation_id: String,
    ws: Shared<'_>,
) -> Result<harness_app::conversations::ConversationTotals, String> {
    let conversation = ws
        .conversation(&conversation_id)
        .await
        .ok_or_else(|| format!("no conversation {conversation_id}"))?;
    // Only a fallback: a transcript that recorded its own model wins.
    let profile_model = ws
        .agent_exact(&conversation.profile_id)
        .await
        .and_then(|p| p.model);
    let ws = Arc::clone(&ws);
    tauri::async_runtime::spawn_blocking(move || {
        crate::chat::totals(
            &ws,
            &conversation.id,
            conversation.cost_usd,
            conversation.priced,
            profile_model.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Say something to a conversation. The one way in.
///
/// It hands the message to the run in flight, which reads it while it works,
/// and starts an ordinary turn when there is none — answering with a null
/// `queue_id` to say which it did. There was a `chat_send` beside this once,
/// and the composer chose between them from its own idea of whether a turn was
/// running. That idea is a render behind and clears on an event that can
/// arrive early, so the choice was made on a guess about state only the
/// backend holds. Deciding it here is not a convenience: it is the only place
/// the answer cannot already be stale.
#[tauri::command]
pub async fn chat_queue(
    text: String,
    conversation_id: String,
    attachments: Option<Vec<String>>,
    // Absent is the model's own default — what every message got before
    // there was a choice.
    effort: Option<String>,
    ws: Shared<'_>,
) -> Result<crate::chat::Queued, String> {
    let ws = Arc::clone(&ws);
    crate::chat::queue(
        &ws,
        conversation_id,
        text,
        attachments.unwrap_or_default(),
        effort,
    )
    .await
}

/// Files to attach to the next message. The picker is native, so the operator
/// chooses with the same dialog they know; Relay only learns the paths.
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
/// is a menu entry, never something Relay installs by itself.
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
    ws.add_agent_from_template(&template_id).await
}

/// Copy an existing profile.
#[tauri::command]
pub async fn agent_duplicate(agent_id: String, ws: Shared<'_>) -> Result<AgentProfile, String> {
    ws.duplicate_agent(&agent_id).await
}

/// Remove a profile. The Director cannot be removed: the review loop needs it.
#[tauri::command]
pub async fn agent_remove(agent_id: String, ws: Shared<'_>) -> Result<Vec<AgentProfile>, String> {
    ws.remove_agent(&agent_id).await
}

/// The tables the Analyst reads: per project, the stats and recent activity
/// Relay already derived. JSON because it is exact, and the model reads it.
async fn analyst_tables(ws: &Arc<Workspace>, only: Option<&str>) -> Result<String, String> {
    let mut out = String::new();
    for project in ws.projects().await {
        if let Some(wanted) = only {
            if wanted != project.id {
                continue;
            }
        }
        let Ok(runtime) = ws.runtime(&project.id).await else {
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
    let conversation = ws
        .open_conversation(Some(harness_app::agents::DIRECTOR_ID.to_string()), project_id)
        .await?;
    crate::chat::send(
        &ws,
        Some(conversation.id.clone()),
        harness_app::director::analyst_prompt(&tables),
        Vec::new(),
        // Nobody is at the composer choosing: the Analyst asks its own
        // question, and the model's default is the right answer to how hard
        // to think about it.
        None,
    )
        .await?;
    Ok(conversation.id)
}



/// Stop the turn a conversation has in flight. The transcript keeps whatever
/// was already said; the busy indicator falls on the cancelled event.
#[tauri::command]
pub async fn chat_stop(conversation_id: String, ws: Shared<'_>) -> Result<(), String> {
    crate::chat::stop_turn(&ws, &conversation_id).await;
    Ok(())
}

/// Write a pasted or dropped attachment to disk, and answer with its path.
///
/// Everything downstream of here speaks in paths — `chat::send` checks the file
/// exists, and `director::with_attachments` tells the agent to read it with its
/// own tools. The clipboard speaks in bytes. This is the one place the two
/// meet, and it is why a screenshot can be pasted into the composer at all:
/// without it there is no path to attach.
///
/// The name is decided in `harness_app::attachments`, where it is testable, and
/// the MIME type decides the extension — a clipboard name is not trusted to say
/// what its own bytes are.
#[tauri::command]
pub async fn chat_save_attachment(
    name: Option<String>,
    mime: String,
    data: String,
    ws: Shared<'_>,
) -> Result<String, String> {
    use base64::Engine as _;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| "that attachment did not survive the trip from the clipboard".to_string())?;

    if let Some(why) = harness_app::attachments::refuse(&mime, bytes.len()) {
        return Err(why);
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let file_name = harness_app::attachments::file_name(name.as_deref(), &mime, stamp)
        .ok_or_else(|| format!("Relay has no name for a {mime}"))?;

    let dir = ws.paths.attachments_dir();
    let path = dir.join(&file_name);
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        Ok::<String, String>(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What an attachment looks like, for the chip that stands for it.
#[derive(serde::Serialize)]
pub struct AttachmentPreview {
    pub path: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    /// A data URI, when the file is an image small enough to inline. `None`
    /// for everything else — the chip then says what it is rather than showing
    /// a broken frame.
    pub image: Option<String>,
    /// The opening of a text file, so a pasted log or patch is recognisable
    /// without opening it. Capped hard; this is a chip, not a reader.
    pub head: Option<String>,
}

/// Enough about an attachment to draw it. A screenshot should look like the
/// screenshot, not like a path: the operator pasted a picture and the chip
/// saying `pasted-1724930400000.png` is a worse answer than the picture.
///
/// Inlining is capped well below the attach ceiling — a chip is ~40px tall and
/// a 20 MB data URI to fill it would cost more than the message it decorates.
#[tauri::command]
pub async fn chat_attachment_preview(path: String) -> Result<AttachmentPreview, String> {
    use base64::Engine as _;
    const INLINE_MAX: u64 = 4 * 1024 * 1024;
    const HEAD_BYTES: usize = 400;

    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::PathBuf::from(&path);
        let meta = std::fs::metadata(&p).map_err(|e| format!("{path}: {e}"))?;
        if !meta.is_file() {
            return Err(format!("{path} is not a file on this machine"));
        }
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let size = meta.len();

        // The extension is the only thing a picked file tells us about itself;
        // `attachments::extension_for` is the same table read the other way.
        let mime = match ext.as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            "svg" => Some("image/svg+xml"),
            "bmp" => Some("image/bmp"),
            _ => None,
        };

        let image = match mime {
            Some(mime) if size <= INLINE_MAX => std::fs::read(&p).ok().map(|bytes| {
                format!(
                    "data:{mime};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                )
            }),
            _ => None,
        };

        let head = if image.is_none()
            && matches!(ext.as_str(), "txt" | "md" | "json" | "toml" | "csv" | "log" | "patch" | "diff" | "rs" | "ts" | "tsx" | "js" | "css" | "html" | "yml" | "yaml")
        {
            std::fs::read(&p).ok().and_then(|bytes| {
                let cut = bytes.len().min(HEAD_BYTES);
                // Never cut a UTF-8 sequence in half: a chip showing U+FFFD is
                // a chip that looks broken.
                let text = String::from_utf8_lossy(&bytes[..cut]);
                let text = text.trim();
                (!text.is_empty()).then(|| text.chars().take(160).collect::<String>())
            })
        } else {
            None
        };

        Ok(AttachmentPreview { path, name, ext, size, image, head })
    })
    .await
    .map_err(|e| e.to_string())?
}
