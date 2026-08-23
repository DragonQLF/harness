//! Agent profiles: the crew the operator configures once and assigns to cards.
//! A profile is turned into a `RunProfile` at the moment a run starts, which is
//! the only place policy meets the engine.

use harness_ports::{Reviewer, RunProfile, WorktreeMode};
use serde::{Deserialize, Serialize};

use crate::settings::Settings;

pub const DIRECTOR_ID: &str = "director";
pub const DEFAULT_WORKER: &str = "builder";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    /// One letter for the avatar.
    pub initial: String,
    /// What it is, in the operator's words.
    pub title: String,
    pub role: String,
    /// The standing instruction handed to every run.
    pub brief: String,
    /// Accent used for this agent across the UI.
    pub tone: String,
    /// `opus` | `sonnet` | `haiku`, or None to let Claude choose.
    pub model: Option<String>,
    /// Capability names, translated into allowed tools for the run.
    pub permissions: Vec<String>,
    pub budget_usd: Option<f64>,
    pub worktree: WorktreeMode,
    pub reviewer: Reviewer,
    /// A paused agent picks up no new work.
    pub paused: bool,
    pub permission_mode: Option<String>,
}

impl Default for AgentProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            initial: String::new(),
            title: String::new(),
            role: String::new(),
            brief: String::new(),
            tone: "accent".to_string(),
            model: None,
            permissions: vec!["Read".into(), "Search".into()],
            budget_usd: None,
            worktree: WorktreeMode::PerCard,
            reviewer: Reviewer::Director,
            paused: false,
            permission_mode: None,
        }
    }
}

impl AgentProfile {
    /// Capabilities the operator ticks map onto concrete tool allowances.
    pub fn allowed_tools(&self) -> Vec<String> {
        let mut tools = Vec::new();
        for permission in &self.permissions {
            match permission.to_ascii_lowercase().as_str() {
                "read" => tools.push("Read".to_string()),
                "search" => {
                    tools.push("Glob".to_string());
                    tools.push("Grep".to_string());
                }
                "edit" => tools.push("Edit".to_string()),
                "write" => tools.push("Write".to_string()),
                "git" => tools.push("Bash(git *)".to_string()),
                "web" => {
                    tools.push("WebSearch".to_string());
                    tools.push("WebFetch".to_string());
                }
                "shell" => tools.push("Bash".to_string()),
                other => tools.push(other.to_string()),
            }
        }
        tools.sort();
        tools.dedup();
        tools
    }

    pub fn run_profile(&self, settings: &Settings) -> RunProfile {
        RunProfile {
            agent_id: self.id.clone(),
            model: self.model.clone(),
            allowed_tools: Some(self.allowed_tools()),
            permission_mode: Some(
                self.permission_mode
                    .clone()
                    .unwrap_or_else(|| settings.permission_mode.clone()),
            ),
            max_budget_usd: self.budget_usd,
            worktree: self.worktree,
            reviewer: if settings.director_reviews_first {
                self.reviewer
            } else if self.reviewer == Reviewer::Director {
                // The operator turned automatic review off; the diff comes to them.
                Reviewer::Human
            } else {
                self.reviewer
            },
        }
    }

    /// Prompt handed to the agent for a card.
    pub fn prompt_for(&self, card_title: &str, extra: Option<&str>) -> String {
        let mut prompt = String::new();
        if !self.brief.trim().is_empty() {
            prompt.push_str(self.brief.trim());
            prompt.push_str("\n\n");
        }
        prompt.push_str("Task: ");
        prompt.push_str(card_title.trim());
        if let Some(extra) = extra {
            if !extra.trim().is_empty() {
                prompt.push_str("\n\nNotes from the operator:\n");
                prompt.push_str(extra.trim());
            }
        }
        prompt
    }
}

/// The crew a fresh install starts with.
pub fn defaults() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: DIRECTOR_ID.into(),
            name: "Director".into(),
            initial: "D".into(),
            title: "Orchestrator".into(),
            role: "Splits your intent into cards, picks the order and reviews every finished diff before it reaches you.".into(),
            brief: "Own the board. Break new intent into cards small enough for one run each, keep at most two cards ready, and review every finished diff against the card before it reaches me.".into(),
            tone: "info".into(),
            model: Some("opus".into()),
            permissions: vec!["Read".into(), "Search".into()],
            budget_usd: Some(1.5),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Human,
            ..Default::default()
        },
        AgentProfile {
            id: DEFAULT_WORKER.into(),
            name: "Builder".into(),
            initial: "B".into(),
            title: "Implementer".into(),
            role: "Does the work inside a fresh worktree and commits when it holds up.".into(),
            brief: "Implement one card at a time. Run the tests before committing, keep the diff scoped to the card, and stop and ask rather than widening your own permissions.".into(),
            tone: "accent".into(),
            model: Some("sonnet".into()),
            permissions: vec![
                "Read".into(),
                "Search".into(),
                "Edit".into(),
                "Write".into(),
                "Git".into(),
            ],
            budget_usd: Some(0.75),
            worktree: WorktreeMode::PerCard,
            reviewer: Reviewer::Director,
            ..Default::default()
        },
        AgentProfile {
            id: "scout".into(),
            name: "Scout".into(),
            initial: "S".into(),
            title: "Researcher".into(),
            role: "Reads the repo and answers questions without touching a file.".into(),
            brief: "Answer questions about the codebase with file paths and line numbers. Never edit. Prefer a short answer with citations over a summary.".into(),
            tone: "ok".into(),
            model: Some("haiku".into()),
            permissions: vec!["Read".into(), "Search".into()],
            budget_usd: Some(0.25),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Nobody,
            ..Default::default()
        },
    ]
}

/// Fill in anything a hand-edited or older profile file left out.
pub fn normalise(mut agents: Vec<AgentProfile>) -> Vec<AgentProfile> {
    if agents.is_empty() {
        return defaults();
    }
    for agent in &mut agents {
        if agent.id.trim().is_empty() {
            agent.id = crate::paths::sanitize(&agent.name);
        }
        if agent.name.trim().is_empty() {
            agent.name = agent.id.clone();
        }
        if agent.initial.trim().is_empty() {
            agent.initial = agent
                .name
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "?".to_string());
        }
    }
    // The Director is not optional: the review loop needs it.
    if !agents.iter().any(|a| a.id == DIRECTOR_ID) {
        let mut with_director = defaults()
            .into_iter()
            .filter(|a| a.id == DIRECTOR_ID)
            .collect::<Vec<_>>();
        with_director.extend(agents);
        return with_director;
    }
    agents
}

pub fn find<'a>(agents: &'a [AgentProfile], id: &str) -> Option<&'a AgentProfile> {
    agents
        .iter()
        .find(|a| a.id == id)
        .or_else(|| agents.iter().find(|a| a.id == DEFAULT_WORKER))
        .or_else(|| agents.iter().find(|a| a.id != DIRECTOR_ID))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_translate_into_tools() {
        let mut agent = AgentProfile {
            permissions: vec!["Read".into(), "Search".into(), "Git".into()],
            ..Default::default()
        };
        assert_eq!(
            agent.allowed_tools(),
            vec![
                "Bash(git *)".to_string(),
                "Glob".to_string(),
                "Grep".to_string(),
                "Read".to_string()
            ]
        );

        agent.permissions = vec!["Read".into(), "Read".into()];
        assert_eq!(agent.allowed_tools(), vec!["Read".to_string()]);
    }

    #[test]
    fn turning_off_automatic_review_sends_diffs_to_the_operator() {
        let agent = AgentProfile {
            reviewer: Reviewer::Director,
            ..Default::default()
        };
        let mut settings = Settings::default();
        assert_eq!(agent.run_profile(&settings).reviewer, Reviewer::Director);

        settings.director_reviews_first = false;
        assert_eq!(agent.run_profile(&settings).reviewer, Reviewer::Human);

        // An agent nobody reviews stays that way.
        let loose = AgentProfile {
            reviewer: Reviewer::Nobody,
            ..Default::default()
        };
        assert_eq!(loose.run_profile(&settings).reviewer, Reviewer::Nobody);
    }

    #[test]
    fn the_prompt_carries_the_brief_and_the_card() {
        let agent = AgentProfile {
            brief: "  Keep it scoped.  ".into(),
            ..Default::default()
        };
        let prompt = agent.prompt_for("Fix the retry", Some("see issue 12"));
        assert!(prompt.starts_with("Keep it scoped."));
        assert!(prompt.contains("Task: Fix the retry"));
        assert!(prompt.contains("see issue 12"));

        let bare = AgentProfile::default().prompt_for("Do a thing", None);
        assert_eq!(bare, "Task: Do a thing");
    }

    #[test]
    fn normalise_fills_gaps_and_keeps_a_director() {
        let normalised = normalise(vec![AgentProfile {
            name: "Odd One".into(),
            ..Default::default()
        }]);
        assert_eq!(normalised.len(), 2);
        assert_eq!(normalised[0].id, DIRECTOR_ID);
        let odd = normalised.iter().find(|a| a.name == "Odd One").unwrap();
        assert_eq!(odd.id, "odd-one");
        assert_eq!(odd.initial, "O");

        assert_eq!(normalise(vec![]).len(), defaults().len());
    }

    #[test]
    fn lookup_falls_back_to_the_default_worker() {
        let agents = defaults();
        assert_eq!(find(&agents, "scout").unwrap().id, "scout");
        assert_eq!(find(&agents, "ghost").unwrap().id, DEFAULT_WORKER);
        assert!(find(&[], "anything").is_none());
    }
}
