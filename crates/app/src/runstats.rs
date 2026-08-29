//! Run history, as the Home screen reads it: 38 weeks of finished runs, three
//! rolling windows, and what the day cost split by who spent it.
//!
//! The event log already carries all of it — a run's ending is
//! `Event::RunFinished`, and who ran it is the owning card's `agent_id` — so
//! nothing here touches the filesystem or git and every number is a pure
//! function of the log plus the board. Line counts are the one exception: they
//! come off git and are set by the caller, the same arrangement
//! `insights::AgentStats` uses.

use std::collections::BTreeMap;

use harness_domain::{Card, Event, RunOutcome};
use harness_ports::StoredEvent;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 86_400_000;

/// The profile id the Director runs under. Everything else on the board is an
/// agent, which is the whole of the All / Director / Agents split.
pub const DIRECTOR_ID: &str = "director";

/// Columns in the heatmap. The design draws 38 of them and the frontend labels
/// the same 38 week starts, so the two have to agree.
pub const WEEKS: usize = 38;

/// Local day index for a timestamp, given the UI's offset from UTC. Same
/// convention as `insights` and the same `tzOffsetMinutes` the other commands
/// are already called with.
fn day_index(ts_ms: u64, tz_offset_minutes: i64) -> i64 {
    let shifted = ts_ms as i64 - tz_offset_minutes * 60_000;
    shifted.div_euclid(DAY_MS as i64)
}

/// Which bucket a timestamp falls in for a window `span_ms` long cut into
/// `buckets` slices, newest slice last. `None` when it is outside the window,
/// which includes a timestamp from the future: a clock that ran ahead is not
/// evidence of a run that has not happened.
fn bucket(now_ms: u64, ts_ms: u64, span_ms: u64, buckets: usize) -> Option<usize> {
    if buckets == 0 || ts_ms > now_ms {
        return None;
    }
    let age = now_ms - ts_ms;
    if age >= span_ms {
        return None;
    }
    // How far into the window it landed, 1..=span_ms, so the newest millisecond
    // is in the last bucket and the oldest is in the first.
    let into = span_ms - age;
    Some((((into - 1) * buckets as u64) / span_ms) as usize)
}

/// Whose runs the screen is asking about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ActorFilter {
    #[default]
    All,
    Director,
    Agents,
}

impl ActorFilter {
    /// Does a run owned by `agent` belong in this view?
    ///
    /// An unknown owner counts as an agent. Only the Director's own profile id
    /// makes a run his, and a card that has left the board cannot be claimed
    /// for him after the fact.
    fn keeps(self, agent: Option<&str>) -> bool {
        match self {
            ActorFilter::All => true,
            ActorFilter::Director => agent == Some(DIRECTOR_ID),
            ActorFilter::Agents => agent != Some(DIRECTOR_ID),
        }
    }
}

/// Money, split the way the Usage card asks for it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
pub struct Spend {
    pub total: f64,
    pub director: f64,
    pub agents: f64,
}

/// One of the three tiles under the heatmap.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RunWindow {
    /// `1h`, `24h` or `7d` — which tile this fills.
    pub id: String,
    /// Runs that ended in the window, however they ended.
    pub finished: u32,
    /// Of those, the ones that completed.
    pub succeeded: u32,
    /// Finished runs per slice of the window, oldest first. This is the path
    /// the sparkline draws, so it is always the full set of slices — a quiet
    /// hour is a zero, not a missing point.
    pub series: Vec<u32>,
}

/// What the Home screen's history cards read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
pub struct RunStats {
    /// Runs that ended inside the heatmap window. The card's headline number,
    /// so it counts what the grid below it draws and not all of history.
    pub total: u32,
    /// Of those, the ones that completed.
    pub succeeded: u32,
    /// `WEEKS` columns of seven days, oldest week first, Sunday first inside a
    /// column. The last column is the week today is in.
    pub heatmap: Vec<Vec<u32>>,
    /// How many columns the heatmap has, so the screen does not assume.
    pub weeks: u32,
    pub last_1h: u32,
    pub last_24h: u32,
    pub last_7d: u32,
    /// Finished runs per hour across the last day, oldest first.
    pub series_24h: Vec<u32>,
    /// The three tiles, in the order they are drawn.
    pub windows: Vec<RunWindow>,
    /// Today's spend by actor. Never filtered by `ActorFilter`: the card shows
    /// all three rows whichever tab the heatmap is on.
    pub spend_today: Spend,
    /// The same split across the whole log.
    pub spend_total: Spend,
    /// Lines the day's commits added and removed. Git's answer, not the log's —
    /// filled in by the caller, and left at zero when git could not be read.
    pub lines_added_today: u64,
    pub lines_removed_today: u64,
}

/// Who owns each card, including cards that have since left the board.
///
/// The board is the current truth and wins, but a discarded card is gone from
/// it while its runs are still in the log — and those runs still had an owner.
/// The assignments in the log are what remembers them.
fn owners(history: &[StoredEvent], cards: &[Card]) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for stored in history {
        if let Event::CardAssigned { card_id, agent_id } = &stored.event {
            out.insert(card_id.to_string(), agent_id.clone());
        }
    }
    for card in cards {
        out.insert(card.id.to_string(), card.agent_id.clone());
    }
    out
}

/// Every number the Home history cards show, from the log and the board.
///
/// `now_ms` is passed rather than read so the whole projection is testable;
/// `tz_offset_minutes` is the browser's `getTimezoneOffset`, minutes behind
/// UTC, exactly as `project_stats` and `agents_stats` already receive it.
pub fn run_stats(
    history: &[StoredEvent],
    cards: &[Card],
    now_ms: u64,
    tz_offset_minutes: i64,
    actor: ActorFilter,
) -> RunStats {
    let owner = owners(history, cards);
    let today = day_index(now_ms, tz_offset_minutes);
    // Epoch day 0 was a Thursday, so +4 puts Sunday at 0 — the row order the
    // design draws and the frontend labels M/W/F against.
    let first_day = today - (today + 4).rem_euclid(7) - ((WEEKS as i64 - 1) * 7);

    let mut stats = RunStats {
        heatmap: vec![vec![0u32; 7]; WEEKS],
        weeks: WEEKS as u32,
        series_24h: vec![0u32; 24],
        ..Default::default()
    };
    let mut hour = vec![0u32; 12]; // five minutes a slice
    let mut week = vec![0u32; 7]; // a day a slice
    let mut hour_finished = 0u32;
    let mut hour_ok = 0u32;
    let mut day_finished = 0u32;
    let mut day_ok = 0u32;
    let mut week_finished = 0u32;
    let mut week_ok = 0u32;

    for stored in history {
        let Event::RunFinished {
            card_id,
            outcome,
            cost_usd,
            ..
        } = &stored.event
        else {
            continue;
        };
        let agent = owner.get(card_id.as_str()).map(String::as_str);
        let day = day_index(stored.ts_ms, tz_offset_minutes);

        // Money is the Usage card's business and it wants all three actors, so
        // the heatmap's tab does not narrow it.
        let cost = cost_usd.unwrap_or(0.0);
        let is_director = agent == Some(DIRECTOR_ID);
        add_spend(&mut stats.spend_total, cost, is_director);
        if day == today {
            add_spend(&mut stats.spend_today, cost, is_director);
        }

        if !actor.keeps(agent) {
            continue;
        }
        let ok = matches!(outcome, RunOutcome::Completed);

        let offset = day - first_day;
        if (0..(WEEKS as i64 * 7)).contains(&offset) {
            stats.heatmap[(offset / 7) as usize][(offset % 7) as usize] += 1;
            stats.total += 1;
            if ok {
                stats.succeeded += 1;
            }
        }

        if let Some(i) = bucket(now_ms, stored.ts_ms, HOUR_MS, hour.len()) {
            hour[i] += 1;
            hour_finished += 1;
            hour_ok += u32::from(ok);
        }
        if let Some(i) = bucket(now_ms, stored.ts_ms, DAY_MS, stats.series_24h.len()) {
            stats.series_24h[i] += 1;
            day_finished += 1;
            day_ok += u32::from(ok);
        }
        if let Some(i) = bucket(now_ms, stored.ts_ms, 7 * DAY_MS, week.len()) {
            week[i] += 1;
            week_finished += 1;
            week_ok += u32::from(ok);
        }
    }

    stats.last_1h = hour_finished;
    stats.last_24h = day_finished;
    stats.last_7d = week_finished;
    stats.windows = vec![
        RunWindow {
            id: "1h".to_string(),
            finished: hour_finished,
            succeeded: hour_ok,
            series: hour,
        },
        RunWindow {
            id: "24h".to_string(),
            finished: day_finished,
            succeeded: day_ok,
            series: stats.series_24h.clone(),
        },
        RunWindow {
            id: "7d".to_string(),
            finished: week_finished,
            succeeded: week_ok,
            series: week,
        },
    ];
    stats
}

fn add_spend(into: &mut Spend, cost: f64, is_director: bool) {
    into.total += cost;
    if is_director {
        into.director += cost;
    } else {
        into.agents += cost;
    }
}

/// Local midnight, in milliseconds, of the day `now_ms` falls in.
///
/// What "today" means to the operator is what the day buckets above mean by it,
/// and the git side has to ask the same question — otherwise the line counts on
/// the card are for a different day than the runs beside them.
pub fn local_midnight_ms(now_ms: u64, tz_offset_minutes: i64) -> u64 {
    let start = day_index(now_ms, tz_offset_minutes) * DAY_MS as i64 + tz_offset_minutes * 60_000;
    start.max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_domain::{CardId, Review, RunId, Status};

    /// A fixed instant with room behind it: 38 weeks of heatmap is 266 days,
    /// and the tests reach back past that on purpose.
    const NOW: u64 = 1_800_000_000_000;

    fn stored(seq: u64, ts_ms: u64, event: Event) -> StoredEvent {
        StoredEvent { seq, ts_ms, event }
    }

    fn finished(seq: u64, ts_ms: u64, card: &str, outcome: RunOutcome, cost: f64) -> StoredEvent {
        stored(
            seq,
            ts_ms,
            Event::RunFinished {
                card_id: CardId::new(card),
                run_id: RunId(format!("run-{seq}")),
                outcome,
                cost_usd: Some(cost),
                turns: Some(3),
            },
        )
    }

    fn done(seq: u64, ts_ms: u64, card: &str) -> StoredEvent {
        finished(seq, ts_ms, card, RunOutcome::Completed, 0.0)
    }

    fn card(id: &str, agent: &str) -> Card {
        Card {
            id: CardId::new(id),
            title: format!("card {id}"),
            status: Status::Done,
            current_run: None,
            agent_id: agent.to_string(),
            cost_usd: 0.0,
            turns: 0,
            runs: 1,
            last_review: None::<Review>,
            hunk_verdicts: Vec::new(),
            session_id: None,
            worktree: None,
            branch: None,
            depends_on: Vec::new(),
            budget_paused: false,
            finished_ms: None,
        }
    }

    #[test]
    fn an_empty_log_still_ships_the_whole_grid() {
        let stats = run_stats(&[], &[], NOW, 0, ActorFilter::All);
        assert_eq!(stats.heatmap.len(), WEEKS);
        assert!(stats.heatmap.iter().all(|w| w.len() == 7));
        assert_eq!(stats.weeks, WEEKS as u32);
        assert_eq!(stats.total, 0);
        assert_eq!(stats.series_24h.len(), 24);
        // Three tiles with a path to draw, all of it flat.
        assert_eq!(stats.windows.len(), 3);
        assert!(stats.windows.iter().all(|w| w.series.len() >= 7));
        assert_eq!(stats.spend_total, Spend::default());
    }

    #[test]
    fn today_lands_in_the_last_column_at_its_own_weekday() {
        let cards = vec![card("c1", "builder")];
        let log = vec![done(1, NOW - HOUR_MS, "c1")];
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);

        let last = stats.heatmap.last().unwrap();
        assert_eq!(last.iter().sum::<u32>(), 1);
        let weekday = (day_index(NOW, 0) + 4).rem_euclid(7) as usize;
        assert_eq!(last[weekday], 1, "the cell is today's, not just this week's");
        assert_eq!(stats.total, 1);
        assert_eq!(stats.succeeded, 1);
    }

    #[test]
    fn runs_older_than_the_window_are_out_of_the_grid_and_the_headline() {
        let cards = vec![card("c1", "builder")];
        let log = vec![
            finished(1, NOW - 2 * DAY_MS, "c1", RunOutcome::Completed, 0.50),
            // One day before the first column starts.
            finished(
                2,
                NOW - (WEEKS as u64 * 7 + 1) * DAY_MS,
                "c1",
                RunOutcome::Completed,
                0.50,
            ),
        ];
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        assert_eq!(stats.total, 1);
        assert_eq!(
            stats.heatmap.iter().flatten().sum::<u32>(),
            1,
            "the headline is the grid's own sum"
        );
        // Out of the heatmap is not out of the money: the total is all of it.
        assert!((stats.spend_total.total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_actor_tabs_split_the_director_from_everybody_else() {
        let cards = vec![card("c1", "builder"), card("c2", DIRECTOR_ID)];
        let log = vec![
            done(1, NOW - HOUR_MS, "c1"),
            done(2, NOW - 2 * HOUR_MS, "c2"),
            done(3, NOW - 3 * HOUR_MS, "c2"),
        ];

        let all = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        let director = run_stats(&log, &cards, NOW, 0, ActorFilter::Director);
        let agents = run_stats(&log, &cards, NOW, 0, ActorFilter::Agents);

        assert_eq!(all.total, 3);
        assert_eq!(director.total, 2);
        assert_eq!(agents.total, 1);
        assert_eq!(director.last_24h, 2);
        assert_eq!(agents.last_24h, 1);
        assert_eq!(director.total + agents.total, all.total, "nothing is lost");
    }

    #[test]
    fn a_card_that_left_the_board_keeps_the_owner_the_log_gave_it() {
        // No cards at all: the board has forgotten c9, the log has not.
        let log = vec![
            stored(
                1,
                NOW - 4 * HOUR_MS,
                Event::CardAssigned {
                    card_id: CardId::new("c9"),
                    agent_id: DIRECTOR_ID.to_string(),
                },
            ),
            done(2, NOW - 3 * HOUR_MS, "c9"),
        ];
        let director = run_stats(&log, &[], NOW, 0, ActorFilter::Director);
        assert_eq!(director.total, 1);
        let agents = run_stats(&log, &[], NOW, 0, ActorFilter::Agents);
        assert_eq!(agents.total, 0);
    }

    #[test]
    fn an_unattributable_run_counts_as_an_agents_run() {
        let log = vec![done(1, NOW - HOUR_MS, "ghost")];
        assert_eq!(run_stats(&log, &[], NOW, 0, ActorFilter::All).total, 1);
        assert_eq!(run_stats(&log, &[], NOW, 0, ActorFilter::Agents).total, 1);
        assert_eq!(run_stats(&log, &[], NOW, 0, ActorFilter::Director).total, 0);
    }

    #[test]
    fn the_three_windows_count_their_own_span_and_nothing_else() {
        let cards = vec![card("c1", "builder")];
        let log = vec![
            done(1, NOW - 10 * 60_000, "c1"),     // 10 min
            done(2, NOW - 5 * HOUR_MS, "c1"),     // 5 h
            done(3, NOW - 3 * DAY_MS, "c1"),      // 3 days
            done(4, NOW - 30 * DAY_MS, "c1"),     // past every window
        ];
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        assert_eq!(stats.last_1h, 1);
        assert_eq!(stats.last_24h, 2);
        assert_eq!(stats.last_7d, 3);
        assert_eq!(stats.total, 4, "the heatmap is wider than any of them");

        let by_id = |id: &str| stats.windows.iter().find(|w| w.id == id).unwrap().clone();
        assert_eq!(by_id("1h").series.iter().sum::<u32>(), 1);
        assert_eq!(by_id("24h").series, stats.series_24h);
        assert_eq!(by_id("7d").series.iter().sum::<u32>(), 3);
    }

    #[test]
    fn a_series_puts_the_newest_run_in_the_last_slice() {
        let cards = vec![card("c1", "builder")];
        // One a minute ago and one 23 hours ago: the two ends of the day.
        let log = vec![done(1, NOW - 60_000, "c1"), done(2, NOW - 23 * HOUR_MS, "c1")];
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        assert_eq!(*stats.series_24h.last().unwrap(), 1);
        assert_eq!(stats.series_24h[0], 1);
        assert_eq!(stats.series_24h.iter().sum::<u32>(), 2);
    }

    #[test]
    fn how_a_run_ended_separates_finished_from_succeeded() {
        let cards = vec![card("c1", "builder")];
        let log = vec![
            finished(1, NOW - HOUR_MS, "c1", RunOutcome::Completed, 0.0),
            finished(2, NOW - 2 * HOUR_MS, "c1", RunOutcome::Failed, 0.0),
            finished(3, NOW - 3 * HOUR_MS, "c1", RunOutcome::Cancelled, 0.0),
        ];
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        assert_eq!(stats.total, 3, "a run that failed still ran");
        assert_eq!(stats.succeeded, 1);
        let day = stats.windows.iter().find(|w| w.id == "24h").unwrap();
        assert_eq!(day.finished, 3);
        assert_eq!(day.succeeded, 1);
    }

    #[test]
    fn spend_splits_by_actor_and_ignores_the_heatmap_tab() {
        let cards = vec![card("c1", "builder"), card("c2", DIRECTOR_ID)];
        let log = vec![
            finished(1, NOW - HOUR_MS, "c1", RunOutcome::Completed, 0.25),
            finished(2, NOW - 2 * HOUR_MS, "c2", RunOutcome::Completed, 0.10),
            // Yesterday: in the total, out of today.
            finished(3, NOW - 30 * HOUR_MS, "c1", RunOutcome::Completed, 1.00),
        ];

        for actor in [ActorFilter::All, ActorFilter::Director, ActorFilter::Agents] {
            let stats = run_stats(&log, &cards, NOW, 0, actor);
            assert!((stats.spend_today.total - 0.35).abs() < 1e-9);
            assert!((stats.spend_today.director - 0.10).abs() < 1e-9);
            assert!((stats.spend_today.agents - 0.25).abs() < 1e-9);
            assert!((stats.spend_total.total - 1.35).abs() < 1e-9);
            assert!((stats.spend_total.agents - 1.25).abs() < 1e-9);
        }
    }

    #[test]
    fn a_run_with_no_recorded_cost_is_free_rather_than_missing() {
        let cards = vec![card("c1", "builder")];
        let log = vec![stored(
            1,
            NOW - HOUR_MS,
            Event::RunFinished {
                card_id: CardId::new("c1"),
                run_id: RunId("r".into()),
                outcome: RunOutcome::Completed,
                cost_usd: None,
                turns: None,
            },
        )];
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        assert_eq!(stats.total, 1);
        assert_eq!(stats.spend_today.total, 0.0);
    }

    #[test]
    fn day_columns_follow_the_operators_clock() {
        let cards = vec![card("c1", "builder")];
        // 30 minutes past midnight UTC is still yesterday two hours behind.
        let midnight = (day_index(NOW, 0) * DAY_MS as i64) as u64;
        let log = vec![done(1, midnight + 30 * 60_000, "c1")];

        let utc = run_stats(&log, &cards, midnight + HOUR_MS, 0, ActorFilter::All);
        let behind = run_stats(&log, &cards, midnight + HOUR_MS, 120, ActorFilter::All);

        let cell = |s: &RunStats| -> (usize, usize) {
            s.heatmap
                .iter()
                .enumerate()
                .find_map(|(w, week)| week.iter().position(|v| *v > 0).map(|d| (w, d)))
                .expect("one run somewhere")
        };
        assert_ne!(cell(&utc), cell(&behind), "the same run, a different day");
        assert_eq!(utc.total, 1);
        assert_eq!(behind.total, 1);
    }

    #[test]
    fn only_finished_runs_are_history() {
        let cards = vec![card("c1", "builder")];
        let log = vec![
            stored(
                1,
                NOW - HOUR_MS,
                Event::RunStarted {
                    card_id: CardId::new("c1"),
                    run_id: RunId("r".into()),
                    worktree: None,
                    branch: None,
                },
            ),
            stored(
                2,
                NOW - HOUR_MS,
                Event::CardCreated {
                    card_id: CardId::new("c1"),
                    title: "x".into(),
                },
            ),
        ];
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        assert_eq!(stats.total, 0, "a run in flight has not happened yet");
        assert_eq!(stats.last_1h, 0);
    }

    #[test]
    fn a_clock_that_ran_ahead_does_not_invent_a_run() {
        let cards = vec![card("c1", "builder")];
        let log = vec![done(1, NOW + HOUR_MS, "c1")];
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        assert_eq!(stats.last_1h, 0);
        assert_eq!(stats.last_24h, 0);
    }

    #[test]
    fn midnight_is_the_same_midnight_the_day_buckets_use() {
        for tz in [0i64, 120, -60, 780] {
            let start = local_midnight_ms(NOW, tz);
            assert_eq!(day_index(start, tz), day_index(NOW, tz));
            assert_eq!(day_index(start - 1, tz), day_index(NOW, tz) - 1);
        }
    }

    #[test]
    fn the_whole_log_is_read_fast_enough_to_open_a_screen() {
        // 40k finished runs is years of a busy board. If this ever stops being
        // instant, `backend-plan.md` has the run-index.jsonl to fall back on.
        let cards = vec![card("c1", "builder")];
        let log: Vec<StoredEvent> = (0..40_000u64)
            .map(|i| done(i, NOW - (i % 200_000) * 60_000, "c1"))
            .collect();
        let at = std::time::Instant::now();
        let stats = run_stats(&log, &cards, NOW, 0, ActorFilter::All);
        let took = at.elapsed();
        assert!(stats.total > 0);
        assert!(took.as_millis() < 250, "run_stats took {took:?}");
    }
}
