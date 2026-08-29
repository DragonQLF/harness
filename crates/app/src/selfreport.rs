//! The Director's mirror: what happens to him, counted.
//!
//! Refusals, expired approvals, failed runs and sent-back cards all die inside
//! their own conversation or transcript today. Nobody aggregates "he hit the
//! same refusal twelve times this week" — which is exactly the signal that
//! would produce an improvement proposal. This module is that aggregation,
//! computed in code over the logs we already write: the model never counts
//! (same principle as the Analyst, #55) — it receives the finished table.
//!
//! Counts and one short example each, never the raw log. Forty identical
//! refusals are one line, not forty transcriptions.

use std::collections::BTreeMap;

use harness_domain::{Event, RunOutcome, Status};
use harness_ports::{RunEvent, RunLogLine, StoredEvent};
use serde::Serialize;
use ts_rs::TS;

const DAY_MS: u64 = 86_400_000;

/// One approval the operator never answered: recorded by the router at the
/// moment its thirty minutes ran out (`approvals.rs`), so expiry survives a
/// restart instead of being indistinguishable from a deliberate no.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct ExpiredApproval {
    #[ts(type = "number")]
    pub ts_ms: u64,
    pub project_id: String,
    pub tool: String,
    pub summary: String,
}

/// One line of the refusal table: same tool, same reason, N times.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct RefusalCount {
    pub tool: String,
    /// The reason as first seen, trimmed to what names the pattern.
    pub reason: String,
    pub count: u32,
}

/// How runs ended badly in the window, budget cuts named apart: a cut is the
/// app doing its job (pause, wip-commit, resume later); a real failure is a
/// problem. Lumping them together would read as breakage where there is none.
#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
pub struct RunFailures {
    pub total: u32,
    pub budget_cuts: u32,
    pub other: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
pub struct SelfReport {
    pub window_days: u32,
    pub refusals: Vec<RefusalCount>,
    pub approvals_expired: u32,
    pub runs_failed: RunFailures,
    /// Runs whose final commit could not be made — the review would have seen
    /// nothing.
    pub commit_errors: u32,
    /// Finished runs where the agent never called `report_work`.
    pub unreported: u32,
    /// Cards that came back from Review to Ready, by anyone's hand.
    pub sent_back: u32,
}

impl SelfReport {
    pub fn is_empty(&self) -> bool {
        self.refusals.is_empty()
            && self.approvals_expired == 0
            && self.runs_failed.total == 0
            && self.commit_errors == 0
            && self.unreported == 0
            && self.sent_back == 0
    }
}

/// What names a pattern: strip the transcript dressing ("failed — …"), flatten
/// whitespace so one reason spread over lines still groups with itself, and cut
/// at a width long enough to be specific and short enough to compare.
fn normalize_reason(summary: &str) -> String {
    let stripped = summary
        .trim()
        .strip_prefix("failed — ")
        .unwrap_or_else(|| summary.trim());
    let mut flat = String::new();
    let mut last_space = false;
    for c in stripped.chars() {
        let space = c.is_whitespace();
        if !space || !last_space {
            flat.push(if space { ' ' } else { c });
        }
        last_space = space;
        if flat.len() >= 110 {
            break;
        }
    }
    flat.trim_end().to_string()
}

const COMMIT_ERROR_PREFIX: &str = "could not commit the work:";
const UNREPORTED_PREFIX: &str = "the agent did not report its work";

/// Aggregate everything that happened to the agents across every project, over
/// the last `window_days`. Inputs are plain log contents, merged by the caller:
/// events from every project's log, transcript lines from run logs and chat
/// transcripts alike, and whatever approvals expired unanswered.
pub fn aggregate(
    events: &[StoredEvent],
    lines: &[RunLogLine],
    expired: &[ExpiredApproval],
    now_ms: u64,
    window_days: u32,
) -> SelfReport {
    let horizon = now_ms.saturating_sub(window_days as u64 * DAY_MS);
    let in_window = |ts_ms: u64| ts_ms >= horizon && ts_ms <= now_ms.saturating_add(DAY_MS);

    let mut report = SelfReport {
        window_days,
        ..Default::default()
    };

    // ---- board events ----
    for stored in events {
        if !in_window(stored.ts_ms) {
            continue;
        }
        match &stored.event {
            Event::RunFinished { outcome, .. } if *outcome == RunOutcome::Failed => {
                report.runs_failed.total += 1;
                report.runs_failed.other += 1;
            }
            Event::BudgetPauseSet { paused: true, .. } => {
                // A budget cut also finished as a Failed run; subtract it back
                // out so the two rows do not double-count one event.
                report.runs_failed.other = report.runs_failed.other.saturating_sub(1);
                report.runs_failed.budget_cuts += 1;
            }
            Event::CardRejected { .. } => report.sent_back += 1,
            Event::CardMoved {
                from: Status::Review,
                to: Status::Ready,
                ..
            } => report.sent_back += 1,
            _ => {}
        }
    }

    // ---- transcripts: refusals and the harness's own notices ----
    // A result block only knows the id of the call; the name travels on the
    // ToolUse that opened it, so pair them while walking.
    let mut tool_by_id: BTreeMap<String, String> = BTreeMap::new();
    let mut refusals: BTreeMap<(String, String), (u32, String)> = BTreeMap::new();

    for line in lines {
        if !in_window(line.ts_ms) {
            continue;
        }
        match &line.event {
            RunEvent::ToolUse {
                tool,
                tool_use_id: Some(id),
                ..
            } => {
                tool_by_id.insert(id.clone(), tool.clone());
            }
            RunEvent::ToolResult {
                tool_use_id,
                ok: false,
                summary,
                ..
            } => {
                let tool = tool_by_id
                    .get(tool_use_id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let reason = normalize_reason(summary);
                let entry = refusals
                    .entry((tool, reason))
                    .or_insert_with(|| (0, String::new()));
                entry.0 += 1;
                if entry.1.is_empty() {
                    entry.1 = summary.trim().chars().take(160).collect();
                }
            }
            RunEvent::Notice { text } => {
                let trimmed = text.trim();
                if trimmed.starts_with(COMMIT_ERROR_PREFIX) {
                    report.commit_errors += 1;
                } else if trimmed.starts_with(UNREPORTED_PREFIX) {
                    report.unreported += 1;
                }
            }
            _ => {}
        }
    }

    report.refusals = refusals
        .into_iter()
        .map(|((tool, reason), (count, _))| RefusalCount {
            tool,
            reason,
            count,
        })
        .collect();
    // Loudest patterns first: the point of the table is what repeats.
    report
        .refusals
        .sort_by(|a, b| b.count.cmp(&a.count).then(a.tool.cmp(&b.tool)));

    report.approvals_expired = expired
        .iter()
        .filter(|e| in_window(e.ts_ms))
        .count() as u32;

    report
}

/// The table itself, ready to hand to the model: counts and one example per
/// pattern, never the log.
pub fn render(report: &SelfReport) -> String {
    let mut out = format!(
        "Self report, last {} day{} (every project, counts only):\n",
        report.window_days,
        if report.window_days == 1 { "" } else { "s" }
    );

    if report.is_empty() {
        out.push_str("\nNothing went wrong in this window.\n");
        return out;
    }

    out.push_str("\nTool refusals:\n");
    if report.refusals.is_empty() {
        out.push_str("- none\n");
    }
    for r in &report.refusals {
        out.push_str(&format!(
            "- {} × {} — {}{}\n",
            r.tool,
            r.count,
            if r.reason.is_empty() { "(no reason given)" } else { &r.reason },
            if r.count > 1 { " ← repeats" } else { "" }
        ));
    }

    out.push_str(&format!(
        "\nApprovals that expired unanswered (30 min): {}\n",
        report.approvals_expired
    ));

    out.push_str(&format!(
        "\nFailed runs: {} total — {} cut by their budget ceiling, {} real failures\n",
        report.runs_failed.total, report.runs_failed.budget_cuts, report.runs_failed.other,
    ));

    out.push_str(&format!("\nCommit errors: {}\n", report.commit_errors));
    out.push_str(&format!(
        "Finished runs where the agent never reported its work: {}\n",
        report.unreported
    ));
    out.push_str(&format!(
        "\nCards sent back from Review to Ready: {}\n",
        report.sent_back
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_domain::{CardId, RunId};

    fn stored(ts_ms: u64, event: Event) -> StoredEvent {
        StoredEvent {
            seq: 1,
            ts_ms,
            event,
        }
    }

    fn line(ts_ms: u64, event: RunEvent) -> RunLogLine {
        RunLogLine { ts_ms, event }
    }

    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn forty_identical_refusals_are_one_line_not_forty() {
        let mut lines = Vec::new();
        for n in 0..40 {
            lines.push(line(
                NOW - 1000,
                RunEvent::ToolUse {
                    tool: "mcp__harness__create_card".into(),
                    summary: String::new(),
                    tool_use_id: Some(format!("t{n}")),
                    parent_tool_use_id: None,
                },
            ));
            lines.push(line(
                NOW - 1000 + n as u64,
                RunEvent::ToolResult {
                    tool_use_id: format!("t{n}"),
                    ok: false,
                    summary: "failed — there is no project to act on. Pass project_id"
                        .into(),
                    detail: None,
                },
            ));
        }

        let report = aggregate(&[], &lines, &[], NOW, 7);
        assert_eq!(report.refusals.len(), 1, "one pattern, one row");
        assert_eq!(report.refusals[0].count, 40);
        assert_eq!(report.refusals[0].tool, "mcp__harness__create_card");
        let rendered = render(&report);
        let hits = rendered.matches("there is no project to act on").count();
        assert_eq!(hits, 1, "the reason appears once, not 40 times");
        assert!(rendered.contains("× 40"));
    }

    #[test]
    fn reasons_that_differ_group_apart_and_whitespace_does_not_split_one() {
        let lines = vec![
            line(NOW, RunEvent::ToolUse { tool: "Bash".into(), summary: String::new(), tool_use_id: Some("a".into()), parent_tool_use_id: None }),
            line(NOW, RunEvent::ToolResult { tool_use_id: "a".into(), ok: false, summary: "failed — denied by operator".into(), detail: None }),
            line(NOW, RunEvent::ToolUse { tool: "Bash".into(), summary: String::new(), tool_use_id: Some("b".into()), parent_tool_use_id: None }),
            line(NOW, RunEvent::ToolResult { tool_use_id: "b".into(), ok: false, summary: "failed — denied\nby   operator".into(), detail: None }),
            line(NOW, RunEvent::ToolUse { tool: "Read".into(), summary: String::new(), tool_use_id: Some("c".into()), parent_tool_use_id: None }),
            line(NOW, RunEvent::ToolResult { tool_use_id: "c".into(), ok: false, summary: "failed — file not found".into(), detail: None }),
        ];
        let report = aggregate(&[], &lines, &[], NOW, 7);
        assert_eq!(report.refusals.len(), 2);
        let bash = report.refusals.iter().find(|r| r.tool == "Bash").unwrap();
        assert_eq!(bash.count, 2, "line breaks and spacing do not split a pattern");
        // Loudest first.
        assert_eq!(report.refusals[0].tool, "Bash");
    }

    #[test]
    fn budget_cuts_are_named_apart_from_real_failures() {
        let card = CardId::new("c_1");
        let run = RunId("r1".into());
        let events = vec![
            stored(
                NOW - DAY_MS,
                Event::RunStarted {
                    card_id: card.clone(),
                    run_id: run.clone(),
                    worktree: None,
                    branch: None,
                },
            ),
            stored(
                NOW - DAY_MS + 1,
                Event::RunFinished {
                    card_id: card.clone(),
                    run_id: run.clone(),
                    outcome: RunOutcome::Failed,
                    cost_usd: Some(0.5),
                    turns: Some(9),
                },
            ),
            stored(NOW - DAY_MS + 2, Event::BudgetPauseSet { card_id: card.clone(), paused: true }),
            stored(
                NOW - DAY_MS + 3,
                Event::RunFinished {
                    card_id: card.clone(),
                    run_id: RunId("r2".into()),
                    outcome: RunOutcome::Failed,
                    cost_usd: None,
                    turns: None,
                },
            ),
        ];

        let report = aggregate(&events, &[], &[], NOW, 7);
        assert_eq!(report.runs_failed.total, 2);
        assert_eq!(report.runs_failed.budget_cuts, 1, "the pause marks the cut");
        assert_eq!(report.runs_failed.other, 1, "no double counting");
    }

    #[test]
    fn commit_errors_unreported_and_sent_backs_are_counted_from_their_own_records() {
        let events = vec![
            stored(
                NOW,
                Event::CardRejected {
                    card_id: CardId::new("c_2"),
                    reason: "wrong shape".into(),
                    by: harness_domain::Actor::Director,
                    hunks: Vec::new(),
                },
            ),
            stored(
                NOW,
                Event::CardMoved {
                    card_id: CardId::new("c_3"),
                    from: Status::Review,
                    to: Status::Ready,
                },
            ),
        ];
        let lines = vec![
            line(NOW, RunEvent::Notice { text: "could not commit the work: git error: index.lock".into() }),
            line(NOW, RunEvent::Notice { text: "the agent did not report its work; the commit body stayed generic".into() }),
            line(NOW, RunEvent::Notice { text: "waiting for your review".into() }),
        ];
        let expired = vec![ExpiredApproval {
            ts_ms: NOW,
            project_id: "p".into(),
            tool: "Bash".into(),
            summary: "command: git push".into(),
        }];

        let report = aggregate(&events, &lines, &expired, NOW, 7);
        assert_eq!(report.commit_errors, 1);
        assert_eq!(report.unreported, 1);
        assert_eq!(report.sent_back, 2, "a rejection and a manual move both count");
        assert_eq!(report.approvals_expired, 1);
    }

    #[test]
    fn the_window_keeps_old_trouble_out() {
        let events = vec![stored(
            NOW - 8 * DAY_MS,
            Event::CardRejected {
                card_id: CardId::new("old"),
                reason: "ancient".into(),
                by: harness_domain::Actor::Human,
                hunks: Vec::new(),
            },
        )];
        let expired = vec![ExpiredApproval {
            ts_ms: NOW - 8 * DAY_MS,
            project_id: "p".into(),
            tool: "Bash".into(),
            summary: String::new(),
        }];
        let report = aggregate(&events, &[], &expired, NOW, 7);
        assert!(report.is_empty());
        assert!(render(&report).contains("Nothing went wrong"));
    }
}
