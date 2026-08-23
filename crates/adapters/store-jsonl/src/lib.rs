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

    /// Where a run's transcript lives. Public because a caller that deletes a
    /// conversation has to delete the same file this writes, and the name is
    /// sanitised on the way in.
    pub fn path_of(&self, run_id: &str) -> PathBuf {
        self.path_for(run_id)
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

    /// A conversation is a run log like any other: the operator turn and the
    /// answer live in one file, which is what makes a chat readable after a
    /// restart without a second transcript store.
    #[test]
    fn a_conversation_transcript_is_just_a_run_log() {
        let dir = temp_dir("chatlog-test");
        let log = JsonlRunLog::open(&dir).unwrap();
        let id = "chat_7b30";

        for (ts, event) in [
            (1u64, RunEvent::UserMessage { text: "how is the board?".into() }),
            (2, RunEvent::Started { session_id: "sess-abc".into() }),
            (3, RunEvent::Text { text: "Two cards are waiting.".into() }),
            (4, RunEvent::Notice { text: "the session could not be resumed".into() }),
        ] {
            log.append(id, &RunLogLine { ts_ms: ts, event }).unwrap();
        }

        let back = log.read(id).unwrap();
        assert_eq!(back.len(), 4);
        match &back[0].event {
            RunEvent::UserMessage { text } => assert_eq!(text, "how is the board?"),
            other => panic!("the operator turn did not survive: {other:?}"),
        }
        match &back[1].event {
            RunEvent::Started { session_id } => assert_eq!(session_id, "sess-abc"),
            other => panic!("expected the session line: {other:?}"),
        }
        assert!(matches!(back[2].event, RunEvent::Text { .. }));
        assert!(matches!(back[3].event, RunEvent::Notice { .. }));

        // The name on disk is sanitised, so a caller deleting a transcript has
        // to ask rather than assume: `chat_7b30` is not `chat_7b30.jsonl`.
        assert!(log.path_of(id).ends_with("chat-7b30.jsonl"), "{:?}", log.path_of(id));

        // Another conversation is another file: two chats never mix.
        log.append(
            "chat_other",
            &RunLogLine { ts_ms: 9, event: RunEvent::Text { text: "elsewhere".into() } },
        )
        .unwrap();
        assert_eq!(log.read(id).unwrap().len(), 4);
        assert_eq!(log.read("chat_other").unwrap().len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Lines written before `user_message` existed still read back: the tag is
    /// additive, so an older transcript is untouched.
    #[test]
    fn transcripts_written_by_an_older_build_still_read() {
        let dir = temp_dir("oldlog-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("run-old.jsonl"),
            "{\"ts_ms\":1,\"kind\":\"text\",\"text\":\"hello\"}\n\
             {\"ts_ms\":2,\"kind\":\"from_the_future\",\"text\":\"?\"}\n\
             {\"ts_ms\":3,\"kind\":\"notice\",\"text\":\"done\"}\n",
        )
        .unwrap();

        let lines = JsonlRunLog::open(&dir).unwrap().read("run-old").unwrap();
        // The unreadable middle line is skipped rather than losing the file.
        assert_eq!(lines.len(), 2);
        assert!(matches!(lines[0].event, RunEvent::Text { .. }));
        assert!(matches!(lines[1].event, RunEvent::Notice { .. }));

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
