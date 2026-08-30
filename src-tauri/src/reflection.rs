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

/// Sized for the model this actually runs on. The Director is an Opus profile
/// with a $1.50 ceiling for a conversation; 30 cents was set as "cheaper than
/// any conversation" and turned out to be less than one turn of it — the
/// transcript for 2026-08-29 spent $0.3243 on `ToolSearch` and `self_report`
/// and was cut before it could propose anything, having created 32k tokens of
/// cache on the way in.
///
/// Still well under a conversation, because the shape of the work has not
/// changed: one table, maybe one doc section, a handful of proposals. What
/// changed is knowing what that costs.
const BUDGET_USD: f64 = 1.00;

/// The one name these conversations carry, so an unanswered one can be found
/// again rather than duplicated.
const LOOK_TITLE: &str = "End-of-day review";

/// Did this run earn the day being marked done?
///
/// Only a look that both ran and ended on its own terms. `heard` alone is not
/// enough — a budget cut emits events too, and treating those as a look banks
/// the failure and buys a day of silence for it.
fn counts_as_a_look(heard: bool, failed: bool) -> bool {
    heard && !failed
}

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

    let inbox = harness_app::chatqueue::Queue::new(&conversation.id);
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
        // The look runs while Relay closes, but the window is still up and
        // the composer with it — so it gets an inbox like any other turn
        // rather than being the one conversation that refuses to be answered.
        inbox: Some(Arc::clone(&inbox) as harness_ports::Inbox),
        subagents: false,
        report_work: false,
        // O porto desta conversa já foi construído por perfil, com as
        // concessões dentro; vazio aqui quer dizer "usa as dele".
        grants: harness_ports::Grants::default(),
        output_style: profile.output_style.clone(),
        // A olhada não é uma pergunta do operador: não há ninguém a escolher.
        effort: None,
    };

    let (ev_tx, mut ev_rx) = tokio::sync::mpsc::channel::<RunEvent>(64);
    let token = CancellationToken::new();
    let turn = crate::conversations::Turn::new(token.clone(), Arc::clone(&inbox));
    let turn_id = turn.id;
    // The look reuses an unanswered conversation, which the operator may have
    // open and be typing into. If it already has a turn, that one is the real
    // one — the look is what gives way.
    if ws.register_chat_turn(&conversation.id, turn).await.is_err() {
        return None;
    }
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
        // A `Done` can carry an error — a budget cut is the one that bit. It
        // arrives on the same message as a success, so without reading it a
        // failed look is indistinguishable from a quiet one.
        let mut failed = false;
        while let Some(ev) = ev_rx.recv().await {
            heard = true;
            match &ev {
                RunEvent::Started { session_id } => {
                    ws_forward.record_chat_session(&conversation_id, session_id).await;
                }
                RunEvent::Text { text } => last_text = text.clone(),
                RunEvent::Done {
                    session_id,
                    cost_usd,
                    error,
                    ..
                } => {
                    if let Some(sid) = session_id {
                        ws_forward.record_chat_session(&conversation_id, sid).await;
                    }
                    ws_forward.record_chat_cost(&conversation_id, *cost_usd).await;
                    failed = error.is_some();
                }
                RunEvent::Failed { .. } => failed = true,
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
                crate::events::ENGINE_RUN,
                harness_engine::RunUpdate {
                    project_id: String::new(),
                    card_id: harness_domain::CardId::new(conversation_id.clone()),
                    run_id: harness_domain::RunId(conversation_id.clone()),
                    ts_ms: SystemClock.now_millis(),
                    event: ev,
                },
            );
        }
        (last_text, heard, failed)
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
            Some("stopped at the wall clock; what was filed is filed".to_string())
        }
        // The operator refused to keep waiting. Their time is theirs; the look
        // is due again on the next close rather than lost.
        _ = skip.cancelled() => {
            token.cancel();
            let _ = (&mut run).await;
            Some("you closed Relay before the look finished; what was filed is filed".to_string())
        }
    };
    let (text, heard, failed) = forward.await;

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

    ws.finish_chat_turn(&conversation.id, Some(turn_id)).await;
    // A look that found nothing is still a look. One that never ran is not,
    // and neither is one the budget cut before it reached the proposing —
    // marking either buys a day of silence for a failure nobody saw. That is
    // exactly what happened on 2026-08-29: the run died on its ceiling after
    // two tool calls, `heard` was true because events had arrived, and the day
    // was banked as done.
    if counts_as_a_look(heard, failed) {
        ws.mark_daily_look();
    }

    Some(match (closing, text.is_empty()) {
        (Some(reason), _) => format!("({reason})"),
        (None, true) => String::from("(the end-of-day look ended without a closing word)"),
        (None, false) => text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O que aconteceu a 2026-08-29: o run morreu no tecto de $0.30 ao fim de
    /// duas chamadas, o `heard` era verdade porque tinham chegado eventos, e o
    /// dia ficou marcado como visto. Custou 32 cêntimos, não propôs nada, e
    /// comprou vinte e quatro horas de silêncio para a própria falha.
    #[test]
    fn a_look_the_budget_cut_does_not_count_as_a_look() {
        assert!(!counts_as_a_look(true, true));
    }

    #[test]
    fn a_look_that_never_started_does_not_count_either() {
        assert!(!counts_as_a_look(false, false));
    }

    /// Uma olhada que correu e não achou nada continua a ser uma olhada: o
    /// silêncio dela é uma resposta, e repeti-la esta noite não muda nada.
    #[test]
    fn a_quiet_look_is_still_a_look() {
        assert!(counts_as_a_look(true, false));
    }

    /// O tecto tem de dar para o modelo que o corre. O Director é um perfil
    /// Opus com $1.50 para uma conversa; o antigo $0.30 era menos do que um
    /// turno dele.
    #[test]
    fn the_budget_leaves_room_for_more_than_two_tool_calls() {
        assert!(BUDGET_USD >= 1.0, "0.3243 foi gasto antes de propor seja o que for");
    }
}
