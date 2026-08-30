//! A tripulação, vista do lado do operador: quem existe, o que corre em quê, e
//! os números de cada um somados por toda a máquina.
//!
//! O que o *modelo* pode mudar num perfil vive noutro sítio
//! (`director_tools::crew`), e de propósito: aqui não há guardo de
//! auto-elevação porque não há modelo — quem chama isto é o ecrã.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use harness_app::agents::AgentProfile;
use harness_app::insights::{self, AgentStats};

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

#[tauri::command]
pub async fn agents_get(ws: Shared<'_>) -> Result<Vec<AgentProfile>, String> {
    Ok(ws.agents().await)
}

#[tauri::command]
pub async fn agents_save(
    agents: Vec<AgentProfile>,
    ws: Shared<'_>,
) -> Result<Vec<AgentProfile>, String> {
    ws.set_agents(agents).await
}

/// One of the two browsers, described for the screen that offers it.
#[derive(Debug, Serialize)]
pub struct BrowserOffer {
    pub id: String,
    pub name: String,
    /// What the operator is agreeing to, in the words the module writes.
    pub note: String,
}

/// The browsers an agent can be given, and what each one costs to give.
#[tauri::command]
pub async fn browser_offers() -> Result<Vec<BrowserOffer>, String> {
    use harness_app::browsers::Browser;
    Ok([Browser::Private, Browser::SignedIn]
        .into_iter()
        .map(|b| BrowserOffer {
            id: b.id().to_string(),
            name: b.name().to_string(),
            note: b.note().to_string(),
        })
        .collect())
}

/// Give one agent one of the browsers.
///
/// A one-click grant rather than a form, because every field of it is decided
/// — the command, the flags, the profile directory, the tool list — and the
/// only real choice is which of the two. The choice that matters is made on the
/// Agents screen, per agent, and it is never made by a model: this is an
/// operator command, and `add_mcp_server` remains the Director's own path with
/// its own self-elevation guard.
#[tauri::command]
pub async fn browser_grant(
    agent_id: String,
    browser_id: String,
    ws: Shared<'_>,
) -> Result<Vec<AgentProfile>, String> {
    let which = harness_app::browsers::Browser::from_id(&browser_id)
        .ok_or_else(|| format!("there is no browser called {browser_id}"))?;
    use harness_ports::ClockPort;
    let grant = harness_app::browsers::grant(
        which,
        &ws.paths.browser_profile_dir(),
        crate::workspace::SystemClock.now_millis(),
    );
    let mut crew = ws.agents().await;
    let slot = crew
        .iter_mut()
        .find(|a| a.id == agent_id)
        .ok_or_else(|| format!("there is no agent called {agent_id}"))?;
    harness_app::grants::upsert_mcp(&mut slot.mcp_servers, grant);
    ws.set_agents(crew).await
}

/// Per-agent numbers, summed across every project so an agent page reads the
/// whole workspace. Line counts come from the git history.
#[tauri::command]
pub async fn agents_stats(
    tz_offset_minutes: Option<i64>,
    ws: Shared<'_>,
) -> Result<Vec<AgentStats>, String> {
    let tz = tz_offset_minutes.unwrap_or(0);
    let mut merged: std::collections::BTreeMap<String, AgentStats> = Default::default();

    for project in ws.projects().await {
        if !Path::new(&project.path).is_dir() {
            continue;
        }
        let Ok(runtime) = ws.runtime(&project.id).await else {
            continue;
        };
        let cards = runtime.engine.snapshot().await?.cards;
        let store = Arc::clone(&runtime.store);
        let git = Arc::clone(&runtime.git);
        // O log inteiro e quatrocentos commits: leitura de disco a sério, e
        // portanto fora do executor async.
        let (history, commits) = tauri::async_runtime::spawn_blocking(move || {
            (
                harness_ports::StorePort::read_all(store.as_ref()).unwrap_or_default(),
                git.recent_commits(400),
            )
        })
        .await
        .map_err(|e| e.to_string())?;

        insights::merge_agent_stats(&mut merged, insights::agent_stats(&history, &cards, tz));
        insights::merge_commit_lines(
            &mut merged,
            commits.into_iter().map(|c| (c.agent, c.added, c.removed)),
        );
    }

    insights::settle_averages(&mut merged);
    Ok(merged.into_values().collect())
}

/// What models an endpoint offers, so the operator picks a name instead of
/// remembering one.
///
/// Fetched here rather than in the window: the CSP that keeps the webview off
/// the network is not something to punch a hole in for a convenience. The
/// answer is cached in app data — the catalogue is four megabytes and changes
/// about as often as models are released, so re-fetching it on every visit to
/// the Agents screen would be rude to both ends.
#[tauri::command]
pub async fn model_catalog(
    provider_id: String,
    base_url: String,
    refresh: Option<bool>,
    ws: Shared<'_>,
) -> Result<Vec<harness_app::catalog::CatalogModel>, String> {
    use harness_app::catalog;

    // A local endpoint is nobody's published list: ask the machine what it has.
    if catalog::is_local(&base_url) {
        let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
        let body = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| format!("could not reach {url}: {e}"))?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        return Ok(catalog::parse_ollama_tags(&body));
    }

    let cache = ws.paths.root().join("models.dev.json");
    let body = if catalog::cache_is_fresh(&cache, refresh.unwrap_or(false)) {
        std::fs::read_to_string(&cache).map_err(|e| e.to_string())?
    } else {
        let fetched = reqwest::Client::new()
            .get("https://models.dev/api.json")
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| format!("could not reach models.dev: {e}"))?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        // A cache write that fails is not worth failing the call over; the
        // catalogue is in hand either way.
        let _ = std::fs::write(&cache, &fetched);
        fetched
    };
    Ok(catalog::parse(&body, &provider_id))
}
