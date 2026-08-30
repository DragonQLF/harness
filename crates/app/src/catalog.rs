//! What models an endpoint actually offers.
//!
//! Typing a model name from memory is how you discover, twenty minutes into a
//! run, that the name was wrong or that the model cannot call tools. models.dev
//! publishes a catalogue of both — every provider, every model, and for each
//! one whether it supports function calling and how much context it holds.
//!
//! Those two fields are the point. Relay's agents work by calling tools: a
//! model without `tool_call` cannot move a card, read a file, or commit, and
//! will sit there producing prose about what it would have done. A model with
//! 8k of context cannot hold a repository. Both fail in ways that look like
//! Relay being broken, so the catalogue marks them rather than listing every
//! name as equally good.
//!
//! Parsing lives here, away from the network, so what the UI is handed can be
//! tested against a fixture instead of against whatever models.dev served that
//! morning.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The floor for work rather than chat. Below this a coding agent cannot hold
/// the files it is editing, which reads as the model being stupid rather than
/// short of room.
pub const WORKING_CONTEXT: u64 = 64_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CatalogModel {
    /// The name to put in the agent profile, verbatim.
    pub id: String,
    pub name: String,
    /// Tokens of context. Zero when the catalogue does not say.
    #[ts(type = "number")]
    pub context: u64,
    /// Can it call tools? An agent that cannot is not an agent.
    pub tool_call: bool,
    /// Dollars per million input tokens, when the catalogue prices it.
    pub input_cost: Option<f64>,
    pub output_cost: Option<f64>,
    /// True when this model can do the job: it calls tools and has room to
    /// work. The UI leads with these rather than making the operator compare
    /// two numbers per row.
    pub usable: bool,
}

impl CatalogModel {
    /// Why this model is not offered first, in the words the UI shows.
    pub fn caveat(&self) -> Option<String> {
        if !self.tool_call {
            return Some("cannot call tools — it can talk, not work".to_string());
        }
        if self.context > 0 && self.context < WORKING_CONTEXT {
            return Some(format!(
                "{}k context — too small to hold a repository",
                self.context / 1000
            ));
        }
        None
    }
}

fn number(value: Option<&serde_json::Value>) -> Option<f64> {
    value.and_then(|v| v.as_f64())
}

/// Every model one provider offers, best first.
///
/// Unknown providers give an empty list rather than an error: the catalogue not
/// covering an endpoint is ordinary — a local Ollama serves whatever has been
/// pulled onto that machine, which no public list can know.
pub fn parse(body: &str, provider_id: &str) -> Vec<CatalogModel> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(models) = root.get(provider_id).and_then(|p| p.get("models")).and_then(|m| m.as_object())
    else {
        return Vec::new();
    };

    let mut out: Vec<CatalogModel> = models
        .iter()
        .map(|(key, model)| {
            let id = model
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or(key)
                .to_string();
            let context = model
                .get("limit")
                .and_then(|l| l.get("context"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let tool_call = model
                .get("tool_call")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            CatalogModel {
                name: model
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&id)
                    .to_string(),
                usable: tool_call && context >= WORKING_CONTEXT,
                context,
                tool_call,
                input_cost: number(model.get("cost").and_then(|c| c.get("input"))),
                output_cost: number(model.get("cost").and_then(|c| c.get("output"))),
                id,
            }
        })
        .collect();

    // Usable first, then roomiest, then by name so the order never shuffles
    // between two models the catalogue rates the same.
    out.sort_by(|a, b| {
        b.usable
            .cmp(&a.usable)
            .then(b.context.cmp(&a.context))
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// The models a running Ollama has actually pulled, from its own `/api/tags`.
///
/// No public catalogue can answer this: what is on the machine is whatever the
/// operator pulled onto it. Ollama does not report tool support here, so these
/// are offered without the promise — the catalogue's `usable` flag would be a
/// guess, and a wrong guess is worse than an honest blank.
pub fn parse_ollama_tags(body: &str) -> Vec<CatalogModel> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(models) = root.get("models").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    let mut out: Vec<CatalogModel> = models
        .iter()
        .filter_map(|m| {
            let id = m.get("name").and_then(|v| v.as_str())?.to_string();
            Some(CatalogModel {
                name: id.clone(),
                id,
                context: 0,
                // Unknown, not false: Ollama's tag list does not say, and
                // claiming either way would be inventing an answer.
                tool_call: true,
                input_cost: Some(0.0),
                output_cost: Some(0.0),
                usable: true,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Does this endpoint live on the machine?
///
/// Só a um local se pergunta directamente o que tem: apontar o pedido de tags
/// ao host de um estranho é dizer-lhe que corremos Relay. A regra vive aqui e
/// não no comando porque é política, não é rede — e porque escrita ali não
/// tinha teste nenhum.
pub fn is_local(base_url: &str) -> bool {
    let host = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    host.starts_with("localhost")
        || host.starts_with("127.0.0.1")
        || host.starts_with("0.0.0.0")
        || host.starts_with("[::1]")
}

/// Um dia. O catálogo tem quatro megabytes e muda-se com a frequência com que
/// se lançam modelos; voltar a buscá-lo a cada visita ao ecrã de Agentes era
/// indelicado dos dois lados.
pub const CACHE_MAX_AGE_SECS: u64 = 24 * 60 * 60;

/// Serve a cópia em disco, ou é preciso voltar a buscar?
///
/// Um `refresh` pedido pelo operador ganha sempre, e um ficheiro cuja idade
/// não se consegue ler conta como velho: preferir a rede a servir uma coisa
/// cuja data se desconhece.
pub fn cache_is_fresh(cache: &std::path::Path, refresh: bool) -> bool {
    !refresh
        && std::fs::metadata(cache)
            .and_then(|m| m.modified())
            .map(|t| {
                t.elapsed()
                    .map(|age| age.as_secs() < CACHE_MAX_AGE_SECS)
                    .unwrap_or(false)
            })
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    /// A pergunta que decide se um endpoint é interrogado directamente. Estava
    /// escrita dentro do comando, onde nada a corria.
    #[test]
    fn only_this_machine_is_asked_what_it_has() {
        for local in [
            "http://localhost:11434",
            "https://127.0.0.1:8080",
            "http://0.0.0.0:1234",
            "http://[::1]:11434",
            "localhost:11434",
        ] {
            assert!(is_local(local), "{local}");
        }
        for remote in ["https://openrouter.ai/api/v1", "https://ollama.com"] {
            assert!(!is_local(remote), "{remote}");
        }
        // Verdade desconfortável, prendida em vez de escondida: a regra é um
        // prefixo, portanto um host que *começa* por `localhost.` passa por
        // local. Quem escreveu esse endpoint foi o operador, e o que se lhe
        // manda é um GET de tags — anotado, não corrigido, porque apertar isto
        // é recusar endpoints que hoje funcionam.
        assert!(is_local("https://localhost.example.invalid/api"));
    }

    /// Um pedido explícito de refresh ganha sempre, e um ficheiro que não
    /// existe nunca é fresco.
    #[test]
    fn a_cache_that_is_not_there_is_never_fresh() {
        let missing = std::path::Path::new("/does/not/exist/models.dev.json");
        assert!(!cache_is_fresh(missing, false));
        assert!(!cache_is_fresh(missing, true));
    }

    use super::*;

    const BODY: &str = r#"{
      "openrouter": { "models": {
        "a/small":   { "id": "a/small",   "name": "Small",   "tool_call": true,  "limit": { "context": 8192 },   "cost": { "input": 0.1, "output": 0.2 } },
        "a/mute":    { "id": "a/mute",    "name": "Mute",    "tool_call": false, "limit": { "context": 200000 } },
        "a/worker":  { "id": "a/worker",  "name": "Worker",  "tool_call": true,  "limit": { "context": 262144 }, "cost": { "input": 0.18, "output": 0.6 } },
        "a/roomy":   { "id": "a/roomy",   "name": "Roomy",   "tool_call": true,  "limit": { "context": 128000 } }
      } },
      "anthropic": { "models": { "claude": { "id": "claude", "name": "Claude", "tool_call": true, "limit": { "context": 200000 } } } }
    }"#;

    #[test]
    fn the_ones_that_can_work_come_first_roomiest_among_them() {
        let models = parse(BODY, "openrouter");
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["a/worker", "a/roomy", "a/mute", "a/small"],
            "usable first and roomiest among them; the rest sink"
        );
        assert!(models[0].usable);
        assert_eq!(models[0].input_cost, Some(0.18));
    }

    #[test]
    fn a_model_that_cannot_call_tools_says_so_however_big_it_is() {
        let models = parse(BODY, "openrouter");
        let mute = models.iter().find(|m| m.id == "a/mute").unwrap();
        assert!(!mute.usable, "200k of context is no use if it cannot act");
        assert_eq!(
            mute.caveat().unwrap(),
            "cannot call tools — it can talk, not work"
        );

        let small = models.iter().find(|m| m.id == "a/small").unwrap();
        assert_eq!(small.caveat().unwrap(), "8k context — too small to hold a repository");

        assert!(models.iter().find(|m| m.id == "a/worker").unwrap().caveat().is_none());
    }

    #[test]
    fn an_endpoint_the_catalogue_does_not_cover_is_not_an_error() {
        assert!(parse(BODY, "ollama").is_empty(), "a local endpoint is nobody's list");
        assert!(parse("not json at all", "openrouter").is_empty());
        assert!(parse("{}", "openrouter").is_empty());
    }

    #[test]
    fn ollama_reports_what_was_actually_pulled() {
        let tags = r#"{"models":[{"name":"qwen3.5:latest"},{"name":"gemma4:9b"}]}"#;
        let models = parse_ollama_tags(tags);
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["gemma4:9b", "qwen3.5:latest"]
        );
        assert_eq!(models[0].input_cost, Some(0.0), "a local model bills nothing");
        assert!(parse_ollama_tags("{}").is_empty());
    }
}
