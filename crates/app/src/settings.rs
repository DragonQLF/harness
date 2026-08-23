//! Operator settings. Persisted as JSON in app data and read on every run, so
//! toggling one in the UI changes the next run without a restart.

use harness_engine::EnginePolicy;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: String,
    pub accent: String,
    /// Run agents through the Node sidecar (Agent SDK). Off falls back to the
    /// `claude` command line adapter.
    pub sidecar: bool,
    /// The Director reads every finished diff before it reaches you.
    pub director_reviews_first: bool,
    /// Wait for running agents to commit work in progress before quitting.
    pub commit_wip_on_close: bool,
    /// Permission mode handed to worker runs when the agent profile is silent.
    pub permission_mode: String,
    /// Soft daily budget, only used to draw the spend meter.
    pub daily_budget_usd: f64,
    /// Tools the operator chose to stop being asked about.
    pub always_allow: Vec<String>,
    /// Project shown when the app opens.
    pub last_project: Option<String>,
    pub user_name: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            accent: "#6b5cf6".to_string(),
            sidecar: true,
            director_reviews_first: true,
            commit_wip_on_close: true,
            permission_mode: "acceptEdits".to_string(),
            daily_budget_usd: 5.0,
            always_allow: Vec::new(),
            last_project: None,
            user_name: "Operator".to_string(),
        }
    }
}

impl Settings {
    pub fn policy(&self) -> EnginePolicy {
        EnginePolicy {
            director_reviews_first: self.director_reviews_first,
            commit_wip_on_close: self.commit_wip_on_close,
        }
    }

    /// Does a standing allowance cover this tool call? Entries are either a
    /// bare tool name (`Bash`) or a name with a prefix (`Bash(git push`).
    pub fn allows(&self, tool: &str, summary: &str) -> bool {
        self.always_allow.iter().any(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return false;
            }
            match entry.split_once('(') {
                Some((name, rest)) => {
                    let prefix = rest.trim_end_matches(')');
                    name.trim().eq_ignore_ascii_case(tool)
                        && (prefix.is_empty() || summary.contains(prefix))
                }
                None => entry.eq_ignore_ascii_case(tool),
            }
        })
    }

    pub fn allow_always(&mut self, tool: &str) {
        if !self.always_allow.iter().any(|e| e.eq_ignore_ascii_case(tool)) {
            self.always_allow.push(tool.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_conservative_profile() {
        let s = Settings::default();
        assert!(s.sidecar);
        assert!(s.director_reviews_first);
        assert_eq!(s.permission_mode, "acceptEdits");
        assert!(s.always_allow.is_empty());
    }

    #[test]
    fn always_allow_matches_by_name_and_prefix() {
        let mut s = Settings::default();
        assert!(!s.allows("Bash", "git push origin main"));

        s.allow_always("Bash");
        assert!(s.allows("bash", "anything"));
        assert!(!s.allows("Write", "anything"));

        s.always_allow = vec!["Bash(git push".to_string()];
        assert!(s.allows("Bash", "command: git push origin main"));
        assert!(!s.allows("Bash", "command: rm -rf /"));
    }

    #[test]
    fn allow_always_does_not_duplicate() {
        let mut s = Settings::default();
        s.allow_always("Write");
        s.allow_always("write");
        assert_eq!(s.always_allow, vec!["Write".to_string()]);
    }

    #[test]
    fn unknown_fields_and_gaps_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"light","future":42}"#).unwrap();
        assert_eq!(s.theme, "light");
        assert!(s.sidecar);
        assert_eq!(s.daily_budget_usd, 5.0);
    }
}
