use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct CardId(String);

impl CardId {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CardId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct RunId(pub String);

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Backlog,
    Ready,
    Running,
    Review,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IllegalMove {
    pub from: Status,
    pub to: Status,
}

impl fmt::Display for IllegalMove {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "illegal move: {:?} -> {:?}", self.from, self.to)
    }
}

impl std::error::Error for IllegalMove {}

impl Status {
    pub const LEGAL_MOVES: &[(Status, Status)] = &[
        (Status::Backlog, Status::Ready),
        (Status::Ready, Status::Backlog),
        (Status::Ready, Status::Running),
        (Status::Running, Status::Ready),
        (Status::Running, Status::Review),
        (Status::Review, Status::Ready),
        (Status::Review, Status::Done),
    ];

    pub fn can_move_to(self, to: Status) -> bool {
        Self::LEGAL_MOVES
            .iter()
            .any(|&(from, dest)| from == self && dest == to)
    }

    pub fn move_to(self, to: Status) -> Result<Status, IllegalMove> {
        if self.can_move_to(to) {
            Ok(to)
        } else {
            Err(IllegalMove { from: self, to })
        }
    }
}

/// Who took a decision. Reviews can come from the Director or from the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    #[default]
    Human,
    Director,
}

/// One block of a card's diff, named the way git names it.
///
/// The header is the identity: git writes `@@ -14,6 +14,8 @@ fn resolve(` once
/// per block, and two blocks of the same file never share one. Carrying it
/// typed is what lets the log say *which* hunk was decided instead of a
/// sentence about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HunkRef {
    /// Path relative to the worktree.
    pub file: String,
    /// The `@@ … @@` line, verbatim.
    pub header: String,
    /// Where the block lands in the file after the change, and how many lines
    /// it covers there. Zero when the diff did not say; the header already
    /// holds the same numbers in text, so this is for reading, not identity.
    #[serde(default)]
    pub new_start: u32,
    #[serde(default)]
    pub new_lines: u32,
}

impl HunkRef {
    pub fn new(file: impl Into<String>, header: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            header: header.into(),
            new_start: 0,
            new_lines: 0,
        }
    }

    /// Two references point at the same block when the file and the header
    /// match. The line range is derived from the header, so comparing it too
    /// would only make identity fragile.
    pub fn names(&self, other: &HunkRef) -> bool {
        self.file == other.file && self.header == other.header
    }

    /// `crates/app/src/code.rs @@ -14,6 +14,8 @@` — the one-line form, so a
    /// reason the operator reads is built out of the typed fields rather than
    /// typed by hand somewhere else.
    pub fn label(&self) -> String {
        format!("{} {}", self.file, self.header.trim())
    }
}

/// One hunk-level verdict, kept on the card while its diff is being read.
///
/// Verdicts pile up as the operator works down the panel and are consumed the
/// moment the diff is fully decided — see [`resolve_review`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HunkVerdict {
    pub hunk: HunkRef,
    pub approved: bool,
    #[serde(default)]
    pub by: Actor,
    /// What the operator said about this block. May be empty on an approval.
    #[serde(default)]
    pub reason: String,
}

/// The last review a card received, kept on the card so the board can show it
/// without walking the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Review {
    pub by: Actor,
    pub approved: bool,
    pub reason: String,
    /// The blocks the verdict was about. Empty when the card was decided as a
    /// whole, which is what the Board's Approve and Send back always mean.
    #[serde(default)]
    pub hunks: Vec<HunkRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Card {
    pub id: CardId,
    pub title: String,
    pub status: Status,
    #[serde(default)]
    pub current_run: Option<RunId>,
    /// Agent profile that owns this card. Cards start on the default worker.
    #[serde(default = "default_agent")]
    pub agent_id: String,
    /// Everything spent on this card so far, across every run.
    #[serde(default)]
    pub cost_usd: f64,
    /// Model turns burned across every run.
    #[serde(default)]
    pub turns: u32,
    /// How many runs this card has had.
    #[serde(default)]
    pub runs: u32,
    #[serde(default)]
    pub last_review: Option<Review>,
    /// Hunk-level verdicts taken since the last run finished, in the order
    /// they were taken. Empty on every card that was decided whole. Cleared
    /// when the card is decided or a new run rewrites the diff underneath it.
    #[serde(default)]
    pub hunk_verdicts: Vec<HunkVerdict>,
    /// The native agent session this card's runs continue, once one has been
    /// reported. Kept on the card so it survives a restart: without it the next
    /// run starts a stranger on work it has already done.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Where the last run worked, and on which branch. Not derivable after the
    /// fact: the worktree mode comes from the agent profile at the moment the
    /// run starts, and that profile may have changed since.
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Cards that must be Done before this one may start. Order, not file
    /// conflict — the path guard knows nothing of it. A dependency that was
    /// discarded no longer blocks anybody.
    #[serde(default)]
    pub depends_on: Vec<CardId>,
    /// Cut by its own budget ceiling: the work is wip-committed, the session
    /// is saved, and continuing means raising the ceiling and pressing Start
    /// again. Distinct from Failed on purpose — money was spent, nothing broke.
    #[serde(default)]
    pub budget_paused: bool,
    /// When this card reached Done, taken from the log's own timestamp for the
    /// event that put it there.
    ///
    /// The domain has no clock, so this is the one field reduced from outside
    /// the event: [`Board::apply_at`] hands over the stored `ts_ms`. `None`
    /// covers three honest cases — the card is not Done, it left Done again,
    /// or the log that records it kept no timestamp (written before they were
    /// stored). A card that fell out of the activity window keeps its finish
    /// time all the same, which is the whole point of it living here.
    #[serde(default)]
    pub finished_ms: Option<u64>,
}

fn default_agent() -> String {
    "builder".to_string()
}

/// What the verdicts taken so far mean for the card as a whole.
///
/// See [`resolve_review`] for the rule; this is only the shape of its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewOutcome {
    /// Some block of the diff has no verdict yet, so the card stays in Review.
    Open { decided: u32, of: u32 },
    /// Every block passed.
    Approve { hunks: Vec<HunkRef> },
    /// Every block was sent back. The card itself is the carrier of the work,
    /// exactly as a whole-card rejection is.
    Reject { hunks: Vec<HunkRef>, reason: String },
    /// Some passed, some did not.
    Partial {
        approved: Vec<HunkRef>,
        rejected: Vec<HunkVerdict>,
    },
}

/// What a set of hunk verdicts means for the card, given the diff as git
/// prints it *now*.
///
/// The diff is the authority on which blocks exist: a verdict whose hunk is no
/// longer in it was taken against a diff that has since moved, and counting it
/// would let a stale opinion close a card. Such verdicts are ignored rather
/// than an error — the operator simply has that block to read again.
///
/// A card with no diff is never resolved here. Nothing was reviewed, so there
/// is nothing for the verdicts to mean; whole-card approve still applies.
pub fn resolve_review(verdicts: &[HunkVerdict], diff: &[HunkRef]) -> ReviewOutcome {
    let of = diff.len() as u32;
    if of == 0 {
        return ReviewOutcome::Open { decided: 0, of: 0 };
    }
    // Last verdict on a block wins: the operator may change their mind before
    // the diff is fully read, and the newer opinion is the one they hold.
    let verdict_for = |hunk: &HunkRef| verdicts.iter().rev().find(|v| v.hunk.names(hunk));

    let mut approved: Vec<HunkRef> = Vec::new();
    let mut rejected: Vec<HunkVerdict> = Vec::new();
    for hunk in diff {
        match verdict_for(hunk) {
            None => {
                return ReviewOutcome::Open {
                    decided: (approved.len() + rejected.len()) as u32,
                    of,
                }
            }
            Some(v) if v.approved => approved.push(hunk.clone()),
            Some(v) => rejected.push(HunkVerdict {
                hunk: hunk.clone(),
                ..v.clone()
            }),
        }
    }

    if rejected.is_empty() {
        return ReviewOutcome::Approve { hunks: approved };
    }
    if approved.is_empty() {
        return ReviewOutcome::Reject {
            reason: sent_back_reason(&rejected),
            hunks: rejected.into_iter().map(|v| v.hunk).collect(),
        };
    }
    ReviewOutcome::Partial { approved, rejected }
}

/// The prose form of a set of rejections, built from the typed verdicts so the
/// sentence in the log cannot drift from the blocks it names.
fn sent_back_reason(rejected: &[HunkVerdict]) -> String {
    let mut out = format!(
        "{} hunk{} sent back",
        rejected.len(),
        if rejected.len() == 1 { "" } else { "s" }
    );
    for verdict in rejected {
        out.push_str("\n- ");
        out.push_str(&verdict.hunk.label());
        if !verdict.reason.trim().is_empty() {
            out.push_str(" — ");
            out.push_str(verdict.reason.trim());
        }
    }
    out
}

/// The title — which is to say the prompt — of the card the rejected blocks
/// become when the rest of a diff is approved.
///
/// The first line is the one-line subject the board and the commit read; the
/// body names every block that was sent back and why, because that is the
/// whole content of the work being carried over.
pub fn follow_up_title(from: &Card, rejected: &[HunkVerdict]) -> String {
    format!(
        "Rework {} hunk{} sent back on {}\n\nApproved as {}, so the rest of that card has landed. \
         What is left is the work below.\n{}",
        rejected.len(),
        if rejected.len() == 1 { "" } else { "s" },
        from.subject(),
        from.id,
        rejected
            .iter()
            .map(|v| {
                let mut line = format!("- {}", v.hunk.label());
                if !v.reason.trim().is_empty() {
                    line.push_str("\n  ");
                    line.push_str(v.reason.trim());
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// The one-line form of a card's title: everything up to the first newline.
///
/// A title is the prompt the agent receives, so it may carry a body under the
/// request — an accepted proposal arrives with its observation and its
/// reasoning attached. The places that need exactly one line (a commit
/// subject, a board line inside a prompt) ask for it here rather than each
/// spilling the body into a place with no room for it.
pub fn one_line(title: &str) -> &str {
    title.trim().lines().next().unwrap_or("").trim_end()
}

impl Card {
    /// What this card is called in one line. See [`one_line`].
    pub fn subject(&self) -> &str {
        one_line(&self.title)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    CardCreated { card_id: CardId, title: String },
    /// The card's text was corrected before anything ran on it. Only ever
    /// emitted while `runs == 0`, so the log never contradicts the card.
    CardEdited { card_id: CardId, title: String },
    CardAssigned { card_id: CardId, agent_id: String },
    CardMoved { card_id: CardId, from: Status, to: Status },
    CardOverridden { card_id: CardId, from: Status, to: Status, reason: String },
    RunStarted {
        card_id: CardId,
        run_id: RunId,
        /// Where this run works. Absent in logs written before it was recorded.
        #[serde(default)]
        worktree: Option<String>,
        #[serde(default)]
        branch: Option<String>,
    },
    RunFinished {
        card_id: CardId,
        run_id: RunId,
        outcome: RunOutcome,
        #[serde(default)]
        cost_usd: Option<f64>,
        #[serde(default)]
        turns: Option<u32>,
    },
    CardApproved {
        card_id: CardId,
        #[serde(default)]
        by: Actor,
        #[serde(default)]
        reason: String,
        /// The blocks the approval was about. Empty on a whole-card approve,
        /// which is what every log written before hunk review holds.
        #[serde(default)]
        hunks: Vec<HunkRef>,
    },
    CardRejected {
        card_id: CardId,
        reason: String,
        #[serde(default)]
        by: Actor,
        /// The blocks that were sent back. Empty on a whole-card rejection.
        #[serde(default)]
        hunks: Vec<HunkRef>,
    },
    /// One block of a card's diff was decided. The card does not move: the
    /// verdicts sit on it until the diff is fully read, and the event that
    /// completes it carries the card's own outcome alongside.
    HunkReviewed {
        card_id: CardId,
        hunk: HunkRef,
        approved: bool,
        #[serde(default)]
        by: Actor,
        #[serde(default)]
        reason: String,
    },
    /// The agent reported the session it is working in. Recorded so a later run
    /// — or the operator, in a terminal — can resume that same conversation
    /// instead of starting over.
    AgentSession { card_id: CardId, session_id: String },
    /// The card is gone from the board. The log keeps the fact and the reason;
    /// the branch a run left behind is the shell's business to clean up.
    CardDiscarded {
        card_id: CardId,
        #[serde(default)]
        reason: String,
    },
    /// The cards a card waits for. Replaces whatever was set before.
    CardDependencies {
        card_id: CardId,
        #[serde(default)]
        depends_on: Vec<CardId>,
    },
    /// The budget-pause flag went up or down. Board-visible so the operator
    /// sees why Start refuses.
    BudgetPauseSet {
        card_id: CardId,
        #[serde(default)]
        paused: bool,
    },
    /// The whole board as it stands, written so the log can be compacted: a
    /// restart replays this one event instead of thousands. Written by the
    /// engine alone; no command produces it.
    BoardSnapshot {
        #[serde(default)]
        cards: Vec<Card>,
    },
    /// What the agent said about its own work, reported through the
    /// `report_work` tool while running. The summary feeds the commit body —
    /// the engine still owns the commit — and the notes are durable facts the
    /// Curator may promote later. Lives here, not in git: memory in the
    /// repository would mean one copy per worktree and write conflicts
    /// between concurrent cards, which is the worst place for one.
    WorkReported {
        card_id: CardId,
        #[serde(default)]
        summary: String,
        #[serde(default)]
        notes: Vec<String>,
    },
}

impl Event {
    pub fn card_id(&self) -> Option<&CardId> {
        match self {
            Event::CardCreated { card_id, .. }
            | Event::CardEdited { card_id, .. }
            | Event::CardAssigned { card_id, .. }
            | Event::CardMoved { card_id, .. }
            | Event::CardOverridden { card_id, .. }
            | Event::RunStarted { card_id, .. }
            | Event::RunFinished { card_id, .. }
            | Event::CardApproved { card_id, .. }
            | Event::CardRejected { card_id, .. }
            | Event::HunkReviewed { card_id, .. }
            | Event::AgentSession { card_id, .. }
            | Event::CardDiscarded { card_id, .. }
            | Event::CardDependencies { card_id, .. }
            | Event::BudgetPauseSet { card_id, .. }
            | Event::WorkReported { card_id, .. } => Some(card_id),
            // A snapshot is the board itself, not something that happened to
            // one card.
            Event::BoardSnapshot { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Command {
    CreateCard { card_id: CardId, title: String },
    /// Correct a card's text. The title is the prompt the agent receives and
    /// the subject of the commit it leaves behind, so a badly written one has
    /// to be fixable without discarding the card — discarding loses the id,
    /// the history, the session and every dependency pointing at it.
    ///
    /// Allowed only while `runs == 0`. After the first run the log, the
    /// transcript and the commit already say what the old title asked for;
    /// rewriting it then makes the record stop matching the card.
    EditCard { card_id: CardId, title: String },
    AssignAgent { card_id: CardId, agent_id: String },
    MoveCard { card_id: CardId, to: Status },
    OverrideCard { card_id: CardId, to: Status, reason: String },
    StartRun {
        card_id: CardId,
        run_id: RunId,
        /// Where the run will work, resolved before it starts.
        worktree: Option<String>,
        branch: Option<String>,
    },
    FinishRun {
        card_id: CardId,
        run_id: RunId,
        outcome: RunOutcome,
        cost_usd: Option<f64>,
        turns: Option<u32>,
    },
    /// Approve the card. `hunks` names the blocks the verdict was about and is
    /// a record, not a filter: a branch merges whole, so an approval always
    /// lands the whole card. Empty is the Board's Approve, unchanged.
    ApproveCard { card_id: CardId, by: Actor, reason: String, hunks: Vec<HunkRef> },
    /// Send the card back. `hunks` names the blocks that were wrong; empty is
    /// the Board's Send back, unchanged.
    RejectCard { card_id: CardId, reason: String, by: Actor, hunks: Vec<HunkRef> },
    /// Decide one block of a card's diff.
    ///
    /// `diff` is the card's blocks as git prints them at the moment the
    /// operator clicks, and it is what makes "every hunk is decided" a fact
    /// the domain can check rather than a guess. When this verdict completes
    /// the diff the card resolves in the same decision, so a replay of the
    /// log reaches the same board in one step.
    ///
    /// `follow_up` is the id the rejected work would be carried on if this
    /// verdict completes a partial rejection. It arrives from the caller
    /// rather than being minted here because the domain has no source of
    /// randomness and replay must land on the same id every time.
    ReviewHunk {
        card_id: CardId,
        hunk: HunkRef,
        approved: bool,
        by: Actor,
        reason: String,
        diff: Vec<HunkRef>,
        follow_up: CardId,
    },
    /// Remember the agent session a run is using.
    RecordSession { card_id: CardId, session_id: String },
    /// Take a card off the board for good.
    DiscardCard { card_id: CardId, reason: String },
    /// Say which cards must be Done before this one may start.
    SetDependencies { card_id: CardId, depends_on: Vec<CardId> },
    /// The agent's own account of finished work: summary for the commit body,
    /// notes for the memory layer. Calling again replaces — the last report
    /// of a run wins, never accumulates silently.
    ReportWork { card_id: CardId, summary: String, notes: Vec<String> },
    /// Mark (or clear) the budget-pause flag. Set when a run dies on its own
    /// ceiling; cleared by starting again with a raised one.
    SetBudgetPause { card_id: CardId, paused: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionError {
    CardNotFound(CardId),
    DuplicateCard(CardId),
    IllegalMove { from: Status, to: Status },
    SameStatus(Status),
    EmptyTitle,
    EmptyReason,
    EmptyAgent,
    EmptySession,
    NotReady(Status),
    NotRunning(Status),
    NotInReview(Status),
    RunMismatch,
    CannotOverrideToRunning,
    /// The card waits on others before a run may start: id and where each
    /// stands.
    DependenciesNotMet(Vec<(CardId, Status)>),
    DependencyCycle(CardId),
    /// `report_work` arrived with neither a summary nor a note.
    EmptyReport,
    /// The card is paused by its own budget ceiling; raise it to continue.
    BudgetPaused(CardId),
    /// The card has already run, so its text is no longer only a request: it
    /// is what the transcript and the commit answered.
    AlreadyRan { card_id: CardId, runs: u32 },
    /// The edit asked for the title the card already has.
    SameTitle,
    /// A verdict named a block that is not in the diff it was taken against.
    UnknownHunk(String),
}

impl fmt::Display for DecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecisionError::CardNotFound(id) => write!(f, "card '{id}' not found"),
            DecisionError::DuplicateCard(id) => write!(f, "card '{id}' already exists"),
            DecisionError::IllegalMove { from, to } => {
                write!(f, "illegal move: {from:?} -> {to:?}")
            }
            DecisionError::SameStatus(status) => write!(f, "card already in {status:?}"),
            DecisionError::EmptyTitle => write!(f, "title cannot be empty"),
            DecisionError::EmptyReason => write!(f, "override requires a non-empty reason"),
            DecisionError::EmptyAgent => write!(f, "agent id cannot be empty"),
            DecisionError::EmptySession => write!(f, "session id cannot be empty"),
            DecisionError::NotReady(status) => {
                write!(f, "card must be Ready to start a run (is {status:?})")
            }
            DecisionError::NotRunning(status) => {
                write!(f, "card is not Running (is {status:?})")
            }
            DecisionError::NotInReview(status) => {
                write!(f, "card is not in Review (is {status:?})")
            }
            DecisionError::RunMismatch => write!(f, "run id does not match the active run"),
            DecisionError::CannotOverrideToRunning => {
                write!(f, "only starting a run puts a card in Running")
            }
            DecisionError::DependenciesNotMet(blocked) => {
                let list = blocked
                    .iter()
                    .map(|(id, status)| format!("{id} ({status:?})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "waiting on: {list}")
            }
            DecisionError::DependencyCycle(id) => {
                write!(f, "these dependencies would make a cycle through {id}")
            }
            DecisionError::EmptyReport => {
                write!(f, "report_work needs a summary or at least one memory note")
            }
            DecisionError::BudgetPaused(id) => {
                write!(
                    f,
                    "{id} is paused by its budget ceiling — raise the agent's budget, then press Start to continue from the saved session"
                )
            }
            DecisionError::AlreadyRan { card_id, runs } => write!(
                f,
                "{card_id} has already run {runs} time(s): its title is what the transcript and \
                 the commit answered, so changing it now would make the record stop matching the \
                 card. Say what should be different instead, or take a new card for the rest"
            ),
            DecisionError::SameTitle => write!(f, "that is the title the card already has"),
            DecisionError::UnknownHunk(label) => write!(
                f,
                "{label} is not in this card's diff any more — read it again before deciding it"
            ),
        }
    }
}

impl std::error::Error for DecisionError {}

#[derive(Debug, Clone, Default)]
pub struct Board {
    cards: HashMap<CardId, Card>,
}

impl Board {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &CardId) -> Option<&Card> {
        self.cards.get(id)
    }

    pub fn cards(&self) -> Vec<&Card> {
        let mut v: Vec<&Card> = self.cards.values().collect();
        v.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
        v
    }

    /// Fold an event whose timestamp is not known — a caller replaying a log
    /// that kept none, or a test that has no clock. See [`Board::apply_at`],
    /// which this is the zero-timestamp case of.
    pub fn apply(&mut self, event: &Event) {
        self.apply_at(event, 0);
    }

    /// Fold an event together with the moment the log recorded it.
    ///
    /// The domain owns no clock, so a time can only ever arrive from the
    /// record. `ts_ms` is the stored timestamp; zero means the record kept
    /// none, and nothing that needs a time is set from it. Replay is
    /// deterministic because the timestamp is stored beside the event, never
    /// read from the machine doing the replaying.
    pub fn apply_at(&mut self, event: &Event, ts_ms: u64) {
        // Whether this card was already finished, read before the fold so the
        // crossing can be seen afterwards.
        let was_done = event
            .card_id()
            .and_then(|id| self.cards.get(id))
            .map(|card| card.status == Status::Done);

        match event {
            Event::CardCreated { card_id, title } => {
                self.cards.insert(
                    card_id.clone(),
                    Card {
                        id: card_id.clone(),
                        title: title.clone(),
                        status: Status::Backlog,
                        current_run: None,
                        agent_id: default_agent(),
                        cost_usd: 0.0,
                        turns: 0,
                        runs: 0,
                        last_review: None,
                        hunk_verdicts: Vec::new(),
                        session_id: None,
                        worktree: None,
                        branch: None,
                        depends_on: Vec::new(),
                        budget_paused: false,
                        finished_ms: None,
                    },
                );
            }
            Event::CardEdited { card_id, title } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.title = title.clone();
                }
            }
            Event::CardAssigned { card_id, agent_id } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.agent_id = agent_id.clone();
                }
            }
            Event::CardMoved { card_id, to, .. } | Event::CardOverridden { card_id, to, .. } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = *to;
                }
            }
            Event::RunStarted {
                card_id,
                run_id,
                worktree,
                branch,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = Status::Running;
                    card.current_run = Some(run_id.clone());
                    card.runs += 1;
                    card.last_review = None;
                    // A new run rewrites the diff, so every verdict taken
                    // against the old one is about blocks that no longer
                    // exist. Keeping them would let a stale opinion close the
                    // card the moment the next review begins.
                    card.hunk_verdicts.clear();
                    // Where it works can move between runs; the session it
                    // continues is left alone, because a new run resumes the
                    // one the last run left behind.
                    if worktree.is_some() {
                        card.worktree = worktree.clone();
                        card.branch = branch.clone();
                    }
                }
            }
            Event::AgentSession { card_id, session_id } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.session_id = Some(session_id.clone());
                }
            }
            Event::RunFinished {
                card_id,
                outcome,
                cost_usd,
                turns,
                ..
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = match outcome {
                        RunOutcome::Completed => Status::Review,
                        RunOutcome::Cancelled | RunOutcome::Failed => Status::Ready,
                    };
                    card.current_run = None;
                    card.cost_usd += cost_usd.unwrap_or(0.0);
                    card.turns += turns.unwrap_or(0);
                }
            }
            Event::CardApproved {
                card_id,
                by,
                reason,
                hunks,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = Status::Done;
                    card.last_review = Some(Review {
                        by: *by,
                        approved: true,
                        reason: reason.clone(),
                        hunks: hunks.clone(),
                    });
                    // The verdicts are spent: they are in this event and in
                    // whatever card carries the rest of the work.
                    card.hunk_verdicts.clear();
                }
            }
            Event::CardRejected {
                card_id,
                reason,
                by,
                hunks,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = Status::Ready;
                    card.last_review = Some(Review {
                        by: *by,
                        approved: false,
                        reason: reason.clone(),
                        hunks: hunks.clone(),
                    });
                    card.hunk_verdicts.clear();
                }
            }
            Event::HunkReviewed {
                card_id,
                hunk,
                approved,
                by,
                reason,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    let verdict = HunkVerdict {
                        hunk: hunk.clone(),
                        approved: *approved,
                        by: *by,
                        reason: reason.clone(),
                    };
                    // Deciding the same block twice replaces: the operator
                    // changed their mind, they did not vote twice.
                    match card.hunk_verdicts.iter_mut().find(|v| v.hunk.names(hunk)) {
                        Some(slot) => *slot = verdict,
                        None => card.hunk_verdicts.push(verdict),
                    }
                }
            }
            Event::CardDiscarded { card_id, .. } => {
                self.cards.remove(card_id);
            }
            Event::CardDependencies {
                card_id,
                depends_on,
            } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.depends_on = depends_on.clone();
                }
            }
            // A snapshot is the board as it stood: whatever came before is
            // folded into it, so replay starts here.
            Event::BoardSnapshot { cards } => {
                self.cards.clear();
                for card in cards {
                    self.cards.insert(card.id.clone(), card.clone());
                }
            }
            // The agent's report changes no board state: the summary is read
            // at commit time from the run, the notes belong to the memory
            // layer. The log keeps both so a restart keeps them too.
            Event::WorkReported { .. } => {}
            Event::BudgetPauseSet { card_id, paused } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.budget_paused = *paused;
                }
            }
        }

        // Done is the one status that carries a time, and it is set here
        // rather than inside the approve arm so that every door into Done —
        // the operator's, the Director's, the auto-approve when an agent has
        // no reviewer, and any future one — records it the same way. A card
        // sent back out of Done drops it: a finish time on a card that is not
        // finished is worse than none.
        //
        // A `BoardSnapshot` names no card and never reaches this, which is
        // what keeps a compacted log's finish times intact.
        if let Some(card_id) = event.card_id() {
            if let Some(card) = self.cards.get_mut(card_id) {
                match (was_done, card.status == Status::Done) {
                    (Some(false), true) => card.finished_ms = (ts_ms > 0).then_some(ts_ms),
                    (_, false) => card.finished_ms = None,
                    _ => {}
                }
            }
        }
    }

    pub fn decide(&self, cmd: &Command) -> Result<Vec<Event>, DecisionError> {
        match cmd {
            Command::CreateCard { card_id, title } => {
                let trimmed = title.trim();
                if trimmed.is_empty() {
                    return Err(DecisionError::EmptyTitle);
                }
                if self.cards.contains_key(card_id) {
                    return Err(DecisionError::DuplicateCard(card_id.clone()));
                }
                Ok(vec![Event::CardCreated {
                    card_id: card_id.clone(),
                    title: trimmed.to_string(),
                }])
            }
            Command::EditCard { card_id, title } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                let trimmed = title.trim();
                if trimmed.is_empty() {
                    return Err(DecisionError::EmptyTitle);
                }
                // The line that makes this safe: a card that has run has a
                // transcript and a commit whose subject is the old title. The
                // count covers Running too — `runs` goes up when a run starts,
                // not when it ends.
                if card.runs > 0 {
                    return Err(DecisionError::AlreadyRan {
                        card_id: card_id.clone(),
                        runs: card.runs,
                    });
                }
                if trimmed == card.title {
                    return Err(DecisionError::SameTitle);
                }
                Ok(vec![Event::CardEdited {
                    card_id: card_id.clone(),
                    title: trimmed.to_string(),
                }])
            }
            Command::DiscardCard { card_id, reason } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                // A running card has a live agent and an open worktree: stop it
                // first, so nothing is deleted from under a process.
                if card.status == Status::Running {
                    return Err(DecisionError::NotRunning(card.status));
                }
                // Cards waiting on this one are freed by the discard — that is
                // the rule, so it goes in the record where the operator will
                // see it rather than happening silently.
                let mut freed: Vec<String> = self
                    .cards
                    .values()
                    .filter(|c| c.depends_on.contains(card_id))
                    .map(|c| c.id.to_string())
                    .collect();
                freed.sort();
                let reason = if freed.is_empty() {
                    reason.trim().to_string()
                } else {
                    format!("{}; frees {}", reason.trim(), freed.join(", "))
                };
                Ok(vec![Event::CardDiscarded {
                    card_id: card_id.clone(),
                    reason,
                }])
            }
            Command::SetDependencies { card_id, depends_on } => {
                if !self.cards.contains_key(card_id) {
                    return Err(DecisionError::CardNotFound(card_id.clone()));
                }
                for dep in depends_on {
                    if *dep == *card_id {
                        return Err(DecisionError::DependencyCycle(card_id.clone()));
                    }
                    if !self.cards.contains_key(dep) {
                        return Err(DecisionError::CardNotFound(dep.clone()));
                    }
                }
                // Following the edges from each dependency must never come
                // back to the card itself.
                let mut stack: Vec<CardId> = depends_on.clone();
                let mut seen: std::collections::HashSet<CardId> =
                    std::iter::once(card_id.clone()).collect();
                while let Some(current) = stack.pop() {
                    if !seen.insert(current.clone()) {
                        continue;
                    }
                    if let Some(node) = self.cards.get(&current) {
                        if node.depends_on.contains(card_id) {
                            return Err(DecisionError::DependencyCycle(card_id.clone()));
                        }
                        stack.extend(node.depends_on.iter().cloned());
                    }
                }
                Ok(vec![Event::CardDependencies {
                    card_id: card_id.clone(),
                    depends_on: depends_on.clone(),
                }])
            }
            Command::AssignAgent { card_id, agent_id } => {
                if !self.cards.contains_key(card_id) {
                    return Err(DecisionError::CardNotFound(card_id.clone()));
                }
                let trimmed = agent_id.trim();
                if trimmed.is_empty() {
                    return Err(DecisionError::EmptyAgent);
                }
                Ok(vec![Event::CardAssigned {
                    card_id: card_id.clone(),
                    agent_id: trimmed.to_string(),
                }])
            }
            Command::MoveCard { card_id, to } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if card.status == *to {
                    return Err(DecisionError::SameStatus(card.status));
                }
                card.status
                    .move_to(*to)
                    .map_err(|e| DecisionError::IllegalMove { from: e.from, to: e.to })?;
                Ok(vec![Event::CardMoved {
                    card_id: card_id.clone(),
                    from: card.status,
                    to: *to,
                }])
            }
            Command::OverrideCard { card_id, to, reason } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if reason.trim().is_empty() {
                    return Err(DecisionError::EmptyReason);
                }
                if card.status == *to {
                    return Err(DecisionError::SameStatus(card.status));
                }
                // An override straight to Running would produce a card that is
                // running nothing: no current_run, no worktree, and FinishRun,
                // StartRun and DiscardCard all refuse it. Only a real run
                // starts one.
                if *to == Status::Running {
                    return Err(DecisionError::CannotOverrideToRunning);
                }
                Ok(vec![Event::CardOverridden {
                    card_id: card_id.clone(),
                    from: card.status,
                    to: *to,
                    reason: reason.trim().to_string(),
                }])
            }
            Command::StartRun {
                card_id,
                run_id,
                worktree,
                branch,
            } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if card.status != Status::Ready {
                    return Err(DecisionError::NotReady(card.status));
                }
                // A card paused by its own budget ceiling stays put until the
                // operator raises it: starting blind is how c_19a1 burned the
                // same quota twice. The engine clears the flag when the new
                // run's ceiling clears what was already spent.
                if card.budget_paused {
                    return Err(DecisionError::BudgetPaused(card_id.clone()));
                }
                // Order the Director asked for: everything this card waits on
                // must be finished. A dependency that was discarded no longer
                // exists, so it cannot block anybody.
                let blocked: Vec<(CardId, Status)> = card
                    .depends_on
                    .iter()
                    .filter_map(|dep| self.cards.get(dep).map(|c| (c.id.clone(), c.status)))
                    .filter(|(_, status)| *status != Status::Done)
                    .collect();
                if !blocked.is_empty() {
                    return Err(DecisionError::DependenciesNotMet(blocked));
                }
                Ok(vec![Event::RunStarted {
                    card_id: card_id.clone(),
                    run_id: run_id.clone(),
                    worktree: worktree.clone(),
                    branch: branch.clone(),
                }])
            }
            Command::SetBudgetPause { card_id, paused } => {
                if !self.cards.contains_key(card_id) {
                    return Err(DecisionError::CardNotFound(card_id.clone()));
                }
                Ok(vec![Event::BudgetPauseSet {
                    card_id: card_id.clone(),
                    paused: *paused,
                }])
            }
            Command::ReportWork { card_id, summary, notes } => {                if !self.cards.contains_key(card_id) {
                    return Err(DecisionError::CardNotFound(card_id.clone()));
                }
                if summary.trim().is_empty() && notes.iter().all(|n| n.trim().is_empty()) {
                    return Err(DecisionError::EmptyReport);
                }
                Ok(vec![Event::WorkReported {
                    card_id: card_id.clone(),
                    summary: summary.trim().to_string(),
                    notes: notes
                        .iter()
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty())
                        .collect(),
                }])
            }
            Command::RecordSession { card_id, session_id } => {
                if !self.cards.contains_key(card_id) {
                    return Err(DecisionError::CardNotFound(card_id.clone()));
                }
                if session_id.trim().is_empty() {
                    return Err(DecisionError::EmptySession);
                }
                Ok(vec![Event::AgentSession {
                    card_id: card_id.clone(),
                    session_id: session_id.trim().to_string(),
                }])
            }
            Command::FinishRun {
                card_id,
                run_id,
                outcome,
                cost_usd,
                turns,
            } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if card.status != Status::Running {
                    return Err(DecisionError::NotRunning(card.status));
                }
                if card.current_run.as_ref() != Some(run_id) {
                    return Err(DecisionError::RunMismatch);
                }
                Ok(vec![Event::RunFinished {
                    card_id: card_id.clone(),
                    run_id: run_id.clone(),
                    outcome: outcome.clone(),
                    cost_usd: *cost_usd,
                    turns: *turns,
                }])
            }
            Command::ApproveCard { card_id, by, reason, hunks } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if card.status != Status::Review {
                    return Err(DecisionError::NotInReview(card.status));
                }
                Ok(vec![Event::CardApproved {
                    card_id: card_id.clone(),
                    by: *by,
                    reason: reason.trim().to_string(),
                    hunks: hunks.clone(),
                }])
            }
            Command::RejectCard { card_id, reason, by, hunks } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if card.status != Status::Review {
                    return Err(DecisionError::NotInReview(card.status));
                }
                if reason.trim().is_empty() {
                    return Err(DecisionError::EmptyReason);
                }
                Ok(vec![Event::CardRejected {
                    card_id: card_id.clone(),
                    reason: reason.trim().to_string(),
                    by: *by,
                    hunks: hunks.clone(),
                }])
            }
            Command::ReviewHunk {
                card_id,
                hunk,
                approved,
                by,
                reason,
                diff,
                follow_up,
            } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if card.status != Status::Review {
                    return Err(DecisionError::NotInReview(card.status));
                }
                // Deciding a block the diff does not hold means the operator
                // is looking at a stale panel. Refusing is the only honest
                // answer: the record would otherwise name a hunk nobody can
                // find.
                if !diff.iter().any(|h| h.names(hunk)) {
                    return Err(DecisionError::UnknownHunk(hunk.label()));
                }

                let verdict = Event::HunkReviewed {
                    card_id: card_id.clone(),
                    hunk: hunk.clone(),
                    approved: *approved,
                    by: *by,
                    reason: reason.trim().to_string(),
                };

                // What the card's verdicts will be once this one is folded in.
                let mut taken = card.hunk_verdicts.clone();
                let next = HunkVerdict {
                    hunk: hunk.clone(),
                    approved: *approved,
                    by: *by,
                    reason: reason.trim().to_string(),
                };
                match taken.iter_mut().find(|v| v.hunk.names(hunk)) {
                    Some(slot) => *slot = next,
                    None => taken.push(next),
                }

                let mut events = vec![verdict];
                match resolve_review(&taken, diff) {
                    // Still blocks to read. The card waits where it is.
                    ReviewOutcome::Open { .. } => {}
                    ReviewOutcome::Approve { hunks } => events.push(Event::CardApproved {
                        card_id: card_id.clone(),
                        by: *by,
                        reason: format!(
                            "{} hunk{} approved",
                            hunks.len(),
                            if hunks.len() == 1 { "" } else { "s" }
                        ),
                        hunks,
                    }),
                    ReviewOutcome::Reject { hunks, reason } => events.push(Event::CardRejected {
                        card_id: card_id.clone(),
                        reason,
                        by: *by,
                        hunks,
                    }),
                    // The rule for a partial rejection. A branch merges whole,
                    // so nothing here can land three quarters of a diff: the
                    // approved work goes in as the card, and what was sent
                    // back is carried on a new card rather than dropped. The
                    // follow-up is created before the approval so a reader of
                    // the log sees the carrier exist before the card closes,
                    // and it lands in Ready for the same reason a rejected
                    // card does — the work is understood and can be started.
                    ReviewOutcome::Partial { approved, rejected } => {
                        if self.cards.contains_key(follow_up) {
                            return Err(DecisionError::DuplicateCard(follow_up.clone()));
                        }
                        events.push(Event::CardCreated {
                            card_id: follow_up.clone(),
                            title: follow_up_title(card, &rejected),
                        });
                        events.push(Event::CardAssigned {
                            card_id: follow_up.clone(),
                            agent_id: card.agent_id.clone(),
                        });
                        events.push(Event::CardMoved {
                            card_id: follow_up.clone(),
                            from: Status::Backlog,
                            to: Status::Ready,
                        });
                        events.push(Event::CardApproved {
                            card_id: card_id.clone(),
                            by: *by,
                            reason: format!(
                                "{} of {} hunks approved; {} carries the rest",
                                approved.len(),
                                approved.len() + rejected.len(),
                                follow_up
                            ),
                            hunks: approved,
                        });
                    }
                }
                Ok(events)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Actor, Board, CardId, Command, DecisionError, Event, RunId, RunOutcome, Status, Status::*,
    };

    #[test]
    fn happy_path_backlog_to_done() {
        assert_eq!(Backlog.move_to(Ready), Ok(Ready));
        assert_eq!(Ready.move_to(Running), Ok(Running));
        assert_eq!(Running.move_to(Review), Ok(Review));
        assert_eq!(Review.move_to(Done), Ok(Done));
    }

    #[test]
    fn rework_cycle_is_legal() {
        assert_eq!(Running.move_to(Ready), Ok(Ready));
        assert_eq!(Review.move_to(Ready), Ok(Ready));
        assert_eq!(Ready.move_to(Running), Ok(Running));
        assert_eq!(Backlog.move_to(Ready), Ok(Ready));
    }

    #[test]
    fn skips_are_rejected() {
        let skips = [
            (Backlog, Running),
            (Backlog, Review),
            (Backlog, Done),
            (Ready, Review),
            (Ready, Done),
            (Running, Backlog),
            (Running, Done),
            (Review, Backlog),
            (Review, Running),
        ];
        for &(from, to) in &skips {
            let err = from.move_to(to).expect_err("skip must be rejected");
            assert_eq!((err.from, err.to), (from, to));
        }
    }

    #[test]
    fn done_is_terminal() {
        for &to in &[Backlog, Ready, Running, Review] {
            assert!(Done.move_to(to).is_err());
        }
    }

    #[test]
    fn same_status_move_is_rejected() {
        for &s in &[Backlog, Ready, Running, Review, Done] {
            assert!(s.move_to(s).is_err());
        }
    }

    #[test]
    fn can_move_to_agrees_with_move_to() {
        use super::Status;
        for &(from, to) in Status::LEGAL_MOVES {
            assert!(from.can_move_to(to));
            assert_eq!(from.move_to(to), Ok(to));
        }
        for &from in &[Backlog, Ready, Running, Review, Done] {
            for &to in &[Backlog, Ready, Running, Review, Done] {
                if !from.can_move_to(to) {
                    assert!(from.move_to(to).is_err());
                }
            }
        }
    }

    #[test]
    fn create_then_move_roundtrip() {
        let mut board = Board::default();
        let id = CardId::new("c1");
        let events = board
            .decide(&Command::CreateCard {
                card_id: id.clone(),
                title: "  hello  ".into(),
            })
            .unwrap();
        assert_eq!(events.len(), 1);
        for e in &events {
            board.apply(e);
        }
        let card = board.get(&id).unwrap();
        assert_eq!(card.title, "hello");
        assert_eq!(card.status, Backlog);

        let events = board
            .decide(&Command::MoveCard { card_id: id.clone(), to: Ready })
            .unwrap();
        for e in &events {
            board.apply(e);
        }
        assert_eq!(board.get(&id).unwrap().status, Ready);
    }

    #[test]
    fn replay_reproduces_the_same_board() {
        let mut driven = Board::default();
        let id = CardId::new("c9");
        let mut log = Vec::new();
        for to in [None, Some(Ready), Some(Running)] {
            let cmd = match to {
                None => Command::CreateCard { card_id: id.clone(), title: "x".into() },
                Some(t) => Command::MoveCard { card_id: id.clone(), to: t },
            };
            let events = driven.decide(&cmd).unwrap();
            for e in &events {
                driven.apply(e);
            }
            log.extend(events);
        }

        let mut replayed = Board::default();
        for e in &log {
            replayed.apply(e);
        }
        assert_eq!(driven.cards(), replayed.cards());
    }

    /// A card's whole life, each event stamped as the log stamps it. Returned
    /// as a log so the replay tests below drive exactly what a restart would.
    fn done_log(id: &CardId) -> Vec<(Event, u64)> {
        vec![
            (
                Event::CardCreated { card_id: id.clone(), title: "ship it".into() },
                1_000,
            ),
            (
                Event::CardMoved { card_id: id.clone(), from: Backlog, to: Ready },
                2_000,
            ),
            (
                Event::RunStarted {
                    card_id: id.clone(),
                    run_id: RunId("r1".into()),
                    worktree: Some("/tmp/c1".into()),
                    branch: Some("harness/c1".into()),
                },
                3_000,
            ),
            (
                Event::RunFinished {
                    card_id: id.clone(),
                    run_id: RunId("r1".into()),
                    outcome: RunOutcome::Completed,
                    cost_usd: Some(0.4),
                    turns: Some(7),
                },
                4_000,
            ),
            (
                Event::CardApproved {
                    card_id: id.clone(),
                    by: Actor::Human,
                    reason: String::new(),
                    hunks: Vec::new(),
                },
                5_000,
            ),
        ]
    }

    #[test]
    fn done_carries_the_moment_it_was_approved() {
        let id = CardId::new("c_fin");
        let mut board = Board::default();
        for (event, ts) in done_log(&id) {
            // Not finished until it is: every step before the approval leaves
            // the field alone.
            board.apply_at(&event, ts);
        }
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Done);
        assert_eq!(card.finished_ms, Some(5_000));
    }

    #[test]
    fn a_finish_time_only_appears_with_the_finish() {
        let id = CardId::new("c_mid");
        let mut board = Board::default();
        for (event, ts) in done_log(&id).into_iter().take(4) {
            board.apply_at(&event, ts);
            assert_eq!(board.get(&id).unwrap().finished_ms, None);
        }
        assert_eq!(board.get(&id).unwrap().status, Review);
    }

    #[test]
    fn replaying_a_stamped_log_reproduces_the_finish_time() {
        let id = CardId::new("c_rep");
        let log = done_log(&id);

        let mut driven = Board::default();
        for (event, ts) in &log {
            driven.apply_at(event, *ts);
        }
        let mut replayed = Board::default();
        for (event, ts) in &log {
            replayed.apply_at(event, *ts);
        }
        assert_eq!(driven.cards(), replayed.cards());
        assert_eq!(replayed.get(&id).unwrap().finished_ms, Some(5_000));

        // Compaction folds the board into one event. The finish time rides on
        // the card itself, so the log that replaces the log keeps it.
        let snapshot = Event::BoardSnapshot {
            cards: driven.cards().into_iter().cloned().collect(),
        };
        let mut compacted = Board::default();
        compacted.apply_at(&snapshot, 6_000);
        assert_eq!(compacted.cards(), driven.cards());
    }

    #[test]
    fn a_log_without_timestamps_still_loads() {
        let id = CardId::new("c_old");
        let mut board = Board::default();
        for (event, _) in done_log(&id) {
            // What a log written before timestamps were stored replays as:
            // `ts_ms` defaults to zero, and no time is invented for it.
            board.apply_at(&event, 0);
        }
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Done);
        assert_eq!(card.finished_ms, None);
    }

    #[test]
    fn leaving_done_drops_the_finish_time() {
        let id = CardId::new("c_back");
        let mut board = Board::default();
        for (event, ts) in done_log(&id) {
            board.apply_at(&event, ts);
        }
        assert_eq!(board.get(&id).unwrap().finished_ms, Some(5_000));

        // An override is the only way back out of Done, and a card sitting in
        // Ready must not still claim a finish.
        let events = board
            .decide(&Command::OverrideCard {
                card_id: id.clone(),
                to: Ready,
                reason: "wrong card approved".into(),
            })
            .unwrap();
        for e in &events {
            board.apply_at(e, 7_000);
        }
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Ready);
        assert_eq!(card.finished_ms, None);
    }

    #[test]
    fn duplicate_create_is_rejected() {
        let mut board = Board::default();
        let id = CardId::new("dup");
        let cmd = Command::CreateCard { card_id: id.clone(), title: "a".into() };
        board.apply(&board.decide(&cmd).unwrap()[0]);
        assert!(matches!(
            board.decide(&cmd),
            Err(DecisionError::DuplicateCard(_))
        ));
    }

    #[test]
    fn unknown_card_moves_are_rejected() {
        let board = Board::default();
        assert!(matches!(
            board.decide(&Command::MoveCard {
                card_id: CardId::new("ghost"),
                to: Ready
            }),
            Err(DecisionError::CardNotFound(_))
        ));
    }

    #[test]
    fn illegal_move_reports_from_and_to() {
        let mut board = Board::default();
        let id = CardId::new("c2");
        board.apply(
            &board
                .decide(&Command::CreateCard { card_id: id.clone(), title: "t".into() })
                .unwrap()[0],
        );
        assert!(matches!(
            board.decide(&Command::MoveCard { card_id: id.clone(), to: Done }),
            Err(DecisionError::IllegalMove { from: Backlog, to: Done })
        ));
    }

    #[test]
    fn override_requires_reason_but_can_leave_done() {
        let mut board = Board::default();
        let id = CardId::new("c3");
        board.apply(
            &board
                .decide(&Command::CreateCard { card_id: id.clone(), title: "t".into() })
                .unwrap()[0],
        );
        for to in [Ready, Running, Review, Done] {
            let evts = board
                .decide(&Command::MoveCard { card_id: id.clone(), to })
                .unwrap();
            for e in evts {
                board.apply(&e);
            }
        }
        assert_eq!(board.get(&id).unwrap().status, Done);

        assert!(matches!(
            board.decide(&Command::OverrideCard {
                card_id: id.clone(),
                to: Backlog,
                reason: "   ".into()
            }),
            Err(DecisionError::EmptyReason)
        ));

        let evts = board
            .decide(&Command::OverrideCard {
                card_id: id.clone(),
                to: Backlog,
                reason: "reopen".into(),
            })
            .unwrap();
        for e in evts {
            board.apply(&e);
        }
        assert_eq!(board.get(&id).unwrap().status, Backlog);
    }

    #[test]
    fn override_to_running_is_refused() {
        let mut board = Board::default();
        let id = CardId::new("c4");
        card_in(&mut board, &id, Ready);

        assert!(matches!(
            board.decide(&Command::OverrideCard {
                card_id: id.clone(),
                to: Running,
                reason: "make it go".into(),
            }),
            Err(DecisionError::CannotOverrideToRunning)
        ));
        assert_eq!(board.get(&id).unwrap().status, Ready);
        assert_eq!(board.get(&id).unwrap().current_run, None);
    }

    fn drive(board: &mut Board, cmd: &Command) {
        for e in board.decide(cmd).unwrap() {
            board.apply(&e);
        }
    }

    /// Order, not file conflict: a card whose dependency has not reached Done
    /// cannot start, and the error names what it is waiting on.
    #[test]
    fn dependencies_hold_a_card_until_they_are_done() {
        let mut board = Board::default();
        let first = CardId::new("d_first");
        let second = CardId::new("d_second");
        drive(&mut board, &Command::CreateCard { card_id: first.clone(), title: "first".into() });
        drive(&mut board, &Command::CreateCard { card_id: second.clone(), title: "second".into() });
        drive(
            &mut board,
            &Command::SetDependencies {
                card_id: second.clone(),
                depends_on: vec![first.clone()],
            },
        );
        assert_eq!(
            board.get(&second).unwrap().depends_on,
            vec![first.clone()]
        );

        drive(&mut board, &Command::MoveCard { card_id: second.clone(), to: Ready });
        assert!(matches!(
            board.decide(&Command::StartRun {
                card_id: second.clone(),
                run_id: RunId("r".into()),
                worktree: None,
                branch: None,
            }),
            Err(DecisionError::DependenciesNotMet(_))
        ));

        // The dependency finishes; the dependent may now run.
        drive(&mut board, &Command::OverrideCard {
            card_id: first.clone(),
            to: Done,
            reason: "finished elsewhere".into(),
        });
        assert!(board
            .decide(&Command::StartRun {
                card_id: second.clone(),
                run_id: RunId("r".into()),
                worktree: None,
                branch: None,
            })
            .is_ok());
    }

    #[test]
    fn discarding_a_dependency_frees_the_dependents_with_a_note() {
        let mut board = Board::default();
        let first = CardId::new("f_first");
        let second = CardId::new("f_second");
        drive(&mut board, &Command::CreateCard { card_id: first.clone(), title: "first".into() });
        drive(&mut board, &Command::CreateCard { card_id: second.clone(), title: "second".into() });
        drive(
            &mut board,
            &Command::SetDependencies {
                card_id: second.clone(),
                depends_on: vec![first.clone()],
            },
        );

        // Off the board means out of the way: the dependent is free, and the
        // discard says so where the operator reads.
        let events = board
            .decide(&Command::DiscardCard {
                card_id: first.clone(),
                reason: "not wanted".into(),
            })
            .unwrap();
        assert!(matches!(
            &events[0],
            Event::CardDiscarded { reason, .. }
                if reason == "not wanted; frees f_second"
        ));
        for e in &events {
            board.apply(e);
        }
        drive(&mut board, &Command::MoveCard { card_id: second.clone(), to: Ready });
        assert!(board
            .decide(&Command::StartRun {
                card_id: second.clone(),
                run_id: RunId("r".into()),
                worktree: None,
                branch: None,
            })
            .is_ok());
    }

    #[test]
    fn dependencies_cannot_be_circular_or_dangle() {
        let mut board = Board::default();
        let a = CardId::new("cyc_a");
        let b = CardId::new("cyc_b");
        let ghost = CardId::new("ghost");
        drive(&mut board, &Command::CreateCard { card_id: a.clone(), title: "a".into() });
        drive(&mut board, &Command::CreateCard { card_id: b.clone(), title: "b".into() });

        assert!(matches!(
            board.decide(&Command::SetDependencies {
                card_id: a.clone(),
                depends_on: vec![a.clone()],
            }),
            Err(DecisionError::DependencyCycle(_))
        ));
        assert!(matches!(
            board.decide(&Command::SetDependencies {
                card_id: a.clone(),
                depends_on: vec![ghost],
            }),
            Err(DecisionError::CardNotFound(_))
        ));

        // b waits on nothing yet; making a wait on b would close no cycle.
        drive(
            &mut board,
            &Command::SetDependencies {
                card_id: b.clone(),
                depends_on: vec![a.clone()],
            },
        );
        assert!(matches!(
            board.decide(&Command::SetDependencies {
                card_id: a.clone(),
                depends_on: vec![b],
            }),
            Err(DecisionError::DependencyCycle(_))
        ));
    }

    /// A snapshot folds everything that came before it into one event; replay
    /// from there lands on exactly the same board.
    #[test]
    fn a_snapshot_replaces_the_board_and_replay_survives_it() {
        let mut live = Board::default();
        let id = CardId::new("snap_1");
        drive(&mut live, &Command::CreateCard { card_id: id.clone(), title: "kept".into() });
        drive(&mut live, &Command::MoveCard { card_id: id.clone(), to: Ready });

        // What compaction writes: one event holding the whole board.
        let snapshot = Event::BoardSnapshot {
            cards: live.cards().into_iter().cloned().collect(),
        };
        let mut replayed = Board::default();
        replayed.apply(&snapshot);
        assert_eq!(live.cards(), replayed.cards());

        // And the log goes on from there.
        let run = Command::StartRun {
            card_id: id.clone(),
            run_id: RunId("post-snap".into()),
            worktree: None,
            branch: None,
        };
        for e in replayed.decide(&run).unwrap() {
            replayed.apply(&e);
        }
        assert_eq!(replayed.get(&id).unwrap().status, Running);
    }

    #[test]
    fn a_work_report_needs_something_to_say_and_is_trimmed() {
        let mut board = Board::default();
        let id = CardId::new("wr");
        drive(&mut board, &Command::CreateCard { card_id: id.clone(), title: "t".into() });

        // Neither a summary nor a note: nothing to record.
        assert!(matches!(
            board.decide(&Command::ReportWork {
                card_id: id.clone(),
                summary: "   ".into(),
                notes: vec![],
            }),
            Err(DecisionError::EmptyReport)
        ));
        // A note alone is enough; whitespace dies on the way in.
        let events = board
            .decide(&Command::ReportWork {
                card_id: id.clone(),
                summary: "  fixed the loop  ".into(),
                notes: vec!["  always retry twice  ".into(), "   ".into()],
            })
            .unwrap();
        assert!(matches!(
            &events[0],
            Event::WorkReported { summary, notes, .. }
                if summary == "fixed the loop"
                    && notes == &vec!["always retry twice".to_string()]
        ));
    }

    /// The handoff's replay check, spelled out: a log that predates
    /// `report_work` entirely still reproduces — the event is additive with
    /// defaulted fields — and a snapshot keeps folding whatever came before.
    #[test]
    fn an_old_log_without_work_reports_still_replays() {
        let raw = concat!(
            r#"{"type":"card_created","card_id":"c1","title":"old"}"#,
            "\n",
            r#"{"type":"card_moved","card_id":"c1","from":"backlog","to":"ready"}"#,
            "\n",
            r#"{"type":"run_started","card_id":"c1","run_id":"run-1"}"#,
            "\n",
            r#"{"type":"work_reported","card_id":"c1","summary":"s"}"#,
            "\n",
            r#"{"type":"run_finished","card_id":"c1","run_id":"run-1","outcome":"completed"}"#,
            "\n",
            r#"{"type":"card_created","card_id":"c2","title":"later"}"#,
            "\n",
            r#"{"type":"board_snapshot","cards":[{"id":"c1","title":"old","status":"done","agent_id":"builder","cost_usd":0,"turns":0,"runs":0}]}"#,
        );
        let mut board = Board::default();
        for line in raw.split('\n') {
            let event: Event = serde_json::from_str(line).expect(line);
            board.apply(&event);
        }
        assert!(matches!(board.get(&CardId::new("c1")).unwrap().status, Status::Done));
        assert!(board.get(&CardId::new("c2")).is_none(), "the snapshot folds c2 away");
    }

    #[test]
    fn empty_title_is_rejected() {
        let board = Board::default();
        assert!(matches!(
            board.decide(&Command::CreateCard {
                card_id: CardId::new("x"),
                title: "   ".into()
            }),
            Err(DecisionError::EmptyTitle)
        ));
    }

    fn card_in(board: &mut Board, id: &CardId, status: Status) {
        board.apply(
            &board
                .decide(&Command::CreateCard {
                    card_id: id.clone(),
                    title: "t".into(),
                })
                .unwrap()[0],
        );
        if status != Backlog {
            board.apply(
                &board
                    .decide(&Command::OverrideCard {
                        card_id: id.clone(),
                        to: status,
                        reason: "setup".into(),
                    })
                    .unwrap()[0],
            );
        }
    }

    #[test]
    fn run_lifecycle_ready_to_running_to_review() {
        use super::RunId;
        let mut board = Board::default();
        let id = CardId::new("r1");
        card_in(&mut board, &id, Ready);
        let run = RunId("run-1".into());

        board.apply(
            &board
                .decide(&Command::StartRun {
                    card_id: id.clone(),
                    run_id: run.clone(),
                    worktree: None,
                    branch: None,
                })
                .unwrap()[0],
        );
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Running);
        assert_eq!(card.current_run, Some(run.clone()));

        assert!(matches!(
            board.decide(&Command::StartRun {
                    card_id: id.clone(),
                    run_id: run.clone(),
                    worktree: None,
                    branch: None,
                }),
            Err(DecisionError::NotReady(Running))
        ));

        board.apply(
            &board
                .decide(&Command::FinishRun {
                    card_id: id.clone(),
                    run_id: run.clone(),
                    outcome: super::RunOutcome::Completed,
                    cost_usd: Some(0.02),
                    turns: Some(3),
                })
                .unwrap()[0],
        );
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Review);
        assert_eq!(card.current_run, None);
    }

    #[test]
    fn cancelled_run_returns_card_to_ready() {
        use super::{RunId, RunOutcome};
        let mut board = Board::default();
        let id = CardId::new("r2");
        card_in(&mut board, &id, Ready);
        let run = RunId("run-2".into());
        board.apply(
            &board
                .decide(&Command::StartRun {
                    card_id: id.clone(),
                    run_id: run.clone(),
                    worktree: None,
                    branch: None,
                })
                .unwrap()[0],
        );
        board.apply(
            &board
                .decide(&Command::FinishRun {
                    card_id: id.clone(),
                    run_id: run.clone(),
                    outcome: RunOutcome::Cancelled,
                    cost_usd: None,
                    turns: None,
                })
                .unwrap()[0],
        );
        assert_eq!(board.get(&id).unwrap().status, Ready);

        let run2 = RunId("run-3".into());
        assert!(board
            .decide(&Command::StartRun {
                card_id: id.clone(),
                run_id: run2,
                worktree: None,
                branch: None,
            })
            .is_ok());
    }

    #[test]
    fn finish_run_rejects_mismatched_run_and_non_running_card() {
        use super::{RunId, RunOutcome};
        let mut board = Board::default();
        let id = CardId::new("r3");
        card_in(&mut board, &id, Backlog);

        assert!(matches!(
            board.decide(&Command::FinishRun {
                card_id: id.clone(),
                run_id: RunId("nope".into()),
                outcome: RunOutcome::Completed,
                cost_usd: None,
                turns: None,
            }),
            Err(DecisionError::NotRunning(Backlog))
        ));

        board.apply(
            &board
                .decide(&Command::OverrideCard {
                    card_id: id.clone(),
                    to: Ready,
                    reason: "setup".into(),
                })
                .unwrap()[0],
        );
        board.apply(
            &board
                .decide(&Command::StartRun {
                    card_id: id.clone(),
                    run_id: RunId("real".into()),
                    worktree: None,
                    branch: None,
                })
                .unwrap()[0],
        );
        assert!(matches!(
            board.decide(&Command::FinishRun {
                card_id: id.clone(),
                run_id: RunId("fake".into()),
                outcome: RunOutcome::Completed,
                cost_usd: None,
                turns: None,
            }),
            Err(DecisionError::RunMismatch)
        ));
    }

    /// The title is the prompt. A card that has not run yet is still only a
    /// request, so correcting it keeps the id, the history and every
    /// dependency pointing at it — which discarding and recreating would lose.
    #[test]
    fn a_cards_title_can_be_corrected_before_it_has_run() {
        let mut board = Board::default();
        let id = CardId::new("e1");
        card_in(&mut board, &id, Backlog);
        drive(
            &mut board,
            &Command::EditCard {
                card_id: id.clone(),
                title: "  say what the agent should actually do  ".into(),
            },
        );
        let card = board.get(&id).unwrap();
        assert_eq!(card.title, "say what the agent should actually do");
        assert_eq!(card.id, id, "the id survives the correction");
        assert_eq!(card.status, Backlog);

        // Ready is still before the first run: the prompt has not been read.
        drive(&mut board, &Command::MoveCard { card_id: id.clone(), to: Ready });
        drive(
            &mut board,
            &Command::EditCard { card_id: id.clone(), title: "once more".into() },
        );
        assert_eq!(board.get(&id).unwrap().title, "once more");

        assert!(matches!(
            board.decide(&Command::EditCard { card_id: id.clone(), title: "   ".into() }),
            Err(DecisionError::EmptyTitle)
        ));
        assert!(matches!(
            board.decide(&Command::EditCard { card_id: id.clone(), title: "once more".into() }),
            Err(DecisionError::SameTitle)
        ));
        assert!(matches!(
            board.decide(&Command::EditCard {
                card_id: CardId::new("nobody"),
                title: "x".into(),
            }),
            Err(DecisionError::CardNotFound(_))
        ));
    }

    /// And the other side: once a run has read the title, the transcript and
    /// the commit answer *that* title. Editing it would make the log stop
    /// matching the card.
    #[test]
    fn a_cards_title_is_frozen_once_it_has_run() {
        use super::{RunId, RunOutcome};
        let mut board = Board::default();
        let id = CardId::new("e2");
        card_in(&mut board, &id, Ready);
        drive(
            &mut board,
            &Command::StartRun {
                card_id: id.clone(),
                run_id: RunId("run-a".into()),
                worktree: None,
                branch: None,
            },
        );
        // Running counts: `runs` goes up when a run starts, not when it ends.
        assert!(matches!(
            board.decide(&Command::EditCard { card_id: id.clone(), title: "too late".into() }),
            Err(DecisionError::AlreadyRan { runs: 1, .. })
        ));

        drive(
            &mut board,
            &Command::FinishRun {
                card_id: id.clone(),
                run_id: RunId("run-a".into()),
                outcome: RunOutcome::Completed,
                cost_usd: None,
                turns: None,
            },
        );
        let refused = board
            .decide(&Command::EditCard { card_id: id.clone(), title: "still too late".into() })
            .unwrap_err();
        assert!(matches!(refused, DecisionError::AlreadyRan { runs: 1, .. }));
        assert!(
            refused.to_string().contains("already run"),
            "the refusal says why: {refused}"
        );
        assert_eq!(board.get(&id).unwrap().title, "t", "the title did not move");
    }

    /// A title may carry a body — an accepted proposal brings its reasoning
    /// with it — and the one-line places must take one line.
    #[test]
    fn a_title_with_a_body_still_has_a_one_line_subject() {
        let mut board = Board::default();
        let id = CardId::new("s1");
        card_in(&mut board, &id, Backlog);
        drive(
            &mut board,
            &Command::EditCard {
                card_id: id.clone(),
                title: "widen propose_improvement\n\nWhat was seen: four refusals".into(),
            },
        );
        let card = board.get(&id).unwrap();
        assert_eq!(card.subject(), "widen propose_improvement");
        assert!(card.title.contains("four refusals"), "the body is kept on the card");
        assert_eq!(super::one_line("  "), "");
        assert_eq!(super::one_line("only one line"), "only one line");
    }

    #[test]
    fn finished_runs_accumulate_cost_and_turns_on_the_card() {
        use super::{RunId, RunOutcome};
        let mut board = Board::default();
        let id = CardId::new("r9");
        card_in(&mut board, &id, Ready);
        for (n, cost) in [("run-a", 0.25), ("run-b", 0.5)] {
            let run = RunId(n.into());
            board.apply(
                &board
                    .decide(&Command::StartRun {
                    card_id: id.clone(),
                    run_id: run.clone(),
                    worktree: None,
                    branch: None,
                })
                    .unwrap()[0],
            );
            board.apply(
                &board
                    .decide(&Command::FinishRun {
                        card_id: id.clone(),
                        run_id: run.clone(),
                        outcome: RunOutcome::Completed,
                        cost_usd: Some(cost),
                        turns: Some(4),
                    })
                    .unwrap()[0],
            );
            board.apply(
                &board
                    .decide(&Command::RejectCard {
                        card_id: id.clone(),
                        reason: "again".into(),
                        by: Actor::Director,
                        hunks: Vec::new(),
                    })
                    .unwrap()[0],
            );
        }
        let card = board.get(&id).unwrap();
        assert_eq!(card.cost_usd, 0.75);
        assert_eq!(card.turns, 8);
        assert_eq!(card.runs, 2);
        let review = card.last_review.clone().unwrap();
        assert_eq!(review.by, Actor::Director);
        assert!(!review.approved);
    }

    #[test]
    fn assigning_an_agent_requires_a_card_and_a_name() {
        let mut board = Board::default();
        let id = CardId::new("a1");
        assert!(matches!(
            board.decide(&Command::AssignAgent { card_id: id.clone(), agent_id: "scout".into() }),
            Err(DecisionError::CardNotFound(_))
        ));
        board.apply(
            &board
                .decide(&Command::CreateCard { card_id: id.clone(), title: "t".into() })
                .unwrap()[0],
        );
        assert_eq!(board.get(&id).unwrap().agent_id, "builder");
        assert!(matches!(
            board.decide(&Command::AssignAgent { card_id: id.clone(), agent_id: "  ".into() }),
            Err(DecisionError::EmptyAgent)
        ));
        board.apply(
            &board
                .decide(&Command::AssignAgent { card_id: id.clone(), agent_id: " scout ".into() })
                .unwrap()[0],
        );
        assert_eq!(board.get(&id).unwrap().agent_id, "scout");
    }

    #[test]
    fn a_card_can_be_discarded_unless_it_is_running() {
        use super::RunId;
        let mut board = Board::default();
        let id = CardId::new("d1");
        card_in(&mut board, &id, Ready);

        // While a run owns it, no.
        board.apply(
            &board
                .decide(&Command::StartRun {
                    card_id: id.clone(),
                    run_id: RunId("r".into()),
                    worktree: None,
                    branch: None,
                })
                .unwrap()[0],
        );
        assert!(matches!(
            board.decide(&Command::DiscardCard {
                card_id: id.clone(),
                reason: "no longer needed".into()
            }),
            Err(DecisionError::NotRunning(Running))
        ));

        // Once it is not running, it goes.
        board.apply(
            &board
                .decide(&Command::FinishRun {
                    card_id: id.clone(),
                    run_id: RunId("r".into()),
                    outcome: super::RunOutcome::Cancelled,
                    cost_usd: None,
                    turns: None,
                })
                .unwrap()[0],
        );
        let events = board
            .decide(&Command::DiscardCard {
                card_id: id.clone(),
                reason: "  no longer needed  ".into(),
            })
            .unwrap();
        assert!(matches!(
            &events[0],
            Event::CardDiscarded { reason, .. } if reason == "no longer needed"
        ));
        for e in &events {
            board.apply(e);
        }
        assert!(board.get(&id).is_none(), "the card is off the board");
        assert!(board.cards().is_empty());

        // And discarding what is not there is refused.
        assert!(matches!(
            board.decide(&Command::DiscardCard {
                card_id: id,
                reason: String::new()
            }),
            Err(DecisionError::CardNotFound(_))
        ));
    }

    #[test]
    fn approval_records_who_decided() {
        let mut board = Board::default();
        let id = CardId::new("v1");
        card_in(&mut board, &id, Review);
        board.apply(
            &board
                .decide(&Command::ApproveCard {
                    card_id: id.clone(),
                    by: Actor::Director,
                    reason: "diff looks right".into(),
                    hunks: Vec::new(),
                })
                .unwrap()[0],
        );
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Done);
        let review = card.last_review.clone().unwrap();
        assert_eq!(review.by, Actor::Director);
        assert!(review.approved);
        assert_eq!(review.reason, "diff looks right");
    }

    #[test]
    fn a_run_records_where_it_worked_and_the_session_it_leaves() {
        let mut board = Board::default();
        let id = CardId::new("c1");
        for cmd in [
            Command::CreateCard { card_id: id.clone(), title: "t".into() },
            Command::MoveCard { card_id: id.clone(), to: Status::Ready },
        ] {
            for e in board.decide(&cmd).unwrap() {
                board.apply(&e);
            }
        }

        let started = board
            .decide(&Command::StartRun {
                card_id: id.clone(),
                run_id: RunId("run-1".into()),
                worktree: Some("C:/data/worktrees/proj/c1".into()),
                branch: Some("harness/c1".into()),
            })
            .unwrap();
        for e in &started {
            board.apply(e);
        }
        let card = board.get(&id).unwrap();
        assert_eq!(card.worktree.as_deref(), Some("C:/data/worktrees/proj/c1"));
        assert_eq!(card.branch.as_deref(), Some("harness/c1"));
        assert!(card.session_id.is_none(), "no session has been reported yet");

        let recorded = board
            .decide(&Command::RecordSession {
                card_id: id.clone(),
                session_id: "  sess-abc  ".into(),
            })
            .unwrap();
        for e in &recorded {
            board.apply(e);
        }
        assert_eq!(board.get(&id).unwrap().session_id.as_deref(), Some("sess-abc"));

        // A later run in the same card keeps the session to resume: it is what
        // the next run continues, and a new one only arrives once it starts.
        for e in board
            .decide(&Command::FinishRun {
                card_id: id.clone(),
                run_id: RunId("run-1".into()),
                outcome: RunOutcome::Completed,
                cost_usd: None,
                turns: None,
            })
            .unwrap()
        {
            board.apply(&e);
        }
        for e in board
            .decide(&Command::RejectCard {
                card_id: id.clone(),
                reason: "again".into(),
                by: Actor::Human,
                hunks: Vec::new(),
            })
            .unwrap()
        {
            board.apply(&e);
        }
        for e in board
            .decide(&Command::StartRun {
                card_id: id.clone(),
                run_id: RunId("run-2".into()),
                worktree: Some("C:/data/worktrees/proj/c1".into()),
                branch: Some("harness/c1".into()),
            })
            .unwrap()
        {
            board.apply(&e);
        }
        assert_eq!(
            board.get(&id).unwrap().session_id.as_deref(),
            Some("sess-abc"),
            "a new run resumes the session the last one left"
        );
    }

    #[test]
    fn a_session_cannot_be_recorded_against_nothing() {
        let mut board = Board::default();
        let id = CardId::new("c1");
        assert!(matches!(
            board.decide(&Command::RecordSession {
                card_id: id.clone(),
                session_id: "sess".into(),
            }),
            Err(DecisionError::CardNotFound(_))
        ));

        for e in board
            .decide(&Command::CreateCard { card_id: id.clone(), title: "t".into() })
            .unwrap()
        {
            board.apply(&e);
        }
        assert!(matches!(
            board.decide(&Command::RecordSession {
                card_id: id.clone(),
                session_id: "   ".into(),
            }),
            Err(DecisionError::EmptySession)
        ));
    }

    #[test]
    fn a_log_written_before_worktrees_were_recorded_still_replays() {
        // Exactly the shape an older build wrote.
        let raw = concat!(
            r#"{"type":"card_created","card_id":"c1","title":"old"}"#,
            "\n",
            r#"{"type":"card_moved","card_id":"c1","from":"backlog","to":"ready"}"#,
            "\n",
            r#"{"type":"run_started","card_id":"c1","run_id":"run-1"}"#,
        );
        let mut board = Board::default();
        for line in raw.split('\n') {
            let event: Event = serde_json::from_str(line).expect(line);
            board.apply(&event);
        }
        let card = board.get(&CardId::new("c1")).unwrap();
        assert_eq!(card.status, Status::Running);
        // Nothing to restore, and nothing broken by its absence.
        assert!(card.worktree.is_none());
        assert!(card.session_id.is_none());
    }

    // ---- hunk-level review -------------------------------------------------

    use super::{follow_up_title, one_line, resolve_review, HunkRef, HunkVerdict, ReviewOutcome};

    /// `@@` headers as git writes them, one per block of a pretend diff.
    fn diff_of(n: u32) -> Vec<HunkRef> {
        (0..n)
            .map(|i| HunkRef {
                file: format!("src/f{i}.rs"),
                header: format!("@@ -{},4 +{},6 @@ fn f{i}(", i * 10 + 1, i * 10 + 1),
                new_start: i * 10 + 1,
                new_lines: 6,
            })
            .collect()
    }

    /// A card that has run once and is sitting in Review, together with the
    /// log that put it there — which is what the replay tests drive.
    fn in_review(id: &CardId, agent: &str) -> (Board, Vec<Event>) {
        let mut board = Board::default();
        let mut log = Vec::new();
        let cmds = [
            Command::CreateCard { card_id: id.clone(), title: "clamp the budget".into() },
            Command::AssignAgent { card_id: id.clone(), agent_id: agent.into() },
            Command::MoveCard { card_id: id.clone(), to: Ready },
            Command::StartRun {
                card_id: id.clone(),
                run_id: RunId("r1".into()),
                worktree: Some("/tmp/c".into()),
                branch: Some("harness/c".into()),
            },
            Command::FinishRun {
                card_id: id.clone(),
                run_id: RunId("r1".into()),
                outcome: RunOutcome::Completed,
                cost_usd: None,
                turns: None,
            },
        ];
        for cmd in cmds {
            for e in board.decide(&cmd).unwrap() {
                board.apply(&e);
                log.push(e);
            }
        }
        assert_eq!(board.get(id).unwrap().status, Review);
        (board, log)
    }

    fn review(
        board: &mut Board,
        log: &mut Vec<Event>,
        card_id: &CardId,
        hunk: &HunkRef,
        approved: bool,
        reason: &str,
        diff: &[HunkRef],
        follow_up: &str,
    ) -> Result<(), DecisionError> {
        let events = board.decide(&Command::ReviewHunk {
            card_id: card_id.clone(),
            hunk: hunk.clone(),
            approved,
            by: Actor::Human,
            reason: reason.into(),
            diff: diff.to_vec(),
            follow_up: CardId::new(follow_up),
        })?;
        for e in &events {
            board.apply(e);
        }
        log.extend(events);
        Ok(())
    }

    #[test]
    fn a_card_with_no_hunk_selection_is_decided_exactly_as_before() {
        let id = CardId::new("c_whole");
        let (mut board, _) = in_review(&id, "builder");
        for e in board
            .decide(&Command::ApproveCard {
                card_id: id.clone(),
                by: Actor::Human,
                reason: "looks right".into(),
                hunks: Vec::new(),
            })
            .unwrap()
        {
            board.apply(&e);
        }
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Done);
        let review = card.last_review.clone().unwrap();
        assert!(review.approved);
        assert_eq!(review.reason, "looks right");
        // No block was named, and none is invented.
        assert!(review.hunks.is_empty());
        // And nothing else appeared on the board.
        assert_eq!(board.cards().len(), 1);
    }

    #[test]
    fn a_verdict_leaves_the_card_in_review_until_every_hunk_is_read() {
        let id = CardId::new("c_open");
        let diff = diff_of(3);
        let (mut board, mut log) = in_review(&id, "builder");

        review(&mut board, &mut log, &id, &diff[0], true, "", &diff, "c_new").unwrap();
        review(&mut board, &mut log, &id, &diff[1], true, "", &diff, "c_new").unwrap();

        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Review, "two of three decided is not a decision");
        assert_eq!(card.hunk_verdicts.len(), 2);
        assert!(card.last_review.is_none());
        assert_eq!(
            resolve_review(&card.hunk_verdicts, &diff),
            ReviewOutcome::Open { decided: 2, of: 3 }
        );
    }

    #[test]
    fn every_hunk_approved_approves_the_card() {
        let id = CardId::new("c_all_yes");
        let diff = diff_of(2);
        let (mut board, mut log) = in_review(&id, "builder");
        for hunk in &diff {
            review(&mut board, &mut log, &id, hunk, true, "", &diff, "c_new").unwrap();
        }
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Done);
        let review = card.last_review.clone().unwrap();
        assert!(review.approved);
        assert_eq!(review.hunks, diff);
        // Spent: the verdicts are in the event, not left on the card.
        assert!(card.hunk_verdicts.is_empty());
        // Nothing was carried over, because nothing was left behind.
        assert_eq!(board.cards().len(), 1);
    }

    #[test]
    fn every_hunk_rejected_sends_the_card_back_and_carries_nothing() {
        let id = CardId::new("c_all_no");
        let diff = diff_of(2);
        let (mut board, mut log) = in_review(&id, "builder");
        review(&mut board, &mut log, &id, &diff[0], false, "wrong clamp", &diff, "c_new").unwrap();
        review(&mut board, &mut log, &id, &diff[1], false, "", &diff, "c_new").unwrap();

        let card = board.get(&id).unwrap();
        // The card itself is the carrier, so there is no follow-up to make.
        assert_eq!(card.status, Ready);
        assert_eq!(board.cards().len(), 1);
        let review = card.last_review.clone().unwrap();
        assert!(!review.approved);
        assert_eq!(review.hunks, diff);
        assert!(review.reason.starts_with("2 hunks sent back"));
        assert!(review.reason.contains("src/f0.rs"));
        assert!(review.reason.contains("wrong clamp"));
    }

    #[test]
    fn rejecting_some_hunks_lands_the_rest_and_carries_them_on_a_follow_up() {
        let id = CardId::new("c_partial");
        let diff = diff_of(4);
        let (mut board, mut log) = in_review(&id, "scout");

        for hunk in &diff[..3] {
            review(&mut board, &mut log, &id, hunk, true, "", &diff, "c_rest").unwrap();
        }
        // Still open with three of four decided.
        assert_eq!(board.get(&id).unwrap().status, Review);
        review(
            &mut board,
            &mut log,
            &id,
            &diff[3],
            false,
            "this clamp is off by one",
            &diff,
            "c_rest",
        )
        .unwrap();

        // The approved work landed.
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Done);
        let review = card.last_review.clone().unwrap();
        assert!(review.approved);
        assert_eq!(review.hunks, diff[..3].to_vec());
        assert!(review.reason.contains("c_rest"));

        // And the rejected work survived on a card of its own.
        let follow_up = board.get(&CardId::new("c_rest")).expect("a follow-up card");
        assert_eq!(follow_up.status, Ready, "it can be started, like any send-back");
        assert_eq!(follow_up.agent_id, "scout", "the same agent owns the rest");
        assert!(follow_up.subject().starts_with("Rework 1 hunk sent back on"));
        assert!(follow_up.title.contains("src/f3.rs"));
        assert!(follow_up.title.contains("this clamp is off by one"));
        assert!(follow_up.title.contains("c_partial"));
    }

    #[test]
    fn the_whole_partial_decision_replays_to_the_same_board() {
        let id = CardId::new("c_replay");
        let diff = diff_of(3);
        let (mut driven, mut log) = in_review(&id, "builder");
        review(&mut driven, &mut log, &id, &diff[0], true, "", &diff, "c_rest").unwrap();
        review(&mut driven, &mut log, &id, &diff[1], false, "no", &diff, "c_rest").unwrap();
        review(&mut driven, &mut log, &id, &diff[2], true, "", &diff, "c_rest").unwrap();

        let mut replayed = Board::default();
        for e in &log {
            replayed.apply(e);
        }
        assert_eq!(driven.cards(), replayed.cards());
        // Both cards, not just the one that was decided.
        assert_eq!(replayed.cards().len(), 2);
        assert_eq!(replayed.get(&id).unwrap().status, Done);
        assert_eq!(replayed.get(&CardId::new("c_rest")).unwrap().status, Ready);

        // The events themselves carry it: no clock, no id generator, no
        // ordering left to the reader.
        let json: Vec<String> = log.iter().map(|e| serde_json::to_string(e).unwrap()).collect();
        let mut from_json = Board::default();
        for line in &json {
            from_json.apply(&serde_json::from_str::<Event>(line).unwrap());
        }
        assert_eq!(driven.cards(), from_json.cards());
    }

    #[test]
    fn deciding_a_hunk_twice_replaces_the_verdict() {
        let id = CardId::new("c_mind");
        let diff = diff_of(2);
        let (mut board, mut log) = in_review(&id, "builder");
        review(&mut board, &mut log, &id, &diff[0], false, "no", &diff, "c_rest").unwrap();
        review(&mut board, &mut log, &id, &diff[0], true, "", &diff, "c_rest").unwrap();
        assert_eq!(board.get(&id).unwrap().hunk_verdicts.len(), 1);

        review(&mut board, &mut log, &id, &diff[1], true, "", &diff, "c_rest").unwrap();
        // Both approved after the change of mind, so nothing is carried over.
        assert_eq!(board.get(&id).unwrap().status, Done);
        assert_eq!(board.cards().len(), 1);
    }

    #[test]
    fn a_hunk_the_diff_no_longer_holds_is_refused() {
        let id = CardId::new("c_stale");
        let diff = diff_of(2);
        let (board, _) = in_review(&id, "builder");
        let gone = HunkRef::new("src/removed.rs", "@@ -1,2 +1,3 @@");
        assert!(matches!(
            board.decide(&Command::ReviewHunk {
                card_id: id.clone(),
                hunk: gone,
                approved: true,
                by: Actor::Human,
                reason: String::new(),
                diff,
                follow_up: CardId::new("c_rest"),
            }),
            Err(DecisionError::UnknownHunk(_))
        ));
    }

    #[test]
    fn a_verdict_against_a_diff_that_has_moved_never_closes_a_card() {
        let old = diff_of(2);
        let now = vec![old[0].clone(), HunkRef::new("src/f9.rs", "@@ -1,2 +1,4 @@")];
        let taken = vec![
            HunkVerdict { hunk: old[0].clone(), approved: true, by: Actor::Human, reason: String::new() },
            HunkVerdict { hunk: old[1].clone(), approved: true, by: Actor::Human, reason: String::new() },
        ];
        // One of the two verdicts is about a block that is gone; the block
        // that replaced it has not been read.
        assert_eq!(
            resolve_review(&taken, &now),
            ReviewOutcome::Open { decided: 1, of: 2 }
        );
    }

    #[test]
    fn a_new_run_throws_away_the_verdicts_it_invalidates() {
        let id = CardId::new("c_rerun");
        let diff = diff_of(2);
        let (mut board, mut log) = in_review(&id, "builder");
        review(&mut board, &mut log, &id, &diff[0], true, "", &diff, "c_rest").unwrap();
        assert_eq!(board.get(&id).unwrap().hunk_verdicts.len(), 1);

        for cmd in [
            Command::MoveCard { card_id: id.clone(), to: Ready },
            Command::StartRun {
                card_id: id.clone(),
                run_id: RunId("r2".into()),
                worktree: None,
                branch: None,
            },
        ] {
            for e in board.decide(&cmd).unwrap() {
                board.apply(&e);
            }
        }
        assert!(board.get(&id).unwrap().hunk_verdicts.is_empty());
    }

    #[test]
    fn a_card_out_of_review_takes_no_verdicts() {
        let id = CardId::new("c_backlog");
        let mut board = Board::default();
        for e in board
            .decide(&Command::CreateCard { card_id: id.clone(), title: "x".into() })
            .unwrap()
        {
            board.apply(&e);
        }
        let diff = diff_of(1);
        assert!(matches!(
            board.decide(&Command::ReviewHunk {
                card_id: id.clone(),
                hunk: diff[0].clone(),
                approved: true,
                by: Actor::Human,
                reason: String::new(),
                diff,
                follow_up: CardId::new("c_rest"),
            }),
            Err(DecisionError::NotInReview(Backlog))
        ));
    }

    #[test]
    fn an_empty_diff_is_never_resolved_by_hunk_verdicts() {
        assert_eq!(
            resolve_review(&[], &[]),
            ReviewOutcome::Open { decided: 0, of: 0 }
        );
    }

    #[test]
    fn the_follow_up_reads_as_the_work_that_is_left() {
        let id = CardId::new("c_src");
        let (board, _) = in_review(&id, "builder");
        let card = board.get(&id).unwrap();
        let diff = diff_of(1);
        let title = follow_up_title(
            card,
            &[HunkVerdict {
                hunk: diff[0].clone(),
                approved: false,
                by: Actor::Human,
                reason: "the clamp is off by one".into(),
            }],
        );
        // The first line is the prompt's subject; everything the agent needs
        // is under it.
        assert_eq!(one_line(&title), "Rework 1 hunk sent back on clamp the budget");
        assert!(title.contains("src/f0.rs @@ -1,4 +1,6 @@ fn f0("));
        assert!(title.contains("the clamp is off by one"));
    }

    #[test]
    fn an_older_log_without_hunks_still_loads() {
        // Exactly the shape written before hunk review existed.
        let raw = concat!(
            r#"{"type":"card_created","card_id":"c1","title":"old"}"#,
            "\n",
            r#"{"type":"card_moved","card_id":"c1","from":"backlog","to":"ready"}"#,
            "\n",
            r#"{"type":"run_started","card_id":"c1","run_id":"run-1"}"#,
            "\n",
            r#"{"type":"run_finished","card_id":"c1","run_id":"run-1","outcome":"completed"}"#,
            "\n",
            r#"{"type":"card_approved","card_id":"c1","by":"human","reason":"fine"}"#,
        );
        let mut board = Board::default();
        for line in raw.split('\n') {
            board.apply(&serde_json::from_str::<Event>(line).expect(line));
        }
        let card = board.get(&CardId::new("c1")).unwrap();
        assert_eq!(card.status, Done);
        assert!(card.last_review.clone().unwrap().hunks.is_empty());
        assert!(card.hunk_verdicts.is_empty());
    }
}
