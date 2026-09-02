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
    /// The standing rules `record_decision` wrote for the open project.
    ///
    /// They were written and never read: the tool created the file and nothing
    /// loaded it back, so every rule the operator dictated reached nobody —
    /// including the one he dictated about not being asked for permission he
    /// had already given.
    pub decisions: &'a str,
    /// Commits that reached Relay's own repository without a card behind them,
    /// already counted and worded by `mirror::describe`. A fact, like the
    /// boards — not a rule. Absent on every project but the mirror, and absent
    /// whenever nothing came in outside the board, which is the normal case.
    pub outside_work: Option<&'a str>,
    /// Proposals of his the operator accepted and he has not acted on yet.
    /// Handed to him as a fact, exactly like `outside_work` above: an
    /// acceptance is permission he cannot otherwise know he was given, and a
    /// permission nobody tells him about is a permission that does nothing.
    pub accepted_proposals: &'a [crate::inbox::Proposal],
    /// Verdicts his own automatic reviewer reached while nobody was talking to
     /// A versão que está a correr, **apenas quando mudou** desde o último turno
    /// desta conversa. Um facto, como os boards — e não uma regra.
    pub new_version: Option<&'a str>,
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
        // One line per card: a title may carry a body under the request, and
        // a list of every board is not where it belongs. `read_diff` and the
        // card itself have the rest.
        out.push_str(&format!(
            "- [{}] {} — {} ({})",
            status_word(card.status),
            harness_domain::one_line(&card.title),
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

/// What Relay is, told once per session rather than every turn.
fn how_harness_works(ctx: &ChatContext) -> String {
    let mut out = String::from(
        "Relay is where this conversation lives: a desktop app on their own machine that \
         runs Claude agents. What it can do for you:\n\
         - A **project** is a git repository with a board. Work you want an agent to carry out \
         becomes a **card**; one card is one agent run, in its own git worktree, reviewed as a \
         diff before it counts.\n\
         - A run produces exactly one commit, built by Relay itself from the agent's \
         report_work call once the run ends — the subject is the card's title, not anything \
         the agent writes to git directly — so never ask an agent to commit as it goes or in \
         steps; there is one commit per run, made after, not several made during.\n\
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

/// Proposals of his the operator accepted, worded for his prompt.
///
/// An acceptance happens *between* turns, on a screen he cannot see, so this
/// has to reach the resumed branch as well — a live conversation is the most
/// likely place for it to land, and a fact only written into the fresh opening
/// would never arrive there at all.
fn accepted(proposals: &[crate::inbox::Proposal]) -> String {
    if proposals.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "The operator accepted these proposals of yours. Accepting is permission, not an order: \
         no card was created and nothing was assigned. Carrying one out is now yours to do — a \
         card on Relay's own board is the usual shape — and when you create it, pass the \
         proposal id as `proposal_id` so it stops being raised at you every turn. If you are \
         not going to act on one, say so plainly so they can settle it.\n\n",
    );
    for p in proposals {
        out.push_str(&format!(
            "- {} ({})\n",
            harness_domain::one_line(&p.title),
            p.id
        ));
        if !p.observation.trim().is_empty() {
            out.push_str(&format!("  what was seen: {}\n", p.observation.trim()));
        }
        if !p.proposal.trim().is_empty() {
            out.push_str(&format!("  what you proposed: {}\n", p.proposal.trim()));
        }
    }
    out.push('\n');
    out
}

/// The opening for a conversation. On a resumed native session this is only
/// what changed since; on a fresh one it is the whole identity.
/// Is this message a slash command rather than something to say?
///
/// The engine reads a command off the front of the prompt, so it only works if
/// it *is* the front. Everything `chat_prompt` puts before a message — the
/// boards, the identity, the version note — would bury it, and what reached
/// the model would be a paragraph ending in `/usage`.
///
/// `//` is not a command: it is how a line that genuinely starts with a slash
/// gets said. A path on its own line (`/etc/hosts`) is the case this must not
/// eat, so a command is a single leading slash followed by a name and nothing
/// else before the first space.
pub fn slash_command(message: &str) -> Option<&str> {
    let line = message.trim();
    let rest = line.strip_prefix('/')?;
    if rest.starts_with('/') {
        return None;
    }
    let name = rest.split_whitespace().next()?;
    if name.is_empty() || name.contains('/') {
        return None;
    }
    let named = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':');
    named.then_some(line)
}

pub fn chat_prompt(ctx: &ChatContext, message: &str) -> String {
    // A command goes as it was typed. Nothing is prepended, because anything
    // prepended stops it being one.
    if let Some(command) = slash_command(message) {
        return command.to_string();
    }

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
        // Antes da mensagem, porque é contexto e não conversa: o binário por
        // baixo desta sessão mudou, e nada mais lho diria.
        if let Some(version) = ctx.new_version {
            prompt.push_str(&format!(
                "(Relay updated itself to {version} since your last turn. Your tools may have \
                 changed. The code you can read is the repository's, which is not necessarily \
                 this build — check before you claim either way.)\n\n"
            ));
        }
        prompt.push_str(&accepted(ctx.accepted_proposals));
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
        "You are {who}{}, talking with {} in Relay — their own agent harness, on their machine.\n\n",
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
        "This conversation runs as your own profile ({}).{}\n\n",
        if ctx.speaker.name.trim().is_empty() { "unnamed" } else { ctx.speaker.name.trim() },
        if ctx.speaker.can_delegate {
            " You may put work on boards and hand it to agents."
        } else {
            " This profile cannot change boards; ask in text instead."
        }
    ));
    // The distinction this pair of paragraphs exists to draw.
    //
    // The first version of this rule said "when something fails or is refused,
    // say so plainly and stop — never route around it", and it was written for
    // a real failure: a refused board tool followed by files hand-written
    // outside the system, which is card-work with no review, no history and no
    // cost attached. That part is still true and still here.
    //
    // But it was written to cover *every* failure, and it read as: hit any
    // wall, hand the turn back. A missing binary, a guard refusing a read, a
    // 404 — all of them ended the turn. That is not carefulness, it is a
    // harness making a capable model less useful than it is on its own.
    //
    // So the stop is now scoped to the one thing it was for: the boundary the
    // operator owns. Everywhere else, keep going.
    prompt.push_str(
        "A refused board tool means the work does not become a card — not that you write the \
         files yourself instead. Card-work outside a card has no review, no history and no cost \
         attached, so that door stays shut. Say what was refused and offer the alternative.\n\n",
    );
    prompt.push_str(
        "Every other obstacle is yours to get past. A missing tool, a command that fails, a \
         path that is guarded, a dependency that is not installed, a file that is not where you \
         expected — try the other way round, read the thing rather than grepping for it, and \
         work out what is actually true. Do not hand back a turn because the first attempt \
         failed. Come back when you have the answer, or when you have genuinely run out of \
         approaches and can say precisely what you tried and what you would need.\n\n",
    );
    // The stop rule, tied to the event rather than to the end of the turn.
    // "Before you continue" is what makes it happen at all — without it the
    // filing waits for a wrap-up that never comes. "Say whether the refusal
    // was right" is the distinction that keeps the inbox worth reading: in one
    // real session AskUserQuestion was refused correctly (no surface for it in
    // the UI) and the Bash guard was refusing reads it had no business
    // refusing. Filed together and undistinguished, both read as noise.
    prompt.push_str(
        "When a tool is refused, that is a finding about the app, not a condition of your turn. \
         File it with propose_improvement and say whether the refusal was right or is a defect — \
         but file it on your way past, not instead of continuing.\n\n",
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

    // The rule that keeps it from turning every question into machinery, and
    // then the one that keeps *that* one from turning every request into a
    // conversation about the request.
    //
    // The first paragraph was written against over-ticketing and it worked. It
    // then over-corrected: asked "this needs an experiment, what would be a
    // good starter — Remotion or something else?", the answer was *"Take your
    // time. Nothing's running, nothing's costing you anything. I'll write card
    // 1 whenever you say."* A question that wanted research and a
    // recommendation got a deferral, and the licence check that settled it
    // only happened after the operator wrote "hm".
    //
    // "Say what you are about to do before you do it" produced that — it reads
    // as announce-and-wait — so it is gone.
    prompt.push_str(
        "Answer the question that was asked. Most messages want a straight answer, an opinion or \
         a plan in prose — not a project, not a card, not an agent. Only put work on a board when \
         they ask for something to be carried out, or when it is plainly too much for one reply.\n\n",
    );
    // Anthropic's tested scope-discipline wording for this model, near-verbatim:
    // it cut unrequested scope changes to nearly zero *without* producing extra
    // clarifying questions, which is the exact trade this prompt kept losing.
    // The two sentences after it are ours, and they earn their place by naming
    // a failure that reproduced here rather than a general virtue: the operator
    // pointed at something and got a discussion of it.
    prompt.push_str(
        "Deliver what they asked for, at the scope they intended. Interpret ambiguity the way a \
         careful colleague would: make routine judgment calls yourself, and check in only when \
         different readings would lead to materially different work. If you conclude the ask is \
         mistaken or a better approach exists, say so in a sentence and keep going with the task \
         as asked — do not quietly narrow, widen, or transform it. Finish the whole task, not just \
         the easy part of it, and report completion only when it is fully done; if you genuinely \
         cannot finish something, do the rest and say plainly what is missing and why.\n\n",
    );
    prompt.push_str(
        "Being pointed at something is the instruction. \"This needs an experiment\" means run \
         the experiment; \"look into X\" means look into X and come back with what you found. A \
         question you could answer better after looking is one to look into now — search, read the \
         repository, run the thing, price the options — and answer with what you found and what \
         you would do.\n\n",
    );
    // The four are the operator's own, lifted from the decision he dictated
    // after being asked twice in one session for permission he had already
    // given. His words: "you are supposed to be autonomous not having to ask me
    // stupid questions". Stated as a closed list because an open one is read as
    // an invitation to add to it.
    prompt.push_str(
        "Four things stop you: money above what was agreed, something destructive or \
         irreversible, a fork in what is being built rather than how, and a grant. Everything \
         else you decide and then report. Where something is reversible, do it and say what you \
         did — they can see a wrong reversible step and correct it in a sentence, and a question \
         costs them their attention, which is the thing they are short of.\n\n",
    );

    // Two shifts this model has that the prompt had no answer for, both
    // documented for it rather than guessed at.
    //
    // Length: Claude Opus 5 writes longer user-facing text than prior models,
    // and `effort` is not the lever — a short conciseness instruction is, and
    // measured about a fifth off. The operator's word for what he was getting
    // was "blah blah blah", which is the same observation from the other side.
    prompt.push_str(
        "Keep responses focused, brief and concise, so they are not overwhelming to read. \
         Caveats and disclaimers are short, with most of the answer on the answer; asked to \
         explain something, give a high-level summary unless they asked for depth.\n\n",
    );
    // Delegation: this model reaches for subagents freely — a direction change
    // from the one before it, which had to be pushed to delegate. It shows in
    // the logs: nine subagents in one conversation, four of them opened *by*
    // subagents. The dead `Task`/`Agent` guard (#124) let that happen; this is
    // the other half, because a guard that refuses is a worse way to say "not
    // worth it" than not reaching for it in the first place.
    if ctx.speaker.can_delegate {
        prompt.push_str(
            "Subagents multiply cost and time: each re-establishes context, re-explores and \
             reports back, and then you read the report. Delegate rarely, and only when the \
             payoff clearly beats that overhead — a genuinely independent, sizeable track, not \
             work you could finish in a handful of tool calls, and never to review or \
             double-check your own work. One is better than several; brief it precisely the \
             first time; and once you have delegated, do not redo it or re-derive what it \
             found.\n\n",
        );
    }

    // Where the work lands matters as much as whether it starts: a month of
    // "faz-me um site" into the open repo leaves three sites and two
    // experiments tangled in one history, and moving later costs — worktrees,
    // cards and memory all stay behind.
    if ctx.speaker.can_delegate {
        prompt.push_str(
            "Before creating cards, ask whether the work belongs to the project that is open. \
             Something new being built — a site, an app, a tool — gets its own project: propose \
             one with create_project and ask where it should live. The open project is for \
             drafts and for work that continues what is already there.\n\n",
        );
        // Working on the app itself is a thing to offer, not a setup step to
        // send them away for.
        prompt.push_str(
            "When they say they want to work on Relay itself — a change to the app \
             rather than to their own code — call `work_on_relay`. It finds this \
             machine's copy of the source or fetches one, and after that Relay is a \
             project like any other: cards, runs and diffs. Do not tell them to go \
             and register a repository first.\n\n",
        );
        // The crew is configurable from the conversation, but only when asked.
        // A Director that hires on its own initiative turns a chat into a
        // payroll; one that cannot hire when asked sends the operator off to a
        // settings screen mid-thought.
        prompt.push_str(
            "You can change the crew when the operator asks for it, and only then: \
             `create_agent` adds one, `edit_agent` changes what an existing one is for, \
             `set_agent_model` points one at a different model, and `grant_agent_tools` \
             widens or narrows what it may do. All of them reach their permission sheet \
             like any other change.\n\n\
             A grant is never remembered: the operator answers it every single time, \
             because approving one reach should not approve every reach after. Say \
             plainly what it would let the agent do before you ask — Shell and Write \
             especially. A new agent starts able to read and search. If they name a \
             model or an endpoint that is not set up, `add_endpoint` adds the row — \
             ollama, ollama-cloud and openrouter are known by name. Never ask them \
             to send you a key: this conversation is written to disk. Add the row, \
             then open the settings screen so they can paste the key themselves. \
             An endpoint with no key refuses every run before it starts, so say that \
             rather than letting them find out mid-run.\n\n",
        );
        // The review posture: what makes the review worth having is what it
        // catches, not how it sounds.
        prompt.push_str(
            "When you review finished work:\n\
             - **Verify instead of believing.** \"I implemented X\" proves nothing — the diff \
             does. Read what changed and compare against what the card asked. If it asked for \
             600-900 word articles, count them. Never approve silently: say what you verified \
             and what you could not.\n\
             - **Distinguish designed from done.** A decision written down is not code running. \
             If the log says something is closed and the code does not show it, that IS the \
             finding.\n\
             - **Say what is missing unasked.** A report that only answers the question is half \
             a report: the hole beside the good work is the part that matters.\n\
             - **Lead with damage.** Worst first, not first-found. Three things fine and one \
             broken? Open with the broken one.\n\
             - **Admit mistakes before moving on.** A reviewer who never corrects himself cannot \
             be trusted when he insists.\n\
             - **Write decisions when they happen** with your record_decision \
             tool, into the project's memory, and announce what you recorded. If the tool \
             fails, say so aloud instead of letting the decision die with this conversation.\n\n",
        );
        // He can only hold the posture above if he can see his own history.
        // But timing is ours, never his: the end-of-day look is scheduled by
        // the app at shutdown, so nothing here tells him a time of day — a
        // ritual he cannot schedule is noise in every other turn. What stays
        // is the ability and the direction of action: see, then propose.
        prompt.push_str(
            "You can see your own week: self_report counts what went wrong (refusals, expired \
             approvals, failed runs, sent-back cards), and read_docs opens DEBT.md and \
             DECISIONS.md, so you can tell apart what the app does not do yet from what sits \
             in DEBT.md waiting to be done. When something repeats and has an obvious \
             correction, file it with propose_improvement — a proposal the operator decides \
             on, never a card created on your own. Once they accept one, you are told so here, \
             and then acting on it is exactly what you should do.\n\n",
        );
    }

    // What changed in Relay's own code without passing through the board. A
    // fact he is handed, because the alternative is meeting behaviour that
    // contradicts what he believes and having no way to find out why.
    if let Some(said) = ctx.outside_work.map(str::trim).filter(|s| !s.is_empty()) {
        prompt.push_str("Since Relay last looked at its own repository: ");
        prompt.push_str(said);
        prompt.push_str("\n\n");
    }

    prompt.push_str(&accepted(ctx.accepted_proposals));

    prompt.push_str(&how_harness_works(ctx));

    // Standing notes travel with every fresh session: they are the operator's
    // "always, everywhere" and the model cannot know them otherwise. On a
    // resumed session they are already in there.
    if !ctx.global_memory.trim().is_empty() {
        prompt.push_str("Standing notes from the operator (always apply):\n");
        prompt.push_str(ctx.global_memory.trim());
        prompt.push_str("\n\n");
    }

    // Rules already settled on this board, in the operator's words or in your
    // own. They outrank your instincts about how to work — that is what makes
    // them decisions rather than notes.
    if !ctx.decisions.trim().is_empty() {
        prompt.push_str(
            "Decisions already recorded on this project. These are settled: follow them \
             without re-litigating, and say so if you think one is wrong rather than \
             quietly working around it.\n",
        );
        prompt.push_str(ctx.decisions.trim());
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

/// Files the operator attached to a message, folded into the message itself.
///
/// Nothing is uploaded anywhere: Relay runs on the operator's machine and the
/// agent already has Read, Glob and Grep — the pathguard only fences *writes*
/// (#39, #62). So an attachment is a pointer, named in the operator's own turn,
/// and the model opens what it needs. That also means the transcript records
/// exactly which files were on the table, which a hidden side channel would not.
pub fn with_attachments(message: &str, files: &[String]) -> String {
    let files: Vec<&str> = files
        .iter()
        .map(|f| f.trim())
        .filter(|f| !f.is_empty())
        .collect();
    let message = message.trim();
    if files.is_empty() {
        return message.to_string();
    }
    let mut out = String::from(message);
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(if files.len() == 1 {
        "Attached file, on this machine — read it with your own tools:\n"
    } else {
        "Attached files, on this machine — read them with your own tools:\n"
    });
    for file in files {
        out.push_str("- ");
        out.push_str(file);
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// The Analyst: reads the numbers the app already computed — never computes
/// its own, models miscount — and answers with an ordered list of what to fix,
/// citing card ids as evidence. It writes nothing; the answer is the work.
pub fn analyst_prompt(tables: &str) -> String {
    format!(
        "You are the Analyst. Below are tables Relay already computed about its own \
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

/// The end-of-day look: a short, self-directed turn run once a day when the
/// operator closes Relay. It is not a conversation — nobody is talking to
/// him. He looks, and either files proposals or says the day was clean.
pub fn daily_look_prompt(outside_work: Option<&str>) -> String {
    let mut out = String::from(
        "This is your end-of-day look. Nobody is talking to you; you are looking at what happened \
         to you and to the agents this week.\n\n\
         Call self_report for the last 7 days. Read what it shows — it is counts, already \
         computed; do not try to recount anything. If a pattern there has an obvious correction, \
         check read_docs (doc \"debt\") first so you do not propose what DEBT.md already tracks; \
         then file one proposal with propose_improvement per distinct problem: title, the counts \
         that show the pattern, and the correction. Propose only when something actually repeats \
         or plainly hurts — one rough day is weather, not a pattern. If nothing warrants a \
         proposal, say so in a sentence and stop. Do not create cards, do not move anything on \
         any board.",
    );
    // Code that changed without passing through a card. Handed to him as a
    // fact rather than left for him to discover as contradiction later.
    if let Some(said) = outside_work.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\n\nAlso, since you last looked: ");
        out.push_str(said);
    }
    out
}

/// What he is told when a finished run needs his eyes.
///
/// It arrives in his own conversation, in the middle of whatever else he is
/// doing, which is the whole point: the operator watches it happen and he
/// still has the thread they were on. So it is written as an interruption to a
/// person who is already in the room — not as a briefing to a stranger, which
/// is what the headless review used to get.
///
/// He is not asked for JSON. He has `read_diff`, `approve_card` and
/// `reject_card`, and the verdict is whichever one he calls: a tool call is a
/// board event with him named on it, where a parsed string was a second thing
/// that could disagree with the first.
pub fn review_prompt(card_id: &str, title: &str) -> String {
    format!(
        "A run just finished on {card_id} — \"{title}\" — and it is waiting in Review for you.\n\n         Call read_diff for {card_id} and judge whether the work does what the card asked. Be          strict about scope: work that widens permissions, touches unrelated files or skips tests          should go back. Then call approve_card or reject_card for {card_id}; sending it back          needs a reason the agent can act on.\n\n         Say in one or two sentences what you found and what you did, so the operator reading          this thread can see the decision rather than only its result. If the diff is not          something you should decide alone, say so and leave the card where it is."
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

    pub(super) fn ctx<'a>(projects: &'a [ProjectBrief]) -> ChatContext<'a> {
        ChatContext {
            new_version: None,
            speaker: director(),
            user_name: "Fernando",
            projects,
            repo: None,
            resumed: false,
            crew: &[],
            global_memory: "",
            decisions: "",
            outside_work: None,
            accepted_proposals: &[],
        }
    }


    /// Aceitar acontece entre turnos, num ecrã que ele não vê. Se a permissão
    /// não lhe chegar ao prompt, aceitar não desbloqueia nada — e o ramo
    /// retomado, que é o que corre numa conversa viva, **retorna** antes de
    /// tudo o resto. Este teste prende as duas metades.
    #[test]
    fn an_accepted_proposal_reaches_him_on_both_branches() {
        let mut inbox = crate::inbox::InboxState::default();
        let p = inbox.propose(
            "prp_7".into(),
            1,
            "widen the pathguard",
            "12 refusals in a week",
            "let writes reach the worktree root",
        );
        inbox.accept(&p.id);
        let waiting: Vec<crate::inbox::Proposal> =
            inbox.awaiting_action().into_iter().cloned().collect();

        for resumed in [false, true] {
            let mut c = ctx(&[]);
            c.resumed = resumed;
            c.accepted_proposals = &waiting;
            let prompt = chat_prompt(&c, "hey");
            assert!(prompt.contains("prp_7"), "resumed={resumed}: {prompt}");
            assert!(prompt.contains("widen the pathguard"), "resumed={resumed}");
            assert!(
                prompt.contains("12 refusals in a week"),
                "resumed={resumed}: the reasons travel with the permission"
            );
        }
    }

    /// Nada aceite, nada dito: um cabeçalho vazio todos os turnos seria ruído.
    #[test]
    fn nothing_accepted_says_nothing() {
        let prompt = chat_prompt(&ctx(&[]), "hey");
        assert!(!prompt.contains("Accepting is permission"), "{prompt}");
    }

    /// Uma sessão retomada não sabe que o binário mudou por baixo dela.
    ///
    /// O Director percebeu uma actualização real porque lhe apareceram
    /// ferramentas novas na lista — deduziu-a pelo efeito. E o ramo retomado
    /// do prompt **retorna** antes de tudo o que é identidade, portanto um
    /// aviso posto no sítio errado nunca lhe chegaria enquanto a conversa
    /// estivesse viva. Este teste prende as duas metades: que é dito, e que é
    /// dito no ramo que corre a cada turno.
    #[test]
    fn a_resumed_session_is_told_when_the_build_changed_under_it() {
        let mut c = ctx(&[]);
        c.resumed = true;
        c.new_version = Some("0.3.4");
        let prompt = chat_prompt(&c, "hey");
        assert!(prompt.contains("0.3.4"), "prompt foi: {prompt}");
        assert!(
            prompt.contains("updated itself"),
            "o aviso não sobreviveu ao ramo retomado: {prompt}"
        );
    }

    /// E cala-se quando nada mudou: o ramo retomado existe para não repetir o
    /// que a sessão já sabe, e uma versão dita a cada turno é ruído.
    #[test]
    fn an_unchanged_build_is_not_worth_a_line() {
        let mut c = ctx(&[]);
        c.resumed = true;
        let prompt = chat_prompt(&c, "hey");
        assert!(!prompt.contains("updated itself"), "prompt foi: {prompt}");
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

    /// The Director read the mirror's own log — commits made under an older
    /// mechanism sitting beside today's — and concluded incremental commits
    /// were possible: an unmeetable "commit as you go" gate landed in three
    /// cards, one got rejected on that false ground, and an agent was told
    /// twice it was wrong when it was right. The commit model has to be
    /// stated, not left to be guessed from history that mixes two eras.
    #[test]
    fn the_commit_model_is_stated_so_it_is_never_guessed_from_history() {
        let prompt = chat_prompt(&ctx(&[]), "hello");
        assert!(
            prompt.contains("exactly one commit"),
            "the one-run-one-commit model must be in the standing prompt: {prompt}"
        );
        assert!(
            prompt.contains("never ask an agent to commit as it goes or in steps"),
            "{prompt}"
        );
    }

    #[test]
    fn it_answers_rather_than_manufacturing_work() {
        let prompt = chat_prompt(&ctx(&[]), "what is a worktree?");
        assert!(prompt.contains("Answer the question that was asked"));
        assert!(prompt.contains("Only put work on a board when"));
    }

    /// The stop rule (#81). Two halves, and both have to be there: filing
    /// *before continuing* is what makes it happen at all, and saying whether
    /// the refusal was right is what keeps the inbox worth reading.
    #[test]
    fn a_refused_tool_is_archived_and_the_turn_carries_on() {
        let prompt = chat_prompt(&ctx(&[]), "read the run log");
        assert!(prompt.contains("that is a finding about the app"), "{prompt}");
        assert!(prompt.contains("propose_improvement"));
        assert!(prompt.contains("whether the refusal was right or is a defect"));
        // Filing happens beside the work, not in place of it. The wording used
        // to be "Before you continue", which put the proposal on the critical
        // path of every obstacle; the turn is what matters, the filing rides
        // along with it.
        assert!(prompt.contains("not instead of continuing"), "{prompt}");
    }

    /// The rule that made a capable model less useful than it is on its own.
    ///
    /// The stop belongs to one boundary — the operator's board — and to
    /// nothing else. A guard, a missing binary, a failed command are obstacles
    /// to get past, not reasons to hand the turn back. This pins both halves,
    /// because deleting the first would reopen the door this rule was written
    /// to shut: card-work done as loose files, with no review and no history.
    #[test]
    fn the_stop_is_scoped_to_the_board_and_nowhere_else() {
        let prompt = chat_prompt(&ctx(&[]), "the build is broken");
        // The door that stays shut.
        assert!(prompt.contains("not that you write the files yourself instead"), "{prompt}");
        assert!(prompt.contains("no review, no history and no cost"), "{prompt}");
        // The licence that replaces the blanket stop.
        assert!(prompt.contains("Every other obstacle is yours to get past"), "{prompt}");
        assert!(
            prompt.contains("Do not hand back a turn because the first attempt failed"),
            "{prompt}"
        );
        // And the blanket version is gone.
        assert!(!prompt.contains("say so plainly and stop"), "the blanket stop came back");
        assert!(!prompt.contains("never route around it"), "the blanket stop came back");
    }

    /// One rule was added, not a posture rewrite: the two the operator ruled
    /// out stay out. A prompt that grows loses adherence to what is already in
    /// it (#75).
    #[test]
    fn no_rule_arrived_beside_the_stop_rule() {
        let prompt = chat_prompt(&ctx(&[]), "hello");
        for absent in ["over-explor", "how long it will take", "time estimate"] {
            assert!(!prompt.contains(absent), "a rule crept in: {absent}");
        }
    }

    #[test]
    fn it_is_not_framed_as_a_software_manager() {
        let prompt = chat_prompt(&ctx(&[]), "hello");
        for gone in [
            "You are the Director of Relay, a desktop tool",
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
        assert!(!prompt.contains("Relay is where this conversation lives"));
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
    fn new_builds_get_proposed_their_own_project() {
        // The c_19a1 lesson: a site born inside the workspace repo because the
        // open project was assumed. The prompt must propose, not assume.
        let projects = vec![ProjectBrief {
            id: "harness".into(),
            name: "harness".into(),
            path: "C:/src/harness".into(),
            active: true,
            cards: vec![],
            charter: None,
        }];
        let mut c = ctx(&projects);
        c.repo = Some("harness");
        let prompt = chat_prompt(&c, "build me a portfolio site");
        assert!(prompt.contains("ask whether the work belongs to the project that is open"));
        assert!(prompt.contains("propose one with create_project"));
    }

    #[test]
    fn the_review_posture_is_what_makes_approval_mean_something() {
        let projects = vec![ProjectBrief {
            id: "harness".into(),
            name: "harness".into(),
            path: "C:/src/harness".into(),
            active: true,
            cards: vec![],
            charter: None,
        }];
        let prompt = chat_prompt(&ctx(&projects), "review c_x");
        for posture in [
            "Verify instead of believing",
            "Distinguish designed from done",
            "Say what is missing unasked",
            "Lead with damage",
            "Admit mistakes before moving on",
            "Never approve silently",
            "Write decisions when they happen",
        ] {
            assert!(prompt.contains(posture), "missing: {posture}");
        }
    }

    /// He can only hold the posture if he can see his own history — and the
    /// standing prompt must not schedule anything: he has no clock. The
    /// end-of-day ritual belongs to the app (daily_look_prompt, fired at
    /// shutdown); here only the ability and the guardrail.
    #[test]
    fn the_mirror_is_offered_without_claiming_a_time_of_day() {
        let projects = vec![ProjectBrief {
            id: "harness".into(),
            name: "harness".into(),
            path: "C:/src/harness".into(),
            active: true,
            cards: vec![],
            charter: None,
        }];
        let prompt = chat_prompt(&ctx(&projects), "anything to note?");
        assert!(prompt.contains("self_report counts what went wrong"));
        assert!(prompt.contains("read_docs opens DEBT.md and DECISIONS.md"));
        assert!(prompt.contains("propose_improvement"));
        assert!(prompt.contains("never a card created on your own"));
        // No clock in the standing prompt: a chat turn can happen at any hour,
        // and "end of day" would either confuse him or invite him to invent
        // the ritual himself mid-conversation.
        for clock in ["day's close", "end of day", "at day", "closing time"] {
            assert!(!prompt.to_lowercase().contains(clock), "time claim: {clock}");
        }

        // A specialist without delegation does not get the mirror either.
        let mut bare = ctx(&[]);
        bare.speaker = Speaker {
            is_director: false,
            can_delegate: false,
            ..director()
        };
        assert!(!chat_prompt(&bare, "hi").contains("self_report"));
    }

    #[test]
    fn the_daily_look_is_looking_not_talking() {
        let prompt = daily_look_prompt(None);
        assert!(prompt.contains("end-of-day look"));
        assert!(prompt.contains("self_report for the last 7 days"));
        assert!(prompt.contains("read_docs (doc \"debt\")"), "check DEBT before proposing");
        assert!(prompt.contains("propose_improvement"));
        assert!(prompt.contains("Do not create cards"));
        assert!(prompt.contains("one rough day is weather, not a pattern"));
    }

    /// Code that changed without a card is handed to him, in both places the
    /// look runs — and he is told to flag it, never to act on it.
    #[test]
    fn work_that_bypassed_the_board_reaches_him_as_a_fact() {
        let said = crate::mirror::describe(
            &crate::mirror::outside_work(&[(
                1_800_000_000_000 - 86_400_000,
                vec!["src/App.tsx".into()],
            )])
            .unwrap(),
            1_800_000_000_000,
        );

        let mut c = ctx(&[]);
        c.outside_work = Some(&said);
        let chat = chat_prompt(&c, "what should I look at?");
        assert!(chat.contains("Since Relay last looked at its own repository"), "{chat}");
        assert!(chat.contains("src/App.tsx"));
        assert!(chat.contains("do not close a card"), "flag, never act");

        let look = daily_look_prompt(Some(&said));
        assert!(look.contains("Also, since you last looked"), "{look}");
        assert!(look.contains("src/App.tsx"));

        // The normal case is silence: nothing came in outside the board, so
        // nothing is said about it.
        assert!(!chat_prompt(&ctx(&[]), "hi").contains("its own repository:"));
        assert!(!daily_look_prompt(None).contains("Also, since you last looked"));
        assert!(!daily_look_prompt(Some("   ")).contains("Also, since you last looked"));
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
                hunks: Vec::new(),
            }),
            hunk_verdicts: Vec::new(),
            session_id: None,
            worktree: None,
            branch: None,
            depends_on: Vec::new(),
            budget_paused: false,
            finished_ms: None,
        };
        let line = CardLine::from_card(&card);
        assert_eq!(line.id, "c_7");
        assert_eq!(line.agent_id, "scout");
        assert_eq!(line.review, Some((true, "scoped".to_string())));
    }

    /// O motor lê o comando à cabeça do prompt. Tudo o que o `chat_prompt`
    /// põe antes da mensagem — os quadros, a identidade, a nota de versão —
    /// enterrava-o, e o que chegava ao modelo era um parágrafo a acabar em
    /// `/usage`. Era por isto que escrever `/` no compositor não fazia nada.
    #[test]
    fn a_slash_command_goes_alone_and_nothing_is_put_before_it() {
        let projects = [ProjectBrief {
            id: "relay".into(),
            name: "Relay".into(),
            path: "/tmp/relay".into(),
            active: true,
            charter: None,
            cards: vec![card("c_1", "alguma coisa", Status::Running)],
        }];
        let prompt = chat_prompt(&ctx(&projects), "/usage");
        assert_eq!(prompt, "/usage");
    }

    #[test]
    fn a_command_keeps_its_arguments() {
        assert_eq!(slash_command("/model opus"), Some("/model opus"));
        assert_eq!(chat_prompt(&ctx(&[]), "  /compact tudo menos o diff  "), "/compact tudo menos o diff");
    }

    /// Uma barra dupla é como se diz uma linha que começa mesmo por barra, e
    /// um caminho não é um comando. Comer qualquer um deles seria trocar uma
    /// mensagem do operador por outra coisa.
    #[test]
    fn a_path_and_an_escaped_slash_are_not_commands() {
        assert_eq!(slash_command("/etc/hosts está errado"), None);
        assert_eq!(slash_command("//usage"), None);
        assert_eq!(slash_command("o que faz o /usage?"), None);
        assert_eq!(slash_command("/"), None);
        let prompt = chat_prompt(&ctx(&[]), "/etc/hosts está errado");
        assert!(prompt.contains("/etc/hosts"), "a mensagem tem de continuar a ser uma mensagem");
        assert!(prompt.len() > "/etc/hosts está errado".len());
    }

    #[test]
    fn attachments_are_named_in_the_operators_own_turn() {
        let one = with_attachments("look at this", &["C:/tmp/shot.png".into()]);
        assert!(one.starts_with("look at this\n\n"));
        assert!(one.contains("Attached file, on this machine"));
        assert!(one.ends_with("- C:/tmp/shot.png"));

        let many = with_attachments(
            "compare",
            &["C:/a.md".into(), "  ".into(), "C:/b.md".into()],
        );
        assert!(many.contains("Attached files"));
        assert!(many.contains("- C:/a.md\n- C:/b.md"));

        // Nothing attached leaves the message exactly as typed.
        assert_eq!(with_attachments(" hello ", &[]), "hello");
        // A message can be nothing but files.
        assert!(with_attachments("", &["C:/a.md".into()]).starts_with("Attached file"));
    }
}

#[cfg(test)]
mod proactive_tests {
    use super::*;

    fn projects() -> Vec<ProjectBrief> {
        vec![ProjectBrief {
            id: "signal".into(),
            name: "signal".into(),
            path: "/src/signal".into(),
            active: true,
            cards: vec![],
            charter: None,
        }]
    }

    fn director(projects: &[ProjectBrief]) -> ChatContext<'_> {
        let mut c = tests::ctx(projects);
        c.speaker.is_director = true;
        c.speaker.can_delegate = true;
        c
    }

    /// Regressão de comportamento, com o transcript de 2026-09-02 por prova.
    ///
    /// Perguntado "isto precisa de uma experiência, qual seria um bom começo —
    /// Remotion ou outra coisa?", a resposta foi *"Take your time. Nothing's
    /// running… I'll write card 1 whenever you say."* — e a verificação da
    /// licença que resolveu a pergunta só aconteceu depois de o operador
    /// escrever "hm".
    ///
    /// O que a produzia era "say what you are about to do before you do it",
    /// que se lê como anunciar e esperar.
    #[test]
    fn the_prompt_no_longer_tells_him_to_announce_and_wait() {
        let p = projects();
        let prompt = chat_prompt(&director(&p), "this needs an experiment, where do we start?");
        assert!(
            !prompt.contains("say what you are about to do before you do it"),
            "a frase que produziu o adiamento voltou ao prompt",
        );
    }

    #[test]
    fn pointing_at_something_is_the_instruction() {
        let p = projects();
        let prompt = chat_prompt(&director(&p), "look into remotion");
        assert!(prompt.contains("Being pointed at something is the instruction."), "{prompt}");
        assert!(prompt.contains("means run the experiment"));
    }

    /// A redacção de âmbito é a que a Anthropic mediu para este modelo: reduziu
    /// mudanças de âmbito a quase zero **sem** gerar perguntas de esclarecimento
    /// a mais. É essa a troca que este prompt vinha a perder — e é por isso que
    /// se usa a dela e não uma escrita à mão.
    #[test]
    fn the_tested_scope_wording_is_the_one_that_ships() {
        let p = projects();
        let prompt = chat_prompt(&director(&p), "build me a thing");
        for phrase in [
            "at the scope they intended",
            "check in only when",
            "keep going with the task as asked",
            "Finish the whole task",
        ] {
            assert!(prompt.contains(phrase), "falta a cláusula: {phrase}");
        }
    }

    #[test]
    fn only_four_things_stop_him() {
        let p = projects();
        let prompt = chat_prompt(&director(&p), "go");
        assert!(prompt.contains("Four things stop you"), "{prompt}");
        for stop in ["money above what was agreed", "destructive or", "a grant"] {
            assert!(prompt.contains(stop), "falta: {stop}");
        }
        assert!(prompt.contains("do it and say what you did"));
    }

    /// Este modelo escreve mais do que os anteriores e o `effort` não é a
    /// alavanca — uma instrução curta de concisão é. O operador chamou-lhe
    /// "blah blah blah".
    #[test]
    fn length_is_addressed_because_effort_does_not_address_it() {
        let p = projects();
        let prompt = chat_prompt(&director(&p), "hi");
        assert!(prompt.contains("focused, brief and concise"), "{prompt}");
    }

    /// Este modelo procura subagentes por iniciativa própria — mudança de
    /// direcção face ao anterior, que era preciso empurrar. Nos logs: nove
    /// numa conversa, quatro deles abertos *por* subagentes.
    #[test]
    fn delegation_is_capped_for_whoever_can_delegate() {
        let p = projects();
        let prompt = chat_prompt(&director(&p), "research this");
        assert!(prompt.contains("Subagents multiply cost and time"), "{prompt}");
        assert!(prompt.contains("never to review or double-check your own work"));

        // Quem não pode delegar não leva um parágrafo sobre delegar.
        let mut lone = director(&p);
        lone.speaker.can_delegate = false;
        assert!(!chat_prompt(&lone, "research this").contains("Subagents multiply"));
    }

    /// A outra metade: as regras já assentes chegam ao turno. O operador ditou
    /// "verified work proceeds without asking", o `record_decision` escreveu-a,
    /// e nada a lia — ver `memory::decisions_from`.
    #[test]
    fn recorded_decisions_reach_the_turn_they_were_written_for() {
        let p = projects();
        let mut c = director(&p);
        c.decisions = "# Verified work proceeds without asking\n\nVerifying IS the decision.";
        let prompt = chat_prompt(&c, "the card is in review");
        assert!(prompt.contains("Decisions already recorded on this project"));
        assert!(prompt.contains("Verifying IS the decision."));

        let quiet = chat_prompt(&director(&p), "hello");
        assert!(!quiet.contains("Decisions already recorded"));
    }

    /// O guarda contra o prompt voltar a acumular o que a auditoria tirou:
    /// linguagem de pressão e andaimes que este modelo já não precisa.
    #[test]
    fn the_prompt_stays_clean_of_dated_patterns() {
        let p = projects();
        let prompt = chat_prompt(&director(&p), "anything");
        for dated in [
            "think step by step",
            "<scratchpad>",
            "double-check your answer",
            "Be thorough",
            "Do not be lazy",
        ] {
            assert!(!prompt.contains(dated), "andaime datado de volta no prompt: {dated}");
        }
        // Ênfase gritada: nenhuma. Uma instrução que precisa de maiúsculas para
        // ser ouvida está a competir com as outras, e este modelo ouve todas.
        for shouted in ["MUST ", "NEVER ", "ALWAYS ", "CRITICAL", "IMPORTANT:"] {
            assert!(!prompt.contains(shouted), "linguagem de pressão: {shouted}");
        }
    }
}
