//! The Director is one identity for the whole workspace, not one per project.
//!
//! It does two jobs in two different scopes:
//!
//! - **Reviewing a diff** happens inside a project's engine, because that is
//!   where the worktree and the board live. See `harness_engine::director`.
//! - **Talking to the operator** happens here, at workspace level: it can see
//!   every board at once, which is what the UI promises when it says
//!   "watching · all projects".
//!
//! This module only builds the prompt. Running it is the shell's job.

use harness_domain::{Card, Status};

/// Card id the Director's conversation is published under, so the UI can tell
/// a chat reply from work on a real card. Reserved: no card may use it.
pub const CARD_ID: &str = "director";

/// One project as the Director sees it when answering a question.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBrief {
    pub name: String,
    pub path: String,
    /// True for the project the operator currently has open.
    pub active: bool,
    pub cards: Vec<CardLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardLine {
    pub id: String,
    pub title: String,
    pub status: Status,
    pub agent_id: String,
    /// Last review, if the card has one: (approved, reason).
    pub review: Option<(bool, String)>,
    /// What the run actually changed, for a card whose work is uncommitted or
    /// waiting: files, lines added, lines removed. Filled in by the shell from
    /// git — without it the Director has no idea what is in a worktree and
    /// starts guessing.
    pub diff: Option<DiffFacts>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffFacts {
    pub files: Vec<String>,
    pub added: u64,
    pub removed: u64,
}

impl CardLine {
    pub fn from_card(card: &Card) -> Self {
        Self {
            id: card.id.as_str().to_string(),
            title: card.title.clone(),
            status: card.status,
            agent_id: card.agent_id.clone(),
            review: card
                .last_review
                .as_ref()
                .map(|r| (r.approved, r.reason.clone())),
            diff: None,
        }
    }

    pub fn with_diff(mut self, diff: Option<DiffFacts>) -> Self {
        self.diff = diff;
        self
    }
}

fn status_word(status: Status) -> &'static str {
    match status {
        Status::Backlog => "later",
        Status::Ready => "ready",
        Status::Running => "working",
        Status::Review => "waiting for review",
        Status::Done => "done",
    }
}

/// The board, written out the way the Director should read it.
fn render(brief: &ProjectBrief) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "## {}{}\n{}\n",
        brief.name,
        if brief.active { " (open right now)" } else { "" },
        brief.path
    ));
    if brief.cards.is_empty() {
        out.push_str("(no cards yet)\n");
        return out;
    }
    for card in &brief.cards {
        out.push_str(&format!(
            "- [{}] {} — {} ({})",
            status_word(card.status),
            card.title,
            card.agent_id,
            card.id
        ));
        if let Some((approved, reason)) = &card.review {
            out.push_str(&format!(
                ", last review {}: {}",
                if *approved { "approved" } else { "sent back" },
                reason
            ));
        }
        out.push('\n');
        // What the worktree actually contains, when the shell could read it.
        // Without this the Director has no way to know a card is one .md file.
        if let Some(diff) = &card.diff {
            // Counts and a handful of names — never the patch. If it needs to
            // read the change it can call read_diff.
            const SHOWN: usize = 4;
            if diff.files.is_empty() {
                out.push_str("  its worktree changed nothing\n");
            } else {
                let mut files = diff
                    .files
                    .iter()
                    .take(SHOWN)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                if diff.files.len() > SHOWN {
                    files.push_str(&format!(" and {} more", diff.files.len() - SHOWN));
                }
                let count = if diff.files.len() == 1 {
                    "1 file".to_string()
                } else {
                    format!("{} files", diff.files.len())
                };
                out.push_str(&format!(
                    "  {count} touched: {files} (+{} -{})\n",
                    diff.added, diff.removed
                ));
            }
        }
    }
    out
}

/// Prompt for a question from the operator. With no projects it is an advisory
/// conversation about what to point Harness at; with projects it is grounded in
/// every board at once.
pub fn ask_prompt(question: &str, briefs: &[ProjectBrief]) -> String {
    let mut prompt = String::from(
        "You are the Director of Harness, a desktop tool that runs Claude Code agents against \
         git repositories. You plan and review; you never write code yourself. One card is one \
         agent run, in its own git worktree.\n\n",
    );

    if briefs.is_empty() {
        prompt.push_str(
            "The operator has not added any project yet, so there is no board and you cannot \
             read any code. Help them decide what to point Harness at and how to break the \
             first piece of work into cards small enough for one run each.\n\n",
        );
    } else {
        prompt.push_str("Every board you are watching:\n\n");
        for brief in briefs {
            prompt.push_str(&render(brief));
            prompt.push('\n');
        }
        let open = briefs.iter().find(|b| b.active);
        match open {
            Some(b) => prompt.push_str(&format!(
                "You are running inside {}, so you may read its files to answer. \
                 Answer about other projects from the board alone.\n\n",
                b.name
            )),
            None => prompt.push_str(
                "No project is open, so answer from the boards above rather than from code.\n\n",
            ),
        }
    }

    prompt.push_str(
        "Answer the operator concisely and practically, in prose. If something is waiting on \
         them, say which card and why.\n\n",
    );
    // Two failure modes seen in practice: inventing the contents of a worktree,
    // and narrating the app instead of operating it.
    prompt.push_str(
        "What you know: the boards above are everything you have been told. A card line may say ",
    );
    prompt.push_str(
        "how many files its worktree touched and the lines added and removed - that is a summary, ",
    );
    prompt.push_str(
        "not the change itself, so call read_diff rather than describing a change you have not ",
    );
    prompt.push_str("read.\n\n");
    prompt.push_str(
        "You act through your tools instead of describing the app: showing the operator a screen, ",
    );
    prompt.push_str(
        "reading a diff and changing the board are things you do, not things you explain. Do what ",
    );
    prompt.push_str(
        "their message asks for and say what you did. Never offer a menu of options, and never ",
    );
    prompt.push_str("claim to have done something a tool did not confirm.\n\n");
    prompt.push_str("Operator: ");
    prompt.push_str(question.trim());
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_domain::{CardId, Review, Actor};

    fn card(id: &str, title: &str, status: Status) -> CardLine {
        CardLine {
            id: id.into(),
            title: title.into(),
            status,
            agent_id: "builder".into(),
            review: None,
            diff: None,
        }
    }

    #[test]
    fn with_no_projects_it_is_an_advisory_conversation() {
        let prompt = ask_prompt("where do I start?", &[]);
        assert!(prompt.contains("has not added any project"));
        assert!(prompt.contains("break the first piece of work"));
        assert!(prompt.trim_end().ends_with("Operator: where do I start?"));
    }

    #[test]
    fn it_sees_every_board_at_once() {
        let briefs = vec![
            ProjectBrief {
                name: "harness".into(),
                path: "C:/src/harness".into(),
                active: true,
                cards: vec![
                    card("c_1", "Retry the sidecar", Status::Running),
                    CardLine {
                        review: Some((false, "allowlist too wide".into())),
                        ..card("c_2", "Scope the allowlist", Status::Review)
                    },
                ],
            },
            ProjectBrief {
                name: "atlas".into(),
                path: "C:/src/atlas".into(),
                active: false,
                cards: vec![],
            },
        ];
        let prompt = ask_prompt("what needs me?", &briefs);

        assert!(prompt.contains("## harness (open right now)"));
        assert!(prompt.contains("## atlas"));
        assert!(prompt.contains("[working] Retry the sidecar — builder (c_1)"));
        assert!(prompt.contains("[waiting for review] Scope the allowlist"));
        assert!(prompt.contains("last review sent back: allowlist too wide"));
        assert!(prompt.contains("(no cards yet)"), "an empty board still gets named");
        assert!(
            prompt.contains("running inside harness"),
            "the open project is where it may read code"
        );
    }

    #[test]
    fn a_cards_diff_facts_reach_the_prompt() {
        let briefs = vec![ProjectBrief {
            name: "harness".into(),
            path: "C:/src/harness".into(),
            active: false,
            cards: vec![card("c_1", "Notes", Status::Review).with_diff(Some(DiffFacts {
                files: vec!["docs/notes.md".into()],
                added: 3,
                removed: 0,
            }))],
        }];
        let prompt = ask_prompt("what is in that card?", &briefs);
        assert!(prompt.contains("1 file touched: docs/notes.md (+3 -0)"));
        // And it is told not to invent what it was not given.
        assert!(prompt.contains("call read_diff rather than describing a change"));
        assert!(
            prompt.contains("call read_diff"),
            "it is told to read rather than guess"
        );
        assert!(
            prompt.contains("never claim to have done something a tool did not confirm"),
            "it must not narrate actions it did not take"
        );
    }

    #[test]
    fn with_projects_but_none_open_it_answers_from_the_boards() {
        let briefs = vec![ProjectBrief {
            name: "atlas".into(),
            path: "C:/src/atlas".into(),
            active: false,
            cards: vec![card("c_9", "Usage rollup", Status::Ready)],
        }];
        let prompt = ask_prompt("anything waiting?", &briefs);
        assert!(prompt.contains("No project is open"));
        assert!(!prompt.contains("you may read its files"));
    }

    #[test]
    fn card_lines_come_from_real_cards() {
        let card = Card {
            id: CardId::new("c_7"),
            title: "Fix the retry".into(),
            status: Status::Review,
            current_run: None,
            agent_id: "scout".into(),
            cost_usd: 0.2,
            turns: 4,
            runs: 1,
            last_review: Some(Review {
                by: Actor::Director,
                approved: true,
                reason: "scoped".into(),
            }),
        };
        let line = CardLine::from_card(&card);
        assert_eq!(line.id, "c_7");
        assert_eq!(line.agent_id, "scout");
        assert_eq!(line.review, Some((true, "scoped".to_string())));
    }
}
