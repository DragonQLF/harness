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

impl Proposal {
    /// The card an accepted proposal becomes, as text.
    ///
    /// A card's title *is* the prompt the agent is given, and for a long time
    /// only the title survived acceptance: the observation and the reasoning —
    /// the whole body of the proposal — were thrown away the moment the
    /// operator said yes, so the builder arrived with none of the reasons that
    /// motivated the work. The first line stays the one-line request, which is
    /// what the board shows and what the commit subject reads; the body
    /// follows it, which is what the agent reads.
    pub fn as_card_text(&self) -> String {
        let mut out = self.title.trim().to_string();
        if !self.observation.trim().is_empty() {
            out.push_str("\n\nWhat was seen: ");
            out.push_str(self.observation.trim());
        }
        if !self.proposal.trim().is_empty() {
            out.push_str("\n\nWhat was proposed: ");
            out.push_str(self.proposal.trim());
        }
        out
    }
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

    /// The bug: accepting a proposal built the card from `proposal.title`
    /// alone, so the observation and the reasoning died in the inbox — and
    /// since the title is the agent's prompt, the builder got the request
    /// without a single reason behind it.
    #[test]
    fn an_accepted_proposal_carries_its_body_to_the_card() {
        let mut inbox = InboxState::default();
        let p = inbox.propose(
            "p1".into(),
            NOW,
            "widen propose_improvement",
            "four tool refusals in one session, each a real capability hole",
            "say that one occurrence is enough to file",
        );
        let text = p.as_card_text();
        assert_eq!(
            harness_domain::one_line(&text),
            "widen propose_improvement",
            "the first line stays the one-line request"
        );
        assert!(text.contains("four tool refusals"), "the observation survives: {text}");
        assert!(text.contains("one occurrence is enough"), "the reasoning survives: {text}");
        assert!(text.lines().count() > 1, "the body is below the title");
    }

    /// A proposal with nothing under the title is still just a title — no
    /// empty headings, no trailing blank lines.
    #[test]
    fn a_proposal_with_no_body_is_still_one_line() {
        let mut inbox = InboxState::default();
        let p = inbox.propose("p1".into(), NOW, "  just a title  ", "", "");
        assert_eq!(p.as_card_text(), "just a title");
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
