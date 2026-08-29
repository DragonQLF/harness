//! The end-of-day look: one self-directed Director turn, once a day, when the
//! operator closes Relay.
//!
//! Never each turn — patterns are visible over weeks, not messages, and a
//! model asked to reflect constantly reflects about nothing. The app owns the
//! timing (`inbox::look_due`); the model never has to know what time it is.
//! What comes out lands as proposals in the inbox, never as cards: accepting
//! one is the operator's decision, and it grants permission rather than
//! creating work — the Director is told in his next turn and acts then.
//!
//! Bounded three ways, because it runs against someone trying to leave:
//! a hard budget, a wall-clock timeout, and the once-a-day gate. Whatever was
//! proposed before the cut is already saved — proposals are written at
//! tool-call time, not at the end.

use std::sync::Arc;
use std::time::Duration;

use harness_app::agents;
use harness_ports::{RunEvent, RunLogLine, RunSpec};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use crate::workspace::{SystemClock, Workspace};
use harness_ports::ClockPort;

/// Hard ceiling for the whole look. A pattern worth proposing shows up in one
/// tool call and two paragraphs; anything longer is wandering.
const WALL_CLOCK: Duration = Duration::from_secs(120);

/// Cheaper than any conversation: he reads one table, maybe one doc section,
/// files at most a handful of proposals.
const BUDGET_USD: f64 = 0.30;

/// The one name these conversations carry, so an unanswered one can be found
/// again rather than duplicated.
const LOOK_TITLE: &str = "End-of-day review";

/// Run the daily look if it is due. Returns what it said (for tests and logs);
/// `None` when it was not due or could not start. Safe to call from several
/// shutdown paths: exactly one wins the claim.
pub async fn maybe_run_daily_look(
    ws: &Arc<Workspace>,
    skip: CancellationToken,
) -> Option<String> {
    if !ws.claim_daily_look() {
        return None;
    }
    // Release on every exit path from here.
    let result = run_bounded(ws, skip).await;
    ws.release_daily_look();
    result
}

async fn run_bounded(ws: &Arc<Workspace>, skip: CancellationToken) -> Option<String> {
    if !ws.daily_look_due() {
        return None;
    }
    let Some(profile) = ws.agent_exact(agents::DIRECTOR_ID).await else {
        return None;
    };
    if !profile.can_chat() {
        return None;
    }

    // A real conversation row: the operator can open it tomorrow and read why
    // a proposal exists, which is the whole auditability of the Mirror chain.
    //
    // A look that never got an answer left one of these behind, and the day
    // stays due, so the next close would start another. One unanswered review
    // is a record; four are a list of times the app was killed. Reuse the empty
    // one instead of stacking husks.
    let unanswered = ws
        .conversations(false)
        .await
        .into_iter()
        .find(|c| c.title == LOOK_TITLE && c.messages == 0);
    let conversation = match unanswered {
        Some(c) => c,
        None => match ws
            .new_conversation(Some(agents::DIRECTOR_ID.to_string()), None)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!("could not open the end-of-day review conversation: {e}");
                return None;
            }
        },
    };
    let _ = ws.rename_conversation(&conversation.id, LOOK_TITLE).await;

    // The opening turn goes in before the run, exactly as `chat::send` writes
    // the operator's words first. Without it the row is empty until the model
    // speaks — and a turn that never speaks leaves a conversation that opens
    // onto nothing, with no trace of what was asked or why it exists.
    // Relay's own repository may have moved without a card behind it. Asked
    // for here, on a deadline of its own, so a slow git cannot hold the close:
    // `look_for_outside_work` gives up in silence rather than waiting.
    let outside = ws.look_for_outside_work().await;
    let asked = harness_app::director::daily_look_prompt(outside.as_deref());
    ws.append_chat_line(
        &conversation.id,
        RunLogLine {
            ts_ms: SystemClock.now_millis(),
            event: RunEvent::UserMessage { text: asked.clone() },
        },
    );

    let spec = RunSpec {
        provider: harness_app::providers::find(&ws.settings().providers, &profile.provider)
            .and_then(|p| p.resolve()),
        prompt: asked,
        cwd: ws.paths.root().to_path_buf(),
        model: profile.model.clone(),
        // Everything he needs sits in the read set; board tools stay out of his
        // hands tonight on purpose — proposing is the whole job.
        allowed_tools: Some(crate::chat::READ_ONLY_TOOLS.iter().map(|t| t.to_string()).collect()),
        max_budget_usd: Some(BUDGET_USD),
        permission_mode: Some("manual".to_string()),
        approver: Some(ws.router.approver_for("workspace")),
        resume_session: None,
        tools: Some(crate::director_tools::runner(
            ws,
            conversation.project_id.clone(),
            true,
            profile.id.clone(),
        )),
        thinking_tokens: Some(2000),
        subagents: false,
        report_work: false,
        // O porto desta conversa já foi construído por perfil, com as
        // concessões dentro; vazio aqui quer dizer "usa as dele".
        grants: harness_ports::Grants::default(),
    };

    let (ev_tx, mut ev_rx) = tokio::sync::mpsc::channel::<RunEvent>(64);
    let token = CancellationToken::new();
    ws.register_chat_turn(&conversation.id, token.clone()).await;
    let run = ws.agent_port().run(spec, ev_tx, token.clone());
    tokio::pin!(run);

    let app = ws.app_handle();
    let conversation_id = conversation.id.clone();
    let ws_forward = Arc::clone(ws);

    // Forward into the transcript and the live channel, exactly like a chat
    // turn: the same typed listener serves both.
    let forward = async move {
        let mut last_text = String::new();
        let mut heard = false;
        while let Some(ev) = ev_rx.recv().await {
            heard = true;
            match &ev {
                RunEvent::Started { session_id } => {
                    ws_forward.record_chat_session(&conversation_id, session_id).await;
                }
                RunEvent::Text { text } => last_text = text.clone(),
                RunEvent::Done {
                    session_id, cost_usd, ..
                } => {
                    if let Some(sid) = session_id {
                        ws_forward.record_chat_session(&conversation_id, sid).await;
                    }
                    ws_forward.record_chat_cost(&conversation_id, *cost_usd).await;
                }
                _ => {}
            }
            if !ev.is_ephemeral() {
                ws_forward.append_chat_line(
                    &conversation_id,
                    RunLogLine {
                        ts_ms: SystemClock.now_millis(),
                        event: ev.clone(),
                    },
                );
            }
            let _ = app.emit(
                "engine://run",
                harness_engine::RunUpdate {
                    project_id: String::new(),
                    card_id: harness_domain::CardId::new(conversation_id.clone()),
                    run_id: harness_domain::RunId(conversation_id.clone()),
                    ts_ms: SystemClock.now_millis(),
                    event: ev,
                },
            );
        }
        (last_text, heard)
    };

    // Whoever finishes first ends the wait: the answer, or the clock. Either
    // way the transcript is drained to the last event — on a timeout the turn
    // is cancelled like any other, and what it already said stays readable.
    // Proposals are never at risk from the cut: they were written when the
    // tool ran, not at the end.
    // Why it stopped, when the agent said. Discarding this was how a look
    // that never reached the model came to read as a look that found nothing:
    // the reason was in hand and thrown away.
    let mut failure: Option<String> = None;
    let closing = tokio::select! {
        outcome = &mut run => {
            if let Err(e) = outcome {
                failure = Some(e);
            }
            None
        }
        _ = tokio::time::sleep(WALL_CLOCK) => {
            token.cancel();
            let _ = (&mut run).await;
            Some("stopped at the wall clock; what was filed is filed")
                .map(str::to_string)
        }
        // The operator refused to keep waiting. Their time is theirs; the look
        // is due again on the next close rather than lost.
        _ = skip.cancelled() => {
            token.cancel();
            let _ = (&mut run).await;
            Some("you closed Relay before the look finished; what was filed is filed")
                .map(str::to_string)
        }
    };
    let (text, heard) = forward.await;

    // A look that produced nothing at all — the agent never started, or the
    // app was closed out from under it — still says so. Silence in a
    // transcript is indistinguishable from a look that found nothing worth
    // proposing, and the two mean opposite things.
    if !heard {
        ws.append_chat_line(
            &conversation.id,
            RunLogLine {
                ts_ms: SystemClock.now_millis(),
                event: RunEvent::Notice {
                    text: match &failure {
                        Some(why) => format!(
                            "The end-of-day look could not run: {why}. Nothing was \
                             proposed, and it is due again on the next close."
                        ),
                        None => String::from(
                            "The end-of-day look ended without a single event and without \
                             an error to explain it. Nothing was proposed. It is due again \
                             on the next close.",
                        ),
                    },
                },
            },
        );
    }

    ws.finish_chat_turn(&conversation.id).await;
    // A look that found nothing is still a look, and a cut one retries tomorrow
    // rather than tonight. One that never ran at all is not: marking it would
    // buy a day of silence for a failure nobody saw.
    if heard {
        ws.mark_daily_look();
    }

    Some(match (closing, text.is_empty()) {
        (Some(reason), _) => format!("({reason})"),
        (None, true) => String::from("(the end-of-day look ended without a closing word)"),
        (None, false) => text,
    })
}
