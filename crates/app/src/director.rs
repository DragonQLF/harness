//! The prompt for a conversation, whoever is speaking.
//!
//! The Director is one identity for the whole workspace, not one per project.
//! It does two jobs in two different scopes:
//!
//! - **Reviewing a diff** happens inside a project's engine, because that is
//!   where the worktree and the board live. See `harness_engine::director`.
//! - **Talking to the operator** happens here, at workspace level: it can see
//!   every board at once, which is what the UI promises when it says
//!   "watching · all projects".
//!
//! A specialist profile in direct chat uses the same builder with a different
//! speaker, so there is one place that decides how a conversation opens.
//!
//! This module only builds strings. Running them is the shell's job.

use harness_domain::{Card, Status};

/// Card id the Director's conversation used to be published under. Still
//  reserved: no card may use it.
pub const CARD_ID: &str = "director";

/// One project as the conversation sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectBrief {
    pub id: String,
    pub name: String,
    pub path: String,
    /// True for the project the operator currently has open.
    pub active: bool,
    pub cards: Vec<CardLine>,
    /// The project's charter (charter.md), when it has one. Filled in by the
    /// shell for the open project; every board carrying its own charter would
    /// bloat every turn for no gain.
    pub charter: Option<String>,
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
    /// git — without it there is no idea what is in a worktree, and guessing
    /// starts.
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

/// Who is talking. Comes from the agent profile, so a specialist chat reads as
/// that specialist rather than as a second Director.
#[derive(Debug, Clone, Default)]
pub struct Speaker<'a> {
    pub name: &'a str,
    /// The one-liner from the profile: "Orchestrator", "Researcher".
    pub title: &'a str,
    /// The standing brief the operator wrote for this profile.
    pub brief: &'a str,
    /// The Director plans and delegates; a specialist answers in its field.
    pub is_director: bool,
    /// May it hand work to other agents?
    pub can_delegate: bool,
    /// What a good answer from this profile looks like, when the operator said.
    pub expected_output: &'a str,
}

#[derive(Debug, Clone, Default)]
pub struct ChatContext<'a> {
    pub speaker: Speaker<'a>,
    pub user_name: &'a str,
    /// Every board, with the open one marked.
    pub projects: &'a [ProjectBrief],
    /// The repository this run can actually read, when one is open.
    pub repo: Option<&'a str>,
    /// Continuing a native Claude session: who it is, what it was told and
    /// everything already said are still in that session, so the opening does
    /// not repeat them.
    pub resumed: bool,
    /// Agents the operator has configured, for delegation.
    pub crew: &'a [(String, String)],
    /// The operator's standing notes (global.md), always worth repeating.
    pub global_memory: &'a str,
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

/// The board, written out the way it should be read.
fn render(brief: &ProjectBrief) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "## {} ({}){}\n{}\n",
        brief.name,
        brief.id,
        if brief.active { " — open right now" } else { "" },
        brief.path
    ));
        // The charter is the one thing that does not change with the board,
        // so it reads before the cards rather than after them.
        if let Some(charter) = &brief.charter {
            out.push_str("Their charter for this project:\n");
            for line in charter.lines() {
                out.push_str(&format!("  {line}\n"));
            }
        }
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
        // Without this there is no way to know a card is one .md file.
        if let Some(diff) = &card.diff {
            // Counts and a handful of names — never the patch. To read the
            // change itself there is read_diff.
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

fn boards(projects: &[ProjectBrief]) -> String {
    let mut out = String::new();
    for brief in projects {
        out.push_str(&render(brief));
        out.push('\n');
    }
    out
}

/// What Harness is, told once per session rather than every turn.
fn how_harness_works(ctx: &ChatContext) -> String {
    let mut out = String::from(
        "Harness is where this conversation lives: a desktop app on their own machine that \
         runs Claude agents. What it can do for you:\n\
         - A **project** is a git repository with a board. Work you want an agent to carry out \
         becomes a **card**; one card is one agent run, in its own git worktree, reviewed as a \
         diff before it counts.\n\
         - **Agent profiles** are the crew: each has a brief, a model, tools and a budget.\n",
    );
    if !ctx.crew.is_empty() {
        out.push_str("- Configured right now: ");
        out.push_str(
            &ctx.crew
                .iter()
                .map(|(id, title)| format!("{id} ({title})"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str(".\n");
    }
    out.push('\n');
    out
}

/// The opening for a conversation. On a resumed native session this is only
/// what changed since; on a fresh one it is the whole identity.
pub fn chat_prompt(ctx: &ChatContext, message: &str) -> String {
    let mut prompt = String::new();

    if ctx.resumed {
        // Identity, brief and history are already in the session being resumed.
        // Repeating them wastes the turn and invites the model to start over;
        // what it cannot know is what changed on the boards while it was away.
        if !ctx.projects.is_empty() {
            prompt.push_str("(Current state of the boards, which may have changed since we last spoke:\n\n");
            prompt.push_str(&boards(ctx.projects));
            prompt.push_str(")\n\n");
        }
        prompt.push_str(ctx.user_name.trim());
        prompt.push_str(": ");
        prompt.push_str(message.trim());
        return prompt;
    }

    let who = if ctx.speaker.name.trim().is_empty() {
        "the Director"
    } else {
        ctx.speaker.name.trim()
    };
    prompt.push_str(&format!(
        "You are {who}{}, talking with {} in Harness — their own agent harness, on their machine.\n\n",
        if ctx.speaker.title.trim().is_empty() {
            String::new()
        } else {
            format!(", {}", ctx.speaker.title.trim())
        },
        ctx.user_name.trim()
    ));
    // Identity, stated: a model that does not know which profile it IS speaks
    // of itself in the third person and invents someone else to blame.
    prompt.push_str(&format!(
        "This conversation runs as your own profile ({}).{} When something fails or is \
         refused, say so plainly and stop — never route around it.\n\n",
        if ctx.speaker.name.trim().is_empty() { "unnamed" } else { ctx.speaker.name.trim() },
        if ctx.speaker.can_delegate {
            " You may put work on boards and hand it to agents."
        } else {
            " This profile cannot change boards; ask in text instead."
        }
    ));
    // The failure mode seen in the wild: a refused board tool followed by
    // hand-written files outside the system. Work outside cards has no
    // review, no history, no cost — a refusal is information for the
    // operator, not an obstacle.
    prompt.push_str(
        "If a board tool is refused or fails, tell the operator and stop. Do not work around \
         refusals by editing files directly; work that never became a card has no review, no \
         history and no cost attached.\n\n",
    );

    if ctx.speaker.is_director {
        prompt.push_str(
            "You are their main assistant and the place they think out loud. Anything they bring \
             you is in scope: software, research, a business idea, a website, planning, money, \
             a personal project, what to do next. Treat this as a conversation with someone \
             capable, not as a ticketing queue.\n\n",
        );
    } else {
        prompt.push_str(
            "This is a direct conversation with you as a specialist, not a task handed to you. \
             Answer in your own field, say when something is outside it, and say who should take \
             it instead.\n\n",
        );
    }

    if !ctx.speaker.brief.trim().is_empty() {
        prompt.push_str("How they asked you to work:\n");
        prompt.push_str(ctx.speaker.brief.trim());
        prompt.push_str("\n\n");
    }
    if !ctx.speaker.expected_output.trim().is_empty() {
        prompt.push_str("What a good answer from you looks like: ");
        prompt.push_str(ctx.speaker.expected_output.trim());
        prompt.push_str("\n\n");
    }

    // The rule that keeps it from turning every question into machinery.
    prompt.push_str(
        "Answer the question that was asked. Most messages want a straight answer, an opinion or \
         a plan in prose — not a project, not a card, not an agent. Only put work on a board when \
         they ask for something to be carried out, or when it is plainly too much for one reply, \
         and say what you are about to do before you do it.\n\n",
    );

    prompt.push_str(&how_harness_works(ctx));

    // Standing notes travel with every fresh session: they are the operator's
    // "always, everywhere" and the model cannot know them otherwise. On a
    // resumed session they are already in there.
    if !ctx.global_memory.trim().is_empty() {
        prompt.push_str("Standing notes from the operator (always apply):\n");
        prompt.push_str(ctx.global_memory.trim());
        prompt.push_str("\n\n");
    }

    if ctx.projects.is_empty() {
        prompt.push_str(
            "No projects are registered yet, so there is no board and no code to read. That is \
             not a problem to solve before you can be useful: answer, plan and advise as normal. \
             A project is worth suggesting only when what they want actually needs files written \
             or code changed — and then it is an offer, not a prerequisite.\n\n",
        );
    } else {
        prompt.push_str("Every board you are watching:\n\n");
        prompt.push_str(&boards(ctx.projects));
        match ctx.repo {
            Some(name) => prompt.push_str(&format!(
                "You are running inside {name}, so you may read its files to answer. For other \
                 projects you have the board above and nothing else.\n\n"
            )),
            None => prompt.push_str(
                "No project is open, so you have the boards above rather than any code. Say so \
                 rather than describing files you cannot see.\n\n",
            ),
        }
    }

    if ctx.speaker.can_delegate {
        prompt.push_str(
            "When work should be done rather than discussed, you can put it on a board and hand \
             it to an agent yourself. Keep a card small enough for one run, pick the profile that \
             fits it, and tell them which agent has it.\n\n",
        );
    }

    // The two failure modes seen in practice: inventing the contents of a
    // worktree, and narrating the app instead of operating it.
    prompt.push_str(
        "Be honest about what you actually know. The boards above are what you were told; a card \
         line may say how many files its worktree touched and the lines added and removed — that \
         is a summary, not the change, so call read_diff rather than describing a change you have \
         not read. Say plainly when you have not looked at something.\n\n",
    );
    prompt.push_str(
        "You act through your tools instead of describing the app: showing them a screen, reading \
         a diff, changing a board are things you do, not things you explain. Never claim to have \
         done something a tool did not confirm, and never offer a menu of options instead of an \
         answer.\n\n",
    );

    prompt.push_str(ctx.user_name.trim());
    prompt.push_str(": ");
    prompt.push_str(message.trim());
    prompt
}

/// The Analyst: reads the numbers the app already computed — never computes
/// its own, models miscount — and answers with an ordered list of what to fix,
/// citing card ids as evidence. It writes nothing; the answer is the work.
pub fn analyst_prompt(tables: &str) -> String {
    format!(
        "You are the Analyst. Below are tables Harness already computed about its own \
         operation: card flow, spend, runs, reviews.\n\n\
         Rules:\n\
         - Interpret; do not recompute. The arithmetic is given and trusted. If two numbers \
         look inconsistent, say so instead of averaging them.\n\
         - Every claim cites evidence: a card id, a number from the tables, or \"not in the \
         data\".\n\
         - End with at most five fixes, most urgent first, each one line: what to change, \
         which card or number shows why.\n\
         - You have no tools and write nothing. The answer is the work.\n\n\
         TABLES:\n{tables}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_domain::{Actor, CardId, Review};

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

    fn director<'a>() -> Speaker<'a> {
        Speaker {
            name: "Director",
            title: "Orchestrator",
            brief: "",
            is_director: true,
            can_delegate: true,
            expected_output: "",
        }
    }

    fn ctx<'a>(projects: &'a [ProjectBrief]) -> ChatContext<'a> {
        ChatContext {
            speaker: director(),
            user_name: "Fernando",
            projects,
            repo: None,
            resumed: false,
            crew: &[],
            global_memory: "",
        }
    }

    #[test]
    fn with_no_projects_it_is_still_a_general_assistant() {
        let prompt = chat_prompt(&ctx(&[]), "should I start a website studio?");
        // The whole point: no repository is not a blocker, and it does not open
        // by asking which repo to add.
        assert!(prompt.contains("No projects are registered yet"));
        assert!(prompt.contains("not a problem to solve before you can be useful"));
        assert!(prompt.contains("an offer, not a prerequisite"));
        assert!(prompt.contains("Anything they bring you is in scope"));
        assert!(
            prompt.trim_end().ends_with("Fernando: should I start a website studio?"),
            "{prompt}"
        );
    }

    #[test]
    fn it_answers_rather_than_manufacturing_work() {
        let prompt = chat_prompt(&ctx(&[]), "what is a worktree?");
        assert!(prompt.contains("Answer the question that was asked"));
        assert!(prompt.contains("Only put work on a board when"));
    }

    #[test]
    fn it_is_not_framed_as_a_software_manager() {
        let prompt = chat_prompt(&ctx(&[]), "hello");
        for gone in [
            "You are the Director of Harness, a desktop tool",
            "you never write code yourself",
            "break the first piece of work into cards",
        ] {
            assert!(!prompt.contains(gone), "still says: {gone}");
        }
        assert!(prompt.contains("software, research, a business idea"));
    }

    #[test]
    fn it_sees_every_board_at_once() {
        let projects = vec![
            ProjectBrief {
                id: "harness".into(),
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
                charter: None,
            },
            ProjectBrief {
                id: "atlas".into(),
                name: "atlas".into(),
                path: "C:/src/atlas".into(),
                active: false,
                cards: vec![],
                charter: None,
            },
        ];
        let mut c = ctx(&projects);
        c.repo = Some("harness");
        let prompt = chat_prompt(&c, "what needs me?");

        assert!(prompt.contains("## harness (harness) — open right now"));
        assert!(prompt.contains("## atlas (atlas)"));
        assert!(prompt.contains("[working] Retry the sidecar — builder (c_1)"));
        assert!(prompt.contains("[waiting for review] Scope the allowlist"));
        assert!(prompt.contains("last review sent back: allowlist too wide"));
        assert!(prompt.contains("(no cards yet)"), "an empty board still gets named");
        assert!(prompt.contains("running inside harness"));
    }

    #[test]
    fn a_resumed_session_does_not_repeat_who_it_is() {
        let projects = vec![ProjectBrief {
            id: "atlas".into(),
            name: "atlas".into(),
            path: "C:/src/atlas".into(),
            active: false,
            cards: vec![card("c_9", "Usage rollup", Status::Ready)],
            charter: None,
        }];
        let mut c = ctx(&projects);
        c.resumed = true;
        let prompt = chat_prompt(&c, "and now?");

        // The session already holds the identity; sending it again would have
        // it start the conversation over.
        assert!(!prompt.contains("You are Director"));
        assert!(!prompt.contains("Harness is where this conversation lives"));
        // But the boards may have moved while it was away.
        assert!(prompt.contains("Usage rollup"));
        assert!(prompt.contains("may have changed since we last spoke"));
        assert!(prompt.trim_end().ends_with("Fernando: and now?"));
    }

    #[test]
    fn a_resumed_session_with_no_projects_is_just_the_message() {
        let mut c = ctx(&[]);
        c.resumed = true;
        assert_eq!(chat_prompt(&c, "  carry on  "), "Fernando: carry on");
    }

    #[test]
    fn a_specialist_speaks_as_itself_not_as_a_second_director() {
        let projects: Vec<ProjectBrief> = vec![];
        let mut c = ctx(&projects);
        c.speaker = Speaker {
            name: "Scout",
            title: "Researcher",
            brief: "Answer with file paths and line numbers. Never edit.",
            is_director: false,
            can_delegate: false,
            expected_output: "A short answer with citations.",
        };
        let prompt = chat_prompt(&c, "where is the approval router?");

        assert!(prompt.contains("You are Scout, Researcher"));
        assert!(prompt.contains("direct conversation with you as a specialist"));
        assert!(prompt.contains("Answer with file paths and line numbers"));
        assert!(prompt.contains("A short answer with citations."));
        // It is not told it may delegate, because its profile says it may not.
        assert!(!prompt.contains("hand it to an agent yourself"));
        assert!(!prompt.contains("Anything they bring you is in scope"));
    }

    #[test]
    fn the_crew_is_named_so_work_can_be_handed_over() {
        let projects: Vec<ProjectBrief> = vec![];
        let mut c = ctx(&projects);
        let crew = vec![
            ("builder".to_string(), "Implementer".to_string()),
            ("scout".to_string(), "Researcher".to_string()),
        ];
        c.crew = &crew;
        let prompt = chat_prompt(&c, "build me a landing page");
        assert!(prompt.contains("builder (Implementer), scout (Researcher)"));
        assert!(prompt.contains("hand it to an agent yourself"));
    }

    #[test]
    fn a_cards_diff_facts_reach_the_prompt() {
        let projects = vec![ProjectBrief {
            id: "harness".into(),
            name: "harness".into(),
            path: "C:/src/harness".into(),
            active: false,
            cards: vec![card("c_1", "Notes", Status::Review).with_diff(Some(DiffFacts {
                files: vec!["docs/notes.md".into()],
                added: 3,
                removed: 0,
            }))],
            charter: None,
        }];
        let prompt = chat_prompt(&ctx(&projects), "what is in that card?");
        assert!(prompt.contains("1 file touched: docs/notes.md (+3 -0)"));
        // And it is told not to invent what it was not given.
        assert!(prompt.contains("call read_diff rather than describing a change"));
        assert!(
            prompt.contains("never claim to have done something a tool did not confirm")
                || prompt.contains("Never claim to have done something a tool did not confirm"),
            "it must not narrate actions it did not take"
        );
    }

    #[test]
    fn with_projects_but_none_open_it_answers_from_the_boards() {
        let projects = vec![ProjectBrief {
            id: "atlas".into(),
            name: "atlas".into(),
            path: "C:/src/atlas".into(),
            active: false,
            cards: vec![card("c_9", "Usage rollup", Status::Ready)],
            charter: None,
        }];
        let prompt = chat_prompt(&ctx(&projects), "anything waiting?");
        assert!(prompt.contains("No project is open"));
        assert!(!prompt.contains("you may read its files"));
    }

    #[test]
    fn curated_memory_rides_with_the_prompt() {
        let projects = vec![ProjectBrief {
            id: "atlas".into(),
            name: "atlas".into(),
            path: "C:/src/atlas".into(),
            active: true,
            cards: vec![],
            charter: Some("Ship weekly. Never touch billing without a human.".into()),
        }];
        let mut c = ctx(&projects);
        c.global_memory = "Write plainly. No marketing adjectives.";
        let prompt = chat_prompt(&c, "what next?");

        assert!(prompt.contains("Standing notes from the operator"));
        assert!(prompt.contains("Write plainly. No marketing adjectives."));
        // The charter reads under its own project, before the cards.
        let charter_at = prompt.find("Their charter for this project").unwrap();
        let boards_at = prompt.find("## atlas").unwrap();
        assert!(boards_at < charter_at, "charter belongs to its board");
        assert!(prompt.contains("Never touch billing without a human."));
        assert!(!prompt.contains("(no cards yet)\nTheir"), "ordering holds");
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
            session_id: None,
            worktree: None,
            branch: None,
            depends_on: Vec::new(),
        };
        let line = CardLine::from_card(&card);
        assert_eq!(line.id, "c_7");
        assert_eq!(line.agent_id, "scout");
        assert_eq!(line.review, Some((true, "scoped".to_string())));
    }
}
