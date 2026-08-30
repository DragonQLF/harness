//! Project checks: the commands the operator considers "green" for a project.
//! Stored per project in app data and run on demand.
//!
//! The same commands answer two different questions. Run at the repository
//! root they say whether the project is green; run inside a card's worktree
//! they say whether *that card's* work is green, which is the only version a
//! board can pin a red pill to. Both results are kept, and neither is derived
//! from the other: the project's build being red says nothing about which card
//! broke it.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct CheckRow {
    pub name: String,
    /// Shell command run from the repository root.
    pub command: String,
    /// `ok`, `warn`, `fail` or `idle`.
    pub status: String,
    pub detail: String,
    #[ts(type = "number")]
    pub ran_ms: u64,
    #[ts(type = "number")]
    pub duration_ms: u64,
}

impl Default for CheckRow {
    fn default() -> Self {
        Self {
            name: String::new(),
            command: String::new(),
            status: "idle".to_string(),
            detail: "not run".to_string(),
            ran_ms: 0,
            duration_ms: 0,
        }
    }
}

/// Suggest the checks a repository obviously has, so the panel is not empty on
/// day one. The operator can edit them.
pub fn suggested_checks(root: &Path) -> Vec<CheckRow> {
    let mut out = Vec::new();
    let mut add = |name: &str, command: &str| {
        out.push(CheckRow {
            name: name.to_string(),
            command: command.to_string(),
            ..Default::default()
        })
    };
    if root.join("Cargo.toml").exists() {
        add("cargo test", "cargo test --workspace");
        add("clippy", "cargo clippy --workspace --all-targets");
    }
    if root.join("package.json").exists() {
        add("typescript", "npx tsc --noEmit");
    }
    if root.join("go.mod").exists() {
        add("go test", "go test ./...");
    }
    if root.join("pyproject.toml").exists() {
        add("pytest", "python -m pytest -q");
    }
    out
}

pub fn read_checks(file: &Path, root: &Path) -> Vec<CheckRow> {
    let stored: Vec<CheckRow> = crate::paths::read_json_or_default(file);
    if stored.is_empty() {
        suggested_checks(root)
    } else {
        stored
    }
}

/// Only what the operator wrote down — no suggestions.
///
/// A suggestion is a menu entry, and the difference matters wherever Relay
/// decides to run something by itself: guessing `cargo test --workspace` and
/// then spending four minutes on it is not a guess anybody asked for.
pub fn stored_checks(file: &Path) -> Vec<CheckRow> {
    crate::paths::read_json_or_default(file)
}

/// One check pass, made in one card's worktree, against one run.
///
/// This is what makes a red pill belong to a card. `run_id` is the run whose
/// commit was on disk when the pass was made: a later run on the same card
/// makes this result stale, and saying which run it answered is the only way
/// to know that.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct CardChecks {
    pub card_id: String,
    /// The run the pass was made against. Empty when the card has no recorded
    /// run — the operator asked for the pass by hand.
    pub run_id: String,
    /// Where the commands ran. Recorded because the worktree mode comes from
    /// the agent profile at start time and may have changed since.
    pub worktree: String,
    /// When the pass finished, in the operator's clock.
    #[ts(type = "number")]
    pub ran_ms: u64,
    pub rows: Vec<CheckRow>,
}

impl CardChecks {
    /// How many of this card's checks came back red. The board's pill.
    pub fn failing(&self) -> usize {
        self.rows.iter().filter(|r| r.status == "fail").count()
    }
}

/// Read a card's last pass. `None` means no pass was ever made for this card,
/// which is not the same fact as "nothing failed" — the board shows no pill
/// rather than a green one it cannot vouch for.
pub fn read_card_checks(file: &Path) -> Option<CardChecks> {
    let raw = std::fs::read_to_string(file).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Run every check in the card's own worktree and record the result against
/// the card. Blocking: the caller owns the thread it happens on.
pub fn run_card_checks(
    worktree: &Path,
    card_id: &str,
    run_id: &str,
    checks: Vec<CheckRow>,
    now_ms: u64,
) -> CardChecks {
    let rows = checks
        .into_iter()
        .map(|check| run_check(worktree, check))
        .collect();
    CardChecks {
        card_id: card_id.to_string(),
        run_id: run_id.to_string(),
        worktree: worktree.to_string_lossy().to_string(),
        ran_ms: now_ms,
        rows,
    }
}

/// Run the configured checks and remember how they went.
pub fn run_check(root: &Path, mut check: CheckRow) -> CheckRow {
    let started = std::time::Instant::now();
    let (program, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd", vec!["/C", &check.command])
    } else {
        ("sh", vec!["-c", &check.command])
    };
    let mut cmd = Command::new(program);
    cmd.args(&args).current_dir(root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let output = cmd.output();
    check.ran_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    check.duration_ms = started.elapsed().as_millis() as u64;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let tail: String = stdout
                .lines()
                .chain(stderr.lines())
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" · ");
            let warned = tail.to_ascii_lowercase().contains("warning");
            check.status = if out.status.success() {
                if warned { "warn" } else { "ok" }
            } else {
                "fail"
            }
            .to_string();
            check.detail = if tail.trim().is_empty() {
                format!("{}ms", check.duration_ms)
            } else {
                tail.chars().take(160).collect()
            };
        }
        Err(e) => {
            check.status = "fail".to_string();
            check.detail = format!("could not run: {e}");
        }
    }
    check
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_follow_the_files_in_the_repository() {
        let dir = std::env::temp_dir().join(format!("harness-checks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(suggested_checks(&dir).is_empty());

        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        let names: Vec<String> = suggested_checks(&dir).into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["cargo test", "clippy"]);
        assert!(suggested_checks(&dir).iter().all(|c| c.status == "idle"));

        std::fs::write(dir.join("package.json"), "{}").unwrap();
        assert_eq!(suggested_checks(&dir).len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_check_records_its_outcome() {
        let dir = std::env::temp_dir().join(format!("harness-run-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let ok = run_check(
            &dir,
            CheckRow {
                name: "hello".into(),
                command: "echo hello".into(),
                ..Default::default()
            },
        );
        assert_eq!(ok.status, "ok");
        assert!(ok.detail.contains("hello"), "detail was {}", ok.detail);
        assert!(ok.ran_ms > 0);

        let bad = run_check(
            &dir,
            CheckRow {
                name: "nope".into(),
                command: "exit 3".into(),
                ..Default::default()
            },
        );
        assert_eq!(bad.status, "fail");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stored_checks_never_invent_a_command() {
        let dir = std::env::temp_dir().join(format!("harness-stored-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        let file = dir.join("checks.json");

        // The suggestion list is not empty for this folder, and that is
        // exactly what must not leak into the automatic path.
        assert!(!suggested_checks(&dir).is_empty());
        assert!(stored_checks(&file).is_empty());

        crate::paths::write_json(
            &file,
            &vec![CheckRow {
                name: "mine".into(),
                command: "true".into(),
                ..Default::default()
            }],
        )
        .unwrap();
        let stored = stored_checks(&file);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "mine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_card_pass_runs_in_its_worktree_and_names_its_run() {
        let dir = std::env::temp_dir().join(format!("harness-card-checks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let worktree = dir.join("c_1234");
        std::fs::create_dir_all(&worktree).unwrap();
        // A file only this worktree has: proof the commands ran there and not
        // at the repository root.
        std::fs::write(worktree.join("marker"), "here").unwrap();

        let pass = run_card_checks(
            &worktree,
            "c_1234",
            "run-9",
            vec![
                CheckRow {
                    name: "sees the worktree".into(),
                    command: if cfg!(windows) {
                        "type marker".into()
                    } else {
                        "cat marker".into()
                    },
                    ..Default::default()
                },
                CheckRow {
                    name: "breaks".into(),
                    command: "exit 1".into(),
                    ..Default::default()
                },
            ],
            1_700_000_000_000,
        );

        assert_eq!(pass.card_id, "c_1234");
        assert_eq!(pass.run_id, "run-9");
        assert_eq!(pass.ran_ms, 1_700_000_000_000);
        assert_eq!(pass.rows[0].status, "ok");
        assert_eq!(pass.rows[1].status, "fail");
        assert_eq!(pass.failing(), 1);

        // A pass survives the round trip to disk, and a card that never had
        // one reads as absent rather than as green.
        let file = dir.join("c_1234.json");
        crate::paths::write_json(&file, &pass).unwrap();
        assert_eq!(read_card_checks(&file).unwrap().failing(), 1);
        assert!(read_card_checks(&dir.join("c_never.json")).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
