use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use harness_domain::Event;
use harness_ports::{RunLogLine, RunLogPort, StoreError, StorePort, StoredEvent};

/// Append-only event log, one JSON object per line, replayed on open.
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

    pub fn path(&self) -> &Path {
        &self.file_path
    }

    fn replay_max_seq(path: &Path) -> std::io::Result<u64> {
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
    fn append_event(&self, e: &Event, ts_ms: u64) -> Result<StoredEvent, StoreError> {
        let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
        let record = serde_json::json!({ "seq": seq, "ts_ms": ts_ms, "event": e });
        let mut file = OpenOptions::new().append(true).open(&self.file_path)?;
        writeln!(file, "{record}")?;
        file.flush()?;
        Ok(StoredEvent {
            seq,
            ts_ms,
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
            let record: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                // A torn tail line is the normal shape of a hard kill: skip it.
                Err(_) => continue,
            };
            let (Some(seq), Some(event)) = (
                record.get("seq").and_then(|v| v.as_u64()),
                record.get("event"),
            ) else {
                continue;
            };
            match serde_json::from_value::<Event>(event.clone()) {
                Ok(event) => out.push(StoredEvent {
                    seq,
                    ts_ms: record.get("ts_ms").and_then(|v| v.as_u64()).unwrap_or(0),
                    event,
                }),
                Err(err) => return Err(StoreError::Serde(err.to_string())),
            }
        }
        Ok(out)
    }
}

/// One JSONL transcript per run, so a restart does not lose the log the
/// Sessions view is showing.
pub struct JsonlRunLog {
    dir: PathBuf,
}

impl JsonlRunLog {
    pub fn open(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    fn path_for(&self, run_id: &str) -> PathBuf {
        let safe: String = run_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
            .collect();
        self.dir.join(format!("{safe}.jsonl"))
    }
}

impl RunLogPort for JsonlRunLog {
    fn append(&self, run_id: &str, line: &RunLogLine) -> Result<(), StoreError> {
        let encoded =
            serde_json::to_string(line).map_err(|e| StoreError::Serde(e.to_string()))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path_for(run_id))?;
        writeln!(file, "{encoded}")?;
        file.flush()?;
        Ok(())
    }

    fn read(&self, run_id: &str) -> Result<Vec<RunLogLine>, StoreError> {
        let path = self.path_for(run_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(path)?;
        let mut out = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<RunLogLine>(&line) {
                out.push(parsed);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_domain::{Board, CardId, Command};
    use harness_ports::RunEvent;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "harness-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn append_and_read_roundtrip() {
        let dir = temp_dir("store-test");
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
            store.append_event(e, 1_700_000_000_000).unwrap();
        }

        drop(store);
        let reopened = JsonlStore::open(&path).unwrap();
        let all = reopened.read_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].seq, 1);
        assert_eq!(all[0].ts_ms, 1_700_000_000_000);
        assert_eq!(all[0].event.card_id(), &CardId::new("c1"));
        assert_eq!(reopened.next_seq.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn torn_tail_lines_are_skipped_on_replay() {
        let dir = temp_dir("torn-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        std::fs::write(
            &path,
            "{\"seq\":1,\"ts_ms\":7,\"event\":{\"type\":\"card_created\",\"card_id\":\"c1\",\"title\":\"a\"}}\n{\"seq\":2,\"eve",
        )
        .unwrap();

        let store = JsonlStore::open(&path).unwrap();
        let all = store.read_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].ts_ms, 7);
        assert_eq!(store.next_seq.load(Ordering::SeqCst), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn events_written_before_timestamps_still_replay() {
        let dir = temp_dir("legacy-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        std::fs::write(
            &path,
            "{\"seq\":1,\"event\":{\"type\":\"card_created\",\"card_id\":\"c1\",\"title\":\"a\"}}\n",
        )
        .unwrap();
        let all = JsonlStore::open(&path).unwrap().read_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].ts_ms, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_log_roundtrips_lines_per_run() {
        let dir = temp_dir("runlog-test");
        let log = JsonlRunLog::open(&dir).unwrap();
        assert!(log.read("nothing-here").unwrap().is_empty());

        log.append(
            "run-1",
            &RunLogLine {
                ts_ms: 5,
                event: RunEvent::Text { text: "hello".into() },
            },
        )
        .unwrap();
        log.append(
            "run-1",
            &RunLogLine {
                ts_ms: 6,
                event: RunEvent::ToolUse {
                    tool: "Read".into(),
                    summary: "lib.rs".into(),
                },
            },
        )
        .unwrap();
        log.append(
            "run-2",
            &RunLogLine {
                ts_ms: 7,
                event: RunEvent::Failed { message: "boom".into() },
            },
        )
        .unwrap();

        let one = log.read("run-1").unwrap();
        assert_eq!(one.len(), 2);
        assert!(matches!(one[0].event, RunEvent::Text { .. }));
        assert_eq!(log.read("run-2").unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
