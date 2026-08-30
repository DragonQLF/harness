//! The project side of the Director: asking for the diff a finished run
//! produced to be read. Conversation lives at workspace level, in
//! `harness_app::director` — one Director watches every board, and since the
//! review moved into that conversation there is exactly one of him.

use std::sync::Arc;

use harness_domain::{CardId, RunId};
use harness_ports::RunEvent;

use crate::Engine;

impl Engine {
    /// Ask whoever the shell nominated to read the diff this run produced.
    ///
    /// This used to spawn a second Director here — a fresh session with no
    /// memory of the conversation, `dontAsk`, and no inbox — which read the
    /// diff and moved the card on its own. Two Directors existed, and only one
    /// of them was the one the operator was talking to; the other worked
    /// invisibly, which is why finished work kept leaving Review with nobody
    /// apparently doing anything.
    ///
    /// Now the engine only asks. The shell runs the review inside the
    /// Director's own conversation, where the operator can watch it happen and
    /// where it still remembers what they have been discussing. Nothing here
    /// knows any of that: this sends four facts down a hook and takes a yes or
    /// a no.
    pub(crate) async fn run_director_review(&mut self, card_id: CardId, run_id: RunId) {
        let Some(hook) = self.review.clone() else {
            // Nobody to ask. Better a card that waits than a card moved by
            // something the operator cannot see.
            self.emit_run(
                &card_id,
                &run_id,
                RunEvent::Notice {
                    text: "no reviewer is available, so the card is waiting for you".into(),
                },
            );
            return;
        };
        let Some(session) = self.sessions.get(&card_id) else {
            return;
        };
        let request = harness_ports::ReviewRequest {
            card_id: card_id.to_string(),
            run_id: run_id.to_string(),
            title: self
                .board
                .get(&card_id)
                .map(|c| c.title.clone())
                .unwrap_or_else(|| card_id.to_string()),
            worktree: session.worktree.to_string_lossy().into_owned(),
        };

        // Off the actor: handing the review over means waiting for a
        // conversation to accept it, and the board must not stand still for
        // that.
        let runs_tx = self.runs_tx.clone();
        let clock = Arc::clone(&self.clock);
        let project_id = self.config.project_id.clone();
        tokio::spawn(async move {
            let taken = hook(request).await;
            // Only the refusal is worth a line. When it is taken, the review
            // itself is the announcement — it happens in front of the operator.
            if !taken {
                let _ = runs_tx.send(crate::RunUpdate {
                    project_id,
                    card_id,
                    run_id,
                    ts_ms: clock.now_millis(),
                    event: RunEvent::Notice {
                        text: "the Director could not take the review, so it is waiting for you"
                            .into(),
                    },
                });
            }
        });
    }
}
