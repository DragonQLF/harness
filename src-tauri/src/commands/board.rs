//! Board, run and history commands. Each one is a thin translation from the
//! UI's intent to an engine command; no state lives here.

use std::path::Path;
use std::sync::Arc;

use harness_domain::{Actor, CardId, Command, Status};
use harness_engine::{ActiveRun, Snapshot};
use harness_ports::RunLogLine;
use serde::Serialize;
use tauri::State;

use harness_app::agents;
use harness_app::insights::{self, ActivityRow, ProjectStats};

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

#[tauri::command]
pub async fn snapshot(project_id: String, ws: Shared<'_>) -> Result<Snapshot, String> {
    ws.runtime(&project_id).await?.engine.snapshot().await
}

#[derive(Debug, Serialize)]
pub struct CreatedCard {
    pub card_id: String,
    pub run_id: Option<String>,
}

/// Add a card and, when asked, take it straight to a running agent.
#[tauri::command]
pub async fn create_card(
    project_id: String,
    title: String,
    agent_id: Option<String>,
    start: bool,
    ready: bool,
    ws: Shared<'_>,
) -> Result<CreatedCard, String> {
    create_card_inner(
        &ws,
        &project_id,
        &title,
        &agent_id.unwrap_or_else(|| agents::DEFAULT_WORKER.to_string()),
        start,
        ready,
    )
    .await
}

/// The card-creating path itself, shared with the Director's tools.
pub(crate) async fn create_card_inner(
    ws: &Arc<Workspace>,
    project_id: &str,
    title: &str,
    agent_id: &str,
    start: bool,
    ready: bool,
) -> Result<CreatedCard, String> {
    let runtime = ws.runtime(project_id).await?;
    let card_id = CardId::new(format!("c_{}", short_id()));
    runtime
        .engine
        .execute(Command::CreateCard {
            card_id: card_id.clone(),
            title: title.trim().to_string(),
        })
        .await?;

    runtime
        .engine
        .execute(Command::AssignAgent {
            card_id: card_id.clone(),
            agent_id: agent_id.to_string(),
        })
        .await?;

    if start || ready {
        runtime
            .engine
            .execute(Command::MoveCard {
                card_id: card_id.clone(),
                to: Status::Ready,
            })
            .await?;
    }

    let run_id = if start {
        Some(start_run_inner(ws, project_id, card_id.clone(), None).await?)
    } else {
        None
    };

    Ok(CreatedCard {
        card_id: card_id.to_string(),
        run_id,
    })
}

pub(crate) fn short_id() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "").chars().take(4).collect()
}

#[tauri::command]
pub async fn move_card(
    project_id: String,
    card_id: String,
    to: Status,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id).await?
        .engine
        .execute(Command::MoveCard {
            card_id: CardId::new(card_id),
            to,
        })
        .await
}

#[tauri::command]
pub async fn override_card(
    project_id: String,
    card_id: String,
    to: Status,
    reason: String,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id).await?
        .engine
        .execute(Command::OverrideCard {
            card_id: CardId::new(card_id),
            to,
            reason,
        })
        .await
}

/// Say which cards must be Done before this one may start. Order, not file
/// conflict; a discarded dependency frees its dependent automatically.
#[tauri::command]
pub async fn set_dependencies(
    project_id: String,
    card_id: String,
    depends_on: Vec<String>,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id).await?
        .engine
        .execute(Command::SetDependencies {
            card_id: CardId::new(card_id),
            depends_on: depends_on.into_iter().map(CardId::new).collect(),
        })
        .await
}

/// Fix a card's title in place.
///
/// The domain refuses this once the card has run, and that guard is the whole
/// reason the command is safe: a card with a run behind it has a transcript
/// and a commit whose subject is the old title, and renaming it would make
/// both of them describe work under a name that no longer exists. Until then
/// the title is just the operator's own wording of what they want, and
/// correcting a typo should not mean discarding the card and writing it again.
#[tauri::command]
pub async fn edit_card(
    project_id: String,
    card_id: String,
    title: String,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id).await?
        .engine
        .execute(Command::EditCard {
            card_id: CardId::new(card_id),
            title,
        })
        .await
}

#[tauri::command]
pub async fn assign_agent(
    project_id: String,
    card_id: String,
    agent_id: String,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id).await?
        .engine
        .execute(Command::AssignAgent {
            card_id: CardId::new(card_id),
            agent_id,
        })
        .await
}

/// Refuse to approve over a red check.
///
/// The checks already run — `card_checks_after_run` fires when a run ends and
/// writes the result against the card. Nothing read it. `CardChecks::failing()`
/// was computed, stored, and consulted by nobody, so a card could be approved
/// with its own build failing and nothing anywhere would say so.
///
/// That is the shape of every defect in the 2026-08-31 write-up: six of them
/// shipped with a green suite, and the lesson taken was that a check nobody is
/// obliged to look at is not a check. This is the obligation. It is deliberately
/// not a judgment about *which* checks matter — the operator configured them,
/// and a check they configured going red is their answer, not Relay's.
///
/// A card with no configured checks passes. Saying "no checks" is a fact about
/// this project, not a failure of this card, and inventing a gate the operator
/// never asked for would stop work for no reason.
pub(crate) fn refuse_approval_over_red_checks(
    paths: &harness_app::paths::AppPaths,
    project_id: &str,
    card_id: &str,
) -> Result<(), String> {
    let Some(pass) = harness_app::checks::read_card_checks(
        &paths.card_checks_file(project_id, card_id),
    ) else {
        return Ok(());
    };
    let failing = pass.failing();
    if failing == 0 {
        return Ok(());
    }
    let names: Vec<&str> = pass
        .rows
        .iter()
        .filter(|r| r.status == "fail")
        .map(|r| r.name.as_str())
        .collect();
    Err(format!(
        "{failing} of this card's checks are failing ({}). \
         Approving would put a red build on the base branch. \
         Send the card back, or fix the check and run it again.",
        names.join(", ")
    ))
}

#[tauri::command]
pub async fn approve_card(
    project_id: String,
    card_id: String,
    reason: Option<String>,
    ws: Shared<'_>,
) -> Result<u64, String> {
    refuse_approval_over_red_checks(&ws.paths, &project_id, &card_id)?;
    ws.runtime(&project_id).await?
        .engine
        .execute(Command::ApproveCard {
            card_id: CardId::new(card_id),
            by: Actor::Human,
            reason: reason.unwrap_or_default(),
            // Whole card: the Board's Approve names no block, which is
            // what an empty selection means everywhere.
            hunks: Vec::new(),
        })
        .await
}

#[tauri::command]
pub async fn reject_card(
    project_id: String,
    card_id: String,
    reason: String,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id).await?
        .engine
        .execute(Command::RejectCard {
            card_id: CardId::new(card_id),
            reason,
            by: Actor::Human,
            hunks: Vec::new(),
        })
        .await
}

/// Take a card off the board for good, and its worktree with it.
#[tauri::command]
pub async fn discard_card(
    project_id: String,
    card_id: String,
    reason: Option<String>,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id).await?
        .engine
        .execute(Command::DiscardCard {
            card_id: CardId::new(card_id),
            reason: reason.unwrap_or_else(|| "deleted by you".to_string()),
        })
        .await
}

/// Hand a card to its agent. The prompt is the agent's brief plus the card
/// title, with anything extra the operator typed appended.
#[tauri::command]
pub async fn start_run(
    project_id: String,
    card_id: String,
    prompt: Option<String>,
    ws: Shared<'_>,
) -> Result<String, String> {
    start_run_inner(&ws, &project_id, CardId::new(card_id), prompt).await
}

pub(crate) async fn start_run_inner(
    ws: &Arc<Workspace>,
    project_id: &str,
    card_id: CardId,
    extra: Option<String>,
) -> Result<String, String> {
    let runtime = ws.runtime(project_id).await?;
    if runtime.project.paused {
        return Err("this project is paused; resume it before starting work".to_string());
    }

    let snap = runtime.engine.snapshot().await?;
    let card = snap
        .cards
        .iter()
        .find(|c| c.id == card_id)
        .ok_or_else(|| format!("unknown card {card_id}"))?;

    let profile = ws
        .agent(&card.agent_id)
        .await
        .ok_or_else(|| format!("no agent profile for {}", card.agent_id))?;
    if profile.paused {
        return Err(format!("{} is paused", profile.name));
    }
    // "Reads the main checkout, never writes" is what every screen says about
    // a profile with no worktree. Until now nothing made it true: such a run
    // gets `cwd = repo_root` and the path guard only refuses writes *outside*
    // `cwd`, so an agent with Write and no worktree edits the operator's own
    // tree — no branch, no diff, nothing to approve or reject.
    //
    // Refused here rather than by quietly withholding the tools, because an
    // agent that finds Write missing halfway through a card fails in a way
    // nobody can act on. This says which two settings disagree.
    if profile.writes_into_the_live_checkout() {
        return Err(format!(
            "{} may write but has no worktree, so its work would land in the live checkout \
             with no branch and no diff to review. Give it a per-card worktree, or take Write \
             and Edit off it.",
            profile.name
        ));
    }

    let settings = ws.settings();
    let mut prompt = profile.prompt_for(&card.title, extra.as_deref());

    // Curated memory, minimal form: the project's charter and the operator's
    // global notes ride with every run. Both are capped by the reader; a
    // missing file contributes nothing. The charter prefers the project's
    // memory directory; the repository root still counts.
    let charter = harness_app::memory::charter_between(
        &ws.paths.project_memory_charter(runtime.project.id.as_str()),
        &Path::new(&runtime.project.path).join("charter.md"),
    );
    if let Some(charter) = charter {
        prompt.push_str("\n\nThis project's charter:\n");
        prompt.push_str(&charter);
    }
    let global = harness_app::memory::global_for(ws.paths.root());
    if let Some(global) = global {
        prompt.push_str("\n\nStanding notes from the operator:\n");
        prompt.push_str(&global);
    }
    // And what has already been settled on this board. `record_decision` has
    // been writing these since it existed and nothing read them back, so a rule
    // the operator dictated never reached the agent it was dictated at.
    if let Some(decisions) = harness_app::memory::decisions_from(
        &ws.paths.project_memory_decisions(runtime.project.id.as_str()),
    ) {
        prompt.push_str("\n\nDecisions already settled on this project — follow them:\n");
        prompt.push_str(&decisions);
    }

    // And what the cards before this one wrote down. `report_work` has always
    // taken these notes and the log has always kept them; nothing ever read
    // them back, so every card started blind and paid to rediscover the same
    // ground — 3 reads before the first card could write, 71 before the fifth.
    // Derived here rather than promoted to files: see `memory::notes_from`.
    let history = {
        let store = Arc::clone(&runtime.store);
        tauri::async_runtime::spawn_blocking(move || {
            harness_ports::StorePort::read_all(store.as_ref()).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??
    };
    if let Some(notes) = harness_app::memory::notes_from(&history, &snap.cards) {
        prompt.push_str("\n\nWhat earlier cards on this board learned:\n");
        prompt.push_str(&notes);
    }

    runtime
        .engine
        .start_run(card_id, prompt, profile.run_profile(&settings, ws.paths.root()))
        .await
        .map(|run_id| run_id.0)
}

#[tauri::command]
pub async fn cancel_run(
    project_id: String,
    card_id: String,
    ws: Shared<'_>,
) -> Result<(), String> {
    ws.runtime(&project_id).await?
        .engine
        .cancel_run(CardId::new(card_id))
        .await
}

#[tauri::command]
pub async fn active_runs(project_id: String, ws: Shared<'_>) -> Result<Vec<ActiveRun>, String> {
    ws.runtime(&project_id).await?.engine.active_runs().await
}

/// The stored transcript of a run, so reopening a session shows its history.
#[tauri::command]
pub async fn run_log(
    project_id: String,
    run_id: String,
    ws: Shared<'_>,
) -> Result<Vec<RunLogLine>, String> {
    let runtime = ws.runtime(&project_id).await?;
    let log = Arc::clone(&runtime.run_log);
    tauri::async_runtime::spawn_blocking(move || {
        harness_ports::RunLogPort::read(log.as_ref(), &run_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What a card actually changed: the facts the review screen states, and the
/// patch it shows. Read from the worktree, so it is the truth on disk rather
/// than anything remembered from the run.
#[derive(Debug, Serialize)]
pub struct CardDiff {
    pub card_id: String,
    pub base: String,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub session_id: Option<String>,
    pub files: Vec<String>,
    pub added: u64,
    pub removed: u64,
    pub patch: String,
}

#[tauri::command]
pub async fn card_diff(
    project_id: String,
    card_id: String,
    ws: Shared<'_>,
) -> Result<CardDiff, String> {
    let runtime = ws.runtime(&project_id).await?;
    let base = runtime.project.base_branch.clone();
    let snap = runtime.engine.snapshot().await?;
    let session = snap.sessions.iter().find(|s| s.card_id.as_str() == card_id).cloned();
    let Some(session) = session else {
        return Ok(CardDiff {
            card_id,
            base,
            branch: None,
            worktree: None,
            session_id: None,
            files: Vec::new(),
            added: 0,
            removed: 0,
            patch: String::new(),
        });
    };
    let git = Arc::clone(&runtime.git);
    let worktree = session.worktree.clone();
    let against = base.clone();
    let read = tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::Path::new(&worktree);
        let (files, added, removed) = git.changed_files(path, &against);
        let patch = git.review_patch(path, &against);
        (files, added, removed, patch)
    })
    .await
    .map_err(|e| e.to_string())?;
    let (files, added, removed, patch) = read;
    Ok(CardDiff {
        card_id,
        base,
        branch: session.branch.clone(),
        worktree: Some(session.worktree.clone()),
        session_id: session.session_id.clone(),
        files,
        added,
        removed,
        patch,
    })
}

/// The Review queue, ordered by the Triador: surface and wait, mechanically
/// scored, so wide diffs and old waits surface before quiet ones.
#[derive(Debug, Serialize)]
pub struct QueueRow {
    pub card_id: String,
    pub title: String,
    pub risk: u64,
    pub reasons: Vec<String>,
}

#[tauri::command]
pub async fn review_queue(
    project_id: String,
    ws: Shared<'_>,
) -> Result<Vec<QueueRow>, String> {
    let runtime = ws.runtime(&project_id).await?;
    let snap = runtime.engine.snapshot().await?;
    let base = runtime.project.base_branch.clone();
    let git = Arc::clone(&runtime.git);

    // Wait times come from the event log: the last run finished, minus any
    // reject that sent the work back out again.
    let store = Arc::clone(&runtime.store);
    let history = tauri::async_runtime::spawn_blocking(move || {
        harness_ports::StorePort::read_all(store.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let now_ms = history.last().map(|h| h.ts_ms).unwrap_or(0);
    let mut waiting: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for stored in &history {
        match &stored.event {
            harness_domain::Event::RunFinished { card_id, .. } => {
                waiting.insert(card_id.to_string(), stored.ts_ms);
            }
            harness_domain::Event::CardRejected { card_id, .. } => {
                waiting.remove(card_id.as_str());
            }
            _ => {}
        }
    }

    let review_cards: Vec<harness_domain::Card> = snap
        .cards
        .iter()
        .filter(|c| c.status == harness_domain::Status::Review)
        .cloned()
        .collect();
    let ids: Vec<String> = review_cards.iter().map(|c| c.id.to_string()).collect();
    let worktree_of = move |card_id: &str| {
        snap.sessions
            .iter()
            .find(|s| s.card_id.as_str() == card_id)
            .map(|s| s.worktree.clone())
    };
    // Surface comes from each card's worktree, off the actor's back.
    let surfaces = tauri::async_runtime::spawn_blocking(move || {
        let mut map = std::collections::HashMap::new();
        for id in ids {
            if let Some(wt) = worktree_of(&id) {
                let (files, added, removed) =
                    git.changed_files(std::path::Path::new(&wt), &base);
                map.insert(
                    id,
                    insights::DiffSurface {
                        files: files.len() as u64,
                        added,
                        removed,
                    },
                );
            }
        }
        map
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(insights::triage(&review_cards, &waiting, &surfaces, now_ms)
        .into_iter()
        .map(|c| QueueRow {
            card_id: c.card_id,
            title: c.title,
            risk: c.risk,
            reasons: c.reasons,
        })
        .collect())
}
#[tauri::command]
pub async fn activity(
    project_id: String,
    limit: Option<usize>,
    ws: Shared<'_>,
) -> Result<Vec<ActivityRow>, String> {
    let runtime = ws.runtime(&project_id).await?;
    let cards = runtime.engine.snapshot().await?.cards;
    let store = Arc::clone(&runtime.store);
    let run_log = Arc::clone(&runtime.run_log);
    let history = tauri::async_runtime::spawn_blocking(move || {
        harness_ports::StorePort::read_all(store.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    // The transcripts are read here, once, rather than a row at a time from the
    // screen: the Sessions table wants a tool count on every line, and asking
    // for two hundred logs over IPC to tally them is the expensive way round.
    tauri::async_runtime::spawn_blocking(move || {
        let mut rows = insights::activity(&history, &cards, limit.unwrap_or(200));
        insights::fill_tool_counts(&mut rows, |run_id| run_log.path_of(run_id));
        rows
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn project_stats(
    project_id: String,
    tz_offset_minutes: Option<i64>,
    ws: Shared<'_>,
) -> Result<ProjectStats, String> {
    let runtime = ws.runtime(&project_id).await?;
    let cards = runtime.engine.snapshot().await?.cards;
    let store = Arc::clone(&runtime.store);
    let history = tauri::async_runtime::spawn_blocking(move || {
        harness_ports::StorePort::read_all(store.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(insights::project_stats(
        &history,
        &cards,
        tz_offset_minutes.unwrap_or(0),
    ))
}

#[cfg(test)]
mod approval_gate_tests {
    use super::*;
    use harness_app::checks::{CardChecks, CheckRow};

    fn paths(tag: &str) -> harness_app::paths::AppPaths {
        let dir = std::env::temp_dir().join(format!(
            "harness-approval-gate-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        harness_app::paths::AppPaths::new(dir).unwrap()
    }

    fn row(name: &str, status: &str) -> CheckRow {
        CheckRow {
            name: name.into(),
            command: "pnpm build".into(),
            status: status.into(),
            detail: String::new(),
            ran_ms: 0,
            duration_ms: 0,
        }
    }

    fn record(p: &harness_app::paths::AppPaths, card: &str, rows: Vec<CheckRow>) {
        let file = p.card_checks_file("proj", card);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            serde_json::to_string(&CardChecks {
                card_id: card.into(),
                run_id: "r1".into(),
                worktree: "/tmp/wt".into(),
                ran_ms: 1,
                rows,
            })
            .unwrap(),
        )
        .unwrap();
    }

    /// A card whose checks were never run, or whose project has none, is not a
    /// card that failed. Inventing a gate the operator never configured would
    /// stop work for no reason.
    #[test]
    fn a_card_with_no_recorded_pass_is_not_blocked() {
        let p = paths("none");
        assert!(refuse_approval_over_red_checks(&p, "proj", "c_1").is_ok());
    }

    #[test]
    fn a_green_pass_approves() {
        let p = paths("green");
        record(&p, "c_2", vec![row("build", "ok"), row("tests", "ok")]);
        assert!(refuse_approval_over_red_checks(&p, "proj", "c_2").is_ok());
    }

    /// The one that matters. The checks already ran and already wrote this
    /// file; `failing()` was computed and read by nobody, so a card could be
    /// approved with its own build red and nothing anywhere said so.
    #[test]
    fn a_red_check_refuses_the_approval_and_names_what_failed() {
        let p = paths("red");
        record(
            &p,
            "c_3",
            vec![row("build", "ok"), row("tests", "fail"), row("types", "fail")],
        );
        let refused = refuse_approval_over_red_checks(&p, "proj", "c_3")
            .expect_err("a red build must not be approvable");
        assert!(refused.contains("tests"), "{refused}");
        assert!(refused.contains("types"), "{refused}");
        assert!(
            refused.contains("2 of this card's checks are failing"),
            "{refused}",
        );
    }

    /// A warning is not a failure. The operator's own vocabulary decides what
    /// blocks, and only `fail` does.
    #[test]
    fn a_warning_does_not_block() {
        let p = paths("warn");
        record(&p, "c_4", vec![row("lint", "warn"), row("build", "ok")]);
        assert!(refuse_approval_over_red_checks(&p, "proj", "c_4").is_ok());
    }
}
