//! The inbox: where the Director's improvement proposals land, and the
//! once-a-day gate for his end-of-day look.
//!
//! The chain is: he notices a pattern (self_report shows him his own week), he
//! proposes with `propose_improvement`, and the proposal waits here. A
//! proposal is never a card — accepting one is the operator's decision, and an
//! accepted card is born in the harness's own project (`_harness`), never in
//! whatever happens to be open (#72).
//!
//! Pure state and transitions; persistence is the shell's job.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Open,
    Accepted,
    Dismissed,
}

/// One proposal waiting on the operator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Proposal {
    pub id: String,
    pub created_ms: u64,
    pub title: String,
    /// What repeats — the evidence, in counts, not a transcript dump.
    pub observation: String,
    /// What he suggests about it.
    pub proposal: String,
    pub status: ProposalStatus,
    /// Set when the operator accepts: where the card was born.
    pub card_id: Option<String>,
    pub project_id: Option<String>,
}

/// The whole inbox plus the mark of the last end-of-day look.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InboxState {
    pub proposals: Vec<Proposal>,
    #[serde(default)]
    pub last_look_ms: u64,
}

/// How long between looks. Patterns only show over days, so once per day is
/// plenty — anything tighter would be reading tea leaves.
const LOOK_INTERVAL_MS: u64 = 20 * 3_600_000;

/// Is the end-of-day look due? Never looked means due now.
pub fn look_due(last_look_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(last_look_ms) >= LOOK_INTERVAL_MS
}

/// How many proposals are kept. An inbox that grows forever stops being read;
/// old settled ones make room for new signals.
const KEPT: usize = 50;

impl InboxState {
    /// Add a proposal. A repeat of something still open folds into the
    /// existing entry instead of stacking copies — twelve refusals should
    /// strengthen one proposal, not spawn twelve.
    pub fn propose(
        &mut self,
        id: String,
        now_ms: u64,
        title: &str,
        observation: &str,
        suggestion: &str,
    ) -> Proposal {
        let title = title.trim();
        if let Some(existing) = self
            .proposals
            .iter_mut()
            .find(|p| p.status == ProposalStatus::Open && p.title.eq_ignore_ascii_case(title))
        {
            existing.observation = observation.trim().to_string();
            existing.proposal = suggestion.trim().to_string();
            existing.created_ms = now_ms;
            return existing.clone();
        }

        let proposal = Proposal {
            id,
            created_ms: now_ms,
            title: title.to_string(),
            observation: observation.trim().to_string(),
            proposal: suggestion.trim().to_string(),
            status: ProposalStatus::Open,
            card_id: None,
            project_id: None,
        };
        self.proposals.insert(0, proposal.clone());
        self.truncate();
        proposal
    }

    fn truncate(&mut self) {
        if self.proposals.len() <= KEPT {
            return;
        }
        // Newest first; keep the open ones in preference to settled history.
        let mut keep = vec![false; self.proposals.len()];
        let mut budget = KEPT;
        for (i, p) in self.proposals.iter().enumerate() {
            if p.status == ProposalStatus::Open && budget > 0 {
                keep[i] = true;
                budget -= 1;
            }
        }
        for (i, p) in self.proposals.iter().enumerate() {
            if p.status != ProposalStatus::Open && budget > 0 {
                keep[i] = true;
                budget -= 1;
            }
        }
        let mut kept: Vec<Proposal> = self
            .proposals
            .iter()
            .zip(keep)
            .filter(|(_, k)| *k)
            .map(|(p, _)| p.clone())
            .collect();
        std::mem::swap(&mut self.proposals, &mut kept);
    }

    /// Accept: records where the card was born. Creating the card is the
    /// caller's job — this module knows nothing about boards.
    pub fn accept(&mut self, id: &str, project_id: &str, card_id: &str) -> Option<Proposal> {
        let slot = self.proposals.iter_mut().find(|p| p.id == id)?;
        if slot.status != ProposalStatus::Open {
            return None;
        }
        slot.status = ProposalStatus::Accepted;
        slot.project_id = Some(project_id.to_string());
        slot.card_id = Some(card_id.to_string());
        Some(slot.clone())
    }

    pub fn dismiss(&mut self, id: &str) -> Option<Proposal> {
        let slot = self.proposals.iter_mut().find(|p| p.id == id)?;
        if slot.status != ProposalStatus::Open {
            return None;
        }
        slot.status = ProposalStatus::Dismissed;
        Some(slot.clone())
    }

    pub fn open(&self) -> Vec<&Proposal> {
        self.proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Open)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn a_repeat_of_an_open_title_folds_instead_of_stacking() {
        let mut inbox = InboxState::default();
        let first = inbox.propose("p1".into(), NOW, "create_card refused", "12× no project", "fix default");
        let again = inbox.propose("p2".into(), NOW + 5, "CREATE_CARD REFUSED", "40× no project", "better fix");
        assert_eq!(first.id, again.id, "same open proposal, updated");
        assert_eq!(inbox.open().len(), 1);
        assert_eq!(again.observation, "40× no project");

        // Once settled, the same title is a new signal again.
        inbox.dismiss(&first.id);
        inbox.propose("p3".into(), NOW + 10, "create_card refused", "again", "");
        assert_eq!(inbox.open().len(), 1);
    }

    #[test]
    fn accept_and_dismiss_settle_and_refuse_to_settle_twice() {
        let mut inbox = InboxState::default();
        let p = inbox.propose("p1".into(), NOW, "t", "o", "s");
        assert!(inbox.accept(&p.id, "_harness", "c_9").is_some());
        assert!(inbox.accept(&p.id, "_harness", "c_9").is_none(), "already settled");
        assert!(inbox.dismiss(&p.id).is_none());
        assert_eq!(
            inbox.proposals[0].card_id.as_deref(),
            Some("c_9"),
            "accept records where the card was born"
        );
    }

    #[test]
    fn the_look_is_due_once_a_day_not_every_turn() {
        assert!(look_due(0, NOW));
        assert!(!look_due(NOW - 3600_000, NOW), "an hour ago is not a day");
        assert!(look_due(NOW - LOOK_INTERVAL_MS, NOW));
        assert!(look_due(NOW - LOOK_INTERVAL_MS - 1, NOW));
    }

    #[test]
    fn truncation_keeps_open_proposals_over_history() {
        let mut inbox = InboxState::default();
        for n in 0..60 {
            inbox.propose(format!("p{n}"), NOW + n, &format!("t{n}"), "o", "s");
            inbox.dismiss(&format!("p{n}"));
        }
        // One open proposal arrives after the pile of settled ones.
        inbox.propose("fresh".into(), NOW + 100, "open one", "o", "s");
        assert!(inbox.proposals.len() <= KEPT + 2);
        assert!(inbox.proposals.iter().any(|p| p.title == "open one"));
    }
}
