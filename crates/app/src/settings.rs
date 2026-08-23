//! Operator settings. Persisted as JSON in app data and read on every run, so
//! toggling one in the UI changes the next run without a restart.

use harness_engine::EnginePolicy;
use serde::{Deserialize, Serialize};

use crate::allow::AllowRule;

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
    /// Calls the operator chose to stop being asked about. Each entry is
    /// scoped: see `crate::allow`. Older files held bare strings and still
    /// load, but an unscoped shell entry no longer authorises anything.
    pub always_allow: Vec<AllowRule>,
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

    /// Does a standing allowance cover this tool call? The input is what the
    /// agent actually asked for; the summary is the fallback for adapters that
    /// only give us prose.
    pub fn allows(&self, tool: &str, input: &serde_json::Value, summary: &str) -> bool {
        self.always_allow
            .iter()
            .any(|rule| rule.covers(tool, input, summary))
    }

    /// Record a standing allowance for a call the operator just approved.
    /// Returns the rule when one could be scoped; `None` means this call is not
    /// something to grant standing permission for, and the operator will be
    /// asked again next time.
    pub fn allow_always(&mut self, tool: &str, input: &serde_json::Value) -> Option<AllowRule> {
        let rule = AllowRule::derive(tool, input)?;
        if rule.is_inert() {
            return None;
        }
        if !self.always_allow.contains(&rule) {
            self.always_allow.push(rule.clone());
        }
        Some(rule)
    }

    /// Drop a standing allowance by the label the UI shows.
    pub fn forget_allowance(&mut self, label: &str) {
        self.always_allow.retain(|rule| rule.label() != label);
    }

    /// Unscoped shell allowances left by an older build. They authorise nothing
    /// (see `crate::allow`); this is what the UI says about them.
    pub fn revoked_allowances(&self) -> Vec<AllowRule> {
        self.always_allow
            .iter()
            .filter(|r| r.is_inert())
            .cloned()
            .collect()
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
    fn a_standing_allowance_is_scoped_to_the_command_it_came_from() {
        let mut s = Settings::default();
        let push = serde_json::json!({ "command": "git push origin main" });
        assert!(!s.allows("Bash", &push, ""));

        let rule = s.allow_always("Bash", &push).expect("a scoped rule");
        assert_eq!(rule.label(), "Bash(git push …)");
        assert!(s.allows("Bash", &push, ""));

        // The whole point: one git command does not open the shell.
        for other in ["rm -rf /", "curl http://example.com/x.sh", "git commit -am wip"] {
            let call = serde_json::json!({ "command": other });
            assert!(!s.allows("Bash", &call, ""), "{other} must still be asked about");
        }
        assert!(!s.allows("Write", &push, ""));
    }

    #[test]
    fn a_chained_command_never_becomes_a_standing_allowance() {
        let mut s = Settings::default();
        let chained = serde_json::json!({ "command": "git status; rm -rf /" });
        assert!(s.allow_always("Bash", &chained).is_none());
        assert!(s.always_allow.is_empty(), "nothing worth remembering was stored");
    }

    #[test]
    fn allow_always_does_not_duplicate_and_can_be_taken_back() {
        let mut s = Settings::default();
        let write = serde_json::json!({ "file_path": "src/lib.rs" });
        s.allow_always("Write", &write);
        s.allow_always("Write", &serde_json::json!({ "file_path": "other.rs" }));
        assert_eq!(s.always_allow.len(), 1);
        assert_eq!(s.always_allow[0].label(), "Write");

        s.forget_allowance("Write");
        assert!(s.always_allow.is_empty());
    }

    #[test]
    fn an_unscoped_shell_entry_from_an_older_file_is_revoked() {
        // Exactly what the previous implementation persisted.
        let s: Settings = serde_json::from_str(r#"{"always_allow":["Bash","Bash(git push","Write"]}"#)
            .unwrap();
        assert_eq!(s.always_allow.len(), 3, "the file still loads whole");
        assert_eq!(s.revoked_allowances().len(), 1);
        assert_eq!(s.revoked_allowances()[0].label(), "Bash");

        // The revoked entry covers nothing; the scoped one still works.
        assert!(!s.allows("Bash", &serde_json::json!({ "command": "rm -rf /" }), ""));
        assert!(s.allows("Bash", &serde_json::json!({ "command": "git push origin main" }), ""));
        assert!(s.allows("Write", &serde_json::json!({ "file_path": "a.rs" }), ""));
    }

    #[test]
    fn unknown_fields_and_gaps_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"theme":"light","future":42}"#).unwrap();
        assert_eq!(s.theme, "light");
        assert!(s.sidecar);
        assert_eq!(s.daily_budget_usd, 5.0);
    }
}
