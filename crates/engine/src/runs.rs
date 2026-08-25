//! Starting, cancelling and finishing agent runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use harness_domain::{Actor, CardId, Command, RunId, RunOutcome};
use harness_ports::{
    RunEvent, RunProfile, RunSpec, Reviewer, Trailers, WorktreeMode, WorktreePath,
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::{Engine, Msg, RunEntry, SessionEntry, worktree_label, SHARED_WORKTREE};

impl Engine {
    /// What must hold before a run may begin. Checked here for a fast answer
    /// and again in `launch_run`, because between asking for a worktree and
    /// being handed one, other messages get processed and the world can move.
    /// `starting` is what makes that window honest: a card (or an agent's
    /// slot) mid-start counts here before it counts in `runs`.
    fn check_run_start(&self, card_id: &CardId, profile: &RunProfile) -> Result<(), String> {
        if self.runs.contains_key(card_id) {
            return Err("card already has an active run".to_string());
        }
        if self.starting.contains_key(card_id) {
            return Err("a start is already under way for this card".to_string());
        }
        let limit = profile.max_concurrent.max(1) as usize;
        let agent = profile.agent_id.as_str();
        let active = self
            .runs
            .values()
            .filter(|entry| entry.agent_id == agent)
            .count()
            + self
                .starting
                .values()
                .filter(|a| a.as_str() == agent)
                .count();
        if active >= limit {
            let unit = if active == 1 { "card" } else { "cards" };
            return Err(format!(
                "{} is already working on {} {}; its limit is {}",
                profile.agent_id, active, unit, limit
            ));
        }
        Ok(())
    }

    /// Clear a start that will not happen. The set entry always goes; a
    /// worktree built fresh for this attempt goes with it — detached, since
    /// `worktree remove --force` takes seconds and nobody is waiting. An
    /// adopted checkout is never ours to delete.
    fn abandon_start(&mut self, card_id: &CardId, created: bool, worktree: Option<WorktreePath>) {
        self.starting.remove(card_id);
        if created {
            if let Some(wt) = worktree {
                let git = Arc::clone(&self.git);
                tokio::spawn(async move {
                    let removed =
                        tokio::task::spawn_blocking(move || git.remove_worktree(&wt)).await;
                    match removed {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => eprintln!("could not remove the abandoned worktree: {e}"),
                        Err(e) => eprintln!("could not remove the abandoned worktree: {e}"),
                    }
                });
            }
        }
    }

    pub(crate) async fn start_run(
        &mut self,
        card_id: CardId,
        prompt: String,
        profile: RunProfile,
        reply: oneshot::Sender<Result<RunId, String>>,
    ) {
        if let Err(e) = self.check_run_start(&card_id, &profile) {
            let _ = reply.send(Err(e));
            return;
        }
        match profile.worktree {
            WorktreeMode::None => {
                let worktree = WorktreePath(self.config.repo_root.clone());
                self.launch_run(card_id, prompt, profile, reply, Ok(worktree), false)
                    .await;
            }
            WorktreeMode::Shared => match self.shared_checkout() {
                Some(existing) => {
                    self.launch_run(
                        card_id,
                        prompt,
                        profile,
                        reply,
                        Ok(existing),
                        false,
                    )
                    .await;
                }
                None => {
                    // From here until the run is registered the card must be
                    // visible, or a second dispatch would build a worktree
                    // that deletes this one's checkout mid-flight.
                    if self.starting.contains_key(&card_id) {
                        let _ = reply.send(Err(
                            "a start is already under way for this card".to_string()
                        ));
                        return;
                    }
                    self.starting.insert(card_id.clone(), profile.agent_id.clone());
                    self.resolve_worktree_off_actor(
                        SHARED_WORKTREE.to_string(),
                        card_id,
                        prompt,
                        profile,
                        reply,
                    );
                }
            },
            WorktreeMode::PerCard => {
                // Same window as above; see the Shared arm.
                if self.starting.contains_key(&card_id) {
                    let _ = reply.send(Err(
                        "a start is already under way for this card".to_string()
                    ));
                    return;
                }
                // An existing checkout is adopted, never destroyed: the last
                // run may have left committed or wip work on that branch, and
                // `create_worktree` removes before it adds (#71's lost site).
                let existing = self.git.worktree_path(&card_id.to_string());
                if existing.is_dir() {
                    let worktree = WorktreePath(existing);
                    self.launch_run(
                        card_id,
                        prompt,
                        profile,
                        reply,
                        Ok(worktree),
                        false,
                    )
                    .await;
                    return;
                }
                self.starting.insert(card_id.clone(), profile.agent_id.clone());
                let name = card_id.to_string();
                self.resolve_worktree_off_actor(name, card_id, prompt, profile, reply);
            }
        }
    }

    /// The shared checkout, adopted when it already exists on disk: recreating
    /// it would take the branch its commits are on. Asking is pure path maths,
    /// so this stays on the actor.
    fn shared_checkout(&mut self) -> Option<WorktreePath> {
        if let Some(cached) = self.shared_worktree.clone() {
            if cached.0.exists() {
                return Some(cached);
            }
        }
        let path = self.git.worktree_path(SHARED_WORKTREE);
        if path.is_dir() {
            let found = WorktreePath(path);
            self.shared_worktree = Some(found.clone());
            return Some(found);
        }
        None
    }

    /// Create a worktree off the actor. `git worktree add` — preceded by
    /// `worktree remove --force` and `branch -D` — takes seconds on a large
    /// repository; doing that inline froze snapshots, cancels and run
    /// completions behind it. The result comes back as a message.
    fn resolve_worktree_off_actor(
        &self,
        name: String,
        card_id: CardId,
        prompt: String,
        profile: RunProfile,
        reply: oneshot::Sender<Result<RunId, String>>,
    ) {
        let git = Arc::clone(&self.git);
        let base = self.config.base_branch.clone();
        let self_tx = self.self_tx.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || git.create_worktree(&name, &base))
                .await
                .map_err(|e| e.to_string())
                .and_then(|inner| inner.map_err(|e| e.to_string()));
            let _ = self_tx
                .send(Msg::WorktreeResolved {
                    card_id,
                    prompt,
                    profile: Box::new(profile),
                    reply,
                    result,
                    created: true,
                })
                .await;
        });
    }

    /// Record the run and set the agent loose. The worktree is settled by the
    /// time this runs — resolved *before* the run is recorded, so a checkout
    /// that cannot be created never leaves a card marked Running with no run
    /// behind it, and the log can say where the work happened. `created` says
    /// the checkout was built for this attempt and is ours to clean up.
    pub(crate) async fn launch_run(
        &mut self,
        card_id: CardId,
        prompt: String,
        profile: RunProfile,
        reply: oneshot::Sender<Result<RunId, String>>,
        worktree_result: Result<WorktreePath, String>,
        created: bool,
    ) {
        // Our own marker comes off first: it existed for the messages between
        // the two phases, and this handler runs without interleaving — left
        // in, the re-check below would find the card "starting" against
        // itself.
        self.starting.remove(&card_id);
        let worktree = match worktree_result {
            Ok(wt) => wt,
            Err(e) => {
                let _ = reply.send(Err(format!("could not create the worktree: {e}")));
                return;
            }
        };
        if let Err(e) = self.check_run_start(&card_id, &profile) {
            // The world moved during the window: discarded, double-dispatched
            // or over the agent's limit. Either way the checkout built for
            // this attempt would sit on disk with nobody owning it.
            self.abandon_start(&card_id, created, Some(worktree.clone()));
            let _ = reply.send(Err(e));
            return;
        }
        let run_id = RunId(uuid::Uuid::new_v4().to_string());

        let branch = match profile.worktree {
            WorktreeMode::None => None,
            _ => worktree
                .0
                .file_name()
                .map(|n| format!("harness/{}", n.to_string_lossy())),
        };

        let events = match self
            .board
            .decide(&Command::StartRun {
                card_id: card_id.clone(),
                run_id: run_id.clone(),
                worktree: Some(worktree.0.to_string_lossy().to_string()),
                branch: branch.clone(),
            }) {
            Ok(events) => events,
            Err(e) => {
                self.abandon_start(&card_id, created, Some(worktree.clone()));
                let _ = reply.send(Err(e.to_string()));
                return;
            }
        };
        if let Err(e) = self.persist(events).await {
            self.abandon_start(&card_id, created, Some(worktree.clone()));
            let _ = reply.send(Err(e));
            return;
        }

        let started_ms = self.now();
        // The commit message a successful run leaves behind is the card's own
        // title — history should read like work, not like ids. Captured before
        // the task spawns; the board owns it until then.
        let card_title = self
            .board
            .get(&card_id)
            .map(|c| c.title.clone())
            .unwrap_or_default();
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
        // Only this task decides when cancellation turns into a wip commit:
        // shutdown clears the flag when policy says closing must not commit.
        let commit_on_cancel = Arc::new(AtomicBool::new(true));
        // The agent's report_work lands here while it runs; the commit reads
        // the latest one. Shared with the spawned task on purpose.
        let work_report = Arc::new(std::sync::Mutex::new(crate::WorkReport::default()));

        // The worker's single harness tool: its own account of the work.
        // Routed through the actor like every other write, so the event log
        // and the slot move together.
        let report_tx = self.self_tx.clone();
        let report_card = card_id.clone();
        let tools: Option<harness_ports::ToolRunner> = Some(Arc::new(move |call| {
            let tx = report_tx.clone();
            let card = report_card.clone();
            Box::pin(async move {
                if call.name != "report_work" {
                    return harness_ports::ToolReply::refused(format!(
                        "no such harness tool: {}",
                        call.name
                    ));
                }
                let summary = call
                    .input
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let notes: Vec<String> = call
                    .input
                    .get("memory_notes")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|n| n.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                let accepted = async {
                    let (ack, ack_rx) = tokio::sync::oneshot::channel();
                    tx.send(Msg::WorkReport { card_id: card, summary, notes, ack })
                        .await
                        .map_err(|_| "harness is shutting down".to_string())?;
                    ack_rx.await.map_err(|_| "harness dropped the report".to_string())?
                }
                .await;
                match accepted {
                    Ok(()) => harness_ports::ToolReply::ok(
                        "reported; the summary becomes the body of Harness's commit",
                    ),
                    Err(reason) => harness_ports::ToolReply::refused(reason),
                }
            })
        }));

        let mut spec = RunSpec {            prompt,
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
            resume_session: resume_session.clone(),
            // The worker's one harness tool: its own account of the work. A
            // write to Harness, not to the repository, so it rides in
            // allowed_tools instead of the approval queue.
            tools,
            thinking_tokens: None,
            // A worker may fan out one level; its children never may.
            subagents: true,
            report_work: true,
        };
        if let Some(allowed) = spec.allowed_tools.as_mut() {
            allowed.push("mcp__harness__report_work".to_string());
            allowed.sort();
            allowed.dedup();
        }

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
        let fut = self.agent.run(spec, ev_tx, token.clone());

        let self_tx = self.self_tx.clone();
        let run_log = self.run_log.clone();
        let runs_tx = self.runs_tx.clone();
        let clock = Arc::clone(&self.clock);
        let project_id = self.config.project_id.clone();
        let git = Arc::clone(&self.git);
        let base = self.config.base_branch.clone();
        let commits_work = profile.worktree != WorktreeMode::None;
        let agent_id = profile.agent_id.clone();
        let entry_agent_id = agent_id.clone();
        let task_agent_id = agent_id.clone();
        let done_profile = profile;
        let task_profile = done_profile.clone();
        let card_for_events = card_id.clone();
        let run_for_events = run_id.clone();
        let done_card = card_id.clone();
        let done_run = run_id.clone();
        let task_worktree = worktree.clone();
        let commit_flag = Arc::clone(&commit_on_cancel);
        let task_title = card_title;
        let task_report = Arc::clone(&work_report);

        let handle = tokio::spawn(async move {
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
            let outcome = result.unwrap_or_else(|e| harness_ports::RunOutcome::Failed {
                message: e,
                cost_usd: None,
                turns: None,
            });

            // What the agent said about its own work, as it last stood. An
            // absent report is normal: the body stays generic and a Notice
            // says so — silence must never pass for a summary.
            let reported = task_report.lock().unwrap().clone();
            let mut unreported = false;
            let mut committed_sha = None;

            // A commit that fails must never pass for one that worked: the
            // Director would then review an empty diff and approve nothing.
            let mut commit_error = None;
            if commits_work {
                let outcome_for_commit = &outcome;
                let result = tokio::task::block_in_place(|| match outcome_for_commit {
                    harness_ports::RunOutcome::Completed { .. } => {
                        let trailers = Trailers(vec![
                            ("Harness-Card".to_string(), done_card.to_string()),
                            ("Harness-Run".to_string(), done_run.to_string()),
                            ("Harness-Agent".to_string(), task_agent_id.clone()),
                        ]);
                        // The subject is the card's title, so `git log` reads
                        // like history; the ids live in the body and in the
                        // trailers, where machines look. The agent's own
                        // summary — when it gave one — is the body.
                        let subject = match task_title.trim() {
                            "" => format!("harness: work for card {done_card}"),
                            title => format!("harness: {title}"),
                        };
                        let run_short: String =
                            done_run.0.chars().take(8).collect();
                        let footer = format!(
                            "harness card {done_card}, run {run_short}, by {}",
                            task_agent_id
                        );
                        let msg = match reported.summary.trim() {
                            "" => format!("{subject}\n\n{footer}"),
                            summary => format!("{subject}\n\n{summary}\n\n{footer}"),
                        };
                        match git.commit(&task_worktree, &msg, &trailers) {
                            Ok(sha) => {
                                committed_sha = Some(sha);
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    }
                    harness_ports::RunOutcome::Cancelled
                    | harness_ports::RunOutcome::Failed { .. } => {
                        if commit_flag.load(Ordering::SeqCst) {
                            match git.commit_wip(&task_worktree) {
                                Ok(_) => Ok(()),
                                Err(e) => Err(e),
                            }
                        } else {
                            Ok(())
                        }
                    }
                });
                if let Err(e) = result {
                    commit_error = Some(e.to_string());
                }
                unreported = matches!(outcome, harness_ports::RunOutcome::Completed { .. })
                    && reported.summary.trim().is_empty()
                    && commit_error.is_none();
                let _ = base;
            }

            if commit_error.is_some() || unreported {
                let text = match &commit_error {
                    Some(reason) => format!("could not commit the work: {reason}"),
                    None => "the agent did not report its work; the commit body stayed generic"
                        .to_string(),
                };
                let ts_ms = clock.now_millis();
                let event = RunEvent::Notice { text };
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
                    profile: Box::new(task_profile),
                    commit_failed: commit_error.is_some(),
                    commit_sha: committed_sha,
                })
                .await;
        });

        self.runs.insert(
            card_id.clone(),
            RunEntry {
                run_id: run_id.clone(),
                token,
                commit_on_cancel,
                work_report,
                handle: Some(handle),
                worktree: worktree.clone(),
                agent_id: entry_agent_id,
                started_ms,
            },
        );
        // The start is no longer in flight: it is a run. Clearing it here and
        // not before keeps the window honest the whole way.
        self.starting.remove(&card_id);
        self.sessions.insert(
            card_id.clone(),
            SessionEntry {
                run_id: Some(run_id.clone()),
                worktree: worktree.0.clone(),
                branch,
                session_id: resume_session.clone(),
                agent_id,
                started_ms,
            },
        );

        let _ = reply.send(Ok(run_id));
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
        commit_sha: Option<String>,
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
            harness_ports::RunOutcome::Failed {
                message,
                cost_usd,
                turns,
            } => {
                self.emit_run(
                    &card_id,
                    &run_id,
                    RunEvent::Notice {
                        text: format!("run failed: {message}"),
                    },
                );
                // A failed run spent what it spent: the card sums it either
                // way, so budgets and analyst numbers stay honest.
                (RunOutcome::Failed, *cost_usd, *turns)
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

        // Mirror mode: the engine builds, off the actor (#46), before anyone
        // reviews. The reviewer dispatch waits for the verdict — a green build
        // is part of what gets approved.
        if let Some(build) = self.config.post_build.clone() {
            if let Some(sha) = &commit_sha {
                self.emit_run(
                    &card_id,
                    &run_id,
                    RunEvent::Notice {
                        text: "building the orchestrator; review starts when it holds".into(),
                    },
                );
                let self_tx = self.self_tx.clone();
                let sha = sha.clone();
                let worktree = self
                    .sessions
                    .get(&card_id)
                    .map(|s| s.worktree.clone())
                    .unwrap_or_default();
                tokio::spawn(async move {
                    let (ok, tail) =
                        run_build(&build, std::path::Path::new(&worktree)).await;
                    let _ = self_tx
                        .send(Msg::BuildDone {
                            card_id,
                            run_id,
                            profile: Box::new(profile),
                            commit_sha: sha,
                            ok,
                            tail,
                            worktree,
                        })
                        .await;
                });
                return;
            }
        }

        self.dispatch_review(card_id, run_id, profile).await;
    }

    /// The tail every finished run shares: who reads the diff.
    pub(crate) async fn dispatch_review(
        &mut self,
        card_id: CardId,
        run_id: RunId,
        profile: RunProfile,
    ) {


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

/// Run the project's build with the card's worktree as cwd. Minutes-long, so
/// it lives in its own task; the engine hears the verdict as a message.
/// Green counts only when the expected artefact exists on disk.
async fn run_build(build: &crate::BuildSpec, worktree: &std::path::Path) -> (bool, String) {
    use tokio::io::AsyncReadExt;
    let mut cmd = tokio::process::Command::new(&build.program);
    cmd.args(&build.args)
        .current_dir(worktree)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        // No console flash: same discipline as every other spawned process.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = match cmd.output().await {
        Ok(o) => o,
        Err(e) => return (false, format!("could not start {}: {e}", build.program)),
    };
    let mut tail = String::new();
    for stream in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(stream);
        tail.push_str(text.trim_end());
        tail.push('\n');
    }
    // Keep the transcript humane: the compiler's last words, not all of them.
    let lines: Vec<&str> = tail.lines().collect();
    let tail = if lines.len() > 30 {
        lines[lines.len() - 30..].join("\n")
    } else {
        tail
    };
    if !output.status.success() {
        return (false, tail);
    }
    let artifact = worktree.join(&build.artifact);
    match std::fs::metadata(&artifact) {
        Ok(_) => (true, tail),
        Err(_) => (
            false,
            format!(
                "build reported success but {} is missing",
                build.artifact
            ),
        ),
    }
}
