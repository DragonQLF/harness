//! The tools the Director may use on Harness itself.
//!
//! These are the only way an agent can touch the app rather than a repository.
//! They are deliberately not in the run's `allowed_tools`, which means the
//! agent SDK routes every one of them through `canUseTool` first — so the
//! operator sees "the Director wants to move c_7b30 to ready" and decides,
//! exactly like any other permission request.

use std::path::Path;
use std::sync::Arc;

use harness_domain::{Actor, CardId, Command, Status};
use harness_ports::{GitPort, ToolCall, ToolReply, WorktreePath};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::workspace::Workspace;

/// Where the Director asked the window to go.
#[derive(Debug, Clone, Serialize)]
pub struct Navigation {
    pub screen: String,
    pub card_id: Option<String>,
    pub why: Option<String>,
}

fn column(raw: &str) -> Option<Status> {
    Some(match raw {
        "later" | "backlog" => Status::Backlog,
        "ready" => Status::Ready,
        "running" | "working" => Status::Running,
        "review" => Status::Review,
        "done" => Status::Done,
        _ => return None,
    })
}

fn text(input: &serde_json::Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Run one Director tool call. Every failure comes back as prose the model can
/// act on, never as a panic or a silent no-op.
pub async fn run(
    ws: &Arc<Workspace>,
    app: &AppHandle,
    project_id: Option<String>,
    call: ToolCall,
) -> ToolReply {
    // Navigation is the one tool that needs no project.
    if call.name == "open_screen" {
        let Some(screen) = text(&call.input, "screen") else {
            return ToolReply::refused("open_screen needs a screen name");
        };
        let nav = Navigation {
            screen: screen.clone(),
            card_id: text(&call.input, "card_id"),
            why: text(&call.input, "why"),
        };
        let _ = app.emit("ui://navigate", &nav);
        return ToolReply::ok(format!("opened {screen} in the operator's window"));
    }

    let Some(project_id) = project_id else {
        return ToolReply::refused(
            "no project is open, so there is no board to act on. Ask the operator to open one \
             from the switcher first.",
        );
    };
    let runtime = match ws.runtime(&project_id) {
        Ok(r) => r,
        Err(e) => return ToolReply::refused(format!("that project is not available: {e}")),
    };

    match call.name.as_str() {
        "create_card" => {
            let Some(title) = text(&call.input, "title") else {
                return ToolReply::refused("create_card needs a title");
            };
            let agent = text(&call.input, "agent_id")
                .unwrap_or_else(|| harness_app::agents::DEFAULT_WORKER.to_string());
            if ws.agent(&agent).is_none() {
                return ToolReply::refused(format!("there is no agent called {agent}"));
            }
            let start = call
                .input
                .get("start")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match crate::commands::board::create_card_inner(
                ws, &project_id, &title, &agent, start, true,
            )
            .await
            {
                Ok(created) => ToolReply::ok(format!(
                    "created {} for {agent}{}",
                    created.card_id,
                    if created.run_id.is_some() {
                        " and started it"
                    } else {
                        ", ready to start"
                    }
                )),
                Err(e) => ToolReply::refused(e),
            }
        }

        "move_card" => {
            let Some(card_id) = text(&call.input, "card_id") else {
                return ToolReply::refused("move_card needs a card_id");
            };
            let Some(to) = text(&call.input, "to").and_then(|t| column(&t)) else {
                return ToolReply::refused(
                    "move_card needs `to` as one of: later, ready, running, review, done",
                );
            };
            // Moving into `running` means starting a run, not just relabelling.
            if to == Status::Running {
                return match crate::commands::board::start_run_inner(
                    ws,
                    &project_id,
                    CardId::new(card_id.clone()),
                    None,
                )
                .await
                {
                    Ok(_) => ToolReply::ok(format!("{card_id} is running now")),
                    Err(e) => ToolReply::refused(e),
                };
            }
            match runtime
                .engine
                .execute(Command::MoveCard {
                    card_id: CardId::new(card_id.clone()),
                    to,
                })
                .await
            {
                Ok(_) => ToolReply::ok(format!("moved {card_id} to {to:?}")),
                Err(e) => ToolReply::refused(format!(
                    "that move is not allowed: {e}. The board only permits the steps in order, \
                     or an override with a reason."
                )),
            }
        }

        "approve_card" | "reject_card" => {
            let Some(card_id) = text(&call.input, "card_id") else {
                return ToolReply::refused("that needs a card_id");
            };
            let reason = text(&call.input, "reason").unwrap_or_default();
            let approving = call.name == "approve_card";
            if !approving && reason.is_empty() {
                return ToolReply::refused("sending a card back needs a reason the agent can act on");
            }
            let cmd = if approving {
                Command::ApproveCard {
                    card_id: CardId::new(card_id.clone()),
                    by: Actor::Director,
                    reason: reason.clone(),
                }
            } else {
                Command::RejectCard {
                    card_id: CardId::new(card_id.clone()),
                    reason: reason.clone(),
                    by: Actor::Director,
                }
            };
            match runtime.engine.execute(cmd).await {
                Ok(_) => ToolReply::ok(if approving {
                    format!("approved {card_id}")
                } else {
                    format!("sent {card_id} back to ready")
                }),
                Err(e) => ToolReply::refused(format!("that card cannot be reviewed now: {e}")),
            }
        }

        "delete_card" => {
            let Some(card_id) = text(&call.input, "card_id") else {
                return ToolReply::refused("delete_card needs a card_id");
            };
            let reason = text(&call.input, "reason").unwrap_or_else(|| "deleted".to_string());
            match runtime
                .engine
                .execute(Command::DiscardCard {
                    card_id: CardId::new(card_id.clone()),
                    reason,
                })
                .await
            {
                Ok(_) => ToolReply::ok(format!("deleted {card_id} and removed its worktree")),
                Err(e) => ToolReply::refused(format!(
                    "cannot delete {card_id}: {e}. A running card has to be stopped first."
                )),
            }
        }

        "read_diff" => {
            let Some(card_id) = text(&call.input, "card_id") else {
                return ToolReply::refused("read_diff needs a card_id");
            };
            let snap = match runtime.engine.snapshot().await {
                Ok(s) => s,
                Err(e) => return ToolReply::refused(e),
            };
            let Some(session) = snap.sessions.iter().find(|s| s.card_id.as_str() == card_id) else {
                return ToolReply::refused(format!(
                    "{card_id} has no worktree, so nothing has been written for it yet"
                ));
            };
            let git = Arc::clone(&runtime.git);
            let base = runtime.project.base_branch.clone();
            let against = base.clone();
            let worktree = WorktreePath(Path::new(&session.worktree).to_path_buf());
            let diff = tauri::async_runtime::spawn_blocking(move || {
                git.diff_summary(&worktree, &against)
            })
            .await;
            match diff {
                Ok(Ok(text)) if !text.trim().is_empty() => ToolReply::ok(text),
                Ok(Ok(_)) => ToolReply::ok(format!("{card_id} changed nothing against {base}")),
                Ok(Err(e)) => ToolReply::refused(format!("could not read that diff: {e}")),
                Err(e) => ToolReply::refused(format!("could not read that diff: {e}")),
            }
        }

        other => ToolReply::refused(format!("Harness has no tool called {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names_match_the_board() {
        assert_eq!(column("later"), Some(Status::Backlog));
        assert_eq!(column("backlog"), Some(Status::Backlog));
        assert_eq!(column("working"), Some(Status::Running));
        assert_eq!(column("done"), Some(Status::Done));
        assert_eq!(column("sideways"), None);
    }

    #[test]
    fn text_fields_are_trimmed_and_never_empty() {
        let input = serde_json::json!({ "a": "  x  ", "b": "   ", "c": 3 });
        assert_eq!(text(&input, "a").as_deref(), Some("x"));
        assert_eq!(text(&input, "b"), None);
        assert_eq!(text(&input, "c"), None);
        assert_eq!(text(&input, "missing"), None);
    }
}
