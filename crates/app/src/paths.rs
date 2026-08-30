//! Every file Relay writes lives under the OS app-data directory, never next
//! to the binary or inside a project the operator pointed us at.
//!
//! ```text
//! %APPDATA%/com.harness.app/
//!   settings.json
//!   agents.json
//!   projects.json
//!   conversations.json     the chat index: which Claude session each continues
//!   conversations/<id>.jsonl   one transcript per conversation
//!   sidecar/               copy of the Node sidecar + its node_modules
//!   projects/<id>/events.jsonl
//!   projects/<id>/runs/<run_id>.jsonl
//!   projects/<id>/checks.json
//!   worktrees/<id>/<card>/ per-card checkouts, outside the repository
//! ```

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let paths = Self { root };
        std::fs::create_dir_all(paths.projects_dir())?;
        std::fs::create_dir_all(paths.worktrees_dir())?;
        Ok(paths)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// O que `/` sabe fazer, da última vez que uma sessão o disse.
    ///
    /// O evento que traz esta lista é efémero — não vai para a transcrição —,
    /// portanto reiniciar a app deixava o compositor sem menu nenhum até ao
    /// primeiro turno seguinte. Guardá-la é o que faz o `/` funcionar à
    /// primeira; a lista é substituída inteira à próxima sessão que a publique.
    /// Onde o browser "signed in" guarda os cookies dele.
    ///
    /// Dentro dos dados do Relay, e não no Chrome do operador: o que lá estiver
    /// foi posto lá de propósito, que é o que torna "entra só no site que o
    /// Director precisa" uma resposta a sério em vez de uma esperança.
    pub fn browser_profile_dir(&self) -> PathBuf {
        self.root.join("browser-profile")
    }

    pub fn commands_file(&self) -> PathBuf {
        self.root.join("commands.json")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn agents_file(&self) -> PathBuf {
        self.root.join("agents.json")
    }

    pub fn projects_file(&self) -> PathBuf {
        self.root.join("projects.json")
    }

    pub fn conversations_file(&self) -> PathBuf {
        self.root.join("conversations.json")
    }

    /// The Director's improvement proposals, and the mark of his last
    /// end-of-day look. Workspace level: proposals are about the app, not any
    /// one project.
    pub fn inbox_file(&self) -> PathBuf {
        self.root.join("inbox.json")
    }

    /// Verdicts his automatic reviewer reached that he has not been told about
    /// yet. Beside the inbox and workspace level for the same reason: a review
    /// belongs to a project, but being told is the Director's, and he is one
    /// across every board.

    /// The last commit of the mirror repository Relay knows about, so it can
    /// tell when its own source moved without a card behind it. App data, not
    /// the operator's repository: Relay's files never live inside one.
    pub fn mirror_watch_file(&self) -> PathBuf {
        self.root.join("mirror-watch.json")
    }

    /// Approvals that expired unanswered, one JSON line each. Written by the
    /// router's expiry sink so a timeout survives a restart as its own fact,
    /// distinct from a deliberate no.
    pub fn approvals_expired_file(&self) -> PathBuf {
        self.root.join("approvals-expired.jsonl")
    }

    /// Chat transcripts. Workspace level, not per project: one Director watches
    /// every board, and a conversation may be pinned to no project at all.
    pub fn conversations_dir(&self) -> PathBuf {
        self.root.join("conversations")
    }

    /// Where a pasted or dropped attachment is written. Beside the
    /// conversations rather than inside one: the same image can be attached to
    /// two threads, and a transcript that outlives its file is worse than a
    /// file that outlives its thread.
    pub fn attachments_dir(&self) -> PathBuf {
        self.root.join("attachments")
    }

    pub fn sidecar_dir(&self) -> PathBuf {
        self.root.join("sidecar")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    pub fn project_dir(&self, project_id: &str) -> PathBuf {
        self.projects_dir().join(sanitize(project_id))
    }

    /// Where a project's curated memory lives: beside its runs and
    /// transcripts, outside any repository. Memory in the repo would mean one
    /// copy per worktree and write conflicts between concurrent cards.
    pub fn project_memory_charter(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("memory").join("charter.md")
    }

    /// Mirror-mode artefacts, parked per card until (one day) an operator
    /// chooses to install one. Outside the repository by design.
    pub fn updates_dir(&self) -> PathBuf {
        self.root().join("updates")
    }

    pub fn events_file(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("events.jsonl")
    }

    pub fn runs_dir(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("runs")
    }

    pub fn checks_file(&self, project_id: &str) -> PathBuf {
        self.project_dir(project_id).join("checks.json")
    }

    /// One card's last check pass, run in that card's own worktree. Beside
    /// `checks.json`, which holds the commands themselves: the list is the
    /// project's, each result belongs to a card.
    pub fn card_checks_file(&self, project_id: &str, card_id: &str) -> PathBuf {
        self.project_dir(project_id)
            .join("checks")
            .join(format!("{}.json", sanitize(card_id)))
    }

    pub fn project_worktrees(&self, project_id: &str) -> PathBuf {
        self.worktrees_dir().join(sanitize(project_id))
    }
}

/// Keep ids safe to use as a single path segment.
pub fn sanitize(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_ascii_lowercase();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}

/// Read a JSON file, falling back to the default when it is missing or corrupt
/// (a broken settings file must never stop the app from opening).
pub fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => T::default(),
    }
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    // Write beside the target and rename, so a crash mid-write cannot truncate
    // the file we would read on the next start.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_become_safe_single_segments() {
        assert_eq!(sanitize("My Repo/Thing"), "my-repo-thing");
        assert_eq!(sanitize("../escape"), "escape");
        assert_eq!(sanitize("///"), "project");
        assert_eq!(sanitize("keep_this-1"), "keep_this-1");
    }

    #[test]
    fn layout_keeps_everything_under_the_root() {
        let root = std::env::temp_dir().join(format!("harness-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths::new(&root).unwrap();
        for p in [
            paths.settings_file(),
            paths.agents_file(),
            paths.projects_file(),
            paths.conversations_file(),
            paths.conversations_dir(),
            paths.events_file("Some Project"),
            paths.runs_dir("Some Project"),
            paths.card_checks_file("Some Project", "../../c_1"),
            paths.project_worktrees("Some Project"),
            paths.sidecar_dir(),
        ] {
            assert!(p.starts_with(&root), "{p:?} escaped the app data root");
        }
        assert!(paths.projects_dir().is_dir());
        assert!(paths.worktrees_dir().is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn json_roundtrips_and_survives_a_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("harness-json-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("thing.json");

        assert_eq!(read_json_or_default::<Vec<String>>(&file), Vec::<String>::new());
        write_json(&file, &vec!["a".to_string(), "b".to_string()]).unwrap();
        assert_eq!(
            read_json_or_default::<Vec<String>>(&file),
            vec!["a".to_string(), "b".to_string()]
        );

        std::fs::write(&file, "{not json").unwrap();
        assert_eq!(read_json_or_default::<Vec<String>>(&file), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
