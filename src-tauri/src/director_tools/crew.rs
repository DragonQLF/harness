//! Quem é a tripulação e em que modelo corre cada um.
//!
//! O `add_endpoint` vive aqui e não com as definições porque é a mesma
//! pergunta vista do outro lado — "onde é que este agente corre" — e partilha
//! com os outros três a redacção de um endpoint que falta ou que não tem chave.

use std::sync::Arc;

use harness_app::agents::AgentProfile;
use harness_ports::{Reviewer, ToolCall, ToolReply, WorktreeMode};

use super::text;
use crate::workspace::Workspace;

/// The endpoints an agent could be pointed at, for a refusal that tells the
/// model what to send instead of only what was wrong.
fn endpoint_names(providers: &[harness_app::providers::Provider]) -> String {
    let mut names: Vec<String> = vec!["anthropic".to_string()];
    names.extend(providers.iter().map(|p| p.id.clone()));
    names.join(", ")
}

/// What to say when the endpoint the operator named is not set up, or is set up
/// without a key. Both end the same way: somebody has to open Settings, and
/// pointing at it beats describing it.
fn missing_endpoint(asked: &str, providers: &[harness_app::providers::Provider]) -> String {
    format!(
        "there is no model endpoint called {asked}. Configured: {}. Adding one is a \
         click in Settings under Model endpoints — take them there with \
         open_screen(\"settings\") rather than describing it, then ask for the key.",
        endpoint_names(providers)
    )
}

/// Appended when the endpoint exists but has nothing to authenticate with.
fn key_warning(provider: &harness_app::providers::Provider) -> String {
    if !provider.needs_key() {
        return String::new();
    }
    format!(
        " — but {} has no key, so every run on it is refused before it starts. Take \
         them to Settings with open_screen(\"settings\") and ask them to paste one.",
        provider.name
    )
}

/// " on qwen3.5 via Ollama Cloud", or " on the Claude login".
fn describe_model(
    agent: &harness_app::agents::AgentProfile,
    providers: &[harness_app::providers::Provider],
) -> String {
    let where_ = match agent.backend {
        harness_ports::Backend::Codex => "Codex, on this machine's ChatGPT plan".to_string(),
        harness_ports::Backend::Claude => harness_app::providers::find(providers, &agent.provider)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "the Claude login".to_string()),
    };
    match agent.model.as_deref() {
        Some(model) if !model.is_empty() => format!(" on {model} via {where_}"),
        _ => format!(" on {where_}"),
    }
}

/// Onde está este agente na tripulação, ou a recusa que nomeia quem existe.
///
/// Devolve o índice e não uma referência: as cinco ferramentas que mexem num
/// perfil precisam da lista inteira para a voltar a guardar, e um `&mut` para
/// dentro dela não deixaria fazê-lo. A mensagem estava escrita cinco vezes.
pub(super) fn slot_of(crew: &[AgentProfile], agent_id: &str) -> Result<usize, ToolReply> {
    crew.iter().position(|a| a.id == agent_id).ok_or_else(|| {
        ToolReply::refused(format!(
            "there is no agent called {agent_id}. The crew is: {}",
            crew.iter()
                .map(|a| a.id.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

pub(super) async fn add_endpoint(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let named = text(&call.input, "name").unwrap_or_default();
        let template = harness_app::providers::templates()
            .into_iter()
            .find(|t| {
                t.id.eq_ignore_ascii_case(&named) || t.name.eq_ignore_ascii_case(&named)
            });
        let url = text(&call.input, "base_url");
        let (name, base_url) = match (&template, url) {
            (Some(t), url) => (t.name.clone(), url.unwrap_or_else(|| t.base_url.clone())),
            (None, Some(url)) if !named.is_empty() => (named.clone(), url),
            (None, _) => {
                return ToolReply::refused(
                    "add_endpoint needs a known name — ollama, ollama-cloud or openrouter —                          or a name and a base_url for something else that speaks the Anthropic                          Messages protocol",
                )
            }
        };

        let mut settings = ws.settings();
        if let Some(existing) = settings
            .providers
            .iter()
            .find(|p| p.base_url.trim_end_matches('/') == base_url.trim_end_matches('/'))
        {
            return ToolReply::refused(format!(
                "{} already points at {base_url}{}",
                existing.name,
                if existing.needs_key() {
                    " — it just has no key yet"
                } else {
                    ""
                }
            ));
        }

        let taken: Vec<harness_app::providers::Provider> = settings.providers.clone();
        let id = template
            .as_ref()
            .map(|t| t.id.clone())
            .filter(|id| !taken.iter().any(|p| &p.id == id))
            .unwrap_or_else(|| harness_app::providers::unique_id(&name, &taken));

        settings.providers.push(harness_app::providers::Provider {
            id: id.clone(),
            name: name.clone(),
            base_url: base_url.clone(),
            token: String::new(),
        });
        match ws.set_settings(settings) {
            Ok(_) => ToolReply::ok(format!(
                "added {name} ({id}) at {base_url}. It has no key yet, and every run on it                      is refused until it does — open the settings screen for them and ask them                      to paste one into its key field. Do not ask them to send it to you here:                      this conversation is written to disk."
            )),
            Err(e) => ToolReply::refused(e),
        }
}

pub(super) async fn create_agent(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let Some(name) = text(&call.input, "name") else {
            return ToolReply::refused("create_agent needs a name");
        };
        let taken: Vec<String> = ws.agents().await.into_iter().map(|a| a.id).collect();
        if ws.agents().await.iter().any(|a| a.name.eq_ignore_ascii_case(&name)) {
            return ToolReply::refused(format!(
                "there is already an agent called {name}; use set_agent_model to change                      the one that exists, or pick another name"
            ));
        }
        let mut made = harness_app::agents::drafted(
            &name,
            &text(&call.input, "title").unwrap_or_default(),
            &text(&call.input, "brief").unwrap_or_default(),
            &taken,
        );
        // The model is the point of asking, so it is set here rather than
        // left for a second round trip.
        if let Some(model) = text(&call.input, "model") {
            made.model = Some(model);
        }
        if let Some(provider) = text(&call.input, "provider") {
            let settings = ws.settings();
            if harness_app::providers::find(&settings.providers, &provider).is_none() {
                return ToolReply::refused(missing_endpoint(&provider, &settings.providers));
            }
            made.provider = provider;
        }
        let providers = ws.settings().providers;
        let warning = harness_app::providers::find(&providers, &made.provider)
            .map(key_warning)
            .unwrap_or_default();
        let summary = format!(
            "created {} ({}){}",
            made.name,
            made.id,
            describe_model(&made, &providers)
        );
        let mut crew = ws.agents().await;
        crew.push(made);
        match ws.set_agents(crew).await {
            Ok(_) => ToolReply::ok(format!(
                "{summary}{warning}. It can read and search; anything more is yours to                      grant on the Agents screen."
            )),
            Err(e) => ToolReply::refused(e),
        }
}

pub(super) async fn edit_agent(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let Some(agent_id) = text(&call.input, "agent_id") else {
            return ToolReply::refused("edit_agent needs an agent_id");
        };
        let mut crew = ws.agents().await;
        let slot = match slot_of(&crew, &agent_id) {
            Ok(i) => &mut crew[i],
            Err(refusal) => return refusal,
        };

        let mut changed: Vec<String> = Vec::new();
        if let Some(name) = text(&call.input, "name") {
            changed.push(format!("name to {name}"));
            slot.name = name;
        }
        if let Some(title) = text(&call.input, "title") {
            changed.push("title".to_string());
            slot.title = title;
        }
        if let Some(brief) = text(&call.input, "brief") {
            changed.push("brief".to_string());
            slot.brief = brief;
        }
        if let Some(budget) = call.input.get("budget_usd").and_then(|v| v.as_f64()) {
            if budget <= 0.0 {
                return ToolReply::refused(
                    "a budget of zero would stop every run before it started;                          leave it out to keep the current one",
                );
            }
            changed.push(format!("budget to ${budget:.2}"));
            slot.budget_usd = Some(budget);
        }
        if let Some(paused) = call.input.get("paused").and_then(|v| v.as_bool()) {
            changed.push(if paused { "paused it".into() } else { "resumed it".to_string() });
            slot.paused = paused;
        }
        // Onde o trabalho deste agente acontece. Estava de fora, e a ausência
        // fechava a única saída: um agente com `worktree: none` corre contra a
        // checkout viva, portanto dar-lhe escrita é editar a árvore do
        // operador sem ramo, sem diff e sem nada para aprovar. O Director via o
        // problema, via a correcção, e não lhe chegava.
        if let Some(worktree) = text(&call.input, "worktree") {
            slot.worktree = match worktree.as_str() {
                "per_card" | "per-card" => WorktreeMode::PerCard,
                "shared" => WorktreeMode::Shared,
                "none" => WorktreeMode::None,
                other => {
                    return ToolReply::refused(format!(
                        "{other} is not a worktree mode. Use per_card, shared or none."
                    ))
                }
            };
            changed.push(format!("worktree to {worktree}"));
        }
        if let Some(reviewer) = text(&call.input, "reviewer") {
            slot.reviewer = match reviewer.as_str() {
                "director" => Reviewer::Director,
                "human" | "you" | "operator" => Reviewer::Human,
                "nobody" | "none" => Reviewer::Nobody,
                other => {
                    return ToolReply::refused(format!(
                        "{other} is not a reviewer. Use director, human or nobody."
                    ))
                }
            };
            changed.push(format!("reviewer to {reviewer}"));
        }

        if changed.is_empty() {
            return ToolReply::refused(
                "edit_agent was given nothing to change. Name the fields to set;                      tools and permissions are not among them.",
            );
        }
        let summary = format!("{}: changed {}", slot.name, changed.join(", "));
        match ws.set_agents(crew).await {
            Ok(_) => ToolReply::ok(summary),
            Err(e) => ToolReply::refused(e),
        }
}

pub(super) async fn set_agent_model(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let Some(agent_id) = text(&call.input, "agent_id") else {
            return ToolReply::refused("set_agent_model needs an agent_id");
        };
        let settings = ws.settings();
        let mut crew = ws.agents().await;
        let slot = match slot_of(&crew, &agent_id) {
            Ok(i) => &mut crew[i],
            Err(refusal) => return refusal,
        };
        // The backend first: it decides whether the other two mean anything.
        // Moving an agent to Codex and setting an Anthropic endpoint in the
        // same call would otherwise store a setting that never applies.
        if let Some(named) = text(&call.input, "backend") {
            let backend = harness_ports::Backend::parse(&named);
            if backend.id() != named.trim().to_ascii_lowercase() {
                return ToolReply::refused(format!(
                    "there is no backend called {named}. It is `claude` or `codex`."
                ));
            }
            if backend != slot.backend {
                slot.backend = backend;
                // The model name belongs to the old backend's vocabulary.
                // Clearing it lets the new one pick its own default, which is
                // the only outcome here that cannot fail mid-run — unless this
                // same call names a model, handled just below.
                slot.model = None;
            }
        }
        if let Some(provider) = text(&call.input, "provider") {
            if slot.backend == harness_ports::Backend::Codex {
                return ToolReply::refused(
                    "a Codex agent has no endpoint to set: it runs on the ChatGPT plan this \
                     machine is logged into. Switch it back to `claude` first.",
                );
            }
            // Absence guarda-se como string vazia, e um modelo não manda uma
            // string vazia num argumento que tem de ser um nome. Qualquer
            // palavra que alguém tentaria para dizer "volta ao login da Claude"
            // limpa — ver `providers::clears_provider` para o que isto custou
            // quando só havia uma.
            if harness_app::providers::clears_provider(&provider) {
                slot.provider = harness_app::providers::ANTHROPIC.to_string();
            } else if harness_app::providers::find(&settings.providers, &provider).is_none() {
                return ToolReply::refused(missing_endpoint(&provider, &settings.providers));
            } else {
                slot.provider = provider;
            }
        }
        if let Some(model) = text(&call.input, "model") {
            if !harness_app::agents::model_fits(slot.backend, &model) {
                return ToolReply::refused(format!(
                    "{model} is not a {} model. Set `backend` in the same call if that is \
                     what you meant.",
                    slot.backend.id()
                ));
            }
            slot.model = Some(model);
        }
        let summary = format!(
            "{} now runs{}{}",
            slot.name,
            describe_model(slot, &settings.providers),
            harness_app::providers::find(&settings.providers, &slot.provider)
                .map(key_warning)
                .unwrap_or_default()
        );
        match ws.set_agents(crew).await {
            Ok(_) => ToolReply::ok(summary),
            Err(e) => ToolReply::refused(e),
        }
}
