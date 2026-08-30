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

use harness_app::agents::{self, AgentProfile};
use harness_ports::{AgentMessage, MessageHook, ReviewHook, ReviewRequest};

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

/// Who reads this diff, and in whose conversation.
///
/// The board's owner when it has one — a profile fenced to that project, able
/// to change a board and able to hold a conversation. Otherwise the Director,
/// which is every project that nobody was put in charge of, and was every
/// project before fences existed.
///
/// A misconfigured owner does not stall the card: if the profile fenced to this
/// board cannot review (no delegation, no chat, paused), `owner_of` does not
/// return it and the work goes to the Director — who is told why, so the answer
/// to "why am I reviewing Ana's board" is in front of him rather than in a
/// settings screen he cannot see.
async fn reviewer_for(ws: &Arc<Workspace>, project_id: &str) -> Option<(AgentProfile, Option<String>)> {
    let crew = ws.agents().await;
    if let Some(owner) = harness_app::agents::owner_of(&crew, project_id) {
        return Some((owner.clone(), None));
    }
    // Nobody owns it, or whoever is fenced to it cannot do the job. Both end
    // with the Director; only the second is worth a sentence.
    let stalled = crew
        .iter()
        .find(|a| a.id != agents::DIRECTOR_ID && a.project.as_deref() == Some(project_id))
        .map(|a| {
            format!(
                "{} is set to own this board but cannot review it right now \
                 (it needs delegation, a chat, and not to be paused), so this came to you.",
                a.name
            )
        });
    let director = ws.agent_exact(agents::DIRECTOR_ID).await?;
    Some((director, stalled))
}

/// Which conversation the review lands in.
///
/// The most recently touched Director thread that is not archived — "the one
/// I'm talking to", as far as the app can honestly know it. Falling back to a
/// fresh one matters: the very first card of a new install finishes before the
/// operator has ever opened the chat, and a review that quietly does not happen
/// is the old silence wearing different clothes.
async fn conversation_for(ws: &Arc<Workspace>, profile_id: &str) -> Option<String> {
    let mut mine: Vec<_> = ws
        .conversations(false)
        .await
        .into_iter()
        .filter(|c| c.profile_id == profile_id)
        .collect();
    mine.sort_by_key(|c| std::cmp::Reverse(c.updated_ms));
    if let Some(c) = mine.into_iter().next() {
        return Some(c.id);
    }
    ws.new_conversation(Some(profile_id.to_string()), None)
        .await
        .map(|c| c.id)
        .map_err(|e| eprintln!("could not open a conversation for the review: {e}"))
        .ok()
}

async fn take(ws: &Arc<Workspace>, request: ReviewRequest) -> bool {
    // Somebody who cannot hold a conversation cannot be asked to review in one.
    // Rather than reviving the headless path behind the operator's back, the
    // card waits for them — and the notice says so.
    let Some((reviewer, why_me)) = reviewer_for(ws, &request.project_id).await else {
        return false;
    };
    if !reviewer.can_chat() || reviewer.paused {
        return false;
    }

    let Some(conversation_id) = conversation_for(ws, &reviewer.id).await else {
        return false;
    };

    let mut prompt = harness_app::director::review_prompt(&request.card_id, &request.title);
    if let Some(said) = why_me {
        prompt.push_str("\n\n");
        prompt.push_str(&said);
    }
    match crate::chat::queue(ws, conversation_id, prompt, Vec::new(), None).await {
        Ok(_) => true,
        Err(e) => {
            eprintln!("the review could not be handed to the Director: {e}");
            false
        }
    }
}

/// The other direction: a worker saying something to the Director mid-run.
///
/// Same road as the review, and for the same reason — it ends in a conversation,
/// which the engine knows nothing about. `chat::queue` again, so if he is
/// already mid-turn the agent's words reach him at his next read rather than
/// waiting for him to finish; nothing here blocks the agent, which goes back to
/// work the moment this returns.
pub fn message_hook(ws: &Arc<Workspace>) -> MessageHook {
    let ws = Arc::clone(ws);
    Arc::new(move |message: AgentMessage| {
        let ws = Arc::clone(&ws);
        Box::pin(async move { carry(&ws, message).await })
    })
}

async fn carry(ws: &Arc<Workspace>, message: AgentMessage) -> Result<(), String> {
    // To whoever is in charge of the board this card is on, for the same reason
    // the review goes there: an agent reporting a blocker should reach the
    // person who can act on it, not always the same person.
    let Some((heard_by, _)) = reviewer_for(ws, &message.project_id).await else {
        return Err("there is nobody available to hear that".to_string());
    };
    if !heard_by.can_chat() || heard_by.paused {
        return Err(format!("{} is not available to hear that", heard_by.name));
    }
    let conversation_id = conversation_for(ws, &heard_by.id)
        .await
        .ok_or_else(|| "there is no conversation to carry that to".to_string())?;

    // Quem fala vem com o que foi dito. Sem isso, quatro builders a trabalhar
    // dão quatro mensagens sem dono, e a primeira pergunta dele seria sempre a
    // mesma.
    let who = ws
        .agent_exact(&message.agent_id)
        .await
        .map(|a| a.name)
        .unwrap_or_else(|| message.agent_id.clone());
    let said = format!(
        "{who}, working on {}, says: {}",
        message.card_id, message.text
    );
    crate::chat::queue(ws, conversation_id, said, Vec::new(), None)
        .await
        .map(|_| ())
        .map_err(|e| format!("that could not be carried to the Director: {e}"))
}
