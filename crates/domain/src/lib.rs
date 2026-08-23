use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub String);

impl fmt::Display for RunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Backlog,
    Ready,
    Running,
    Review,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    #[default]
    Human,
    Director,
}

/// The last review a card received, kept on the card so the board can show it
/// without walking the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub by: Actor,
    pub approved: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

fn default_agent() -> String {
    "builder".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    CardCreated { card_id: CardId, title: String },
    CardAssigned { card_id: CardId, agent_id: String },
    CardMoved { card_id: CardId, from: Status, to: Status },
    CardOverridden { card_id: CardId, from: Status, to: Status, reason: String },
    RunStarted { card_id: CardId, run_id: RunId },
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
    },
    CardRejected {
        card_id: CardId,
        reason: String,
        #[serde(default)]
        by: Actor,
    },
    /// The card is gone from the board. The log keeps the fact and the reason;
    /// the branch a run left behind is the shell's business to clean up.
    CardDiscarded {
        card_id: CardId,
        #[serde(default)]
        reason: String,
    },
}

impl Event {
    pub fn card_id(&self) -> &CardId {
        match self {
            Event::CardCreated { card_id, .. }
            | Event::CardAssigned { card_id, .. }
            | Event::CardMoved { card_id, .. }
            | Event::CardOverridden { card_id, .. }
            | Event::RunStarted { card_id, .. }
            | Event::RunFinished { card_id, .. }
            | Event::CardApproved { card_id, .. }
            | Event::CardRejected { card_id, .. }
            | Event::CardDiscarded { card_id, .. } => card_id,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Command {
    CreateCard { card_id: CardId, title: String },
    AssignAgent { card_id: CardId, agent_id: String },
    MoveCard { card_id: CardId, to: Status },
    OverrideCard { card_id: CardId, to: Status, reason: String },
    StartRun { card_id: CardId, run_id: RunId },
    FinishRun {
        card_id: CardId,
        run_id: RunId,
        outcome: RunOutcome,
        cost_usd: Option<f64>,
        turns: Option<u32>,
    },
    ApproveCard { card_id: CardId, by: Actor, reason: String },
    RejectCard { card_id: CardId, reason: String, by: Actor },
    /// Take a card off the board for good.
    DiscardCard { card_id: CardId, reason: String },
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
    NotReady(Status),
    NotRunning(Status),
    NotInReview(Status),
    RunMismatch,
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

    pub fn apply(&mut self, event: &Event) {
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
                    },
                );
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
            Event::RunStarted { card_id, run_id } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = Status::Running;
                    card.current_run = Some(run_id.clone());
                    card.runs += 1;
                    card.last_review = None;
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
            Event::CardApproved { card_id, by, reason } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = Status::Done;
                    card.last_review = Some(Review {
                        by: *by,
                        approved: true,
                        reason: reason.clone(),
                    });
                }
            }
            Event::CardRejected { card_id, reason, by } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = Status::Ready;
                    card.last_review = Some(Review {
                        by: *by,
                        approved: false,
                        reason: reason.clone(),
                    });
                }
            }
            Event::CardDiscarded { card_id, .. } => {
                self.cards.remove(card_id);
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
                Ok(vec![Event::CardDiscarded {
                    card_id: card_id.clone(),
                    reason: reason.trim().to_string(),
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
                Ok(vec![Event::CardOverridden {
                    card_id: card_id.clone(),
                    from: card.status,
                    to: *to,
                    reason: reason.trim().to_string(),
                }])
            }
            Command::StartRun { card_id, run_id } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if card.status != Status::Ready {
                    return Err(DecisionError::NotReady(card.status));
                }
                Ok(vec![Event::RunStarted {
                    card_id: card_id.clone(),
                    run_id: run_id.clone(),
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
            Command::ApproveCard { card_id, by, reason } => {
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
                }])
            }
            Command::RejectCard { card_id, reason, by } => {
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
                }])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Actor, Board, CardId, Command, DecisionError, Event, Status, Status::*};

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
                .decide(&Command::StartRun { card_id: id.clone(), run_id: run.clone() })
                .unwrap()[0],
        );
        let card = board.get(&id).unwrap();
        assert_eq!(card.status, Running);
        assert_eq!(card.current_run, Some(run.clone()));

        assert!(matches!(
            board.decide(&Command::StartRun { card_id: id.clone(), run_id: run.clone() }),
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
                .decide(&Command::StartRun { card_id: id.clone(), run_id: run.clone() })
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
            .decide(&Command::StartRun { card_id: id.clone(), run_id: run2 })
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
                    .decide(&Command::StartRun { card_id: id.clone(), run_id: run.clone() })
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
}
