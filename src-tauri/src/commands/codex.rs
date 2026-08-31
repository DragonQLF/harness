//! What Codex has to say about itself, and how to look at what it made.
//!
//! Two questions the rest of the app cannot answer. **What is left on the
//! plan** replaces the cost meter for an agent that does not bill per run — see
//! `harness_agent_codex::PlanUsage` for why a dollar figure would be an
//! invented number. **What does this image look like** is the other half of the
//! image tool: the path is real, but a webview cannot open it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

/// Is Codex usable on this machine, and on what?
///
/// Shaped like `ClaudeStatus` on purpose: the first-run screen asks the same
/// two questions of both — is the binary there, and is somebody logged in — and
/// a second shape would mean a second way of drawing the same answer.
#[derive(Debug, Serialize)]
pub struct CodexStatus {
    pub cli_found: bool,
    pub cli_version: Option<String>,
    pub logged_in: bool,
    /// `chatgpt` when the login is a subscription, `apikey` when it is a key.
    /// The distinction matters to what the screen can promise: only one of the
    /// two has a plan window to show.
    pub auth_mode: Option<String>,
}

#[tauri::command]
pub async fn codex_status() -> CodexStatus {
    let version = crate::sidecar::no_window(&mut std::process::Command::new("codex"))
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    // Read from disk rather than by asking the binary: `codex login status`
    // would be a second process for a question one file already answers, and
    // this runs on every visit to the Agents screen.
    let auth = codex_home().map(|h| h.join("auth.json")).and_then(|p| {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
    });
    let auth_mode = auth
        .as_ref()
        .and_then(|a| a.get("auth_mode").and_then(|m| m.as_str()))
        .map(str::to_string);
    let has_key = auth
        .as_ref()
        .and_then(|a| a.get("OPENAI_API_KEY").and_then(|k| k.as_str()))
        .is_some_and(|k| !k.is_empty())
        || std::env::var_os("OPENAI_API_KEY").is_some_and(|v| !v.is_empty());
    let has_tokens = auth
        .as_ref()
        .and_then(|a| a.pointer("/tokens/access_token"))
        .is_some();

    CodexStatus {
        cli_found: version.is_some(),
        cli_version: version,
        logged_in: has_tokens || has_key,
        // The plan's name is not read from here. It is in the id token, and
        // decoding a JWT to print "plus" when `codex_plan_usage` already
        // returns it — from the provider, at the moment it is asked — would be
        // a second, staler source for one word.
        auth_mode: auth_mode.or_else(|| has_key.then(|| "apikey".to_string())),
    }
}

/// How much of the plan this month's work has spent.
///
/// Asked of Codex rather than counted here. Relay knows what its own runs cost
/// in tokens, but the plan is spent by everything on the machine — a `codex`
/// in a terminal, the IDE extension — so a number Relay added up itself would
/// be lower than the truth and look like a bug the first time somebody
/// compared it with `/status`.
#[tauri::command]
pub async fn codex_plan_usage(
    ws: Shared<'_>,
) -> Result<harness_agent_codex::PlanUsage, String> {
    let home = ws.paths.codex_home();
    harness_agent_codex::plan_usage("codex", Some(&home)).await
}

/// One image from disk, as a data URL the window can draw.
///
/// The fence is `harness_app::preview::readable`, which is where the reasoning
/// and the test are. What this adds is the list of roots: app data (where a
/// generated image lands), Codex's own home (where one made outside Relay
/// lands), and the repositories the operator registered (where an agent may
/// have copied one). Anywhere else is refused — the path arrives inside a
/// transcript a model wrote, so it is untrusted text.
#[tauri::command]
pub async fn preview_image(path: String, ws: Shared<'_>) -> Result<String, String> {
    use base64::Engine;

    let path = PathBuf::from(&path);
    let mut roots = vec![ws.paths.root().to_path_buf()];
    if let Some(home) = codex_home() {
        roots.push(home);
    }
    roots.extend(
        ws.projects()
            .await
            .into_iter()
            .map(|p| PathBuf::from(p.path)),
    );

    if !harness_app::preview::readable(&path, &roots) {
        return Err(format!(
            "{} is not an image Relay may show. It has to be a png, jpeg, webp or gif \
             inside a registered project, Relay's own data, or Codex's.",
            path.display()
        ));
    }
    let mime = harness_app::preview::mime_for(&path).unwrap_or("image/png");
    let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if size > harness_app::preview::MAX_BYTES {
        return Err(format!(
            "that image is {} MB — too big to draw inline. Open it in a viewer instead.",
            size / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Where Codex keeps its own state, respecting an operator who has moved it.
fn codex_home() -> Option<PathBuf> {
    if let Ok(set) = std::env::var("CODEX_HOME") {
        if !set.trim().is_empty() {
            return Some(PathBuf::from(set));
        }
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| Path::new(&h).join(".codex"))
}
