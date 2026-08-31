//! Where an agent's model runs.
//!
//! Relay talks to models by spawning something that speaks the Anthropic
//! Messages protocol — the agent SDK in the sidecar, or the `claude` command
//! line. Both read the same three environment variables, so pointing an agent
//! at a different model is not an integration: it is an endpoint and a token.
//!
//! Three of those endpoints are worth naming, because they are the reason
//! anyone wants this:
//!
//! - **Ollama** serves an Anthropic-compatible endpoint on localhost, so a run
//!   costs nothing and nothing leaves the machine.
//! - **Ollama Cloud** serves the same protocol at ollama.com — verified, not
//!   assumed: `/v1/messages` there answers 401 rather than 404, so the endpoint
//!   exists and only wants a key. No local daemon in the way.
//! - **OpenRouter** serves one over the wire and forwards to whichever provider
//!   actually holds the model, passing tool calls and thinking through intact —
//!   which matters here, because an agent that cannot call tools cannot work a
//!   card.
//!
//! A provider is stored once and referenced by id, so the key is typed once
//! rather than per agent, and changing it does not mean editing the crew.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use harness_ports::ModelProvider;

/// The default: whatever the machine is already logged into. Not a stored
/// provider — the absence of one.
pub const ANTHROPIC: &str = "";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Provider {
    /// Stable id an agent profile points at.
    pub id: String,
    pub name: String,
    /// Base URL of the Anthropic-compatible endpoint.
    pub base_url: String,
    /// The token that endpoint wants. Ollama ignores its value but wants one
    /// present; OpenRouter wants a real key.
    pub token: String,
}

impl Default for Provider {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            base_url: String::new(),
            token: String::new(),
        }
    }
}

impl Provider {
    /// What a run needs to reach it, or `None` when it is not usable — an
    /// endpoint with no URL would send the run to Anthropic while the operator
    /// believed otherwise, which is worse than refusing.
    pub fn resolve(&self) -> Option<ModelProvider> {
        let base_url = self.base_url.trim();
        if base_url.is_empty() {
            return None;
        }
        Some(ModelProvider {
            base_url: base_url.trim_end_matches('/').to_string(),
            // Ollama's own documentation uses the literal `ollama` here. An
            // empty token is refused by more endpoints than it is accepted by,
            // so a provider with no key still gets a value rather than a
            // confusing 401.
            auth_token: match self.token.trim() {
                "" => "relay".to_string(),
                token => token.to_string(),
            },
        })
    }
}

impl Provider {
    /// Is this endpoint going to refuse every run for want of a key?
    ///
    /// A local Ollama does not care what the token says; anything over the wire
    /// does. Without this the failure is a 401 in the middle of a run, which
    /// reads as the model being broken rather than as a field left blank in a
    /// settings screen the operator has not opened.
    pub fn needs_key(&self) -> bool {
        if !self.token.trim().is_empty() {
            return false;
        }
        let host = self
            .base_url
            .trim()
            .trim_start_matches("http://")
            .trim_start_matches("https://");
        !(host.starts_with("localhost")
            || host.starts_with("127.0.0.1")
            || host.starts_with("0.0.0.0")
            || host.starts_with("[::1]"))
    }
}

/// Starting points offered in the UI, so the two endpoints worth naming are one
/// click rather than a URL the operator has to go and look up. Neither is
/// installed by choosing it: a template is a filled-in form, not a commitment.
pub fn templates() -> Vec<Provider> {
    vec![
        Provider {
            id: "ollama".into(),
            name: "Ollama (local)".into(),
            base_url: "http://localhost:11434".into(),
            token: "ollama".into(),
        },
        Provider {
            // models.dev keys this endpoint's catalogue under exactly this id,
            // and the picker looks it up by it — renaming it silently empties
            // the model list.
            id: "ollama-cloud".into(),
            name: "Ollama Cloud".into(),
            base_url: "https://ollama.com".into(),
            token: String::new(),
        },
        Provider {
            id: "openrouter".into(),
            name: "OpenRouter".into(),
            base_url: "https://openrouter.ai/api".into(),
            token: String::new(),
        },
    ]
}

/// The provider an agent profile named, if it still exists. A profile pointing
/// at a deleted provider falls back to Anthropic rather than failing to start:
/// the run is the operator's work, and losing it to a stale reference helps
/// nobody.
pub fn find<'a>(providers: &'a [Provider], id: &str) -> Option<&'a Provider> {
    if id == ANTHROPIC {
        return None;
    }
    providers.iter().find(|p| p.id == id)
}

/// Is this what the operator means by "back to the Claude login"?
///
/// The empty string is how *absence* is stored, and a model cannot send an
/// empty string through a tool argument that must be a non-empty name — so
/// `anthropic` was the only way to clear an endpoint, and nothing said so out
/// loud. The Director concluded it was impossible, worked around it by editing
/// `agents.json` by hand, and wrote up "set_agent_model cannot unset a
/// provider" as a Relay defect. It could; it just could not be discovered.
///
/// So every word somebody would reasonably try now means the same thing, and
/// the tool says which one it is.
pub fn clears_provider(named: &str) -> bool {
    matches!(
        named.trim().to_ascii_lowercase().as_str(),
        "" | "anthropic" | "none" | "default" | "claude" | "claude login"
    )
}

/// An id nobody is using, derived from the name.
pub fn unique_id(name: &str, taken: &[Provider]) -> String {
    let base: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let base = base.trim_matches('-').to_string();
    let base = if base.is_empty() { "provider".to_string() } else { base };
    if !taken.iter().any(|p| p.id == base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|c| !taken.iter().any(|p| &p.id == c)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_without_a_url_is_not_a_provider() {
        let blank = Provider { id: "x".into(), base_url: "   ".into(), ..Default::default() };
        assert!(
            blank.resolve().is_none(),
            "an endpoint with no URL would quietly send the run to Anthropic"
        );
    }

    #[test]
    fn resolving_fills_a_token_and_trims_the_url() {
        let ollama = Provider {
            id: "ollama".into(),
            name: "Ollama".into(),
            base_url: "http://localhost:11434/".into(),
            token: String::new(),
        };
        let resolved = ollama.resolve().unwrap();
        assert_eq!(resolved.base_url, "http://localhost:11434", "the slash is dropped");
        assert_eq!(resolved.auth_token, "relay", "an endpoint still wants a token");

        let keyed = Provider { token: "sk-or-abc".into(), ..ollama };
        assert_eq!(keyed.resolve().unwrap().auth_token, "sk-or-abc");
    }

    #[test]
    fn the_environment_blanks_the_api_key() {
        let env = ModelProvider {
            base_url: "http://localhost:11434".into(),
            auth_token: "ollama".into(),
        }
        .env();
        let key = env.iter().find(|(k, _)| *k == "ANTHROPIC_API_KEY").unwrap();
        assert_eq!(
            key.1, "",
            "a key left in the environment outranks the base url, and the run would \
             go to Anthropic while the operator believed it was local"
        );
    }

    #[test]
    fn a_profile_pointing_at_a_deleted_provider_falls_back() {
        let providers = templates();
        assert!(find(&providers, "ollama").is_some());
        assert!(find(&providers, ANTHROPIC).is_none(), "the default is the absence of one");
        assert!(find(&providers, "gone").is_none(), "a stale reference must not fail the run");
    }

    #[test]
    fn an_endpoint_over_the_wire_says_when_its_key_is_missing() {
        let local = Provider {
            base_url: "http://localhost:11434".into(),
            token: String::new(),
            ..Default::default()
        };
        assert!(!local.needs_key(), "a local Ollama does not care what the token says");

        let cloud = Provider {
            base_url: "https://ollama.com".into(),
            token: String::new(),
            ..Default::default()
        };
        assert!(cloud.needs_key(), "otherwise this is a 401 in the middle of a run");
        assert!(!Provider { token: "sk-or-x".into(), ..cloud }.needs_key());
    }

    #[test]
    fn ids_do_not_collide() {
        let taken = vec![
            Provider { id: "ollama".into(), ..Default::default() },
            Provider { id: "ollama-2".into(), ..Default::default() },
        ];
        assert_eq!(unique_id("Ollama", &taken), "ollama-3");
        assert_eq!(unique_id("  ", &taken), "provider");
    }
}

#[cfg(test)]
mod clearing_tests {
    use super::*;

    /// Limpar um endpoint tinha uma palavra só e não estava escrita em lado
    /// nenhum. Agora tem as que qualquer pessoa tentaria.
    #[test]
    fn every_reasonable_way_of_saying_the_claude_login_clears_it() {
        for said in ["anthropic", "Anthropic", "none", "default", "claude", "Claude login", "  ", ""] {
            assert!(clears_provider(said), "{said:?} should clear the endpoint");
        }
        // E um id a sério continua a ser um id a sério.
        for said in ["openrouter", "ollama-cloud", "ollama"] {
            assert!(!clears_provider(said), "{said:?} is an endpoint, not a clear");
        }
    }
}
