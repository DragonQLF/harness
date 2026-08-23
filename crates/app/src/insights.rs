//! Numbers the UI shows, derived from the event log rather than stored twice.
//! Everything here is a pure function of events plus the current board, so it
//! can be tested without an engine.

use std::collections::BTreeMap;

use harness_domain::{Actor, Card, Event, RunOutcome, Status};
use harness_ports::StoredEvent;
use serde::Serialize;

const DAY_MS: i64 = 86_400_000;

/// Local day index for a timestamp, given the UI's offset from UTC.
fn day_index(ts_ms: u64, tz_offset_minutes: i64) -> i64 {
    let shifted = ts_ms as i64 - tz_offset_minutes * 60_000;
    shifted.div_euclid(DAY_MS)
}

pub fn today_index(tz_offset_minutes: i64) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    day_index(now, tz_offset_minutes)
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityRow {
    pub seq: u64,
    pub ts_ms: u64,
    /// `card`, `run`, `approval` or `review` — the Activity filters.
    pub kind: &'static str,
    /// Human label, e.g. "Run finished".
    pub label: String,
    pub card_id: String,
    pub detail: String,
}

pub fn activity(history: &[StoredEvent], cards: &[Card], limit: usize) -> Vec<ActivityRow> {
    let titles: BTreeMap<&str, &str> = cards
        .iter()
        .map(|c| (c.id.as_str(), c.title.as_str()))
        .collect();

    // A recorded session is bookkeeping, not something that happened on the
    // board, so it never becomes a row.
    let shown: Vec<&StoredEvent> = history
        .iter()
        .filter(|s| !matches!(s.event, Event::AgentSession { .. }))
        .collect();
    let start = shown.len().saturating_sub(limit);
    let mut rows: Vec<ActivityRow> = shown[start..]
        .iter()
        .map(|stored| {
            let card_id = stored.event.card_id().to_string();
            let title = titles.get(card_id.as_str()).copied().unwrap_or("");
            let (kind, label, detail) = match &stored.event {
                Event::CardCreated { title, .. } => ("card", "Card created", title.clone()),
                Event::CardAssigned { agent_id, .. } => {
                    ("card", "Agent assigned", agent_id.clone())
                }
                Event::CardMoved { from, to, .. } => (
                    "card",
                    "Card moved",
                    format!("{} → {}", status_name(*from), status_name(*to)),
                ),
                Event::CardOverridden { to, reason, .. } => (
                    "card",
                    "Card overridden",
                    format!("→ {} ({reason})", status_name(*to)),
                ),
                Event::RunStarted { run_id, .. } => (
                    "run",
                    "Run started",
                    if title.is_empty() {
                        format!("run {}", short(&run_id.0))
                    } else {
                        title.to_string()
                    },
                ),
                Event::RunFinished {
                    outcome, cost_usd, ..
                } => (
                    "run",
                    match outcome {
                        RunOutcome::Completed => "Run finished",
                        RunOutcome::Cancelled => "Run stopped",
                        RunOutcome::Failed => "Run failed",
                    },
                    match cost_usd {
                        Some(c) => format!("${c:.4}"),
                        None => "no cost recorded".to_string(),
                    },
                ),
                Event::CardApproved { by, reason, .. } => (
                    "review",
                    match by {
                        Actor::Director => "Approved by the Director",
                        Actor::Human => "Approved by you",
                    },
                    reason.clone(),
                ),
                Event::CardDiscarded { reason, .. } => (
                    "card",
                    "Card deleted",
                    if reason.is_empty() {
                        "removed from the board".to_string()
                    } else {
                        reason.clone()
                    },
                ),
                // Filtered out above; the match still has to be total.
                Event::AgentSession { session_id, .. } => {
                    ("run", "Session recorded", short(session_id))
                }
                Event::CardRejected { reason, by, .. } => (
                    "review",
                    match by {
                        Actor::Director => "Sent back by the Director",
                        Actor::Human => "Sent back by you",
                    },
                    reason.clone(),
                ),
            };
            ActivityRow {
                seq: stored.seq,
                ts_ms: stored.ts_ms,
                kind,
                label: label.to_string(),
                card_id,
                detail,
            }
        })
        .collect();
    rows.reverse();
    rows
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

pub fn status_name(status: Status) -> &'static str {
    match status {
        Status::Backlog => "Later",
        Status::Ready => "Ready",
        Status::Running => "Working",
        Status::Review => "Review",
        Status::Done => "Done",
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectStats {
    pub cards: usize,
    pub backlog: usize,
    pub ready: usize,
    pub running: usize,
    pub review: usize,
    pub done: usize,
    pub runs_total: u32,
    pub runs_today: u32,
    pub spend_total: f64,
    pub spend_today: f64,
    pub done_today: usize,
    /// Average spend across cards that cost something.
    pub cost_per_card: f64,
    /// Runs finished per day for the last seven days, oldest first.
    pub week_runs: Vec<u32>,
    /// Lines the agents wrote per day, oldest first (from card turns is not
    /// available, so this counts runs and is filled in by the git side).
    pub last_event_ms: u64,
}

pub fn project_stats(
    history: &[StoredEvent],
    cards: &[Card],
    tz_offset_minutes: i64,
) -> ProjectStats {
    let today = today_index(tz_offset_minutes);
    let mut stats = ProjectStats {
        cards: cards.len(),
        week_runs: vec![0; 7],
        ..Default::default()
    };

    for card in cards {
        match card.status {
            Status::Backlog => stats.backlog += 1,
            Status::Ready => stats.ready += 1,
            Status::Running => stats.running += 1,
            Status::Review => stats.review += 1,
            Status::Done => stats.done += 1,
        }
        stats.spend_total += card.cost_usd;
    }

    let paid: Vec<f64> = cards
        .iter()
        .map(|c| c.cost_usd)
        .filter(|c| *c > 0.0)
        .collect();
    if !paid.is_empty() {
        stats.cost_per_card = paid.iter().sum::<f64>() / paid.len() as f64;
    }

    for stored in history {
        stats.last_event_ms = stats.last_event_ms.max(stored.ts_ms);
        let day = day_index(stored.ts_ms, tz_offset_minutes);
        let days_ago = today - day;
        match &stored.event {
            Event::RunFinished { cost_usd, .. } => {
                stats.runs_total += 1;
                if days_ago == 0 {
                    stats.runs_today += 1;
                    stats.spend_today += cost_usd.unwrap_or(0.0);
                }
                if (0..7).contains(&days_ago) {
                    let idx = (6 - days_ago) as usize;
                    stats.week_runs[idx] += 1;
                }
            }
            Event::CardApproved { .. } if days_ago == 0 => stats.done_today += 1,
            _ => {}
        }
    }

    stats
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentStats {
    pub agent_id: String,
    pub runs: u32,
    pub cards: usize,
    pub cards_done: usize,
    pub spend: f64,
    pub avg_cost: f64,
    pub turns: u32,
    /// Diffs this agent decided on, when it is the Director.
    pub reviews: u32,
    pub sent_back: u32,
    /// Runs per day for the last seven days, oldest first.
    pub week_runs: Vec<u32>,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub commits: u64,
}

/// Per-agent numbers across one project's log. Line counts come from git and
/// are added by the caller.
pub fn agent_stats(
    history: &[StoredEvent],
    cards: &[Card],
    tz_offset_minutes: i64,
) -> BTreeMap<String, AgentStats> {
    let today = today_index(tz_offset_minutes);
    let owner: BTreeMap<&str, &str> = cards
        .iter()
        .map(|c| (c.id.as_str(), c.agent_id.as_str()))
        .collect();
    let mut out: BTreeMap<String, AgentStats> = BTreeMap::new();

    fn slot<'m>(map: &'m mut BTreeMap<String, AgentStats>, id: &str) -> &'m mut AgentStats {
        map.entry(id.to_string()).or_insert_with(|| AgentStats {
            agent_id: id.to_string(),
            week_runs: vec![0; 7],
            ..Default::default()
        })
    }

    for card in cards {
        let stats = slot(&mut out, card.agent_id.as_str());
        stats.cards += 1;
        if card.status == Status::Done {
            stats.cards_done += 1;
        }
        stats.spend += card.cost_usd;
        stats.turns += card.turns;
    }

    for stored in history {
        let card_id = stored.event.card_id().to_string();
        let agent = owner.get(card_id.as_str()).copied().unwrap_or("builder");
        let day_ago = today - day_index(stored.ts_ms, tz_offset_minutes);
        match &stored.event {
            Event::RunFinished { .. } => {
                let stats = slot(&mut out, agent);
                stats.runs += 1;
                if (0..7).contains(&day_ago) {
                    let idx = (6 - day_ago) as usize;
                    stats.week_runs[idx] += 1;
                }
            }
            Event::CardApproved { by: Actor::Director, .. } => {
                slot(&mut out, "director").reviews += 1;
            }
            Event::CardRejected { by: Actor::Director, .. } => {
                let stats = slot(&mut out, "director");
                stats.reviews += 1;
                stats.sent_back += 1;
            }
            _ => {}
        }
    }

    for stats in out.values_mut() {
        if stats.runs > 0 {
            stats.avg_cost = stats.spend / stats.runs as f64;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_domain::{Board, CardId, Command, RunId};

    fn stored(seq: u64, ts_ms: u64, event: Event) -> StoredEvent {
        StoredEvent { seq, ts_ms, event }
    }

    /// A board driven through a full card lifecycle, with its log.
    fn fixture(now_ms: u64) -> (Vec<StoredEvent>, Vec<Card>) {
        let mut board = Board::default();
        let mut log = Vec::new();
        let mut seq = 0;
        let push = |board: &mut Board, log: &mut Vec<StoredEvent>, seq: &mut u64, ts: u64, cmd: Command| {
            for event in board.decide(&cmd).expect("legal command") {
                board.apply(&event);
                *seq += 1;
                log.push(stored(*seq, ts, event));
            }
        };

        let id = CardId::new("c1");
        push(&mut board, &mut log, &mut seq, now_ms, Command::CreateCard { card_id: id.clone(), title: "Retry the sidecar".into() });
        push(&mut board, &mut log, &mut seq, now_ms, Command::AssignAgent { card_id: id.clone(), agent_id: "builder".into() });
        push(&mut board, &mut log, &mut seq, now_ms, Command::MoveCard { card_id: id.clone(), to: Status::Ready });
        let run = RunId("run-1".into());
        push(&mut board, &mut log, &mut seq, now_ms, Command::StartRun { card_id: id.clone(), run_id: run.clone(), worktree: None, branch: None });
        push(&mut board, &mut log, &mut seq, now_ms, Command::FinishRun { card_id: id.clone(), run_id: run, outcome: RunOutcome::Completed, cost_usd: Some(0.25), turns: Some(9) });
        push(&mut board, &mut log, &mut seq, now_ms, Command::ApproveCard { card_id: id.clone(), by: Actor::Director, reason: "scoped".into() });

        // A second card, still waiting, run a week ago.
        let old = CardId::new("c2");
        let week_ago = now_ms - 6 * DAY_MS as u64;
        push(&mut board, &mut log, &mut seq, week_ago, Command::CreateCard { card_id: old.clone(), title: "Old work".into() });
        push(&mut board, &mut log, &mut seq, week_ago, Command::AssignAgent { card_id: old.clone(), agent_id: "scout".into() });
        push(&mut board, &mut log, &mut seq, week_ago, Command::MoveCard { card_id: old.clone(), to: Status::Ready });
        let run2 = RunId("run-2".into());
        push(&mut board, &mut log, &mut seq, week_ago, Command::StartRun { card_id: old.clone(), run_id: run2.clone(), worktree: None, branch: None });
        push(&mut board, &mut log, &mut seq, week_ago, Command::FinishRun { card_id: old.clone(), run_id: run2, outcome: RunOutcome::Completed, cost_usd: Some(0.05), turns: Some(2) });

        let cards: Vec<Card> = board.cards().into_iter().cloned().collect();
        (log, cards)
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    #[test]
    fn project_stats_count_today_separately_from_the_week() {
        let now_ms = now();
        let (log, cards) = fixture(now_ms);
        let stats = project_stats(&log, &cards, 0);

        assert_eq!(stats.cards, 2);
        assert_eq!(stats.done, 1);
        assert_eq!(stats.review, 1);
        assert_eq!(stats.runs_total, 2);
        assert_eq!(stats.runs_today, 1);
        assert!((stats.spend_today - 0.25).abs() < 1e-9);
        assert!((stats.spend_total - 0.30).abs() < 1e-9);
        assert_eq!(stats.done_today, 1);
        assert!((stats.cost_per_card - 0.15).abs() < 1e-9);
        assert_eq!(stats.week_runs.len(), 7);
        assert_eq!(stats.week_runs.iter().sum::<u32>(), 2);
        assert_eq!(*stats.week_runs.last().unwrap(), 1, "today is the last bucket");
    }

    #[test]
    fn agent_stats_split_work_by_owner_and_credit_reviews_to_the_director() {
        let now_ms = now();
        let (log, cards) = fixture(now_ms);
        let stats = agent_stats(&log, &cards, 0);

        let builder = stats.get("builder").expect("builder");
        assert_eq!(builder.runs, 1);
        assert_eq!(builder.cards_done, 1);
        assert_eq!(builder.turns, 9);
        assert!((builder.spend - 0.25).abs() < 1e-9);
        assert!((builder.avg_cost - 0.25).abs() < 1e-9);

        let scout = stats.get("scout").expect("scout");
        assert_eq!(scout.runs, 1);
        assert_eq!(scout.cards_done, 0);

        let director = stats.get("director").expect("director");
        assert_eq!(director.reviews, 1);
        assert_eq!(director.sent_back, 0);
    }

    #[test]
    fn activity_reads_newest_first_with_a_kind_for_every_row() {
        let now_ms = now();
        let (log, cards) = fixture(now_ms);
        let rows = activity(&log, &cards, 100);

        assert_eq!(rows.len(), log.len());
        assert!(rows[0].seq > rows[1].seq, "newest first");
        assert!(rows.iter().all(|r| !r.label.is_empty()));
        assert!(rows.iter().all(|r| ["card", "run", "review", "approval"].contains(&r.kind)));

        let approved = rows.iter().find(|r| r.label.contains("Approved")).unwrap();
        assert_eq!(approved.kind, "review");
        assert_eq!(approved.detail, "scoped");

        let moved = rows.iter().find(|r| r.label == "Card moved").unwrap();
        assert_eq!(moved.detail, "Later → Ready");

        // The limit keeps the newest rows.
        let tail = activity(&log, &cards, 2);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[0].seq, log.last().unwrap().seq);
    }

    #[test]
    fn day_buckets_respect_the_operators_timezone() {
        // 00:30 UTC is still the previous day two hours behind.
        let ts = 1_700_000_000_000u64;
        let utc = day_index(ts, 0);
        assert_eq!(day_index(ts, 24 * 60), utc - 1);
        assert_eq!(day_index(ts, -24 * 60), utc + 1);
    }
}
