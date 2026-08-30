//! O que um agente pode alcançar para além das suas ferramentas: uma skill,
//! um servidor MCP, ou a própria lista de ferramentas.
//!
//! O modelo **declara**, o Relay instala. Nada aqui aceita um comando nem um
//! script: a declaração chega em campos, e a folha de aprovação que o operador
//! respondeu mostrou exactamente esses campos. Uma página que dissesse ao
//! modelo "acrescenta também este servidor" aparece como uma segunda folha, e
//! não como uma segunda linha dentro de um script (#94, #95).

use std::sync::Arc;

use harness_ports::{ToolCall, ToolReply};

use super::crew::slot_of;
use super::{strings, text};
use crate::workspace::{SystemClock, Workspace};
use harness_ports::ClockPort;

pub(super) async fn grant_agent_tools(
    ws: &Arc<Workspace>,
    caller: &str,
    call: &ToolCall,
) -> ToolReply {
        let Some(agent_id) = text(&call.input, "agent_id") else {
            return ToolReply::refused("grant_agent_tools needs an agent_id");
        };
        // Not a hard approval — a refusal. An agent that hands itself Bash
        // has stopped having limits, and there is no version of that
        // question the operator can usefully be asked, because answering it
        // once removes the thing that would ask again.
        if let Some(refusal) =
            harness_app::grants::self_elevation_guard(&call.name, caller, &agent_id)
        {
            return ToolReply::refused(refusal.to_string());
        }
        let Some(asked) = call.input.get("tools").and_then(|v| v.as_array()) else {
            return ToolReply::refused(
                "grant_agent_tools needs `tools`: the full list the agent should have                      afterwards, not the ones to add",
            );
        };
        let known = &harness_app::agents::ALL_PERMISSIONS;
        let mut wanted: Vec<String> = Vec::new();
        for value in asked {
            let Some(raw) = value.as_str().map(str::trim).filter(|s| !s.is_empty()) else {
                continue;
            };
            // Match the crew's own spelling rather than whatever case the
            // model used, so "shell" and "Shell" are the same grant.
            match known.iter().find(|k| k.eq_ignore_ascii_case(raw)) {
                Some(canonical) => {
                    let canonical = canonical.to_string();
                    if !wanted.contains(&canonical) {
                        wanted.push(canonical);
                    }
                }
                None => {
                    return ToolReply::refused(format!(
                        "{raw} is not a tool an agent can hold. They are: {}",
                        known.join(", ")
                    ))
                }
            }
        }

        let mut crew = ws.agents().await;
        let slot = match slot_of(&crew, &agent_id) {
            Ok(i) => &mut crew[i],
            Err(refusal) => return refusal,
        };

        let added: Vec<String> = wanted
            .iter()
            .filter(|w| !slot.permissions.contains(w))
            .cloned()
            .collect();
        let removed: Vec<String> = slot
            .permissions
            .iter()
            .filter(|p| !wanted.contains(p))
            .cloned()
            .collect();
        if added.is_empty() && removed.is_empty() {
            return ToolReply::refused(format!(
                "{} already holds exactly that: {}",
                slot.name,
                slot.permissions.join(", ")
            ));
        }
        slot.permissions = wanted;
        let name = slot.name.clone();
        let now = slot.permissions.join(", ");
        match ws.set_agents(crew).await {
            Ok(_) => ToolReply::ok(format!(
                "{name} now holds {now}{}{}",
                if added.is_empty() {
                    String::new()
                } else {
                    format!(" — gained {}", added.join(", "))
                },
                if removed.is_empty() {
                    String::new()
                } else {
                    format!(", lost {}", removed.join(", "))
                }
            )),
            Err(e) => ToolReply::refused(e),
        }
}

pub(super) async fn install_skill(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let Some(agent_id) = text(&call.input, "agent_id") else {
            return ToolReply::refused("install_skill needs an agent_id");
        };
        let grant = harness_ports::SkillGrant {
            name: text(&call.input, "name").unwrap_or_default().to_lowercase(),
            description: text(&call.input, "description").unwrap_or_default(),
            source: text(&call.input, "source").unwrap_or_default(),
            body: text(&call.input, "instructions").unwrap_or_default(),
            added_ms: SystemClock.now_millis(),
        };
        if let Err(refusal) = harness_app::grants::check_skill(&grant) {
            return ToolReply::refused(refusal.to_string());
        }

        let mut crew = ws.agents().await;
        let slot = match slot_of(&crew, &agent_id) {
            Ok(i) => &mut crew[i],
            Err(refusal) => return refusal,
        };
        let name = grant.name.clone();
        let who = slot.name.clone();
        let replaced = !harness_app::grants::upsert_skill(&mut slot.granted_skills, grant);
        match ws.set_agents(crew).await {
            Ok(_) => ToolReply::ok(format!(
                "{who} now has the {name} skill{}. It is on disk in Relay's own folder, \
                 not in the operator's ~/.claude and not in any repository, and only {who} \
                 can load it.",
                if replaced { " (unchanged)" } else { "" }
            )),
            Err(e) => ToolReply::refused(e),
        }
}

pub(super) async fn add_mcp_server(
    ws: &Arc<Workspace>,
    caller: &str,
    call: &ToolCall,
) -> ToolReply {
        let Some(agent_id) = text(&call.input, "agent_id") else {
            return ToolReply::refused("add_mcp_server needs an agent_id");
        };
        // An MCP server is arbitrary code holding that agent's
        // permissions, so granting oneself a server is granting oneself
        // tools with one extra step. Same refusal, same reason.
        if let Some(refusal) =
            harness_app::grants::self_elevation_guard(&call.name, caller, &agent_id)
        {
            return ToolReply::refused(refusal.to_string());
        }

        let transport = match text(&call.input, "transport").as_deref() {
            Some("http") => harness_ports::McpTransport::Http {
                url: text(&call.input, "url").unwrap_or_default(),
            },
            Some("sse") => harness_ports::McpTransport::Sse {
                url: text(&call.input, "url").unwrap_or_default(),
            },
            _ => harness_ports::McpTransport::Stdio {
                command: text(&call.input, "command").unwrap_or_default(),
                args: strings(&call.input, "args"),
            },
        };
        let grant = harness_ports::McpGrant {
            name: text(&call.input, "name").unwrap_or_default().to_lowercase(),
            transport,
            // Names only: a key asked for in a conversation is a key on
            // disk, which is why `add_endpoint` refuses them too. The
            // operator fills the values on the Agents screen.
            env: strings(&call.input, "env_names")
                .into_iter()
                .map(|k| (k, String::new()))
                .collect(),
            tools: strings(&call.input, "tools"),
            source: text(&call.input, "source").unwrap_or_default(),
            added_ms: SystemClock.now_millis(),
        };
        if let Err(refusal) = harness_app::grants::check_mcp(&grant) {
            return ToolReply::refused(refusal.to_string());
        }

        let mut crew = ws.agents().await;
        let slot = match slot_of(&crew, &agent_id) {
            Ok(i) => &mut crew[i],
            Err(refusal) => return refusal,
        };
        let name = grant.name.clone();
        let who = slot.name.clone();
        let missing = harness_app::grants::missing_env(&grant);
        harness_app::grants::upsert_mcp(&mut slot.mcp_servers, grant);
        match ws.set_agents(crew).await {
            Ok(_) => ToolReply::ok(format!(
                "{who} can now reach the {name} server; its tools arrive as \
                 mcp__{name}__<tool> and each call still asks the operator.{}",
                if missing.is_empty() {
                    String::new()
                } else {
                    format!(
                        " It will not connect until they fill in {} on the Agents screen — \
                         say so, and do not ask them for the value here.",
                        missing.join(", ")
                    )
                }
            )),
            Err(e) => ToolReply::refused(e),
        }
}

pub(super) async fn revoke_grant(ws: &Arc<Workspace>, call: &ToolCall) -> ToolReply {
        let Some(agent_id) = text(&call.input, "agent_id") else {
            return ToolReply::refused("revoke_grant needs an agent_id");
        };
        let Some(name) = text(&call.input, "name") else {
            return ToolReply::refused("revoke_grant needs the name to remove");
        };
        let skill = text(&call.input, "kind").as_deref() != Some("mcp");

        let mut crew = ws.agents().await;
        let slot = match slot_of(&crew, &agent_id) {
            Ok(i) => &mut crew[i],
            Err(refusal) => return refusal,
        };
        let before = if skill {
            slot.granted_skills.len()
        } else {
            slot.mcp_servers.len()
        };
        if skill {
            slot.granted_skills.retain(|g| g.name != name);
        } else {
            slot.mcp_servers.retain(|g| g.name != name);
        }
        let after = if skill {
            slot.granted_skills.len()
        } else {
            slot.mcp_servers.len()
        };
        if before == after {
            return ToolReply::refused(format!("{} has nothing called {name}", slot.name));
        }
        let who = slot.name.clone();
        match ws.set_agents(crew).await {
            Ok(_) => ToolReply::ok(format!("{name} is no longer available to {who}")),
            Err(e) => ToolReply::refused(e),
        }
}
