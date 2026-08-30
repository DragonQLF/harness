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

use std::path::Path;
use std::sync::Arc;

use harness_domain::{Actor, CardId, Command, Status};
use harness_ports::{ClockPort, GitPort, Reviewer, ToolCall, ToolReply, WorktreePath};
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

/// Where a new card is born, as the two flags `create_card_inner` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Birth {
    start: bool,
    ready: bool,
}

/// Read the column a card was asked for. The Director asked for `later` and
/// got `ready`, then had to call `move_card` — two permission sheets for one
/// action, twice in one session. `None` for a column that cannot hold a new
/// card; an absent column keeps the old default, which was `ready`.
fn birth(asked: Option<&str>, start_flag: bool) -> Option<Birth> {
    let landing = match asked {
        None => Status::Ready,
        Some(raw) => match column(raw) {
            Some(s @ (Status::Backlog | Status::Ready | Status::Running)) => s,
            // Review and Done are where a run leaves a card, never where one
            // starts: a card born there has no run and no diff behind it.
            _ => return None,
        },
    };
    let start = start_flag || landing == Status::Running;
    Some(Birth {
        start,
        ready: start || landing == Status::Ready,
    })
}

/// The endpoints an agent could be pointed at, for a refusal that tells the
/// model what to send instead of only what was wrong.
fn endpoint_names(providers: &[harness_app::providers::Provider]) -> String {
    let mut names: Vec<String> = vec!["anthropic".to_string()];
    names.extend(providers.iter().map(|p| p.id.clone()));
    names.join(", ")
}

/// What to say when the endpoint the operator named is not set up, or is set up
/// without a key. Both end the same way: somebody has to open Settings, and
/// pointing at it beats describing it.
fn missing_endpoint(asked: &str, providers: &[harness_app::providers::Provider]) -> String {
    format!(
        "there is no model endpoint called {asked}. Configured: {}. Adding one is a \
         click in Settings under Model endpoints — take them there with \
         open_screen(\"settings\") rather than describing it, then ask for the key.",
        endpoint_names(providers)
    )
}

/// Appended when the endpoint exists but has nothing to authenticate with.
fn key_warning(provider: &harness_app::providers::Provider) -> String {
    if !provider.needs_key() {
        return String::new();
    }
    format!(
        " — but {} has no key, so every run on it is refused before it starts. Take \
         them to Settings with open_screen(\"settings\") and ask them to paste one.",
        provider.name
    )
}

/// " on qwen3.5 via Ollama Cloud", or " on the Claude login".
fn describe_model(
    agent: &harness_app::agents::AgentProfile,
    providers: &[harness_app::providers::Provider],
) -> String {
    let where_ = harness_app::providers::find(providers, &agent.provider)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "the Claude login".to_string());
    match agent.model.as_deref() {
        Some(model) if !model.is_empty() => format!(" on {model} via {where_}"),
        _ => format!(" on {where_}"),
    }
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

/// The ToolRunner handed to a conversational run: every harness tool call is
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
/// another one. `delegating` is whether this profile may change a board at all.
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
        let projects = ws.projects().await;
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

    // The mirror: what happened to the agents, counted by code. Counts and
    // one example per pattern, never the raw log.
    if call.name == "self_report" {
        let days = call
            .input
            .get("days")
            .and_then(|v| v.as_u64())
            .map(|d| d.clamp(1, 30) as u32)
            .unwrap_or(7);
        let report = ws.collect_self_report(days);
        return ToolReply::ok(harness_app::selfreport::render(&report));
    }

    // Designed versus done: the two records that say so live in the harness
    // repository's docs/ folder. Reading is capped and searchable; the whole
    // decision log does not fit in a reply and should not try.
    if call.name == "read_docs" {
        let Some(docs) = ws.harness_docs_dir().await else {
            return ToolReply::refused(
                "the harness repository is not registered as a project here, so DEBT.md and \
                 DECISIONS.md are out of reach — ask the operator to add it",
            );
        };
        let doc = match text(&call.input, "doc").as_deref().and_then(harness_app::devdocs::Doc::parse) {
            Some(d) => d,
            None => {
                return ToolReply::refused("read_docs needs doc as \"debt\" or \"decisions\"");
            }
        };
        return match harness_app::devdocs::render(&docs, doc, text(&call.input, "find").as_deref())
        {
            Ok(rendered) => ToolReply::ok(rendered),
            Err(e) => ToolReply::refused(e),
        };
    }

    // A proposal, not a card: it lands in the operator's inbox and dies there
    // unless they accept it. Accepting mints nothing either — it is permission,
    // handed back to him in his next turn to act on.
    if call.name == "propose_improvement" {
        let title = text(&call.input, "title").unwrap_or_default();
        let observation = text(&call.input, "observation").unwrap_or_default();
        let suggestion = text(&call.input, "proposal").unwrap_or_default();
        if title.is_empty() || observation.is_empty() || suggestion.is_empty() {
            return ToolReply::refused(
                "propose_improvement needs title, observation (what you saw — one occurrence is \
                 enough) and proposal (the correction)",
            );
        }
        return match ws.propose_improvement(&title, &observation, &suggestion) {
            Ok(_) => ToolReply::ok(
                "filed in the operator's inbox — they decide whether it becomes work; announce \
                 that you proposed it",
            ),
            Err(e) => ToolReply::refused(e),
        };
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
        return match ws.create_project(&parent, &name).await {
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
    if ws.project(&project_id).await.is_none() {
        return ToolReply::refused(format!(
            "there is no project called {project_id}. Call list_projects to see the real ids."
        ));
    }

    // A decision made in conversation dies with the conversation unless it
    // lands on disk the moment it happens. Dated, append-only, in the
    // project's own memory — outside any repository (#59).
    if call.name == "record_decision" {
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
    let runtime = match ws.runtime(&project_id).await {
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
            let Some(profile) = ws.agent_exact(&agent).await else {
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
            let flag = call
                .input
                .get("start")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let Some(Birth { start, ready }) =
                birth(text(&call.input, "column").as_deref(), flag)
            else {
                return ToolReply::refused(
                    "create_card takes column as later, ready or running. A card cannot be born \
                     in review or done — those are where a run leaves it.",
                );
            };
            match crate::commands::board::create_card_inner(
                ws, &project_id, &title, &agent, start, ready,
            )
            .await
            {
                Ok(created) => {
                    // Closing the loop on an accepted proposal: this is the
                    // only place the card he was given permission to make can
                    // be tied back to the permission, and without the tie the
                    // acceptance would be raised at him for ever.
                    let acted = text(&call.input, "proposal_id").and_then(|id| {
                        ws.record_proposal_action(&id, &project_id, created.card_id.as_str())
                    });
                    ToolReply::ok(format!(
                        "created {} for {agent}{where_}{}{}",
                        created.card_id,
                        if created.run_id.is_some() {
                            " and started it"
                        } else if ready {
                            ", ready to start"
                        } else {
                            ", in later"
                        },
                        match acted {
                            Some(p) => format!(
                                " — the accepted proposal {} is now carried out and will stop \
                                 being raised",
                                p.id
                            ),
                            None => String::new(),
                        }
                    ))
                }
                Err(e) => ToolReply::refused(e),
            }
        }

        // Correcting a badly written card, rather than discarding it and
        // losing the id, the history, the session and the dependencies that
        // point at it. The domain refuses it once the card has run.
        "message_agent" => {
            let Some(card_id) = text(&call.input, "card_id") else {
                return ToolReply::refused("that needs a card_id");
            };
            let Some(said) = text(&call.input, "text") else {
                return ToolReply::refused("that needs something to say");
            };
            match runtime.engine.message_run(CardId::new(card_id.clone()), said).await {
                Ok(_) => ToolReply::ok(format!(
                    "said to {card_id}{where_}; it reads that at its next read, without stopping"
                )),
                Err(e) => ToolReply::refused(e),
            }
        }

        "edit_card" => {
            let Some(card_id) = text(&call.input, "card_id") else {
                return ToolReply::refused("edit_card needs a card_id");
            };
            let Some(title) = text(&call.input, "title") else {
                return ToolReply::refused("edit_card needs the title it should have instead");
            };
            match runtime
                .engine
                .execute(Command::EditCard {
                    card_id: CardId::new(card_id.clone()),
                    title: title.clone(),
                })
                .await
            {
                Ok(_) => ToolReply::ok(format!("{card_id} now reads \"{title}\"{where_}")),
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
                    hunks: Vec::new(),
                }
            } else {
                Command::RejectCard {
                    card_id: CardId::new(card_id.clone()),
                    reason: reason.clone(),
                    by: Actor::Director,
                    hunks: Vec::new(),
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

        // Both of these change the crew, so neither is read-only: they arrive
        // at the operator's permission sheet like a card move does. The
        // operator asked for the Director to be able to do this; they did not
        // ask to stop being told about it.
        // Asking to work on Relay itself should not mean being told to go and
        // register a repository first. The Director does what the Projects
        // screen would have done, and the operator sees the same permission
        // sheet either way.
        "work_on_relay" => {
            match crate::commands::project::ensure_mirror(ws).await {
                Ok(project) => ToolReply::ok(format!(
                    "{} is now Relay's own source, at {}. Cards for the app go there,                      including the ones you make from accepted proposals, and read_docs reads its docs/.",
                    project.name, project.path
                )),
                Err(e) => ToolReply::refused(e),
            }
        }

        // Adding the row, but never the key. A conversation is written to disk
        // as a transcript, so a tool that accepted a key would be a tool that
        // wrote it there — and the operator pasting it into chat "because the
        // Director asked" is a habit worth not teaching. The row lands empty
        // and the key is typed once, into a password field, on the screen the
        // Director can open for them.
        "add_endpoint" => {
            let named = text(&call.input, "name").unwrap_or_default();
            let template = harness_app::providers::templates()
                .into_iter()
                .find(|t| {
                    t.id.eq_ignore_ascii_case(&named) || t.name.eq_ignore_ascii_case(&named)
                });
            let url = text(&call.input, "base_url");
            let (name, base_url) = match (&template, url) {
                (Some(t), url) => (t.name.clone(), url.unwrap_or_else(|| t.base_url.clone())),
                (None, Some(url)) if !named.is_empty() => (named.clone(), url),
                (None, _) => {
                    return ToolReply::refused(
                        "add_endpoint needs a known name — ollama, ollama-cloud or openrouter —                          or a name and a base_url for something else that speaks the Anthropic                          Messages protocol",
                    )
                }
            };

            let mut settings = ws.settings();
            if let Some(existing) = settings
                .providers
                .iter()
                .find(|p| p.base_url.trim_end_matches('/') == base_url.trim_end_matches('/'))
            {
                return ToolReply::refused(format!(
                    "{} already points at {base_url}{}",
                    existing.name,
                    if existing.needs_key() {
                        " — it just has no key yet"
                    } else {
                        ""
                    }
                ));
            }

            let taken: Vec<harness_app::providers::Provider> = settings.providers.clone();
            let id = template
                .as_ref()
                .map(|t| t.id.clone())
                .filter(|id| !taken.iter().any(|p| &p.id == id))
                .unwrap_or_else(|| harness_app::providers::unique_id(&name, &taken));

            settings.providers.push(harness_app::providers::Provider {
                id: id.clone(),
                name: name.clone(),
                base_url: base_url.clone(),
                token: String::new(),
            });
            match ws.set_settings(settings) {
                Ok(_) => ToolReply::ok(format!(
                    "added {name} ({id}) at {base_url}. It has no key yet, and every run on it                      is refused until it does — open the settings screen for them and ask them                      to paste one into its key field. Do not ask them to send it to you here:                      this conversation is written to disk."
                )),
                Err(e) => ToolReply::refused(e),
            }
        }

        "create_agent" => {
            let Some(name) = text(&call.input, "name") else {
                return ToolReply::refused("create_agent needs a name");
            };
            let taken: Vec<String> = ws.agents().await.into_iter().map(|a| a.id).collect();
            if ws.agents().await.iter().any(|a| a.name.eq_ignore_ascii_case(&name)) {
                return ToolReply::refused(format!(
                    "there is already an agent called {name}; use set_agent_model to change                      the one that exists, or pick another name"
                ));
            }
            let mut made = harness_app::agents::drafted(
                &name,
                &text(&call.input, "title").unwrap_or_default(),
                &text(&call.input, "brief").unwrap_or_default(),
                &taken,
            );
            // The model is the point of asking, so it is set here rather than
            // left for a second round trip.
            if let Some(model) = text(&call.input, "model") {
                made.model = Some(model);
            }
            if let Some(provider) = text(&call.input, "provider") {
                let settings = ws.settings();
                if harness_app::providers::find(&settings.providers, &provider).is_none() {
                    return ToolReply::refused(missing_endpoint(&provider, &settings.providers));
                }
                made.provider = provider;
            }
            let providers = ws.settings().providers;
            let warning = harness_app::providers::find(&providers, &made.provider)
                .map(key_warning)
                .unwrap_or_default();
            let summary = format!(
                "created {} ({}){}",
                made.name,
                made.id,
                describe_model(&made, &providers)
            );
            let mut crew = ws.agents().await;
            crew.push(made);
            match ws.set_agents(crew).await {
                Ok(_) => ToolReply::ok(format!(
                    "{summary}{warning}. It can read and search; anything more is yours to                      grant on the Agents screen."
                )),
                Err(e) => ToolReply::refused(e),
            }
        }

        "edit_agent" => {
            let Some(agent_id) = text(&call.input, "agent_id") else {
                return ToolReply::refused("edit_agent needs an agent_id");
            };
            let mut crew = ws.agents().await;
            let known: Vec<String> = crew.iter().map(|a| a.id.clone()).collect();
            let Some(slot) = crew.iter_mut().find(|a| a.id == agent_id) else {
                return ToolReply::refused(format!(
                    "there is no agent called {agent_id}. The crew is: {}",
                    known.join(", ")
                ));
            };

            let mut changed: Vec<String> = Vec::new();
            if let Some(name) = text(&call.input, "name") {
                changed.push(format!("name to {name}"));
                slot.name = name;
            }
            if let Some(title) = text(&call.input, "title") {
                changed.push("title".to_string());
                slot.title = title;
            }
            if let Some(brief) = text(&call.input, "brief") {
                changed.push("brief".to_string());
                slot.brief = brief;
            }
            if let Some(budget) = call.input.get("budget_usd").and_then(|v| v.as_f64()) {
                if budget <= 0.0 {
                    return ToolReply::refused(
                        "a budget of zero would stop every run before it started;                          leave it out to keep the current one",
                    );
                }
                changed.push(format!("budget to ${budget:.2}"));
                slot.budget_usd = Some(budget);
            }
            if let Some(paused) = call.input.get("paused").and_then(|v| v.as_bool()) {
                changed.push(if paused { "paused it".into() } else { "resumed it".to_string() });
                slot.paused = paused;
            }
            if let Some(reviewer) = text(&call.input, "reviewer") {
                slot.reviewer = match reviewer.as_str() {
                    "director" => Reviewer::Director,
                    "human" | "you" | "operator" => Reviewer::Human,
                    "nobody" | "none" => Reviewer::Nobody,
                    other => {
                        return ToolReply::refused(format!(
                            "{other} is not a reviewer. Use director, human or nobody."
                        ))
                    }
                };
                changed.push(format!("reviewer to {reviewer}"));
            }

            if changed.is_empty() {
                return ToolReply::refused(
                    "edit_agent was given nothing to change. Name the fields to set;                      tools and permissions are not among them.",
                );
            }
            let summary = format!("{}: changed {}", slot.name, changed.join(", "));
            match ws.set_agents(crew).await {
                Ok(_) => ToolReply::ok(summary),
                Err(e) => ToolReply::refused(e),
            }
        }

        "grant_agent_tools" => {
            let Some(agent_id) = text(&call.input, "agent_id") else {
                return ToolReply::refused("grant_agent_tools needs an agent_id");
            };
            // Not a hard approval — a refusal. An agent that hands itself Bash
            // has stopped having limits, and there is no version of that
            // question the operator can usefully be asked, because answering it
            // once removes the thing that would ask again.
            if let Some(refusal) =
                harness_app::grants::self_elevation_guard(&call.name, caller, &agent_id)
            {
                return ToolReply::refused(refusal.to_string());
            }
            let Some(asked) = call.input.get("tools").and_then(|v| v.as_array()) else {
                return ToolReply::refused(
                    "grant_agent_tools needs `tools`: the full list the agent should have                      afterwards, not the ones to add",
                );
            };
            let known = &harness_app::agents::ALL_PERMISSIONS;
            let mut wanted: Vec<String> = Vec::new();
            for value in asked {
                let Some(raw) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                    continue;
                };
                // Match the crew's own spelling rather than whatever case the
                // model used, so "shell" and "Shell" are the same grant.
                match known.iter().find(|k| k.eq_ignore_ascii_case(raw)) {
                    Some(canonical) => {
                        let canonical = canonical.to_string();
                        if !wanted.contains(&canonical) {
                            wanted.push(canonical);
                        }
                    }
                    None => {
                        return ToolReply::refused(format!(
                            "{raw} is not a tool an agent can hold. They are: {}",
                            known.join(", ")
                        ))
                    }
                }
            }

            let mut crew = ws.agents().await;
            let known_ids: Vec<String> = crew.iter().map(|a| a.id.clone()).collect();
            let Some(slot) = crew.iter_mut().find(|a| a.id == agent_id) else {
                return ToolReply::refused(format!(
                    "there is no agent called {agent_id}. The crew is: {}",
                    known_ids.join(", ")
                ));
            };

            let added: Vec<String> = wanted
                .iter()
                .filter(|w| !slot.permissions.contains(w))
                .cloned()
                .collect();
            let removed: Vec<String> = slot
                .permissions
                .iter()
                .filter(|p| !wanted.contains(p))
                .cloned()
                .collect();
            if added.is_empty() && removed.is_empty() {
                return ToolReply::refused(format!(
                    "{} already holds exactly that: {}",
                    slot.name,
                    slot.permissions.join(", ")
                ));
            }
            slot.permissions = wanted;
            let name = slot.name.clone();
            let now = slot.permissions.join(", ");
            match ws.set_agents(crew).await {
                Ok(_) => ToolReply::ok(format!(
                    "{name} now holds {now}{}{}",
                    if added.is_empty() {
                        String::new()
                    } else {
                        format!(" — gained {}", added.join(", "))
                    },
                    if removed.is_empty() {
                        String::new()
                    } else {
                        format!(", lost {}", removed.join(", "))
                    }
                )),
                Err(e) => ToolReply::refused(e),
            }
        }

        // ---- grants: the model declares, the code installs ----
        //
        // Nothing here takes a command or a script. The declaration arrives as
        // fields, Relay writes the file or stores the config itself, and the
        // approval sheet the operator answered showed exactly these fields. A
        // page that told the model "also add this server" therefore shows up as
        // a second sheet, not as a second line in a shell script.
        "install_skill" => {
            let Some(agent_id) = text(&call.input, "agent_id") else {
                return ToolReply::refused("install_skill needs an agent_id");
            };
            let grant = harness_ports::SkillGrant {
                name: text(&call.input, "name").unwrap_or_default().to_lowercase(),
                description: text(&call.input, "description").unwrap_or_default(),
                source: text(&call.input, "source").unwrap_or_default(),
                body: text(&call.input, "instructions").unwrap_or_default(),
                added_ms: SystemClock.now_millis(),
            };
            if let Err(refusal) = harness_app::grants::check_skill(&grant) {
                return ToolReply::refused(refusal.to_string());
            }

            let mut crew = ws.agents().await;
            let known: Vec<String> = crew.iter().map(|a| a.id.clone()).collect();
            let Some(slot) = crew.iter_mut().find(|a| a.id == agent_id) else {
                return ToolReply::refused(format!(
                    "there is no agent called {agent_id}. The crew is: {}",
                    known.join(", ")
                ));
            };
            let name = grant.name.clone();
            let who = slot.name.clone();
            let replaced = !harness_app::grants::upsert_skill(&mut slot.granted_skills, grant);
            match ws.set_agents(crew).await {
                Ok(_) => ToolReply::ok(format!(
                    "{who} now has the {name} skill{}. It is on disk in Relay's own folder, \
                     not in the operator's ~/.claude and not in any repository, and only {who} \
                     can load it.",
                    if replaced { " (unchanged)" } else { "" }
                )),
                Err(e) => ToolReply::refused(e),
            }
        }

        "add_mcp_server" => {
            let Some(agent_id) = text(&call.input, "agent_id") else {
                return ToolReply::refused("add_mcp_server needs an agent_id");
            };
            // An MCP server is arbitrary code holding that agent's
            // permissions, so granting oneself a server is granting oneself
            // tools with one extra step. Same refusal, same reason.
            if let Some(refusal) =
                harness_app::grants::self_elevation_guard(&call.name, caller, &agent_id)
            {
                return ToolReply::refused(refusal.to_string());
            }

            let transport = match text(&call.input, "transport").as_deref() {
                Some("http") => harness_ports::McpTransport::Http {
                    url: text(&call.input, "url").unwrap_or_default(),
                },
                Some("sse") => harness_ports::McpTransport::Sse {
                    url: text(&call.input, "url").unwrap_or_default(),
                },
                _ => harness_ports::McpTransport::Stdio {
                    command: text(&call.input, "command").unwrap_or_default(),
                    args: strings(&call.input, "args"),
                },
            };
            let grant = harness_ports::McpGrant {
                name: text(&call.input, "name").unwrap_or_default().to_lowercase(),
                transport,
                // Names only: a key asked for in a conversation is a key on
                // disk, which is why `add_endpoint` refuses them too. The
                // operator fills the values on the Agents screen.
                env: strings(&call.input, "env_names")
                    .into_iter()
                    .map(|k| (k, String::new()))
                    .collect(),
                tools: strings(&call.input, "tools"),
                source: text(&call.input, "source").unwrap_or_default(),
                added_ms: SystemClock.now_millis(),
            };
            if let Err(refusal) = harness_app::grants::check_mcp(&grant) {
                return ToolReply::refused(refusal.to_string());
            }

            let mut crew = ws.agents().await;
            let known: Vec<String> = crew.iter().map(|a| a.id.clone()).collect();
            let Some(slot) = crew.iter_mut().find(|a| a.id == agent_id) else {
                return ToolReply::refused(format!(
                    "there is no agent called {agent_id}. The crew is: {}",
                    known.join(", ")
                ));
            };
            let name = grant.name.clone();
            let who = slot.name.clone();
            let missing = harness_app::grants::missing_env(&grant);
            harness_app::grants::upsert_mcp(&mut slot.mcp_servers, grant);
            match ws.set_agents(crew).await {
                Ok(_) => ToolReply::ok(format!(
                    "{who} can now reach the {name} server; its tools arrive as \
                     mcp__{name}__<tool> and each call still asks the operator.{}",
                    if missing.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " It will not connect until they fill in {} on the Agents screen — \
                             say so, and do not ask them for the value here.",
                            missing.join(", ")
                        )
                    }
                )),
                Err(e) => ToolReply::refused(e),
            }
        }

        "revoke_grant" => {
            let Some(agent_id) = text(&call.input, "agent_id") else {
                return ToolReply::refused("revoke_grant needs an agent_id");
            };
            let Some(name) = text(&call.input, "name") else {
                return ToolReply::refused("revoke_grant needs the name to remove");
            };
            let skill = text(&call.input, "kind").as_deref() != Some("mcp");

            let mut crew = ws.agents().await;
            let known: Vec<String> = crew.iter().map(|a| a.id.clone()).collect();
            let Some(slot) = crew.iter_mut().find(|a| a.id == agent_id) else {
                return ToolReply::refused(format!(
                    "there is no agent called {agent_id}. The crew is: {}",
                    known.join(", ")
                ));
            };
            let before = if skill {
                slot.granted_skills.len()
            } else {
                slot.mcp_servers.len()
            };
            if skill {
                slot.granted_skills.retain(|g| g.name != name);
            } else {
                slot.mcp_servers.retain(|g| g.name != name);
            }
            let after = if skill {
                slot.granted_skills.len()
            } else {
                slot.mcp_servers.len()
            };
            if before == after {
                return ToolReply::refused(format!("{} has nothing called {name}", slot.name));
            }
            let who = slot.name.clone();
            match ws.set_agents(crew).await {
                Ok(_) => ToolReply::ok(format!("{name} is no longer available to {who}")),
                Err(e) => ToolReply::refused(e),
            }
        }

        "set_agent_model" => {
            let Some(agent_id) = text(&call.input, "agent_id") else {
                return ToolReply::refused("set_agent_model needs an agent_id");
            };
            let settings = ws.settings();
            let mut crew = ws.agents().await;
            let Some(slot) = crew.iter_mut().find(|a| a.id == agent_id) else {
                return ToolReply::refused(format!(
                    "there is no agent called {agent_id}. The crew is: {}",
                    ws.agents().await
                        .iter()
                        .map(|a| a.id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            };
            if let Some(provider) = text(&call.input, "provider") {
                // The empty string is the Anthropic login, and is spelled
                // "anthropic" here so the model never has to send a blank.
                if provider.eq_ignore_ascii_case("anthropic") {
                    slot.provider = harness_app::providers::ANTHROPIC.to_string();
                } else if harness_app::providers::find(&settings.providers, &provider).is_none() {
                    return ToolReply::refused(missing_endpoint(&provider, &settings.providers));
                } else {
                    slot.provider = provider;
                }
            }
            if let Some(model) = text(&call.input, "model") {
                slot.model = Some(model);
            }
            let summary = format!(
                "{} now runs{}{}",
                slot.name,
                describe_model(slot, &settings.providers),
                harness_app::providers::find(&settings.providers, &slot.provider)
                    .map(key_warning)
                    .unwrap_or_default()
            );
            match ws.set_agents(crew).await {
                Ok(_) => ToolReply::ok(summary),
                Err(e) => ToolReply::refused(e),
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

        other => ToolReply::refused(format!("Relay has no tool called {other}")),
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

    /// The two-approvals bug: a card asked for in `later` must be born there,
    /// with no `move_card` behind it.
    #[test]
    fn a_card_is_born_in_the_column_that_was_asked_for() {
        assert_eq!(birth(Some("later"), false), Some(Birth { start: false, ready: false }));
        assert_eq!(birth(Some("backlog"), false), Some(Birth { start: false, ready: false }));
        assert_eq!(birth(Some("ready"), false), Some(Birth { start: false, ready: true }));
        // `running` is a run starting, not a label.
        assert_eq!(birth(Some("running"), false), Some(Birth { start: true, ready: true }));
        // No column at all keeps what every caller got before this existed.
        assert_eq!(birth(None, false), Some(Birth { start: false, ready: true }));
        // The old `start` flag still wins over any column that is not running.
        assert_eq!(birth(Some("later"), true), Some(Birth { start: true, ready: true }));
        // Review and Done are where a run leaves a card, not where one starts.
        assert_eq!(birth(Some("review"), false), None);
        assert_eq!(birth(Some("done"), false), None);
        assert_eq!(birth(Some("sideways"), false), None);
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
