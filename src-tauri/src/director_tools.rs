//! The tools an agent may use on Harness itself.
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

use std::path::Path;
use std::sync::Arc;

use harness_domain::{Actor, CardId, Command, Status};
use harness_ports::{GitPort, ToolCall, ToolReply, WorktreePath};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::workspace::{SystemClock, Workspace};

/// Where the agent asked the window to go.
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

/// Tools that only tell the agent something. Everything else needs the profile
/// to be allowed to delegate.
fn is_read_only(name: &str) -> bool {
    matches!(
        name,
        "open_screen" | "read_diff" | "list_projects" | "record_decision"
    )
}

/// UTC date as YYYY-MM-DD from a millisecond stamp (Howard Hinnant's
/// civil_from_days). No chrono dependency for one filename.
fn utc_date_string(now_ms: u64) -> String {
    let days = (now_ms / 1000 / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Run one tool call. Every failure comes back as prose the model can act on,
/// never as a panic or a silent no-op.
///
/// `pinned_project` is the project this conversation can read; a call may name
/// another one. `delegating` is whether this profile may change a board at all.
pub async fn run(
    ws: &Arc<Workspace>,
    app: &AppHandle,
    pinned_project: Option<String>,
    delegating: bool,
    call: ToolCall,
) -> ToolReply {
    if !delegating && !is_read_only(&call.name) {
        // Configuration, never roles: the reader may BE the Director.
        return ToolReply::refused(format!(
            "this profile does not have delegation enabled - turn on \"can delegate\" \
             in the agent settings to let this profile change boards",
        ));
    }

    // Navigation and the project list need no project of their own.
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

    if call.name == "list_projects" {
        let projects = ws.projects();
        if projects.is_empty() {
            return ToolReply::ok(
                "There are no projects yet. create_project makes one (a git repository with a \
                 board); most questions do not need one.",
            );
        }
        let mut out = String::new();
        for project in projects {
            let live = Path::new(&project.path).is_dir();
            out.push_str(&format!(
                "- {} (id {}) at {}{}{}\n",
                project.name,
                project.id,
                project.path,
                if project.paused { " — paused" } else { "" },
                if live { "" } else { " — folder is missing" }
            ));
        }
        return ToolReply::ok(out);
    }

    if call.name == "create_project" {
        let Some(name) = text(&call.input, "name") else {
            return ToolReply::refused("create_project needs a name");
        };
        let Some(parent) = text(&call.input, "parent_path") else {
            return ToolReply::refused(
                "create_project needs parent_path: the folder to create the project inside. Ask \
                 the operator where it should live rather than guessing.",
            );
        };
        return match ws.create_project(&parent, &name) {
            Ok(project) => ToolReply::ok(format!(
                "created {} (id {}) at {} — a git repository with an empty board",
                project.name, project.id, project.path
            )),
            Err(e) => ToolReply::refused(format!("could not create that project: {e}")),
        };
    }

    // Everything below acts on one board.
    let named = text(&call.input, "project_id");
    let project_id = match named.or(pinned_project) {
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
    if ws.project(&project_id).is_none() {
        return ToolReply::refused(format!(
            "there is no project called {project_id}. Call list_projects to see the real ids."
        ));
    }

    // A decision made in conversation dies with the conversation unless it
    // lands on disk the moment it happens. Dated, append-only, in the
    // project's own memory — outside any repository (#59).
    if call.name == "record_decision" {
        use harness_ports::ClockPort;
        let title = text(&call.input, "title").unwrap_or_default();
        let content = text(&call.input, "content").unwrap_or_default();
        if title.trim().is_empty() || content.trim().is_empty() {
            return ToolReply::refused("record_decision needs a title and content");
        }
        let dir = ws
            .paths
            .project_dir(&project_id)
            .join("memory")
            .join("decisions");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return ToolReply::refused(format!("could not create the memory folder: {e}"));
        }
        let now_ms = SystemClock.now_millis();
        let date = utc_date_string(now_ms);
        let slug: String = {
            let cleaned: String = title
                .trim()
                .to_lowercase()
                .chars()
                .take(40)
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            cleaned.trim_matches('-').to_string()
        };
        let mut n = 1;
        loop {
            let candidate = dir.join(format!("{date}-{slug}-{n:02}.md"));
            if !candidate.exists() {
                if let Err(e) =
                    std::fs::write(&candidate, format!("# {title}\n\n{content}\n"))
                {
                    return ToolReply::refused(format!(
                        "could not write the decision: {e}"
                    ));
                }
                return ToolReply::ok(format!(
                    "recorded as {} - announce that you wrote it",
                    candidate.display()
                ));
            }
            n += 1;
        }
    }
    let runtime = match ws.runtime(&project_id) {
        Ok(r) => r,
        Err(e) => return ToolReply::refused(format!("that project is not available: {e}")),
    };
    let where_ = format!(" in {project_id}");

    match call.name.as_str() {
        "create_card" => {
            let Some(title) = text(&call.input, "title") else {
                return ToolReply::refused("create_card needs a title");
            };
            let agent = text(&call.input, "agent_id")
                .unwrap_or_else(|| harness_app::agents::DEFAULT_WORKER.to_string());
            let Some(profile) = ws.agent_exact(&agent) else {
                return ToolReply::refused(format!(
                    "there is no agent called {agent}. The crew is configured on the Agents screen."
                ));
            };
            if !profile.can_take_work() {
                return ToolReply::refused(format!(
                    "{} cannot be given cards{}",
                    profile.name,
                    if profile.paused {
                        " — it is paused"
                    } else {
                        " — task execution is turned off on its profile"
                    }
                ));
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
                    "created {} for {agent}{where_}{}",
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
                    Ok(_) => ToolReply::ok(format!("{card_id} is running now{where_}")),
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
                Ok(_) => ToolReply::ok(format!("moved {card_id} to {to:?}{where_}")),
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
                    format!("approved {card_id}{where_}")
                } else {
                    format!("sent {card_id} back to ready{where_}")
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

    #[test]
    fn only_reading_and_navigating_are_open_to_every_profile() {
        for open in ["open_screen", "read_diff", "list_projects"] {
            assert!(is_read_only(open), "{open} should need no delegation");
        }
        for guarded in [
            "create_card",
            "move_card",
            "approve_card",
            "reject_card",
            "delete_card",
            "create_project",
        ] {
            assert!(!is_read_only(guarded), "{guarded} must need delegation");
        }
    }
}
