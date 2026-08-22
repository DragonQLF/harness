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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub id: CardId,
    pub title: String,
    pub status: Status,
    #[serde(default)]
    pub current_run: Option<RunId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    CardCreated { card_id: CardId, title: String },
    CardMoved { card_id: CardId, from: Status, to: Status },
    CardOverridden { card_id: CardId, from: Status, to: Status, reason: String },
    RunStarted { card_id: CardId, run_id: RunId },
    RunFinished { card_id: CardId, run_id: RunId, outcome: RunOutcome },
    CardApproved { card_id: CardId },
    CardRejected { card_id: CardId, reason: String },
}

impl Event {
    pub fn card_id(&self) -> &CardId {
        match self {
            Event::CardCreated { card_id, .. }
            | Event::CardMoved { card_id, .. }
            | Event::CardOverridden { card_id, .. }
            | Event::RunStarted { card_id, .. }
            | Event::RunFinished { card_id, .. } | Event::CardApproved { card_id } | Event::CardRejected { card_id, .. } => card_id,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Command {
    CreateCard { card_id: CardId, title: String },
    MoveCard { card_id: CardId, to: Status },
    OverrideCard { card_id: CardId, to: Status, reason: String },
    StartRun { card_id: CardId, run_id: RunId },
    FinishRun { card_id: CardId, run_id: RunId, outcome: RunOutcome },
    ApproveCard { card_id: CardId },
    RejectCard { card_id: CardId, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionError {
    CardNotFound(CardId),
    DuplicateCard(CardId),
    IllegalMove { from: Status, to: Status },
    SameStatus(Status),
    EmptyTitle,
    EmptyReason,
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
                    },
                );
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
                }
            }
            Event::RunFinished { card_id, outcome, .. } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = match outcome {
                        RunOutcome::Completed => Status::Review,
                        RunOutcome::Cancelled | RunOutcome::Failed => Status::Ready,
                    };
                    card.current_run = None;
                }
            }
            Event::CardApproved { card_id } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = Status::Done;
                }
            }
            Event::CardRejected { card_id, .. } => {
                if let Some(card) = self.cards.get_mut(card_id) {
                    card.status = Status::Ready;
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
            Command::FinishRun { card_id, run_id, outcome } => {
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
                }])
            }
            Command::ApproveCard { card_id } => {
                let card = self
                    .cards
                    .get(card_id)
                    .ok_or_else(|| DecisionError::CardNotFound(card_id.clone()))?;
                if card.status != Status::Review {
                    return Err(DecisionError::NotInReview(card.status));
                }
                Ok(vec![Event::CardApproved {
                    card_id: card_id.clone(),
                }])
            }
            Command::RejectCard { card_id, reason } => {
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
                }])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Board, CardId, Command, DecisionError, Status, Status::*};

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
            }),
            Err(DecisionError::RunMismatch)
        ));
    }
}
