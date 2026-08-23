//! The project side of the Director: reading the diff a finished run produced
//! and deciding whether it holds up. Conversation lives at workspace level, in
//! `harness_app::director` — one Director watches every board.

use std::sync::Arc;

use harness_domain::{Actor, CardId, Command, RunId};
use harness_ports::{RunEvent, RunSpec, WorktreePath};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{extract_json_object, Engine, Msg};

impl Engine {
    /// Read the diff a finished run produced and approve it or send it back.
    pub(crate) async fn run_director_review(&mut self, card_id: CardId, run_id: RunId) {
        let Some(session) = self.sessions.get(&card_id) else {
            return;
        };
        let worktree = session.worktree.clone();

        let git = Arc::clone(&self.git);
        let wt = WorktreePath(worktree.clone());
        let base = self.config.base_branch.clone();
        let diff = tokio::task::block_in_place(move || git.diff_summary(&wt, &base))
            .unwrap_or_else(|_| "(diff unavailable)".to_string());

        let title = self
            .board
            .get(&card_id)
            .map(|c| c.title.clone())
            .unwrap_or_else(|| card_id.to_string());

        let prompt = format!(
            "You are the Director reviewing work done for the card '{title}' ({card_id}).\n\n\
             Judge whether the diff below fulfils the task. Be strict about scope: work that \
             widens permissions, touches unrelated files or skips tests should be sent back.\n\n\
             Respond with ONLY a JSON object, no other text:\n\
             {{\"decision\": \"approve\"|\"reject\", \"reason\": \"<one short sentence>\"}}\n\n\
             DIFF:\n{diff}"
        );

        self.emit_run(
            &card_id,
            &run_id,
            RunEvent::Notice {
                text: "director is reading the diff".into(),
            },
        );

        let spec = RunSpec {
            prompt,
            cwd: worktree,
            model: self.config.director_model.clone(),
            allowed_tools: Some(self.config.director_allowed_tools.clone()),
            max_budget_usd: None,
            permission_mode: Some("dontAsk".to_string()),
            approver: None,
            resume_session: None,
            tools: None,
            thinking_tokens: None,
        };

        let (ev_tx, mut ev_rx) = mpsc::channel::<RunEvent>(64);
        let fut = self.director.run(spec, ev_tx, CancellationToken::new());

        let self_tx = self.self_tx.clone();
        let runs_tx = self.runs_tx.clone();
        let clock = Arc::clone(&self.clock);
        let project_id = self.config.project_id.clone();
        let review_card = card_id;
        let review_run = run_id;
        let last_result = Arc::new(std::sync::Mutex::new(None::<String>));
        let lr = Arc::clone(&last_result);

        tokio::spawn(async move {
            let forward = async {
                while let Some(ev) = ev_rx.recv().await {
                    match &ev {
                        RunEvent::Done { result: Some(r), .. } => {
                            *lr.lock().unwrap() = Some(r.clone());
                        }
                        RunEvent::Text { text } => {
                            let _ = runs_tx.send(crate::RunUpdate {
                                project_id: project_id.clone(),
                                card_id: review_card.clone(),
                                run_id: review_run.clone(),
                                ts_ms: clock.now_millis(),
                                event: RunEvent::Notice {
                                    text: format!("director: {text}"),
                                },
                            });
                        }
                        _ => {}
                    }
                }
            };
            let (res, _) = tokio::join!(fut, forward);
            let outcome = res.unwrap_or_else(harness_ports::RunOutcome::Failed);
            let verdict = last_result.lock().unwrap().clone();
            let _ = self_tx
                .send(Msg::DirectorDone {
                    card_id: review_card,
                    outcome: Box::new(outcome),
                    verdict,
                })
                .await;
        });
    }

    pub(crate) async fn handle_director_done(
        &mut self,
        card_id: CardId,
        outcome: harness_ports::RunOutcome,
        verdict: Option<String>,
    ) {
        let run_id = self
            .sessions
            .get(&card_id)
            .and_then(|s| s.run_id.clone())
            .unwrap_or_else(|| RunId("review".into()));

        if !matches!(outcome, harness_ports::RunOutcome::Completed { .. }) {
            self.emit_run(
                &card_id,
                &run_id,
                RunEvent::Notice {
                    text: "director could not finish the review; card stays in review".into(),
                },
            );
            return;
        }

        let parsed = verdict
            .as_deref()
            .and_then(extract_json_object)
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                let decision = v.get("decision")?.as_str()?.to_ascii_lowercase();
                let reason = v
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                Some((decision, reason))
            });

        let (cmd, note) = match parsed {
            Some((d, reason)) if d == "approve" => {
                let reason = if reason.is_empty() {
                    "the diff matches the card".to_string()
                } else {
                    reason
                };
                (
                    Command::ApproveCard {
                        card_id: card_id.clone(),
                        by: Actor::Director,
                        reason: reason.clone(),
                    },
                    format!("director approved: {reason}"),
                )
            }
            Some((d, reason)) if d == "reject" => {
                let reason = if reason.is_empty() {
                    "the diff does not match the card".to_string()
                } else {
                    reason
                };
                (
                    Command::RejectCard {
                        card_id: card_id.clone(),
                        reason: reason.clone(),
                        by: Actor::Director,
                    },
                    format!("director sent it back: {reason}"),
                )
            }
            _ => {
                self.emit_run(
                    &card_id,
                    &run_id,
                    RunEvent::Notice {
                        text: "director verdict was unreadable; card stays in review".into(),
                    },
                );
                return;
            }
        };

        match self.board.decide(&cmd) {
            Ok(events) => {
                if let Err(e) = self.persist(events).await {
                    eprintln!("director persist failed for {card_id}: {e}");
                    return;
                }
                self.emit_run(&card_id, &run_id, RunEvent::Notice { text: note });
            }
            Err(e) => eprintln!("director decision rejected for {card_id}: {e}"),
        }
    }
}
