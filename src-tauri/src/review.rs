//! Where a finished diff gets read: inside the Director's own conversation.
//!
//! The engine used to answer this itself. When a card's reviewer was the
//! Director it spawned a second one right there — a fresh session with no
//! memory of anything, `permission_mode: dontAsk`, no inbox — which read the
//! diff, returned a line of JSON and moved the card. It worked, and it was a
//! ghost: two Directors existed, only one of them was the one the operator was
//! talking to, and the other did its work where nobody could see it. Cards left
//! Review with nothing visibly happening, and the Director in the conversation
//! could not answer for a decision it had never made.
//!
//! So the review comes here instead. `chat::queue` is the whole mechanism, and
//! it is the same one the composer uses: if a turn is already running it pushes
//! the request into that run's inbox and he reads it at his next natural read;
//! if not, it starts an ordinary turn. Either way it is *his* session, so he
//! keeps the thread the operator has been on, and either way the answer arrives
//! on `engine://run` keyed by the conversation — which is to say, on screen.
//!
//! He is given no special verdict channel. `read_diff`, `approve_card` and
//! `reject_card` are tools he already holds, and the board event they produce
//! already carries his name. A parsed JSON verdict was a second account of the
//! same decision, and two accounts of one decision can disagree.

use std::sync::Arc;

use harness_app::agents;
use harness_ports::{ReviewHook, ReviewRequest};

use crate::workspace::Workspace;

/// The hook handed to the engine. `false` means nobody took it, and the card
/// stays in Review for the operator — which is the honest outcome, and the one
/// the ghost never allowed.
pub fn hook(ws: &Arc<Workspace>) -> ReviewHook {
    let ws = Arc::clone(ws);
    Arc::new(move |request: ReviewRequest| {
        let ws = Arc::clone(&ws);
        Box::pin(async move { take(&ws, request).await })
    })
}

/// Which conversation the review lands in.
///
/// The most recently touched Director thread that is not archived — "the one
/// I'm talking to", as far as the app can honestly know it. Falling back to a
/// fresh one matters: the very first card of a new install finishes before the
/// operator has ever opened the chat, and a review that quietly does not happen
/// is the old silence wearing different clothes.
async fn conversation_for(ws: &Arc<Workspace>) -> Option<String> {
    let mut mine: Vec<_> = ws
        .conversations(false)
        .await
        .into_iter()
        .filter(|c| c.profile_id == agents::DIRECTOR_ID)
        .collect();
    mine.sort_by_key(|c| std::cmp::Reverse(c.updated_ms));
    if let Some(c) = mine.into_iter().next() {
        return Some(c.id);
    }
    ws.new_conversation(Some(agents::DIRECTOR_ID.to_string()), None)
        .await
        .map(|c| c.id)
        .map_err(|e| eprintln!("could not open a conversation for the review: {e}"))
        .ok()
}

async fn take(ws: &Arc<Workspace>, request: ReviewRequest) -> bool {
    // A Director who cannot hold a conversation cannot be asked to review in
    // one. Rather than reviving the headless path behind the operator's back,
    // the card waits for them — and the notice says so.
    match ws.agent_exact(agents::DIRECTOR_ID).await {
        Some(profile) if profile.can_chat() && !profile.paused => {}
        _ => return false,
    }

    let Some(conversation_id) = conversation_for(ws).await else {
        return false;
    };

    let prompt = harness_app::director::review_prompt(&request.card_id, &request.title);
    match crate::chat::queue(ws, conversation_id, prompt, Vec::new(), None).await {
        Ok(_) => true,
        Err(e) => {
            eprintln!("the review could not be handed to the Director: {e}");
            false
        }
    }
}
