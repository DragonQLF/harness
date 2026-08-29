//! The project registry: git repositories the operator pointed Relay at.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Absolute path to the repository root.
    pub path: String,
    /// Up to two letters for the avatar.
    pub glyph: String,
    /// `accent`, `info`, `ok` or `warn`.
    pub tone: String,
    pub base_branch: String,
    #[ts(type = "number")]
    pub added_ms: u64,
    /// A paused project starts no new runs.
    pub paused: bool,
    /// Mirror mode: the engine builds this project after every commit and
    /// parks the artefact under appdata/updates for review. The orchestrator's own flag.
    pub mirror: bool,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            path: String::new(),
            glyph: String::new(),
            tone: "accent".to_string(),
            base_branch: "main".to_string(),
            added_ms: 0,
            paused: false,
            mirror: false,
        }
    }
}

pub const TONES: [&str; 4] = ["accent", "info", "ok", "warn"];

/// The harness's own project — the one mirror mode builds. Work on Relay
/// itself is born here, never in whatever the operator has open (#72).
pub fn mirror_project(projects: &[Project]) -> Option<&Project> {
    projects.iter().find(|p| p.mirror)
}

/// Mirror mode belongs to exactly one project. Turning it on for `id` turns it
/// off everywhere else, so `mirror_project` is never asked to choose between
/// two homes — the toggle in the UI is one flag with several off positions,
/// not several independent flags.
pub fn only_mirror(projects: &mut [Project], id: &str) {
    for project in projects.iter_mut() {
        project.mirror = project.id == id;
    }
}

/// Initials for the project avatar.
pub fn glyph_for(name: &str) -> String {
    let letters: String = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    if letters.is_empty() {
        "P".to_string()
    } else {
        letters.to_uppercase()
    }
}

/// What a folder looks like before we agree to adopt it, so the UI can offer
/// the right next step instead of a flat refusal.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
pub struct FolderInfo {
    pub path: String,
    pub exists: bool,
    pub is_repo: bool,
    /// Empty enough that initialising a repository in it is uncontroversial.
    pub empty: bool,
    /// Name we would give the project.
    pub name: String,
    /// Already registered under this path.
    pub already_added: bool,
    /// What the UI should offer: `open`, `init`, `confirm_init` or `missing`.
    pub next: &'static str,
}

impl FolderInfo {
    pub fn describe(
        path: &str,
        exists: bool,
        is_repo: bool,
        empty: bool,
        already_added: bool,
    ) -> Self {
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Project".to_string());
        let next = if !exists {
            "missing"
        } else if is_repo {
            "open"
        } else if empty {
            "init"
        } else {
            // Files but no repository: only with an explicit yes.
            "confirm_init"
        };
        Self {
            path: path.to_string(),
            exists,
            is_repo,
            empty,
            name,
            already_added,
            next,
        }
    }
}

/// Pick an id nobody is using yet, derived from the display name.
pub fn unique_id(display: &str, taken: &[String]) -> String {
    let base = crate::paths::sanitize(display);
    let mut id = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t == &id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_come_from_the_first_letters() {
        assert_eq!(glyph_for("relay"), "R");
        assert_eq!(glyph_for("atlas api"), "AA");
        assert_eq!(glyph_for("seven-web-site"), "SW");
        assert_eq!(glyph_for("   "), "P");
        assert_eq!(glyph_for("2fa"), "2");
    }

    #[test]
    fn ids_do_not_collide() {
        let taken = vec!["relay".to_string(), "relay-2".to_string()];
        assert_eq!(unique_id("Relay", &taken), "relay-3");
        assert_eq!(unique_id("Other Thing", &taken), "other-thing");
    }

    #[test]
    fn a_folder_gets_the_right_next_step() {
        let repo = FolderInfo::describe("C:/src/thing", true, true, false, false);
        assert_eq!(repo.next, "open");
        assert_eq!(repo.name, "thing");

        let fresh = FolderInfo::describe("C:/src/new", true, false, true, false);
        assert_eq!(fresh.next, "init");

        let occupied = FolderInfo::describe("C:/src/docs", true, false, false, false);
        assert_eq!(occupied.next, "confirm_init", "never git init silently");

        let gone = FolderInfo::describe("C:/src/gone", false, false, false, false);
        assert_eq!(gone.next, "missing");
    }

    #[test]
    fn a_stored_project_keeps_defaults_for_missing_fields() {
        let project: Project =
            serde_json::from_str(r#"{"id":"x","name":"X","path":"C:/x"}"#).unwrap();
        assert_eq!(project.base_branch, "main");
        assert_eq!(project.tone, "accent");
        assert!(!project.paused);
    }

    #[test]
    fn the_mirror_project_is_the_harnesss_own_home() {
        let plain = Project { id: "site".into(), ..Default::default() };
        let mirror = Project { id: "_harness".into(), mirror: true, ..Default::default() };
        let projects = vec![plain, mirror];
        assert_eq!(
            mirror_project(&projects).map(|p| p.id.as_str()),
            Some("_harness")
        );
        assert!(mirror_project(&[]).is_none());
    }

    #[test]
    fn turning_mirror_on_turns_it_off_everywhere_else() {
        let mut projects = vec![
            Project { id: "site".into(), ..Default::default() },
            Project { id: "_harness".into(), mirror: true, ..Default::default() },
            Project { id: "tools".into(), mirror: true, ..Default::default() },
        ];
        only_mirror(&mut projects, "site");
        assert_eq!(
            projects.iter().filter(|p| p.mirror).map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["site"],
        );
        // Naming nobody clears the flag: that is how the toggle turns off.
        only_mirror(&mut projects, "");
        assert!(mirror_project(&projects).is_none());
    }
}
