//! The tools an agent may use on Relay itself.
//!
//! These are the only way an agent can touch the app rather than a repository.
//! The ones that *change* something are deliberately not in the run's
//! `allowed_tools`, which means the agent SDK routes them through `canUseTool`
//! first — so the operator sees "the Director wants to move c_7b30 to ready"
//! and decides, exactly like any other permission request. Reading and
//! navigating are granted outright, because they change nothing (decision #29).
//!
//! Scope: a conversation is pinned to at most one project, but the Director
//! watches every board, so every board tool takes an optional `project_id` and
//! falls back to the pinned one. Without that it could describe work in a
//! project it had no way to touch.
//!
//! O ficheiro era um só, e um `match` de mil linhas sobre nomes de ferramentas
//! é uma lista de assuntos, não um assunto. Está partido por **quem é dono do
//! que muda**: o quadro, a tripulação, as concessões, os projectos, e o que o
//! Relay sabe de si próprio. O que fica aqui é a única coisa comum às cinco —
//! o guardo da delegação, a escolha do projecto, e a tabela que diz qual das
//! cinco atende cada nome.

mod board;
mod crew;
mod grants;
mod knowledge;
mod projects;

use std::sync::Arc;

use harness_ports::{ToolCall, ToolReply};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::workspace::Workspace;

/// Where the agent asked the window to go.
#[derive(Debug, Clone, Serialize)]
pub struct Navigation {
    pub screen: String,
    pub card_id: Option<String>,
    pub why: Option<String>,
}

fn text(input: &serde_json::Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A list of non-empty strings under a key, or nothing. Empty entries are
/// dropped rather than stored: a declared tool called "" is not a tool the
/// operator can have read on the approval sheet.
fn strings(input: &serde_json::Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Tools that only tell the agent something. Everything else needs the profile
/// to be allowed to delegate. The mirror tools read our own logs or write into
/// our own inbox — nothing on any board, so they ride free like `record_decision`
/// (#76's justification: our layer, reversible).
fn is_read_only(name: &str) -> bool {
    matches!(
        name,
        "open_screen"
            | "read_diff"
            | "list_projects"
            | "record_decision"
            | "self_report"
            | "read_docs"
            | "propose_improvement"
    )
}

/// carried out here, against the same engine commands the UI uses. Shared by
/// operator chats and the end-of-day look so there is one wiring, not two.
pub fn runner(
    ws: &Arc<Workspace>,
    pinned_project: Option<String>,
    delegating: bool,
    caller: String,
) -> harness_ports::ToolRunner {
    let tool_ws = Arc::clone(ws);
    let tool_app = ws.app_handle();
    Arc::new(move |call| {
        let ws = Arc::clone(&tool_ws);
        let app = tool_app.clone();
        let project = pinned_project.clone();
        let caller = caller.clone();
        Box::pin(async move {
            crate::director_tools::run(&ws, &app, project, delegating, &caller, call).await
        })
    })
}

/// Run one tool call. Every failure comes back as prose the model can act on,
/// never as a panic or a silent no-op.
///
/// `pinned_project` is the project this conversation can read; a call may name
/// another one. `delegating` is whether this profile may change a board at all,
/// and `caller` is the profile asking — which is what the self-elevation guard
/// in `grants` compares against its target.
pub async fn run(
    ws: &Arc<Workspace>,
    app: &AppHandle,
    pinned_project: Option<String>,
    delegating: bool,
    caller: &str,
    call: ToolCall,
) -> ToolReply {
    if !delegating && !is_read_only(&call.name) {
        // Configuration, never roles: the reader may BE the Director.
        return ToolReply::refused(
            "this profile does not have delegation enabled - turn on \"can delegate\" \
             in the agent settings to let this profile change boards",
        );
    }

    // Navegar e listar não precisam de projecto nenhum: são as que respondem
    // antes de haver um sobre que agir.
    match call.name.as_str() {
        "open_screen" => {
                let Some(screen) = text(&call.input, "screen") else {
                    return ToolReply::refused("open_screen needs a screen name");
                };
                let nav = Navigation {
                    screen: screen.clone(),
                    card_id: text(&call.input, "card_id"),
                    why: text(&call.input, "why"),
                };
                let _ = app.emit(crate::events::NAVIGATE, &nav);
                return ToolReply::ok(format!("opened {screen} in the operator's window"));
        }
        "list_projects" => return projects::list_projects(ws).await,
        "create_project" => return projects::create_project(ws, &call).await,
        "self_report" => return knowledge::self_report(ws, &call),
        "read_docs" => return knowledge::read_docs(ws, &call).await,
        "propose_improvement" => return knowledge::propose_improvement(ws, &call),
        _ => {}
    }

    // Everything below acts on one board.
    //
    // A profile fenced to one project (`AgentProfile::project`) resolves to it
    // when nothing is named, and is refused when something else is. The fence
    // is checked here because here is where every board tool passes: putting it
    // in each handler would mean a tool added later quietly escapes it.
    let fenced = ws.agent_exact(caller).await.and_then(|a| a.project);
    let named = text(&call.input, "project_id");
    if let (Some(fence), Some(asked)) = (fenced.as_deref(), named.as_deref()) {
        if asked != fence {
            return ToolReply::refused(format!(
                "this profile is fenced to {fence} and cannot act on {asked}. Ask the operator to \
                 move the fence on the Agents screen, or hand the work to whoever owns that board."
            ));
        }
    }
    let project_id = match named.or(fenced).or(pinned_project) {
        Some(id) => id,
        None => {
            // Three ways out, said in order: name one, have it opened, or —
            // for something new being built from scratch — create the project
            // this work should have belonged to all along.
            return ToolReply::refused(
                "there is no project to act on. Pass project_id, ask the operator to open one, \
                 or — if this is something new to build — propose create_project and ask where \
                 it should live. list_projects shows what exists.",
            )
        }
    };
    if ws.project(&project_id).await.is_none() {
        return ToolReply::refused(format!(
            "there is no project called {project_id}. Call list_projects to see the real ids."
        ));
    }

    // `record_decision` escreve na memória do projecto, não no quadro dele, e
    // por isso não espera pelo engine.
    if call.name == "record_decision" {
        return knowledge::record_decision(ws, &project_id, &call);
    }

    let runtime = match ws.runtime(&project_id).await {
        Ok(r) => r,
        Err(e) => return ToolReply::refused(format!("that project is not available: {e}")),
    };
    let where_ = format!(" in {project_id}");

    match call.name.as_str() {
        "create_card" => board::create_card(ws, &project_id, &where_, &call).await,
        "message_agent" => board::message_agent(&runtime, &where_, &call).await,
        "edit_card" => board::edit_card(&runtime, &where_, &call).await,
        "move_card" => board::move_card(ws, &runtime, &project_id, &where_, &call).await,
        "approve_card" | "reject_card" => board::review_card(&runtime, &where_, &call).await,
        "delete_card" => board::delete_card(&runtime, &call).await,
        "read_diff" => board::read_diff(&runtime, &call).await,

        "work_on_relay" => projects::work_on_relay(ws).await,

        "add_endpoint" => crew::add_endpoint(ws, &call).await,
        "create_agent" => crew::create_agent(ws, &call).await,
        "edit_agent" => crew::edit_agent(ws, &call).await,
        "set_agent_model" => crew::set_agent_model(ws, &call).await,

        "grant_agent_tools" => grants::grant_agent_tools(ws, caller, &call).await,
        "install_skill" => grants::install_skill(ws, &call).await,
        "add_mcp_server" => grants::add_mcp_server(ws, caller, &call).await,
        "revoke_grant" => grants::revoke_grant(ws, &call).await,

        other => ToolReply::refused(format!("Relay has no tool called {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_fields_are_trimmed_and_never_empty() {
        let input = serde_json::json!({ "a": "  x  ", "b": "   ", "c": 3 });
        assert_eq!(text(&input, "a").as_deref(), Some("x"));
        assert_eq!(text(&input, "b"), None);
        assert_eq!(text(&input, "c"), None);
        assert_eq!(text(&input, "missing"), None);
    }

    /// Uma lista declarada com uma entrada vazia não é uma lista que o operador
    /// possa ter lido na folha de aprovação.
    #[test]
    fn empty_entries_never_become_declarations() {
        let input = serde_json::json!({ "tools": ["Read", "  ", "", "Write"], "none": 3 });
        assert_eq!(strings(&input, "tools"), vec!["Read", "Write"]);
        assert!(strings(&input, "none").is_empty());
        assert!(strings(&input, "missing").is_empty());
    }

    #[test]
    fn only_reading_and_navigating_are_open_to_every_profile() {
        for open in [
            "open_screen",
            "read_diff",
            "list_projects",
            "record_decision",
            "self_report",
            "read_docs",
            "propose_improvement",
        ] {
            assert!(is_read_only(open), "{open} should need no delegation");
        }
        for guarded in [
            "create_card",
            "edit_card",
            "move_card",
            "approve_card",
            "reject_card",
            "message_agent",
            "delete_card",
            "create_project",
            "create_agent",
            "add_endpoint",
            "work_on_relay",
            "set_agent_model",
            "edit_agent",
            "grant_agent_tools",
            // The three grants. A skill is markdown entering another agent's
            // prompt, a server is arbitrary code with that agent's
            // permissions, and a tool is plain elevation: none of them is a
            // read, and none of them rides free.
            "install_skill",
            "add_mcp_server",
            "revoke_grant",
        ] {
            assert!(!is_read_only(guarded), "{guarded} must need delegation");
        }
    }
}
