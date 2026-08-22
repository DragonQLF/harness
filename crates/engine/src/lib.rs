use std::sync::Arc;

use harness_domain::{Board, Card};
use harness_ports::{ClockPort, StoredEvent, StorePort};
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};

const QUEUE_CAPACITY: usize = 256;
const BROADCAST_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Serialize)]
pub struct Envelope {
    pub seq: u64,
    pub ts_ms: u64,
    #[serde(flatten)]
    pub event: harness_domain::Event,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub last_seq: u64,
    pub cards: Vec<Card>,
}

#[derive(Debug)]
enum Msg {
    Command {
        cmd: harness_domain::Command,
        reply: oneshot::Sender<Result<u64, String>>,
    },
    Snapshot {
        reply: oneshot::Sender<Snapshot>,
    },
}

#[derive(Clone)]
pub struct EngineHandle {
    tx: mpsc::Sender<Msg>,
}

impl EngineHandle {
    pub async fn execute(&self, cmd: harness_domain::Command) -> Result<u64, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Msg::Command { cmd, reply: reply_tx })
            .await
            .map_err(|_| "engine is not running".to_string())?;
        match reply_rx.await {
            Ok(outcome) => outcome,
            Err(_) => Err("engine dropped the reply".to_string()),
        }
    }

    pub async fn snapshot(&self) -> Result<Snapshot, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Msg::Snapshot { reply: reply_tx })
            .await
            .map_err(|_| "engine is not running".to_string())?;
        match reply_rx.await {
            Ok(snap) => Ok(snap),
            Err(_) => Err("engine dropped the reply".to_string()),
        }
    }
}

pub fn rebuild(history: &[StoredEvent]) -> Board {
    let mut board = Board::default();
    for stored in history {
        board.apply(&stored.event);
    }
    board
}

pub struct Engine<S: StorePort, C: ClockPort> {
    rx: mpsc::Receiver<Msg>,
    board: Board,
    last_seq: u64,
    store: Arc<S>,
    clock: Arc<C>,
    bcast_tx: broadcast::Sender<Envelope>,
}

impl<S, C> Engine<S, C>
where
    S: StorePort + 'static,
    C: ClockPort + 'static,
{
    #[allow(dead_code)]
    fn new(store: Arc<S>, clock: Arc<C>, history: Vec<StoredEvent>) -> Self {
        let board = rebuild(&history);
        let last_seq = history.last().map(|s| s.seq).unwrap_or(0);
        Self {
            rx: mpsc::channel(QUEUE_CAPACITY).1,
            board,
            last_seq,
            store,
            clock,
            bcast_tx: broadcast::channel(BROADCAST_CAPACITY).0,
        }
    }

    pub fn spawn(
        store: Arc<S>,
        clock: Arc<C>,
        history: Vec<StoredEvent>,
    ) -> (EngineHandle, broadcast::Receiver<Envelope>) {
        let mut board = Board::default();
        for stored in &history {
            board.apply(&stored.event);
        }
        let last_seq = history.last().map(|s| s.seq).unwrap_or(0);
        let (bcast_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let bcast_rx = bcast_tx.subscribe();
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let mut engine = Self {
            rx,
            board,
            last_seq,
            store,
            clock,
            bcast_tx,
        };
        tokio::spawn(async move {
            engine.run().await;
        });
        (EngineHandle { tx }, bcast_rx)
    }

    async fn run(&mut self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                Msg::Command { cmd, reply } => {
                    let outcome = self.execute(cmd).await;
                    let _ = reply.send(outcome);
                }
                Msg::Snapshot { reply } => {
                    let snap = Snapshot {
                        last_seq: self.last_seq,
                        cards: self.board.cards().into_iter().cloned().collect(),
                    };
                    let _ = reply.send(snap);
                }
            }
        }
    }

    async fn execute(&mut self, cmd: harness_domain::Command) -> Result<u64, String> {
        let events = match self.board.decide(&cmd) {
            Ok(events) => events,
            Err(e) => return Err(e.to_string()),
        };
        for event in events {
            let stored = {
                let store = Arc::clone(&self.store);
                let ev = event.clone();
                tokio::task::block_in_place(move || store.append_event(&ev))
                    .map_err(|e| e.to_string())?
            };
            self.board.apply(&stored.event);
            self.last_seq = stored.seq;
            let envelope = Envelope {
                seq: stored.seq,
                ts_ms: self.clock.now_millis(),
                event: stored.event,
            };
            let _ = self.bcast_tx.send(envelope);
        }
        Ok(self.last_seq)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    use harness_domain::{CardId, Command, Event, Status};
    use harness_ports::{StoreError, StorePort};

    use super::*;

    struct MemStore {
        records: Mutex<Vec<StoredEvent>>,
        next: AtomicU64,
    }

    impl Default for MemStore {
        fn default() -> Self {
            Self {
                records: Mutex::new(Vec::new()),
                next: AtomicU64::new(1),
            }
        }
    }

    impl MemStore {
        fn count(&self) -> usize {
            self.records.lock().unwrap().len()
        }
    }

    impl StorePort for MemStore {
        fn append_event(&self, e: &harness_domain::Event) -> Result<StoredEvent, StoreError> {
            let seq = self.next.fetch_add(1, Ordering::SeqCst);
            self.records.lock().unwrap().push(StoredEvent {
                seq,
                event: e.clone(),
            });
            Ok(StoredEvent {
                seq,
                event: e.clone(),
            })
        }

        fn read_all(&self) -> Result<Vec<StoredEvent>, StoreError> {
            Ok(self.records.lock().unwrap().clone())
        }
    }

    struct FixedClock;

    impl ClockPort for FixedClock {
        fn now_millis(&self) -> u64 {
            42
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn accepted_commands_persist_broadcast_and_update_state() {
        let store = Arc::new(MemStore::default());
        let (handle, mut sub) = Engine::spawn(store.clone(), Arc::new(FixedClock), vec![]);

        let id = CardId::new("c1");
        let seq = handle
            .execute(Command::CreateCard {
                card_id: id.clone(),
                title: "t".into(),
            })
            .await
            .unwrap();
        assert_eq!(seq, 1);

        let env = sub.recv().await.unwrap();
        assert_eq!(env.seq, 1);
        assert!(matches!(env.event, Event::CardCreated { .. }));

        handle
            .execute(Command::MoveCard { card_id: id.clone(), to: Status::Ready })
            .await
            .unwrap();
        let env2 = sub.recv().await.unwrap();
        assert_eq!(env2.seq, 2);

        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.last_seq, 2);
        assert_eq!(snap.cards.len(), 1);
        assert_eq!(snap.cards[0].status, Status::Ready);
        assert_eq!(store.count(), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejected_commands_leave_no_trace() {
        let store = Arc::new(MemStore::default());
        let (handle, mut sub) = Engine::spawn(store.clone(), Arc::new(FixedClock), vec![]);

        let id = CardId::new("c1");
        handle
            .execute(Command::CreateCard {
                card_id: id.clone(),
                title: "t".into(),
            })
            .await
            .unwrap();
        let env = sub.recv().await.unwrap();
        assert_eq!(env.seq, 1);

        let err = handle
            .execute(Command::MoveCard { card_id: id, to: Status::Done })
            .await
            .unwrap_err();
        assert!(err.contains("illegal"), "got: {err}");

        assert_eq!(store.count(), 1);
        assert!(sub.try_recv().is_err());

        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.cards[0].status, Status::Backlog);
        assert_eq!(snap.last_seq, 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn history_rebuild_restores_board_and_sequence() {
        let store = Arc::new(MemStore::default());
        let id = CardId::new("seed");

        {
            let (handle, _) = Engine::spawn(store.clone(), Arc::new(FixedClock), vec![]);
            handle
                .execute(Command::CreateCard {
                    card_id: id.clone(),
                    title: "t".into(),
                })
                .await
                .unwrap();
            handle
                .execute(Command::MoveCard { card_id: id.clone(), to: Status::Ready })
                .await
                .unwrap();
        }

        let history = store.read_all().unwrap();
        assert_eq!(history.len(), 2);

        let (handle, _) = Engine::spawn(store, Arc::new(FixedClock), history);
        let snap = handle.snapshot().await.unwrap();
        assert_eq!(snap.last_seq, 2);
        assert_eq!(snap.cards[0].status, Status::Ready);

        let seq = handle
            .execute(Command::MoveCard { card_id: id, to: Status::Running })
            .await
            .unwrap();
        assert_eq!(seq, 3);
    }
}
