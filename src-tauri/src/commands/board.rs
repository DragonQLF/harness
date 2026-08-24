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
    ws.runtime(&project_id)?.engine.snapshot().await
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
    let runtime = ws.runtime(project_id)?;
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

fn short_id() -> String {
    uuid::Uuid::new_v4().to_string().replace('-', "").chars().take(4).collect()
}

#[tauri::command]
pub async fn move_card(
    project_id: String,
    card_id: String,
    to: Status,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id)?
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
    ws.runtime(&project_id)?
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
    ws.runtime(&project_id)?
        .engine
        .execute(Command::SetDependencies {
            card_id: CardId::new(card_id),
            depends_on: depends_on.into_iter().map(CardId::new).collect(),
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
    ws.runtime(&project_id)?
        .engine
        .execute(Command::AssignAgent {
            card_id: CardId::new(card_id),
            agent_id,
        })
        .await
}

#[tauri::command]
pub async fn approve_card(
    project_id: String,
    card_id: String,
    reason: Option<String>,
    ws: Shared<'_>,
) -> Result<u64, String> {
    ws.runtime(&project_id)?
        .engine
        .execute(Command::ApproveCard {
            card_id: CardId::new(card_id),
            by: Actor::Human,
            reason: reason.unwrap_or_default(),
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
    ws.runtime(&project_id)?
        .engine
        .execute(Command::RejectCard {
            card_id: CardId::new(card_id),
            reason,
            by: Actor::Human,
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
    ws.runtime(&project_id)?
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
    let runtime = ws.runtime(project_id)?;
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
        .ok_or_else(|| format!("no agent profile for {}", card.agent_id))?;
    if profile.paused {
        return Err(format!("{} is paused", profile.name));
    }

    let settings = ws.settings();
    let mut prompt = profile.prompt_for(&card.title, extra.as_deref());

    // Curated memory, minimal form: the project's charter and the operator's
    // global notes ride with every run. Both are capped by the reader; a
    // missing file contributes nothing.
    let charter = harness_app::memory::charter_for(Path::new(&runtime.project.path));
    if let Some(charter) = charter {
        prompt.push_str("\n\nThis project's charter:\n");
        prompt.push_str(&charter);
    }
    let global = harness_app::memory::global_for(ws.paths.root());
    if let Some(global) = global {
        prompt.push_str("\n\nStanding notes from the operator:\n");
        prompt.push_str(&global);
    }

    runtime
        .engine
        .start_run(card_id, prompt, profile.run_profile(&settings))
        .await
        .map(|run_id| run_id.0)
}

#[tauri::command]
pub async fn cancel_run(
    project_id: String,
    card_id: String,
    ws: Shared<'_>,
) -> Result<(), String> {
    ws.runtime(&project_id)?
        .engine
        .cancel_run(CardId::new(card_id))
        .await
}

#[tauri::command]
pub async fn active_runs(project_id: String, ws: Shared<'_>) -> Result<Vec<ActiveRun>, String> {
    ws.runtime(&project_id)?.engine.active_runs().await
}

/// The stored transcript of a run, so reopening a session shows its history.
#[tauri::command]
pub async fn run_log(
    project_id: String,
    run_id: String,
    ws: Shared<'_>,
) -> Result<Vec<RunLogLine>, String> {
    let runtime = ws.runtime(&project_id)?;
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
    let runtime = ws.runtime(&project_id)?;
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
    let runtime = ws.runtime(&project_id)?;
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
    let runtime = ws.runtime(&project_id)?;
    let cards = runtime.engine.snapshot().await?.cards;
    let store = Arc::clone(&runtime.store);
    let history = tauri::async_runtime::spawn_blocking(move || {
        harness_ports::StorePort::read_all(store.as_ref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(insights::activity(&history, &cards, limit.unwrap_or(200)))
}

#[tauri::command]
pub async fn project_stats(
    project_id: String,
    tz_offset_minutes: Option<i64>,
    ws: Shared<'_>,
) -> Result<ProjectStats, String> {
    let runtime = ws.runtime(&project_id)?;
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
