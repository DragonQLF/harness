//! Conversations: the operator talking to a profile, and the persistence that
//! lets them pick the thread back up after a restart.
//!
//! This replaces the hand-rolled block that used to live in `workspace.rs`. It
//! sits at workspace level, not in a project engine, for the reason recorded in
//! decision #19: one Director watches every board, and the engine has no notion
//! of a conversation. Nothing here touches the board directly — that goes
//! through `director_tools`, which reuses the same engine commands the UI does.
//!
//! Two things make a conversation survive:
//!
//! - the **native Claude session id**, captured from the SDK and handed back as
//!   `resume_session` on the next message, so the model keeps its own history;
//! - the **run log**, the same `RunLogPort` every run transcript already uses,
//!   so the words are readable even when the native session is gone.
//!
//! There is deliberately no second copy of the transcript: the index says which
//! session and which file, and the file holds the words.

use std::path::PathBuf;
use std::sync::Arc;

use harness_agent_claude::ClaudeCliAgent;
use harness_agent_sidecar::SidecarAgent;
use harness_app::agents;
use harness_app::conversations::Conversation;
use harness_app::director::{self, ChatContext, Speaker};
use harness_domain::{CardId, RunId};
use harness_engine::RunUpdate;
use harness_ports::{
    AgentPort, RunEvent, RunLogLine, RunLogPort, RunOutcome, RunSpec, ToolRunner,
};
use tauri::Emitter;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::sidecar;
use crate::workspace::{SwitchingAgent, SystemClock};
use crate::workspace::Workspace;
use harness_ports::ClockPort;

/// Relay tools that only read, navigate, or write into our own layer.
/// Granted outright, because none of them changes a board — see decisions #29
/// and #76. Everything that *changes* a board is absent on purpose, so it
/// reaches the approver. The mirror tools (`self_report`, `read_docs`,
/// `propose_improvement`) count our own history and file proposals; a proposal
/// is not an action on the world until the operator accepts it.
pub(crate) const READ_ONLY_TOOLS: [&str; 7] = [
    "mcp__harness__open_screen",
    "mcp__harness__read_diff",
    "mcp__harness__list_projects",
    "mcp__harness__record_decision",
    "mcp__harness__self_report",
    "mcp__harness__read_docs",
    "mcp__harness__propose_improvement",
];

/// Send one message. Returns as soon as the run is under way: the answer
/// arrives on the `engine://run` channel, keyed by the conversation id.
pub async fn send(
    ws: &Arc<Workspace>,
    conversation_id: Option<String>,
    text: String,
    attachments: Vec<String>,
) -> Result<Conversation, String> {
    // A file that is not there is worse than no file: the model would go
    // looking and report a failure the operator caused. Refuse now, by name.
    for file in &attachments {
        if !PathBuf::from(file).is_file() {
            return Err(format!("{file} is not a file on this machine"));
        }
    }
    let message = director::with_attachments(&text, &attachments);
    if message.is_empty() {
        return Err("nothing to send".to_string());
    }

    // Which conversation, and which profile is speaking.
    let conversation = match conversation_id {
        Some(id) => ws
            .conversation(&id)
            .await
            .ok_or_else(|| format!("no conversation {id}"))?,
        None => ws.open_conversation(None, None).await?,
    };
    let profile = ws
        .agent_exact(&conversation.profile_id)
        .await
        .ok_or_else(|| format!("no agent profile called {}", conversation.profile_id))?;
    if !profile.can_chat() {
        return Err(if profile.paused {
            format!("{} is paused", profile.name)
        } else {
            format!("{} is not set up for conversations", profile.name)
        });
    }

    let settings = ws.settings();
    // Every board it watches, with the pinned one marked: that is what makes
    // one Director rather than one per project.
    let briefs = ws.project_briefs(conversation.project_id.as_deref()).await?;

    // Reading code only makes sense inside a project, and only the one this
    // conversation is pinned to.
    let repo = match conversation.project_id.as_deref() {
        Some(id) => ws.project(id).await,
        None => None,
    }
    .filter(|p| PathBuf::from(&p.path).is_dir());
    let cwd = repo
        .as_ref()
        .map(|p| PathBuf::from(&p.path))
        .unwrap_or_else(|| ws.paths.root().to_path_buf());

    let crew: Vec<(String, String)> = ws
        .agents()
        .await
        .into_iter()
        .filter(|a| a.id != profile.id && a.can_take_work())
        .map(|a| (a.id, a.title))
        .collect();

    let resume_session = conversation.resumes().map(str::to_string);
    // global.md: small, always in the prompt on a fresh session.
    let global_memory =
        harness_app::memory::global_for(ws.paths.root()).unwrap_or_default();
    let outside_work = ws.outside_work().await;
    // A versão só se diz quando mudou. Numa sessão retomada nada mais lho
    // contaria: as ferramentas aparecem-lhe na lista sem explicação, e deduzir
    // uma actualização pelo efeito é a pior maneira de a saber.
    let running = env!("CARGO_PKG_VERSION");
    let new_version = (conversation.seen_version.as_deref() != Some(running)).then_some(running);
    if new_version.is_some() {
        ws.record_chat_version(&conversation.id, running).await;
    }
    let prompt = director::chat_prompt(
        &ChatContext {
            speaker: Speaker {
                name: &profile.name,
                title: &profile.title,
                brief: &profile.brief,
                is_director: profile.id == agents::DIRECTOR_ID,
                can_delegate: profile.can_delegate,
                expected_output: &profile.expected_output,
            },
            user_name: &settings.user_name,
            projects: &briefs,
            repo: repo.as_ref().map(|p| p.name.as_str()),
            // Identity and history live in the session being resumed; sending
            // them again would have it start the conversation over.
            resumed: resume_session.is_some(),
            crew: &crew,
            global_memory: &global_memory,
            // Only what the last look found; the look itself runs at startup
            // and at the close, never on a turn the operator is waiting for.
            outside_work: outside_work.as_deref(),
            new_version,
        },
        &message,
    );

    // The operator's own turn goes into the transcript first, so a conversation
    // reads as a conversation rather than as a list of answers.
    let now = SystemClock.now_millis();
    ws.append_chat_line(
        &conversation.id,
        RunLogLine {
            ts_ms: now,
            event: RunEvent::UserMessage {
                text: message.clone(),
            },
        },
    );
    let conversation = ws.record_chat_message(&conversation.id, &message).await?;

    // Relay's own tools. The mutating ones are not in `allowed_tools`, so the
    // SDK sends each call through the approver first: the operator sees "the
    // Director wants to move c_7b30" before anything moves.
    let delegating = profile.can_delegate;
    let tools: ToolRunner = crate::director_tools::runner(
        ws,
        conversation.project_id.clone(),
        delegating,
        profile.id.clone(),
    );

    let mut allowed_tools = profile.allowed_tools();
    allowed_tools.extend(READ_ONLY_TOOLS.iter().map(|t| t.to_string()));
    allowed_tools.sort();
    allowed_tools.dedup();

    let spec = RunSpec {
        provider: harness_app::providers::find(&settings.providers, &profile.provider)
            .and_then(|p| p.resolve()),
        prompt,
        cwd,
        model: profile.model.clone(),
        allowed_tools: Some(allowed_tools),
        max_budget_usd: profile.budget_usd,
        // `dontAsk` denies anything outside `allowed_tools` without ever
        // consulting the approver, which would make the board tools dead
        // (decision #27). `manual` routes them to the operator instead.
        permission_mode: Some("manual".to_string()),
        approver: Some(ws.router.approver_for(
            conversation.project_id.as_deref().unwrap_or("workspace"),
        )),
        resume_session: resume_session.clone(),
        tools: Some(tools),
        // The operator watches it think while it works, so give it room to.
        thinking_tokens: Some(4000),
        // A conversation acts through Relay's own tools; no fan-out, and
        // nothing to report — its work is the conversation itself.
        subagents: false,
        report_work: false,
        // O porto desta conversa já foi construído por perfil, com as
        // concessões dentro; vazio aqui quer dizer "usa as dele".
        grants: harness_ports::Grants::default(),
    };

    // What this profile was granted, resolved now rather than held anywhere:
    // an approval that landed a minute ago is in the profile, so the next turn
    // carries it. Nothing is inherited — a profile with no grants gets exactly
    // the isolated run it got before any of this existed.
    let agent = crate::chat::granted_agent_for(
        ws,
        harness_app::grants::for_profile(ws.paths.root(), &profile),
    );
    let (ev_tx, mut ev_rx) = mpsc::channel::<RunEvent>(64);
    // Registered by conversation id so the operator has a stop: a turn that
    // never emits `done` must not leave them without an exit.
    let token = CancellationToken::new();
    ws.register_chat_turn(&conversation.id, token.clone()).await;
    let fut = agent.run(spec, ev_tx, token);

    let app = ws.app_handle();
    let ws = Arc::clone(ws);
    let conversation_id = conversation.id.clone();
    let project_id = conversation.project_id.clone().unwrap_or_default();
    let resumed = resume_session.is_some();

    tauri::async_runtime::spawn(async move {
        // Did this run actually reach a live session? A resume of a session
        // that no longer exists comes back with no `started`, no text and an
        // error result — so this is what separates "the thread continued" from
        // "there was nothing to continue".
        let answered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        // Published in the same shape as any run and keyed by the conversation
        // id, so the UI has one typed listener for both.
        let publish = {
            let app = app.clone();
            let conversation_id = conversation_id.clone();
            let project_id = project_id.clone();
            move |event: RunEvent| {
                let _ = app.emit(
                    "engine://run",
                    RunUpdate {
                        project_id: project_id.clone(),
                        card_id: CardId::new(conversation_id.clone()),
                        run_id: RunId(conversation_id.clone()),
                        ts_ms: SystemClock.now_millis(),
                        event,
                    },
                );
            }
        };

        let seen = std::sync::Arc::clone(&answered);
        let forward = async {
            while let Some(ev) = ev_rx.recv().await {
                // The session id is the whole reason this conversation can be
                // continued tomorrow. It arrives twice — on init and on the
                // result — and either is worth keeping.
                match &ev {
                    RunEvent::Started { session_id } => {
                        seen.store(true, std::sync::atomic::Ordering::Relaxed);
                        ws.record_chat_session(&conversation_id, session_id).await;
                    }
                    RunEvent::Text { .. } | RunEvent::Delta { .. } => {
                        seen.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    RunEvent::Done {
                        session_id,
                        cost_usd,
                        ..
                    } => {
                        if let Some(sid) = session_id {
                            ws.record_chat_session(&conversation_id, sid).await;
                        }
                        ws.record_chat_cost(&conversation_id, *cost_usd).await;
                    }
                    _ => {}
                }
                // Deltas are for the live view only; the `Text` that follows is
                // what the transcript keeps (decision #25).
                if !ev.is_ephemeral() {
                    ws.append_chat_line(
                        &conversation_id,
                        RunLogLine {
                            ts_ms: SystemClock.now_millis(),
                            event: ev.clone(),
                        },
                    );
                }
                publish(ev);
            }
        };

        let (result, _) = tokio::join!(fut, forward);

        // The UI clears its thinking state on the last event, so one always
        // goes out.
        match result {
            Ok(RunOutcome::Completed { .. }) => {}
            Ok(RunOutcome::Cancelled) => {
                let event = RunEvent::Done {
                    session_id: None,
                    cost_usd: None,
                    turns: None,
                    result: None,
                    error: None,
                };
                ws.append_chat_line(
                    &conversation_id,
                    RunLogLine {
                        ts_ms: SystemClock.now_millis(),
                        event: event.clone(),
                    },
                );
                publish(event);
            }
            Ok(RunOutcome::Failed { message, .. }) | Err(message) => {
                // A resume that could not be honoured is the one failure worth
                // explaining rather than just reporting: the thread above is
                // still readable, but the model no longer remembers it.
                let lost = resumed && !answered.load(std::sync::atomic::Ordering::Relaxed);
                if lost {
                    ws.record_chat_resume_failure(&conversation_id).await;
                    let notice = RunEvent::Notice {
                        text: format!(
                            "the Claude session for this conversation could not be resumed \
                             ({message}). Everything above is still here to read, but the model \
                             has lost its own memory of it — your next message starts a new \
                             session."
                        ),
                    };
                    ws.append_chat_line(
                        &conversation_id,
                        RunLogLine {
                            ts_ms: SystemClock.now_millis(),
                            event: notice.clone(),
                        },
                    );
                    publish(notice);
                }
                let event = RunEvent::Failed { message };
                ws.append_chat_line(
                    &conversation_id,
                    RunLogLine {
                        ts_ms: SystemClock.now_millis(),
                        event: event.clone(),
                    },
                );
                publish(event);
            }
        }
        // The turn is over, however it ended: done, failed or cancelled.
        ws.finish_chat_turn(&conversation_id).await;
    });

    Ok(conversation)
}

/// Stop the turn a conversation has in flight. No-op when there is none.
pub async fn stop_turn(ws: &Workspace, conversation_id: &str) {
    if let Some(token) = ws.finish_chat_turn(conversation_id).await {
        token.cancel();
    }
}

/// The agent used for conversations: the sidecar or the command line, decided
/// per run so the Settings toggle applies immediately (decision #13).
pub fn agent_for(ws: &Workspace) -> Arc<dyn AgentPort> {
    granted_agent_for(ws, harness_ports::Grants::default())
}

/// The same port, carrying what one agent was granted.
///
/// Grants hang off the **port**, not off the `RunSpec`, because a conversation
/// builds its own port and serves exactly one profile. The engine holds a
/// single port shared by every card run, so this cannot reach worker runs
/// without the engine passing the grants through — see `DEBT.md`.
///
/// The CLI adapter ignores them: `claude --print` has no equivalent of the
/// SDK's `plugins` option, and inventing one would mean writing into the
/// operator's `~/.claude`, which is the thing this design exists to avoid.
pub fn granted_agent_for(ws: &Workspace, grants: harness_ports::Grants) -> Arc<dyn AgentPort> {
    let script = sidecar::script_in(ws.sidecar_dir());
    Arc::new(SwitchingAgent {
        sidecar: Arc::new(SidecarAgent::new("node", script.clone()).with_grants(grants)),
        cli: Arc::new(ClaudeCliAgent::new("claude")),
        settings: Arc::clone(&ws.settings),
    })
}

/// Read a conversation back from disk.
pub fn transcript(ws: &Workspace, conversation_id: &str) -> Result<Vec<RunLogLine>, String> {
    RunLogPort::read(ws.chat_log(), conversation_id).map_err(|e| e.to_string())
}
