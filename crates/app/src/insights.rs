//! Numbers the UI shows, derived from the event log rather than stored twice.
//! Everything here is a pure function of events plus the current board, so it
//! can be tested without an engine.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use harness_domain::{Actor, Card, Event, RunOutcome, Status};
use harness_ports::StoredEvent;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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

#[derive(Debug, Clone, Serialize, TS)]
pub struct ActivityRow {
    #[ts(type = "number")]
    pub seq: u64,
    #[ts(type = "number")]
    pub ts_ms: u64,
    /// `card`, `run`, `approval` or `review` — the Activity filters.
    #[ts(type = "string")]
    pub kind: &'static str,
    /// Human label, e.g. "Run finished".
    pub label: String,
    /// Foi esta linha uma aprovação? O rótulo diz-no em português corrente, e
    /// o ecrã chegou a lê-lo por prefixo — o que faz uma contagem cair para
    /// zero em silêncio no dia em que alguém reescrever a frase. Quem sabe o
    /// que a linha é, diz.
    pub approved: bool,
    pub card_id: String,
    pub detail: String,
    /// The run this row is about, when it is one. Absent on card rows, and on
    /// the older logs written before it was carried here. Without it a screen
    /// can only ask `run_log` for the newest run of a card, because that is the
    /// one id the snapshot happens to hold.
    #[serde(default)]
    pub run_id: Option<String>,
    /// How the run ended, as the log recorded it. `None` on a start — it has
    /// not ended — so the reader pairs a start with its ending by run id
    /// instead of reading the human label, which is prose and will be reworded.
    #[serde(default)]
    pub outcome: Option<RunOutcome>,
    /// The tools this run called, in the order it first called them. Empty
    /// when the row is not a run, or when its transcript is no longer on disk:
    /// both are "nothing to say", and the screen says so with an em-dash.
    #[serde(default)]
    pub tools: Vec<ToolCount>,
}

/// One tool a run called, and how many times.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct ToolCount {
    pub tool: String,
    pub count: u32,
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
        .filter_map(|stored| {
            let card_id = stored.event.card_id()?.to_string();
            let title = titles.get(card_id.as_str()).copied().unwrap_or("");
            let (kind, label, detail) = match &stored.event {
                Event::CardCreated { title, .. } => ("card", "Card created", title.clone()),
                // The title is the prompt: a correction to it is a change to
                // the work, and belongs on the feed like any other.
                Event::CardEdited { title, .. } => ("card", "Card reworded", title.clone()),
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
                // One block of a diff, decided on its own. The card's own
                // outcome arrives as its own row when the diff is finished,
                // so this line is about the block and names it.
                Event::HunkReviewed {
                    hunk,
                    approved,
                    reason,
                    ..
                } => (
                    "review",
                    if *approved { "Hunk approved" } else { "Hunk sent back" },
                    if reason.trim().is_empty() {
                        hunk.label()
                    } else {
                        format!("{} — {}", hunk.label(), reason.trim())
                    },
                ),
                Event::CardDependencies { depends_on, .. } => (
                    "card",
                    "Order set",
                    if depends_on.is_empty() {
                        "no dependencies".to_string()
                    } else {
                        depends_on
                            .iter()
                            .map(|d| d.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                ),
                // A snapshot is the board itself, not something that happened;
                // compaction keeps the feed about work, not housekeeping.
                Event::BoardSnapshot { .. } => return None,
                // The pause flag is board state, and the operator must see
                // why a Start refused.
                Event::BudgetPauseSet { paused, .. } => (
                    "card",
                    if *paused {
                        "Paused by budget"
                    } else {
                        "Budget pause lifted"
                    },
                    String::new(),
                ),
                // The agent's private account of its work: it feeds the commit
                // and the memory layer. On the activity feed it is noise.
                Event::WorkReported { summary, notes, .. } => {
                    let count = notes.len();
                    let unit = if count == 1 { "note" } else { "notes" };
                    (
                        "run",
                        "Work reported",
                        match (summary.is_empty(), count) {
                            (true, 0) => "no details given".to_string(),
                            (true, _) => format!("{count} memory {unit}"),
                            (_, 0) => "summary only".to_string(),
                            _ => format!("summary + {count} memory {unit}"),
                        },
                    )
                }
            };
            // The events already carry the run; the row used to drop it, and a
            // dropped id is a transcript nothing can ask for again.
            let (run_id, outcome) = match &stored.event {
                Event::RunStarted { run_id, .. } => (Some(run_id.0.clone()), None),
                Event::RunFinished {
                    run_id, outcome, ..
                } => (Some(run_id.0.clone()), Some(outcome.clone())),
                _ => (None, None),
            };
            Some(ActivityRow {
                seq: stored.seq,
                ts_ms: stored.ts_ms,
                kind,
                approved: matches!(stored.event, Event::CardApproved { .. }),
                label: label.to_string(),
                card_id,
                detail,
                run_id,
                outcome,
                tools: Vec::new(),
            })
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

// ---- what a run touched ----------------------------------------------------

/// The tool's own word, as the transcript prints it.
///
/// The harness hands the agent its board tools through an MCP server, which
/// prefixes their names; `src/state/events.ts` strips the same prefix when it
/// draws a transcript line. The Sessions column sits beside that transcript, so
/// it has to arrive at the same word or the two disagree about one call.
fn tool_label(raw: &str) -> String {
    let stripped = raw
        .strip_prefix("mcp__harness__")
        .or_else(|| raw.strip_prefix("harness"))
        .unwrap_or(raw);
    let stripped = stripped.strip_prefix("__").unwrap_or(stripped);
    if stripped.is_empty() {
        raw.to_string()
    } else {
        stripped.to_string()
    }
}

/// Tool calls in one JSONL transcript, first call first.
///
/// A `tool_result` body can be megabytes of captured output, so a line is only
/// handed to the parser once its text says it is a call. The column wants the
/// name and the tally; nothing here reads an argument or a result.
pub fn tool_counts(transcript: &str) -> Vec<ToolCount> {
    #[derive(Deserialize)]
    struct Call {
        tool: Option<String>,
    }

    let mut order: Vec<String> = Vec::new();
    let mut tally: HashMap<String, u32> = HashMap::new();
    for line in transcript.lines() {
        if !line.contains("\"kind\":\"tool_use\"") {
            continue;
        }
        let Ok(call) = serde_json::from_str::<Call>(line) else {
            continue;
        };
        // A line that says it is a call and names no tool: a malformed write,
        // or a log from before the field existed. It is not a call we can name.
        let Some(raw) = call.tool else {
            continue;
        };
        let name = tool_label(&raw);
        match tally.get_mut(&name) {
            Some(n) => *n += 1,
            None => {
                order.push(name.clone());
                tally.insert(name, 1);
            }
        }
    }
    order
        .into_iter()
        .map(|tool| {
            let count = tally.get(&tool).copied().unwrap_or(0);
            ToolCount { tool, count }
        })
        .collect()
}

/// Tool tallies per transcript, remembered against the file's length.
///
/// A finished run's transcript never changes again, so it is read once and the
/// screen's next refresh costs a `stat`; a live one only ever grows, and the
/// length is what tells us it did. Keyed by path rather than run id, because a
/// run id is only unique inside its own project.
#[derive(Default)]
pub struct ToolCache {
    inner: Mutex<HashMap<PathBuf, (u64, Vec<ToolCount>)>>,
}

impl ToolCache {
    pub fn counts(&self, file: &Path) -> Vec<ToolCount> {
        let Ok(len) = std::fs::metadata(file).map(|m| m.len()) else {
            return Vec::new();
        };
        if let Ok(map) = self.inner.lock() {
            if let Some((at, counts)) = map.get(file) {
                if *at == len {
                    return counts.clone();
                }
            }
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            return Vec::new();
        };
        let counts = tool_counts(&text);
        if let Ok(mut map) = self.inner.lock() {
            map.insert(file.to_path_buf(), (len, counts.clone()));
        }
        counts
    }

    /// Drop a transcript, for when it is deleted from disk.
    pub fn forget(&self, file: &Path) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(file);
        }
    }
}

pub static TOOL_CACHE: LazyLock<ToolCache> = LazyLock::new(ToolCache::default);

/// Fill in what each run touched, so one `activity` response carries the whole
/// table. The alternative is the screen asking for every row's transcript by
/// hand — two hundred round trips, each one shipping back a full log to count
/// six lines of it.
///
/// `transcript` maps a run id to its file: naming that file is the run log
/// adapter's rule, and this module does not get to guess at it.
pub fn fill_tool_counts(rows: &mut [ActivityRow], transcript: impl Fn(&str) -> PathBuf) {
    for row in rows.iter_mut() {
        let Some(run_id) = row.run_id.as_deref() else {
            continue;
        };
        row.tools = TOOL_CACHE.counts(&transcript(run_id));
    }
}

// ---- taking the transcripts off the machine --------------------------------

/// What an export wrote, so the screen states the outcome rather than assuming
/// one.
#[derive(Debug, Clone, Serialize, TS)]
pub struct TranscriptExport {
    /// The folder that was created, in full.
    pub dir: String,
    pub files: usize,
    #[ts(type = "number")]
    pub bytes: u64,
    /// Runs whose transcript was not on disk. Named, not silently dropped: a
    /// short export the operator cannot account for is worse than a warning.
    pub missing: Vec<String>,
}

/// The folder an export creates inside the one the operator picked.
pub fn export_folder_name(project_id: &str) -> String {
    format!("{}-transcripts", crate::paths::sanitize(project_id))
}

/// Copy run transcripts into `dest_root/<name>`, or the next free name beside
/// it. Never writes over an existing folder: an export is a copy the operator
/// is about to hand to someone, and silently merging two of them loses the
/// boundary between the runs.
pub fn export_transcripts(
    dest_root: &Path,
    name: &str,
    runs: &[(String, PathBuf)],
) -> Result<TranscriptExport, String> {
    let dir = free_dir(dest_root, name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {dir:?}: {e}"))?;

    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut missing = Vec::new();
    for (run_id, src) in runs {
        // The source file's own name is already the safe one the run log
        // minted; rebuilding it from the run id here would be a second rule.
        let Some(file_name) = src.file_name().filter(|_| src.is_file()) else {
            missing.push(run_id.clone());
            continue;
        };
        match std::fs::copy(src, dir.join(file_name)) {
            Ok(n) => {
                files += 1;
                bytes += n;
            }
            Err(_) => missing.push(run_id.clone()),
        }
    }

    Ok(TranscriptExport {
        dir: dir.to_string_lossy().to_string(),
        files,
        bytes,
        missing,
    })
}

fn free_dir(root: &Path, name: &str) -> PathBuf {
    let first = root.join(name);
    if !first.exists() {
        return first;
    }
    for n in 2..1000 {
        let next = root.join(format!("{name}-{n}"));
        if !next.exists() {
            return next;
        }
    }
    first
}

#[derive(Debug, Clone, Default, Serialize, TS)]
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
    #[ts(type = "number")]
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

#[derive(Debug, Clone, Default, Serialize, TS)]
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
    #[ts(type = "number")]
    pub lines_added: u64,
    #[ts(type = "number")]
    pub lines_removed: u64,
    #[ts(type = "number")]
    pub commits: u64,
}

/// Somar um projecto ao total de toda a gente.
///
/// O ecrã de Agentes lê a máquina inteira, não um quadro: o mesmo agente
/// aparece em vários projectos e os números dele são a soma. Esta dobra estava
/// escrita dentro do `#[tauri::command]`, com um `week_runs` de sete zeros
/// criado à mão em dois sítios — e um deles a somar índice a índice sem nada a
/// garantir que os dois vectores tinham o mesmo comprimento.
pub fn merge_agent_stats(into: &mut BTreeMap<String, AgentStats>, from: BTreeMap<String, AgentStats>) {
    for (id, stats) in from {
        let slot = into.entry(id.clone()).or_insert_with(|| blank(&id));
        slot.runs += stats.runs;
        slot.cards += stats.cards;
        slot.cards_done += stats.cards_done;
        slot.spend += stats.spend;
        slot.turns += stats.turns;
        slot.reviews += stats.reviews;
        slot.sent_back += stats.sent_back;
        for (i, v) in stats.week_runs.iter().enumerate() {
            if let Some(cell) = slot.week_runs.get_mut(i) {
                *cell += v;
            }
        }
    }
}

/// As linhas escritas, que não vêm do log de eventos mas das trailers dos
/// commits. Um commit sem `Harness-Agent` não é de agente nenhum e não conta.
pub fn merge_commit_lines(
    into: &mut BTreeMap<String, AgentStats>,
    commits: impl IntoIterator<Item = (Option<String>, u64, u64)>,
) {
    for (agent, added, removed) in commits {
        let Some(agent) = agent else { continue };
        let slot = into.entry(agent.clone()).or_insert_with(|| blank(&agent));
        slot.lines_added += added;
        slot.lines_removed += removed;
        slot.commits += 1;
    }
}

/// A média só existe onde houve runs: dividir por zero dava um `NaN` que o
/// ecrã desenharia como um número.
pub fn settle_averages(stats: &mut BTreeMap<String, AgentStats>) {
    for s in stats.values_mut() {
        if s.runs > 0 {
            s.avg_cost = s.spend / s.runs as f64;
        }
    }
}

fn blank(agent_id: &str) -> AgentStats {
    AgentStats {
        agent_id: agent_id.to_string(),
        week_runs: vec![0; 7],
        ..Default::default()
    }
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
        let Some(id) = stored.event.card_id() else {
            continue;
        };
        let card_id = id.to_string();
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
    /// Dois projectos, um agente: o ecrã mostra a soma. E a média assenta uma
    /// vez no fim, sobre o total — não uma vez por projecto, que daria a média
    /// das médias.
    #[test]
    fn one_agent_across_two_projects_is_one_row() {
        use super::{merge_agent_stats, merge_commit_lines, settle_averages, AgentStats};
        let one = |runs, spend| {
            let mut m = std::collections::BTreeMap::new();
            m.insert(
                "builder".to_string(),
                AgentStats {
                    agent_id: "builder".into(),
                    runs,
                    spend,
                    week_runs: vec![1, 0, 0, 0, 0, 0, 0],
                    ..Default::default()
                },
            );
            m
        };
        let mut all = std::collections::BTreeMap::new();
        merge_agent_stats(&mut all, one(2, 1.0));
        merge_agent_stats(&mut all, one(2, 3.0));
        merge_commit_lines(
            &mut all,
            [
                (Some("builder".to_string()), 10, 2),
                // Um commit do operador, sem trailer: não é de agente nenhum.
                (None, 999, 999),
            ],
        );
        settle_averages(&mut all);

        assert_eq!(all.len(), 1, "um agente, uma linha");
        let b = &all["builder"];
        assert_eq!(b.runs, 4);
        assert_eq!(b.spend, 4.0);
        assert_eq!(b.week_runs[0], 2, "os dias somam-se dia a dia");
        assert_eq!(b.lines_added, 10);
        assert_eq!(b.commits, 1);
        assert_eq!(b.avg_cost, 1.0, "a média é do total, não a média das médias");
    }

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
        push(&mut board, &mut log, &mut seq, now_ms, Command::ApproveCard { card_id: id.clone(), by: Actor::Director, reason: "scoped".into(), hunks: Vec::new() });

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
    fn run_rows_carry_the_run_and_how_it_ended() {
        let now_ms = now();
        let (log, cards) = fixture(now_ms);
        let rows = activity(&log, &cards, 100);

        let starts: Vec<&ActivityRow> = rows
            .iter()
            .filter(|r| r.run_id.is_some() && r.outcome.is_none())
            .collect();
        assert_eq!(starts.len(), 2, "both runs started");
        assert!(starts.iter().any(|r| r.run_id.as_deref() == Some("run-1")));
        assert!(starts.iter().any(|r| r.run_id.as_deref() == Some("run-2")));

        // A start and its ending are found by id, never by reading the label.
        let ended = rows
            .iter()
            .find(|r| r.run_id.as_deref() == Some("run-1") && r.outcome.is_some())
            .expect("run-1 ended");
        assert_eq!(ended.outcome, Some(RunOutcome::Completed));

        // A card event is not a run, and says so rather than guessing.
        let created = rows.iter().find(|r| r.label == "Card created").unwrap();
        assert!(created.run_id.is_none());
        assert!(created.outcome.is_none());
        assert!(created.tools.is_empty());
    }

    #[test]
    fn tool_counts_tally_calls_and_read_nothing_else() {
        let transcript = [
            r#"{"ts_ms":1,"kind":"started","session_id":"s"}"#,
            r#"{"ts_ms":2,"kind":"tool_use","tool":"Read","summary":"a.rs"}"#,
            r#"{"ts_ms":3,"kind":"tool_use","tool":"Edit","summary":"b.rs"}"#,
            r#"{"ts_ms":4,"kind":"tool_use","tool":"Edit","summary":"c.rs"}"#,
            r#"{"ts_ms":5,"kind":"tool_use","tool":"mcp__harness__read_diff","summary":""}"#,
            r#"{"ts_ms":6,"kind":"tool_use"}"#,
            r#"{"ts_ms":7,"kind":"tool_result","tool_use_id":"t","ok":true,"summary":"ok"}"#,
            "",
            "half a line, never finished",
        ]
        .join("\n");

        assert_eq!(
            tool_counts(&transcript),
            vec![
                ToolCount { tool: "Read".into(), count: 1 },
                ToolCount { tool: "Edit".into(), count: 2 },
                ToolCount { tool: "read_diff".into(), count: 1 },
            ],
            "first call first, and the harness prefix off"
        );
        assert!(tool_counts("").is_empty());
    }

    /// A directory of this test's own, cleaned before use.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("harness-insights-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_run_row_gets_its_tools_and_a_missing_transcript_is_empty() {
        let dir = scratch("tools");
        std::fs::write(
            dir.join("run-1.jsonl"),
            format!(
                "{}\n{}\n",
                r#"{"ts_ms":1,"kind":"tool_use","tool":"Read","summary":"a"}"#,
                r#"{"ts_ms":2,"kind":"tool_use","tool":"Read","summary":"b"}"#
            ),
        )
        .unwrap();

        let now_ms = now();
        let (log, cards) = fixture(now_ms);
        let mut rows = activity(&log, &cards, 100);
        let at = dir.clone();
        fill_tool_counts(&mut rows, |id| at.join(format!("{id}.jsonl")));

        // Both the start and the ending of run-1 describe the same transcript.
        for row in rows.iter().filter(|r| r.run_id.as_deref() == Some("run-1")) {
            assert_eq!(row.tools, vec![ToolCount { tool: "Read".into(), count: 2 }]);
        }
        // run-2 never wrote one; that is an empty cell, not an invented one.
        assert!(rows
            .iter()
            .filter(|r| r.run_id.as_deref() == Some("run-2"))
            .all(|r| r.tools.is_empty()));

        // The cache follows the file's length, so an appended call is seen.
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(dir.join("run-1.jsonl")).unwrap();
        writeln!(f, r#"{{"ts_ms":3,"kind":"tool_use","tool":"Bash","summary":"cargo"}}"#).unwrap();
        drop(f);
        let mut again = activity(&log, &cards, 100);
        let at = dir.clone();
        fill_tool_counts(&mut again, |id| at.join(format!("{id}.jsonl")));
        let grown = again.iter().find(|r| r.run_id.as_deref() == Some("run-1")).unwrap();
        assert_eq!(grown.tools.len(), 2, "the new call is counted");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_export_copies_the_transcripts_and_never_writes_over_one() {
        let root = scratch("export");
        let runs = root.join("runs");
        std::fs::create_dir_all(&runs).unwrap();
        std::fs::write(runs.join("run-1.jsonl"), "{\"ts_ms\":1,\"kind\":\"started\"}\n").unwrap();
        let dest = root.join("dest");
        std::fs::create_dir_all(&dest).unwrap();

        let sources = vec![
            ("run-1".to_string(), runs.join("run-1.jsonl")),
            ("run-gone".to_string(), runs.join("run-gone.jsonl")),
        ];
        let name = export_folder_name("Some Project");
        assert_eq!(name, "some-project-transcripts");

        let first = export_transcripts(&dest, &name, &sources).unwrap();
        assert_eq!(first.files, 1);
        assert_eq!(first.missing, vec!["run-gone".to_string()]);
        assert!(first.bytes > 0);
        assert!(std::path::Path::new(&first.dir).join("run-1.jsonl").is_file());

        // A second export beside the first, not on top of it.
        let second = export_transcripts(&dest, &name, &sources).unwrap();
        assert_ne!(second.dir, first.dir);
        assert!(second.dir.ends_with("-2"));

        let _ = std::fs::remove_dir_all(&root);
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

/// One card waiting in Review, scored by how much attention it deserves.
/// The Triador orders the queue with this; it does not judge the work, only
/// the surface and the wait.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ReviewCandidate {
    pub card_id: String,
    pub title: String,
    /// Higher wants eyes first. Mechanical, explainable, no model involved.
    #[ts(type = "number")]
    pub risk: u64,
    /// Why the score says so, in words a person can check.
    pub reasons: Vec<String>,
}

/// What the worktree changed, gathered by the shell from git.
pub struct DiffSurface {
    pub files: u64,
    pub added: u64,
    pub removed: u64,
}

/// Order the Review queue. Surface dominates (a wide diff is where surprises
/// live), then how long the card has been waiting. Protected-file weighting
/// belongs to a later pass, when the operator can name their protected paths.
pub fn triage(
    cards: &[Card],
    waiting_since_ms: &std::collections::HashMap<String, u64>,
    surfaces: &std::collections::HashMap<String, DiffSurface>,
    now_ms: u64,
) -> Vec<ReviewCandidate> {
    let mut out: Vec<ReviewCandidate> = cards
        .iter()
        .filter(|c| c.status == Status::Review)
        .map(|card| {
            let mut reasons = Vec::new();
            let mut risk: u64 = 0;
            if let Some(s) = surfaces.get(card.id.as_str()) {
                if s.files > 0 {
                    reasons.push(format!("{} {}", s.files, if s.files == 1 { "file" } else { "files" }));
                    risk += s.files * 4;
                }
                if s.added + s.removed > 0 {
                    reasons.push(format!("+{} −{}", s.added, s.removed));
                    risk += (s.added + s.removed) / 25;
                }
            }
            if let Some(&since) = waiting_since_ms.get(card.id.as_str()) {
                let hours = now_ms.saturating_sub(since) / 3_600_000;
                if hours >= 24 {
                    reasons.push(format!("waiting {} days", hours / 24));
                    risk += (hours / 24) * 6;
                } else if hours >= 2 {
                    reasons.push(format!("waiting {} h", hours));
                    risk += hours * 2;
                }
            }
            ReviewCandidate {
                card_id: card.id.to_string(),
                title: card.title.clone(),
                risk,
                reasons,
            }
        })
        .collect();
    out.sort_by(|a, b| b.risk.cmp(&a.risk).then(a.card_id.cmp(&b.card_id)));
    out
}

#[cfg(test)]
mod triage_tests {
    use super::*;
    use harness_domain::{Actor, CardId, Review};

    fn card(id: &str, status: Status) -> Card {
        Card {
            id: CardId::new(id),
            title: format!("card {id}"),
            status,
            current_run: None,
            agent_id: "builder".into(),
            cost_usd: 0.0,
            turns: 0,
            runs: 1,
            last_review: Some(Review { by: Actor::Director, approved: true, reason: String::new(), hunks: Vec::new() }),
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
    fn wide_diffs_and_long_waits_float_to_the_top() {
        let now = 1_000_000_000_000u64;
        let cards = vec![
            card("small", Status::Review),
            card("wide", Status::Review),
            card("done_one", Status::Done),
        ];
        let mut since = std::collections::HashMap::new();
        since.insert("small".to_string(), now - 3 * 3_600_000); // 3 h
        since.insert("wide".to_string(), now - 50 * 3_600_000); // ~2 days
        let mut surfaces = std::collections::HashMap::new();
        surfaces.insert(
            "small".to_string(),
            DiffSurface { files: 1, added: 4, removed: 0 },
        );
        surfaces.insert(
            "wide".to_string(),
            DiffSurface { files: 14, added: 320, removed: 60 },
        );

        let queue = triage(&cards, &since, &surfaces, now);
        assert_eq!(queue.len(), 2, "only Review cards are queued");
        assert_eq!(queue[0].card_id, "wide");
        assert!(queue[0].risk > queue[1].risk);
        assert!(queue[0].reasons.iter().any(|r| r.contains("files")));
        assert!(queue[0].reasons.iter().any(|r| r.contains("waiting 2 days")));
    }
}
