//! Verdicts an automatic review produced, waiting for the Director's next turn.
//!
//! Decision #12 made `reviewer` a profile field, so a card can be judged by the
//! Director without anybody asking. Decision #19 then removed `director_chat`
//! and `Msg::DirectorChat` from the engine — correctly: the engine has no
//! business knowing that conversations exist. What nobody decided was the
//! consequence: the verdict had nowhere left to go, so the Director in the
//! conversation never learned that his own reviewer had judged anything. The
//! silence was a side effect of a good refactor, not a choice.
//!
//! The fix is the road `outside_work` already uses, and the one accepted
//! proposals now use: the fact is **stored on one side and fetched on the
//! other**. The engine keeps doing exactly what it already did — persist
//! `CardApproved` / `CardRejected` with the actor on them — and the chat side
//! comes and gets it. No handle, no channel, no notion of conversation
//! anywhere below this module.
//!
//! Who reviewed is already typed, which is what makes the operator's brake
//! honest here: only `Actor::Director` is a verdict he needs to hear about.
//! `reviewer: you` leaves an `Actor::Human` review, and `reviewer: nobody`
//! auto-approves as `Actor::Human` too — both stay silent, because a decision
//! the operator took is not news to the Director.

use std::path::{Path, PathBuf};

use harness_domain::{Actor, Event};
use serde::{Deserialize, Serialize};

/// One automatic verdict, as his next turn should read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    pub project_id: String,
    pub card_id: String,
    /// The card's own words, when the board could still be asked for them.
    pub title: String,
    pub approved: bool,
    pub reason: String,
}

/// A board event, as a verdict he should hear about — or nothing.
///
/// `None` for every human decision. The operator approving a card on the
/// Review screen is not news to the Director; his own reviewer's judgement is.
pub fn from_event(project_id: &str, title: &str, event: &Event) -> Option<Verdict> {
    let (card_id, by, reason, approved) = match event {
        Event::CardApproved {
            card_id, by, reason, ..
        } => (card_id, by, reason, true),
        Event::CardRejected {
            card_id, by, reason, ..
        } => (card_id, by, reason, false),
        _ => return None,
    };
    if *by != Actor::Director {
        return None;
    }
    Some(Verdict {
        project_id: project_id.to_string(),
        card_id: card_id.to_string(),
        title: title.trim().to_string(),
        approved,
        reason: reason.trim().to_string(),
    })
}

/// How many verdicts are held. A Director who was away for a hundred reviews
/// needs the last few, not a hundred-line prompt.
const KEPT: usize = 10;

/// Verdicts that happened while nobody was talking to him.
///
/// Delivered once and then past, the same discipline `outside_work` has: a
/// verdict repeated every turn stops being information and becomes wallpaper,
/// and the board in his prompt already carries the standing state of every
/// card. This is the *news*, not the record.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Pending {
    #[serde(default)]
    waiting: Vec<Verdict>,
}

impl Pending {
    /// File a verdict. A card judged twice keeps only the later verdict —
    /// a superseded one is not something to hand him alongside its successor.
    pub fn record(&mut self, verdict: Verdict) {
        self.waiting
            .retain(|v| !(v.card_id == verdict.card_id && v.project_id == verdict.project_id));
        self.waiting.push(verdict);
        if self.waiting.len() > KEPT {
            let cut = self.waiting.len() - KEPT;
            self.waiting.drain(..cut);
        }
    }

    /// Hand over everything waiting and forget it. Called once, by the turn
    /// that is about to tell him.
    pub fn take(&mut self) -> Vec<Verdict> {
        std::mem::take(&mut self.waiting)
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }
}

/// The same thing, on disk.
///
/// A review finishes on its own schedule, and the operator closes the window
/// when they are done for the day — so the most likely moment for a verdict to
/// arrive is one where nobody is there to be told. Held only in memory it
/// would be gone by the time he could hear it, which is the original bug in a
/// smaller shape.
///
/// **The removal is persisted, not only the arrival.** `take` is what makes a
/// verdict news rather than wallpaper; if the take lived in memory alone, a
/// restart between the take and the next write would hand him the same verdict
/// a second time — and the whole point of the discipline is that it is said
/// once.
///
/// Writing is `paths::write_json`, which is tmp-then-rename, so a crash mid-write
/// cannot truncate what the next start reads. Reading is
/// `paths::read_json_or_default`: an absent file is an old install, an
/// unreadable one is a bad day, and both are an empty store. Relay opening
/// with no verdicts is fine; Relay refusing to open because of this file is
/// not.
#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    pending: Pending,
}

impl Store {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let pending = crate::paths::read_json_or_default(&path);
        Self { path, pending }
    }

    pub fn record(&mut self, verdict: Verdict) {
        self.pending.record(verdict);
        self.flush();
    }

    pub fn take(&mut self) -> Vec<Verdict> {
        let taken = self.pending.take();
        if !taken.is_empty() {
            self.flush();
        }
        taken
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// A verdict that could not be written is still a verdict he gets this
    /// session: the failure is said aloud and the run goes on, exactly as the
    /// inbox does. Losing the app over it would be the worse trade.
    fn flush(&self) {
        if let Err(e) = crate::paths::write_json(&self.path, &self.pending) {
            eprintln!("could not save the review verdicts: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_domain::CardId;

    fn approved(by: Actor) -> Event {
        Event::CardApproved {
            card_id: CardId::new("c_7"),
            by,
            reason: "the diff matches the card".into(),
            hunks: Vec::new(),
        }
    }

    /// `reviewer: director` is the whole point: he judged it, so he is told.
    #[test]
    fn a_director_review_becomes_a_verdict_he_receives() {
        let v = from_event("proj", "widen the pathguard", &approved(Actor::Director))
            .expect("his own reviewer's verdict is news");
        assert_eq!(v.card_id, "c_7");
        assert_eq!(v.title, "widen the pathguard");
        assert!(v.approved);
        assert_eq!(v.reason, "the diff matches the card");

        let sent_back = Event::CardRejected {
            card_id: CardId::new("c_8"),
            reason: "widens permissions the card never asked for".into(),
            by: Actor::Director,
            hunks: Vec::new(),
        };
        let v = from_event("proj", "t", &sent_back).expect("a rejection is news too");
        assert!(!v.approved);
        assert_eq!(v.reason, "widens permissions the card never asked for");
    }

    /// `you` and `nobody` are the operator's brake. Both leave an `Actor::Human`
    /// review — the first because they read the diff themselves, the second
    /// because the engine closes the card on their behalf — and neither is
    /// something to report back to the Director as a judgement of his.
    #[test]
    fn the_operators_brake_stays_silent() {
        assert_eq!(
            from_event("proj", "t", &approved(Actor::Human)),
            None,
            "reviewer: you — the operator decided, that is not his reviewer speaking"
        );
        // What `Reviewer::Nobody` actually persists, word for word.
        let auto = Event::CardApproved {
            card_id: CardId::new("c_9"),
            by: Actor::Human,
            reason: "no reviewer configured for this agent".into(),
            hunks: Vec::new(),
        };
        assert_eq!(from_event("proj", "t", &auto), None, "reviewer: nobody");
    }

    #[test]
    fn events_that_are_not_verdicts_are_not_verdicts() {
        let moved = Event::CardMoved {
            card_id: CardId::new("c_7"),
            from: harness_domain::Status::Ready,
            to: harness_domain::Status::Running,
        };
        assert_eq!(from_event("proj", "t", &moved), None);
    }

    /// Delivered once, then past — `outside_work`'s discipline. The standing
    /// state of a card is already in the board he is handed; this is the news.
    #[test]
    fn a_verdict_is_handed_over_once() {
        let mut pending = Pending::default();
        pending.record(from_event("p", "t", &approved(Actor::Director)).unwrap());
        assert!(!pending.is_empty());
        assert_eq!(pending.take().len(), 1);
        assert!(pending.is_empty(), "a second turn is not told again");
        assert!(pending.take().is_empty());
    }

    #[test]
    fn a_card_judged_twice_keeps_only_the_later_verdict() {
        let mut pending = Pending::default();
        pending.record(from_event("p", "t", &approved(Actor::Director)).unwrap());
        pending.record(Verdict {
            project_id: "p".into(),
            card_id: "c_7".into(),
            title: "t".into(),
            approved: false,
            reason: "on second reading it skips the tests".into(),
        });
        let handed = pending.take();
        assert_eq!(handed.len(), 1, "not the verdict and its successor");
        assert!(!handed[0].approved);
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("harness-verdicts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("verdicts.json")
    }

    fn verdict(card: &str) -> Verdict {
        Verdict {
            project_id: "_harness".into(),
            card_id: card.into(),
            title: "widen the pathguard".into(),
            approved: false,
            reason: "it skips the tests".into(),
        }
    }

    /// The window is shut when most reviews finish. A verdict that only lived
    /// in memory would be gone by the time he could be told — the original bug
    /// in a smaller shape.
    #[test]
    fn a_verdict_filed_while_the_window_was_shut_is_there_when_it_opens() {
        let path = scratch("roundtrip");
        let mut store = Store::open(&path);
        store.record(verdict("c_7"));
        drop(store);

        let mut reopened = Store::open(&path);
        assert!(!reopened.is_empty(), "the file outlived the process");
        let handed = reopened.take();
        assert_eq!(handed.len(), 1);
        assert_eq!(handed[0].card_id, "c_7");
        assert_eq!(handed[0].reason, "it skips the tests");
        assert!(!handed[0].approved);
    }

    /// Take-once has to survive too: the removal is written, not only the
    /// arrival. Otherwise a restart between the two says it all over again.
    #[test]
    fn a_taken_verdict_does_not_come_back_after_a_reload() {
        let path = scratch("taken");
        let mut store = Store::open(&path);
        store.record(verdict("c_7"));
        assert_eq!(store.take().len(), 1);
        drop(store);

        let mut reopened = Store::open(&path);
        assert!(reopened.is_empty(), "said once, and once only");
        assert!(reopened.take().is_empty());
    }

    /// An old install has no such file, and a corrupt one is a bad day. Both
    /// are an empty store — never a refusal to start.
    #[test]
    fn a_missing_or_unreadable_file_is_an_empty_store() {
        let path = scratch("missing");
        assert!(Store::open(&path).is_empty(), "no migration step needed");

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(Store::open(&path).is_empty(), "corrupt reads as empty");
    }

    /// The cap is what keeps the file from growing without bound, so it has to
    /// hold across the save and the load, not only in memory.
    #[test]
    fn the_cap_holds_across_a_save_and_a_load() {
        let path = scratch("cap");
        let mut store = Store::open(&path);
        for n in 0..40 {
            store.record(verdict(&format!("c_{n}")));
        }
        drop(store);

        let mut reopened = Store::open(&path);
        let handed = reopened.take();
        assert_eq!(handed.len(), KEPT);
        assert_eq!(handed[KEPT - 1].card_id, "c_39", "the newest survive");
    }

    #[test]
    fn a_long_absence_does_not_become_a_hundred_line_prompt() {
        let mut pending = Pending::default();
        for n in 0..40 {
            pending.record(Verdict {
                project_id: "p".into(),
                card_id: format!("c_{n}"),
                title: "t".into(),
                approved: true,
                reason: "fine".into(),
            });
        }
        let handed = pending.take();
        assert_eq!(handed.len(), KEPT);
        assert_eq!(handed[KEPT - 1].card_id, "c_39", "the newest survive");
    }
}
