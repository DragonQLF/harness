//! The inbox: where the Director's improvement proposals land, and the
//! once-a-day gate for his end-of-day look.
//!
//! The chain is: he notices a pattern (self_report shows him his own week), he
//! proposes with `propose_improvement`, and the proposal waits here. A
//! proposal is never a card. Accepting one is the operator granting
//! **permission**, not ordering work: nothing is minted, nothing is assigned —
//! the accepted proposal is simply handed back to the Director as a fact in
//! his next turn, and carrying it out is then his to do.
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
    #[ts(type = "number")]
    pub created_ms: u64,
    pub title: String,
    /// What repeats — the evidence, in counts, not a transcript dump.
    pub observation: String,
    /// What he suggests about it.
    pub proposal: String,
    pub status: ProposalStatus,
    /// What the Director *later* did about an accepted proposal, once he did
    /// it. Empty at the moment of acceptance and for as long as it is still
    /// only permission. `serde(default)` because these two are the oldest
    /// fields in the file and an inbox.json written before, or after, this
    /// shape must still load — a proposal on the operator's disk is not
    /// something a format change gets to drop.
    #[serde(default)]
    pub card_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
}

impl Proposal {
    /// The proposal as text: what he is handed back once it is accepted, and
    /// what a card born from it should say.
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
        // Newest first; keep the live ones in preference to settled history.
        // Live now includes an acceptance nobody has acted on yet: that is a
        // permission the operator granted, and pruning it would silently take
        // it back.
        let live = |p: &Proposal| {
            p.status == ProposalStatus::Open
                || (p.status == ProposalStatus::Accepted && p.card_id.is_none())
        };
        let mut keep = vec![false; self.proposals.len()];
        let mut budget = KEPT;
        for (i, p) in self.proposals.iter().enumerate() {
            if live(p) && budget > 0 {
                keep[i] = true;
                budget -= 1;
            }
        }
        for (i, p) in self.proposals.iter().enumerate() {
            if !live(p) && budget > 0 {
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

    /// Accept: permission granted, and nothing else. No project, no card, no
    /// board — accepting used to mint a card on the spot, which made the
    /// operator's "yes" an order rather than a licence, and made acceptance
    /// impossible at all on a machine with nowhere to put the card.
    pub fn accept(&mut self, id: &str) -> Option<Proposal> {
        let slot = self.proposals.iter_mut().find(|p| p.id == id)?;
        if slot.status != ProposalStatus::Open {
            return None;
        }
        slot.status = ProposalStatus::Accepted;
        Some(slot.clone())
    }

    /// The Director carried an accepted proposal out: record where, so it
    /// stops being raised at him every turn.
    pub fn record_action(&mut self, id: &str, project_id: &str, card_id: &str) -> Option<Proposal> {
        let slot = self.proposals.iter_mut().find(|p| p.id == id)?;
        if slot.status != ProposalStatus::Accepted || slot.card_id.is_some() {
            return None;
        }
        slot.project_id = Some(project_id.to_string());
        slot.card_id = Some(card_id.to_string());
        Some(slot.clone())
    }

    /// Settle a proposal. Open means the operator said no; accepted-and-not-yet
    /// acted-on means they changed their mind or handled it themselves — either
    /// way it must stop reaching the Director. An accepted proposal he already
    /// acted on is history and stays as it is.
    pub fn dismiss(&mut self, id: &str) -> Option<Proposal> {
        let slot = self.proposals.iter_mut().find(|p| p.id == id)?;
        let settleable = slot.status == ProposalStatus::Open
            || (slot.status == ProposalStatus::Accepted && slot.card_id.is_none());
        if !settleable {
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

    /// Accepted, and nothing done about it yet: exactly what the Director has
    /// standing permission for and has not used. This is what his next turn is
    /// told, oldest first so the one that has waited longest reads first.
    pub fn awaiting_action(&self) -> Vec<&Proposal> {
        let mut waiting: Vec<&Proposal> = self
            .proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Accepted && p.card_id.is_none())
            .collect();
        waiting.sort_by_key(|p| p.created_ms);
        waiting
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
        assert!(inbox.accept(&p.id).is_some());
        assert!(inbox.accept(&p.id).is_none(), "already settled");
        assert!(inbox.dismiss(&p.id).is_some(), "the operator may still withdraw permission");
        assert!(inbox.dismiss(&p.id).is_none(), "and only once");
    }

    /// The whole point of the change: accepting is permission, not work.
    /// It needs no project, touches no board, and leaves nothing behind but
    /// the operator's yes.
    #[test]
    fn accepting_needs_no_project_and_mints_nothing() {
        let mut inbox = InboxState::default();
        let p = inbox.propose("p1".into(), NOW, "widen the pathguard", "12× refused", "allow it");
        let accepted = inbox.accept(&p.id).expect("accept takes an id and nothing else");
        assert_eq!(accepted.status, ProposalStatus::Accepted);
        assert_eq!(accepted.card_id, None, "no card is born on accept");
        assert_eq!(accepted.project_id, None, "and nowhere is chosen for it");
        assert!(inbox.open().is_empty(), "it left the operator's queue");
    }

    /// Acceptance is what reaches the Director, and it has to stop reaching
    /// him: once he has acted, and if the operator changes their mind.
    #[test]
    fn an_accepted_proposal_waits_for_him_until_it_is_acted_on_or_withdrawn() {
        let mut inbox = InboxState::default();
        let a = inbox.propose("p1".into(), NOW, "a", "o", "s");
        let b = inbox.propose("p2".into(), NOW + 1, "b", "o", "s");
        inbox.propose("p3".into(), NOW + 2, "c", "o", "s");
        assert!(inbox.awaiting_action().is_empty(), "open is not permission");

        inbox.accept(&a.id);
        inbox.accept(&b.id);
        assert_eq!(
            inbox.awaiting_action().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["p1", "p2"],
            "oldest first"
        );

        assert!(inbox.record_action(&a.id, "_harness", "c_9").is_some());
        assert!(
            inbox.record_action(&a.id, "_harness", "c_10").is_none(),
            "acted on once"
        );
        assert!(inbox.dismiss(&b.id).is_some(), "the operator settles the other");
        assert!(inbox.awaiting_action().is_empty());
        assert_eq!(inbox.proposals.iter().find(|p| p.id == "p1").unwrap().card_id.as_deref(), Some("c_9"));
    }

    /// The other half of the durability question, asked rather than assumed:
    /// a permission granted before a restart must still be a permission after
    /// one. It is, and by construction — acceptance is a *state* written into
    /// inbox.json, not a delivery, so `awaiting_action` recomputes it from the
    /// file every time. That is the honest difference from a verdict, which is
    /// news and is said once.
    #[test]
    fn an_accepted_proposal_survives_a_restart() {
        let mut inbox = InboxState::default();
        let p = inbox.propose("p1".into(), NOW, "widen the pathguard", "12×", "allow it");
        inbox.accept(&p.id);

        let on_disk = serde_json::to_string(&inbox).unwrap();
        let reloaded: InboxState = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(
            reloaded.awaiting_action().len(),
            1,
            "the permission outlives the process"
        );

        // And once he has acted, the restart does not raise it again.
        let mut reloaded = reloaded;
        reloaded.record_action(&p.id, "_harness", "c_9");
        let again: InboxState =
            serde_json::from_str(&serde_json::to_string(&reloaded).unwrap()).unwrap();
        assert!(again.awaiting_action().is_empty());
    }

    /// This is state on a real machine. An inbox.json written by the version
    /// where accepting minted a card must still load — with its record intact —
    /// and one written without those fields at all must not crash the app on
    /// startup.
    #[test]
    fn an_old_inbox_file_still_loads() {
        let old = r#"{
          "proposals": [
            {
              "id": "prp_1", "created_ms": 1800000000000, "title": "old accepted",
              "observation": "o", "proposal": "s", "status": "accepted",
              "card_id": "c_1", "project_id": "_harness"
            },
            {
              "id": "prp_2", "created_ms": 1800000000001, "title": "no such fields",
              "observation": "o", "proposal": "s", "status": "open"
            }
          ],
          "last_look_ms": 1800000000000
        }"#;
        let inbox: InboxState = serde_json::from_str(old).expect("old inbox.json must still load");
        assert_eq!(inbox.proposals.len(), 2, "nothing is discarded");
        assert_eq!(inbox.proposals[0].card_id.as_deref(), Some("c_1"));
        assert_eq!(inbox.proposals[1].card_id, None);
        assert!(
            inbox.awaiting_action().is_empty(),
            "an old acceptance already has its card; it is not pending permission"
        );
        assert_eq!(inbox.open().len(), 1);
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
