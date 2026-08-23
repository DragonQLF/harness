//! Starting, cancelling and finishing agent runs.

use std::sync::Arc;

use harness_domain::{Actor, CardId, Command, RunId, RunOutcome};
use harness_ports::{
    RunEvent, RunProfile, RunSpec, Reviewer, Trailers, WorktreeMode, WorktreePath,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{Engine, Msg, RunEntry, SessionEntry, worktree_label, SHARED_WORKTREE};

impl Engine {
    pub(crate) async fn start_run(
        &mut self,
        card_id: CardId,
        prompt: String,
        profile: RunProfile,
    ) -> Result<RunId, String> {
        if self.runs.contains_key(&card_id) {
            return Err("card already has an active run".to_string());
        }
        let run_id = RunId(uuid::Uuid::new_v4().to_string());

        // The worktree is resolved *before* the run is recorded, for two
        // reasons: a checkout that cannot be created must not leave a card
        // marked Running with no run behind it, and the log needs to say where
        // the work happened — that is not derivable afterwards, because the
        // profile that chose the mode may have changed by then.
        let worktree = self.worktree_for(&card_id, profile.worktree)?;
        let branch = match profile.worktree {
            WorktreeMode::None => None,
            _ => worktree
                .0
                .file_name()
                .map(|n| format!("harness/{}", n.to_string_lossy())),
        };

        let events = self
            .board
            .decide(&Command::StartRun {
                card_id: card_id.clone(),
                run_id: run_id.clone(),
                worktree: Some(worktree.0.to_string_lossy().to_string()),
                branch: branch.clone(),
            })
            .map_err(|e| e.to_string())?;
        self.persist(events).await?;

        let started_ms = self.now();
        // What the last run left behind, whether that was a minute ago or
        // before the last restart: the board carries it now, so it is the
        // board that is asked.
        let resume_session = self
            .board
            .get(&card_id)
            .and_then(|c| c.session_id.clone())
            .or_else(|| {
                self.sessions
                    .get(&card_id)
                    .and_then(|s| s.session_id.clone())
            });

        let token = CancellationToken::new();
        self.runs.insert(
            card_id.clone(),
            RunEntry {
                run_id: run_id.clone(),
                token: token.clone(),
                worktree: worktree.clone(),
                agent_id: profile.agent_id.clone(),
                started_ms,
            },
        );
        self.sessions.insert(
            card_id.clone(),
            SessionEntry {
                run_id: Some(run_id.clone()),
                worktree: worktree.0.clone(),
                branch,
                session_id: resume_session.clone(),
                agent_id: profile.agent_id.clone(),
                started_ms,
            },
        );

        let spec = RunSpec {
            prompt,
            cwd: worktree.0.clone(),
            model: profile.model.clone(),
            allowed_tools: Some(
                profile
                    .allowed_tools
                    .clone()
                    .unwrap_or_else(|| self.config.worker_allowed_tools.clone()),
            ),
            max_budget_usd: profile.max_budget_usd,
            permission_mode: Some(
                profile
                    .permission_mode
                    .clone()
                    .unwrap_or_else(|| self.config.permission_mode.clone()),
            ),
            approver: self.approver.clone(),
            resume_session,
            // Only the Director's conversation gets Harness's own tools; a
            // worker acts on the repository, not on the app.
            tools: None,
            thinking_tokens: None,
        };

        self.emit_run(
            &card_id,
            &run_id,
            RunEvent::Notice {
                text: format!(
                    "{} started in the {} worktree",
                    profile.agent_id,
                    worktree_label(profile.worktree)
                ),
            },
        );

        let (ev_tx, mut ev_rx) = mpsc::channel::<RunEvent>(256);
        let fut = self.agent.run(spec, ev_tx, token);

        let self_tx = self.self_tx.clone();
        let run_log = self.run_log.clone();
        let runs_tx = self.runs_tx.clone();
        let clock = Arc::clone(&self.clock);
        let project_id = self.config.project_id.clone();
        let git = Arc::clone(&self.git);
        let base = self.config.base_branch.clone();
        let commits_work = profile.worktree != WorktreeMode::None;
        let agent_id = profile.agent_id.clone();
        let done_profile = profile;
        let card_for_events = card_id.clone();
        let run_for_events = run_id.clone();
        let done_card = card_id;
        let done_run = run_id.clone();

        tokio::spawn(async move {
            // Forwarding lives in the spawned task so a slow UI never blocks the
            // actor; the log write is the durable copy.
            let forward = async {
                while let Some(ev) = ev_rx.recv().await {
                    if let RunEvent::Started { session_id } = &ev {
                        let _ = self_tx
                            .send(Msg::AgentSession {
                                card_id: card_for_events.clone(),
                                session_id: session_id.clone(),
                            })
                            .await;
                    }
                    let ts_ms = clock.now_millis();
                    // Deltas are for the live view only; the `Text` event that
                    // follows them is what the transcript keeps.
                    if let (Some(log), false) = (&run_log, ev.is_ephemeral()) {
                        let _ = log.append(
                            run_for_events.0.as_str(),
                            &harness_ports::RunLogLine {
                                ts_ms,
                                event: ev.clone(),
                            },
                        );
                    }
                    let _ = runs_tx.send(crate::RunUpdate {
                        project_id: project_id.clone(),
                        card_id: card_for_events.clone(),
                        run_id: run_for_events.clone(),
                        ts_ms,
                        event: ev,
                    });
                }
            };
            let (result, _) = tokio::join!(fut, forward);
            let outcome = result.unwrap_or_else(harness_ports::RunOutcome::Failed);

            // A commit that fails must never pass for one that worked: the
            // Director would then review an empty diff and approve nothing.
            let mut commit_error = None;
            if commits_work {
                let worktree = worktree.clone();
                let outcome_for_commit = &outcome;
                let result = tokio::task::block_in_place(|| match outcome_for_commit {
                    harness_ports::RunOutcome::Completed { .. } => {
                        let trailers = Trailers(vec![
                            ("Harness-Card".to_string(), done_card.to_string()),
                            ("Harness-Run".to_string(), done_run.to_string()),
                            ("Harness-Agent".to_string(), agent_id.clone()),
                        ]);
                        let msg = format!("harness: work for card {done_card}");
                        git.commit(&worktree, &msg, &trailers).map(|_| ())
                    }
                    harness_ports::RunOutcome::Cancelled
                    | harness_ports::RunOutcome::Failed(_) => {
                        git.commit_wip(&worktree).map(|_| ())
                    }
                });
                if let Err(e) = result {
                    commit_error = Some(e.to_string());
                }
                let _ = base;
            }

            if let Some(reason) = &commit_error {
                let ts_ms = clock.now_millis();
                let event = RunEvent::Notice {
                    text: format!("could not commit the work: {reason}"),
                };
                if let Some(log) = &run_log {
                    let _ = log.append(
                        run_for_events.0.as_str(),
                        &harness_ports::RunLogLine {
                            ts_ms,
                            event: event.clone(),
                        },
                    );
                }
                let _ = runs_tx.send(crate::RunUpdate {
                    project_id: project_id.clone(),
                    card_id: card_for_events.clone(),
                    run_id: run_for_events.clone(),
                    ts_ms,
                    event,
                });
            }

            let _ = self_tx
                .send(Msg::RunDone {
                    card_id: done_card,
                    run_id: done_run,
                    outcome: Box::new(outcome),
                    profile: Box::new(done_profile),
                    commit_failed: commit_error.is_some(),
                })
                .await;
        });

        Ok(run_id)
    }

    /// Resolve where this run works, creating the worktree when needed.
    fn worktree_for(
        &mut self,
        card_id: &CardId,
        mode: WorktreeMode,
    ) -> Result<WorktreePath, String> {
        match mode {
            WorktreeMode::None => Ok(WorktreePath(self.config.repo_root.clone())),
            WorktreeMode::Shared => {
                if let Some(existing) = &self.shared_worktree {
                    if existing.0.exists() {
                        return Ok(existing.clone());
                    }
                }
                // After a restart the engine has no memory of the shared
                // checkout, but it is still on disk. Recreating it would take
                // the branch its commits are on with it, so an existing one is
                // adopted rather than rebuilt.
                let existing = self.git.worktree_path(SHARED_WORKTREE);
                if existing.is_dir() {
                    let found = WorktreePath(existing);
                    self.shared_worktree = Some(found.clone());
                    return Ok(found);
                }
                let git = Arc::clone(&self.git);
                let base = self.config.base_branch.clone();
                let created =
                    tokio::task::block_in_place(move || git.create_worktree(SHARED_WORKTREE, &base))
                        .map_err(|e| e.to_string())?;
                self.shared_worktree = Some(created.clone());
                Ok(created)
            }
            WorktreeMode::PerCard => {
                let git = Arc::clone(&self.git);
                let cid = card_id.to_string();
                let base = self.config.base_branch.clone();
                tokio::task::block_in_place(move || git.create_worktree(&cid, &base))
                    .map_err(|e| e.to_string())
            }
        }
    }

    pub(crate) fn cancel_run(&mut self, card_id: &CardId) -> Result<(), String> {
        match self.runs.get(card_id) {
            Some(entry) => {
                entry.token.cancel();
                Ok(())
            }
            None => Err("no active run for this card".to_string()),
        }
    }

    pub(crate) async fn finish_run(
        &mut self,
        card_id: CardId,
        run_id: RunId,
        outcome: harness_ports::RunOutcome,
        profile: RunProfile,
        commit_failed: bool,
    ) {
        self.runs.remove(&card_id);
        let (domain_outcome, cost_usd, turns) = match &outcome {
            harness_ports::RunOutcome::Completed {
                cost_usd,
                turns,
                session_id,
            } => {
                // The result carries the session too, and for a run that never
                // emitted an init event it is the only place it appears.
                // Recorded on the board, so a restart keeps it.
                if let Some(sid) = session_id.clone() {
                    self.record_session(card_id.clone(), sid).await;
                }
                (RunOutcome::Completed, *cost_usd, *turns)
            }
            harness_ports::RunOutcome::Cancelled => (RunOutcome::Cancelled, None, None),
            harness_ports::RunOutcome::Failed(msg) => {
                self.emit_run(
                    &card_id,
                    &run_id,
                    RunEvent::Notice {
                        text: format!("run failed: {msg}"),
                    },
                );
                (RunOutcome::Failed, None, None)
            }
        };

        let events = match self.board.decide(&Command::FinishRun {
            card_id: card_id.clone(),
            run_id: run_id.clone(),
            outcome: domain_outcome,
            cost_usd,
            turns,
        }) {
            Ok(events) => events,
            Err(e) => {
                eprintln!("finish_run rejected for {card_id}/{run_id}: {e}");
                return;
            }
        };
        if let Err(e) = self.persist(events).await {
            eprintln!("finish_run persist failed for {card_id}: {e}");
            return;
        }

        if !matches!(outcome, harness_ports::RunOutcome::Completed { .. }) {
            return;
        }

        // With nothing committed there is no diff: sending it to the Director
        // would have it approve an empty change. It waits for the operator.
        if commit_failed {
            self.emit_run(
                &card_id,
                &run_id,
                RunEvent::Notice {
                    text: "nothing was committed, so the card is waiting for you".into(),
                },
            );
            return;
        }

        match profile.reviewer {
            Reviewer::Director if self.policy.director_reviews_first => {
                self.run_director_review(card_id, run_id).await;
            }
            Reviewer::Director | Reviewer::Human => {
                self.emit_run(
                    &card_id,
                    &run_id,
                    RunEvent::Notice {
                        text: "waiting for your review".into(),
                    },
                );
            }
            Reviewer::Nobody => {
                let cmd = Command::ApproveCard {
                    card_id: card_id.clone(),
                    by: Actor::Human,
                    reason: "no reviewer configured for this agent".into(),
                };
                match self.board.decide(&cmd) {
                    Ok(events) => {
                        let _ = self.persist(events).await;
                        self.emit_run(
                            &card_id,
                            &run_id,
                            RunEvent::Notice {
                                text: "no reviewer configured, card closed".into(),
                            },
                        );
                    }
                    Err(e) => eprintln!("auto-approve rejected for {card_id}: {e}"),
                }
            }
        }
    }
}
