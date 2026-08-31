//! Making a picture.
//!
//! The one thing Codex has that a Claude agent wants. OpenAI's image model is
//! reachable two ways: the Images API, which wants an `OPENAI_API_KEY`, and
//! Codex's own built-in tool, which wants nothing — its skill file says so in
//! as many words, and it spends the ChatGPT plan this machine is already
//! logged into. Relay already speaks to that binary for whole runs, so the tool
//! is that same adapter asked for one turn.
//!
//! Which means every agent gets it, on either backend. A Claude agent calls
//! `generate_image` and a Codex process makes the picture; a Codex agent
//! already has the tool natively and never reaches this file.
//!
//! **The file is not put anywhere.** Codex saves under its own home and hands
//! back a path; that path is what the agent is told, and what it writes into
//! its answer as markdown. Copying it into the repository would be Relay
//! deciding an image is an asset — which is the agent's call, made with a
//! `cp` it can already run.

use std::sync::Arc;

use harness_ports::{ToolCall, ToolReply};

use super::text;
use crate::workspace::Workspace;

/// Long enough for a real generation, short enough that a wedged process does
/// not hold a conversation open all afternoon. Measured: a plain icon took
/// about a minute end to end, most of it the model deciding rather than the
/// image rendering.
const TIMEOUT_SECS: u64 = 300;

pub(super) async fn generate_image(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
    let Some(prompt) = text(&call.input, "prompt") else {
        return ToolReply::refused("generate_image needs a prompt describing the image");
    };

    // Codex refuses to start outside a directory it can read. The app-data root
    // is Relay's own and always exists, which is the right answer for a tool
    // that is not working on any particular repository — and it keeps the
    // sandbox pointed away from whatever worktree the caller happens to be in.
    let cwd = ws.paths.root().to_path_buf();
    let home = ws.paths.codex_home();

    let made = tokio::time::timeout(
        std::time::Duration::from_secs(TIMEOUT_SECS),
        harness_agent_codex::generate_image("codex", Some(&home), &prompt, &cwd),
    )
    .await;

    match made {
        Ok(Ok(path)) => ToolReply::ok(format!(
            "Saved to {path}. Show it by writing `![a short description]({path})` in your \
             answer — Relay renders that inline. Copy it into the repository yourself if it \
             belongs there.",
        )),
        Ok(Err(why)) => ToolReply::refused(format!("Codex could not make that image: {why}")),
        Err(_) => ToolReply::refused(format!(
            "Codex did not finish the image in {TIMEOUT_SECS}s. Nothing was saved."
        )),
    }
}
