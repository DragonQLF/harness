//! Project checks: the commands the operator considers "green" for a project.
//! Stored per project in app data and run on demand.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct CheckRow {
    pub name: String,
    /// Shell command run from the repository root.
    pub command: String,
    /// `ok`, `warn`, `fail` or `idle`.
    pub status: String,
    pub detail: String,
    pub ran_ms: u64,
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
}
