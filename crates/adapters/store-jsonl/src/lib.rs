use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use harness_domain::Event;
use harness_ports::{StoredEvent, StoreError, StorePort};

pub struct JsonlStore {
    file_path: PathBuf,
    next_seq: AtomicU64,
}

impl JsonlStore {
    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let file_path = path.into();
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;
        file.sync_all()?;
        let next_seq = Self::replay_max_seq(&file_path)? + 1;
        Ok(Self {
            file_path,
            next_seq: AtomicU64::new(next_seq),
        })
    }

    fn replay_max_seq(path: &std::path::Path) -> std::io::Result<u64> {
        let file = File::open(path)?;
        let mut max = 0u64;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(seq) = record.get("seq").and_then(|v| v.as_u64()) {
                    max = max.max(seq);
                }
            }
        }
        Ok(max)
    }
}

impl StorePort for JsonlStore {
    fn append_event(&self, e: &Event) -> Result<StoredEvent, StoreError> {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let record = serde_json::json!({ "seq": seq, "event": e });
        let mut file = OpenOptions::new().append(true).open(&self.file_path)?;
        writeln!(file, "{record}")?;
        file.flush()?;
        Ok(StoredEvent {
            seq,
            event: e.clone(),
        })
    }

    fn read_all(&self) -> Result<Vec<StoredEvent>, StoreError> {
        let file = File::open(&self.file_path)?;
        let mut out = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(record) => {
                    if let (Some(seq), Some(event)) =
                        (record.get("seq").and_then(|v| v.as_u64()), record.get("event"))
                    {
                        match serde_json::from_value::<Event>(event.clone()) {
                            Ok(event) => out.push(StoredEvent { seq, event }),
                            Err(err) => return Err(StoreError::Serde(err.to_string())),
                        }
                    }
                }
                Err(_) => continue,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_domain::{CardId, Command, Board};

    #[test]
    fn append_and_read_roundtrip() {
        let dir = std::env::temp_dir().join(format!("harness-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("events.jsonl");

        let store = JsonlStore::open(&path).unwrap();
        assert_eq!(store.next_seq.load(Ordering::SeqCst), 1);

        let board = Board::default();
        let events = board
            .decide(&Command::CreateCard {
                card_id: CardId::new("c1"),
                title: "first".into(),
            })
            .unwrap();
        for e in &events {
            store.append_event(e).unwrap();
        }

        drop(store);
        let reopened = JsonlStore::open(&path).unwrap();
        let all = reopened.read_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].seq, 1);
        assert_eq!(
            all[0].event.card_id(),
            &CardId::new("c1"),
        );
        assert_eq!(reopened.next_seq.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn torn_tail_lines_are_skipped_on_replay() {
        let dir = std::env::temp_dir().join(format!("harness-torn-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        std::fs::write(
            &path,
            "{\"seq\":1,\"event\":{\"type\":\"card_created\",\"card_id\":\"c1\",\"title\":\"a\"}}\n{\"seq\":2,\"eve",
        )
        .unwrap();

        let store = JsonlStore::open(&path).unwrap();
        let all = store.read_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(store.next_seq.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
