//! A live turn's inbox: what the operator typed while the model was working.
//!
//! Relay used to refuse the composer for the whole length of a turn, so a
//! correction could only be said after the work it was correcting. This is the
//! other half of the fix — the sidecar's streaming input is the mechanism, and
//! this is the bookkeeping that mechanism needs and cannot do itself.
//!
//! Three facts have to survive, and none of them can be inferred from a
//! channel:
//!
//! - **order**, because two corrections in the wrong order are two different
//!   instructions;
//! - **what is still undelivered**, so a run that ends first does not swallow
//!   a message — one queue may hold messages that were never handed over and
//!   messages that were handed over but not yet acknowledged, and both are
//!   undelivered until the run says otherwise;
//! - **that the inbox is shut**, so a message typed a millisecond after the
//!   turn ended is refused here rather than lost somewhere downstream.
//!
//! No I/O and no clock: the shell mints the ids and the adapter does the
//! writing, so the ordering is testable without a sidecar.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use harness_ports::{InboxPort, QueuedMessage};
use tokio::sync::Semaphore;

/// The queue one conversation's live turn reads from.
///
/// `waiting` and `handed` are two halves of the same answer. A message leaves
/// `waiting` when the adapter takes it, and only leaves `handed` when the run
/// says the model has it; anything in either when the turn ends never reached
/// the model.
pub struct Queue {
    conversation_id: String,
    state: Mutex<State>,
    /// One permit per waiting message. `acquire` is cancel-safe — a dropped
    /// future takes nothing — which is what lets the adapter race this against
    /// the sidecar's own output without losing a line.
    ready: Semaphore,
    seq: AtomicU64,
}

#[derive(Default)]
struct State {
    waiting: VecDeque<QueuedMessage>,
    handed: Vec<QueuedMessage>,
    closed: bool,
}

impl Queue {
    pub fn new(conversation_id: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            conversation_id: conversation_id.into(),
            state: Mutex::new(State::default()),
            ready: Semaphore::new(0),
            seq: AtomicU64::new(0),
        })
    }

    /// Accept a message for the turn in flight.
    ///
    /// `Err` means the turn is over and this has to become one of its own —
    /// the one case where the caller must not treat the message as queued.
    pub fn push(&self, text: impl Into<String>) -> Result<QueuedMessage, String> {
        let n = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let message = QueuedMessage {
            id: format!("{}#q{n}", self.conversation_id),
            text: text.into(),
        };
        {
            let mut state = self.lock();
            if state.closed {
                return Err("that turn has already finished".to_string());
            }
            state.waiting.push_back(message.clone());
        }
        // After the push, never before: a reader woken by the permit must find
        // the message already in the queue.
        self.ready.add_permits(1);
        Ok(message)
    }

    /// Shut the inbox and answer with everything that never reached the model,
    /// oldest first. Handed-but-unacknowledged messages come first because
    /// they are the older ones — they left the queue before anything still
    /// waiting was even looked at.
    pub fn close(&self) -> Vec<QueuedMessage> {
        let mut state = self.lock();
        state.closed = true;
        let mut lost = std::mem::take(&mut state.handed);
        lost.extend(state.waiting.drain(..));
        drop(state);
        // Wakes any reader parked on `next`, which then answers `None`.
        self.ready.close();
        lost
    }

    /// What is queued and not yet handed over. For tests and for anyone asking
    /// what a stop would drop.
    pub fn waiting(&self) -> Vec<QueuedMessage> {
        self.lock().waiting.iter().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while holding this lock would only ever be a panic inside
        // this file; carrying on with the state as it was is better than
        // poisoning every later message.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl InboxPort for Queue {
    fn next(self: Arc<Self>) -> Pin<Box<dyn Future<Output = Option<QueuedMessage>> + Send>> {
        Box::pin(async move {
            // The permit is what makes this cancel-safe: a future dropped
            // before it resolves has taken nothing, so the message is still
            // there for the next read.
            let permit = self.ready.acquire().await.ok()?;
            permit.forget();
            let mut state = self.lock();
            let message = state.waiting.pop_front()?;
            state.handed.push(message.clone());
            Some(message)
        })
    }

    fn mark_read(&self, id: &str) {
        let mut state = self.lock();
        state.handed.retain(|m| m.id != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_ports::InboxPort;

    fn queue() -> Arc<Queue> {
        Queue::new("chat_1")
    }

    #[test]
    fn two_messages_queued_quickly_keep_their_order() {
        let q = queue();
        let first = q.push("run the tests").unwrap();
        let second = q.push("actually, only the sidecar ones").unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(
            q.waiting().iter().map(|m| m.text.clone()).collect::<Vec<_>>(),
            vec!["run the tests", "actually, only the sidecar ones"],
        );
    }

    #[tokio::test]
    async fn the_run_reads_them_in_the_order_they_were_typed() {
        let q = queue();
        q.push("one").unwrap();
        q.push("two").unwrap();
        q.push("three").unwrap();

        let mut read = Vec::new();
        for _ in 0..3 {
            read.push(Arc::clone(&q).next().await.unwrap().text);
        }
        assert_eq!(read, vec!["one", "two", "three"]);
    }

    #[tokio::test]
    async fn a_closed_inbox_refuses_and_stops_answering() {
        let q = queue();
        assert!(q.close().is_empty());
        assert!(q.push("too late").is_err(), "the turn is over");
        assert!(Arc::clone(&q).next().await.is_none(), "and nothing more comes out");
    }

    #[tokio::test]
    async fn what_the_model_never_saw_comes_back_on_close() {
        let q = queue();
        q.push("first").unwrap();
        q.push("second").unwrap();
        // The run took the first one but never said it had it — the sidecar
        // closed its generator in between, which is the race this exists for.
        let taken = Arc::clone(&q).next().await.unwrap();
        assert_eq!(taken.text, "first");

        let lost = q.close();
        assert_eq!(
            lost.iter().map(|m| m.text.clone()).collect::<Vec<_>>(),
            vec!["first", "second"],
            "handed but unacknowledged is still undelivered, and still first",
        );
    }

    #[tokio::test]
    async fn an_acknowledged_message_is_not_reported_lost() {
        let q = queue();
        q.push("first").unwrap();
        q.push("second").unwrap();
        let taken = Arc::clone(&q).next().await.unwrap();
        q.mark_read(&taken.id);

        let lost = q.close();
        assert_eq!(
            lost.iter().map(|m| m.text.clone()).collect::<Vec<_>>(),
            vec!["second"],
            "only what the model never got",
        );
    }

    #[tokio::test]
    async fn a_reader_parked_on_an_empty_inbox_is_woken_by_the_next_message() {
        let q = queue();
        let reader = Arc::clone(&q);
        let handle = tokio::spawn(async move { reader.next().await });
        // Nothing to read yet; the push is what unparks it.
        tokio::task::yield_now().await;
        q.push("late arrival").unwrap();
        assert_eq!(handle.await.unwrap().unwrap().text, "late arrival");
    }

    #[tokio::test]
    async fn closing_wakes_a_parked_reader_rather_than_hanging_the_run() {
        let q = queue();
        let reader = Arc::clone(&q);
        let handle = tokio::spawn(async move { reader.next().await });
        tokio::task::yield_now().await;
        q.close();
        assert!(handle.await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_dropped_read_does_not_swallow_the_message() {
        let q = queue();
        q.push("keep me").unwrap();
        // What the adapter's `select!` does when the sidecar speaks first.
        {
            let pending = Arc::clone(&q).next();
            drop(pending);
        }
        assert_eq!(Arc::clone(&q).next().await.unwrap().text, "keep me");
    }
}
