use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Backlog,
    Ready,
    Running,
    Review,
    Done,
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

#[cfg(test)]
mod tests {
    use super::{CardId, Status, Status::*};

    #[test]
    fn card_id_display_and_as_str() {
        let id = CardId::new("card-42");
        assert_eq!(id.to_string(), "card-42");
        assert_eq!(id.as_str(), "card-42");
    }

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
}
