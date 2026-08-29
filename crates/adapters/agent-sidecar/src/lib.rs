use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;

use harness_ports::{
    ApprovalRequest, Grants, Inbox, McpTransport, QueuedMessage, RunEvent, RunOutcome, RunSpec,
    ToolCall,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct SidecarAgent {
    program: String,
    script: PathBuf,
    /// What this port's runs may load beyond Relay's own wiring.
    ///
    /// It hangs off the port, not off the `RunSpec`, and that is the shape and
    /// not an accident: grants belong to one agent, and an agent port built for
    /// one conversation serves exactly one agent. A port shared by every run —
    /// which is what a project engine holds — cannot carry them, and that is
    /// the boundary recorded in `DEBT.md`.
    grants: Grants,
}

impl SidecarAgent {
    pub fn new(program: impl Into<String>, script: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            script: script.into(),
            grants: Grants::default(),
        }
    }

    pub fn with_grants(mut self, grants: Grants) -> Self {
        self.grants = grants;
        self
    }
}

/// The MCP servers, in the shape the Agent SDK takes them.
///
/// `harness` is never emitted here: a granted server by that name would shadow
/// Relay's own in-process tools, and `crates/app/src/grants.rs` refuses the
/// name before it can ever be stored. This is the second lock on the same door.
fn mcp_json(grants: &Grants) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for server in &grants.mcp_servers {
        if server.name == "harness" {
            continue;
        }
        let config = match &server.transport {
            McpTransport::Stdio { command, args } => serde_json::json!({
                "type": "stdio",
                "command": command,
                "args": args,
                "env": server.env,
            }),
            McpTransport::Http { url } => serde_json::json!({ "type": "http", "url": url }),
            McpTransport::Sse { url } => serde_json::json!({ "type": "sse", "url": url }),
        };
        map.insert(server.name.clone(), config);
    }
    serde_json::Value::Object(map)
}

/// Why a `done` event is really a failure, if it is. The sidecar puts the
/// error text here because the SDK reports an error result on the same message
/// shape as a success — see `sidecar/index.mjs`.
fn done_error(event: &serde_json::Value) -> Option<String> {
    event
        .get("error")
        .and_then(|e| e.as_str())
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(str::to_string)
}

fn summarize(input: &serde_json::Value) -> String {
    match input {
        serde_json::Value::Object(map) => map
            .iter()
            .take(3)
            .map(|(k, v)| {
                let rendered: String = match v {
                    serde_json::Value::String(s) => s.chars().take(80).collect::<String>(),
                    other => other.to_string().chars().take(80).collect(),
                };
                format!("{k}: {rendered}")
            })
            .collect::<Vec<_>>()
            .join(" | "),
        other => other.to_string().chars().take(120).collect(),
    }
}

/// The next thing the operator said, or never — so the run's select loop can
/// carry a branch for an inbox it may not have.
async fn next_queued(inbox: Option<Inbox>) -> Option<QueuedMessage> {
    match inbox {
        Some(inbox) => inbox.next().await,
        None => std::future::pending().await,
    }
}

struct LineSink<'a> {
    stdin: &'a mut tokio::process::ChildStdin,
}

impl LineSink<'_> {
    async fn send(&mut self, value: serde_json::Value) -> Result<(), String> {
        let line = format!("{value}\n");
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("sidecar stdin write failed: {e}"))
    }
}

async fn drive(
    child: &mut Child,
    spec: RunSpec,
    grants: Grants,
    tx: mpsc::Sender<RunEvent>,
    cancel: CancellationToken,
) -> Result<RunOutcome, String> {
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "sidecar stdin unavailable".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "sidecar stdout unavailable".to_string())?;

    let id = uuid::Uuid::new_v4().to_string();
    let run_msg = serde_json::json!({
        "type": "run",
        "id": id,
        "spec": {
            "prompt": spec.prompt,
            "cwd": spec.cwd.to_string_lossy(),
            "permission_mode": spec.permission_mode,
            "allowed_tools": spec.allowed_tools,
            "model": spec.model,
            "max_budget_usd": spec.max_budget_usd,
            "resume_session": spec.resume_session,
            // Whether this run may act on Relay itself.
            "harness_tools": spec.tools.is_some() && !spec.report_work,
            // Worker runs carry exactly one harness tool: report_work.
            "report_work": spec.report_work,
            "thinking_tokens": spec.thinking_tokens,
            // Whether this run may spawn subagents of its own.
            "subagents": spec.subagents,
            // The explicit list on top of the isolation: a directory of skills
            // that belongs to this agent alone, and the MCP servers it was
            // granted. Absent for an agent that was granted nothing, which is
            // every agent until an operator approves one.
            "skills_dir": grants.skills_dir.as_ref().map(|p| p.to_string_lossy()),
            "mcp_servers": mcp_json(&grants),
            // One of the engine's own style names. Relay ships none, so this
            // is passed through unchecked — an unknown name is the engine's to
            // reject, and inventing a whitelist here would date the moment it
            // adds one.
            "output_style": spec.output_style,
            // Chosen per message, so it rides on the run and never on the
            // profile.
            "effort": spec.effort,
        }
    });
    LineSink { stdin: &mut stdin }.send(run_msg).await?;

    let mut lines = BufReader::new(stdout).lines();
    let mut session_id: Option<String> = None;
    let mut cost_usd: Option<f64> = None;
    let mut turns: Option<u32> = None;
    let mut saw_done = false;
    // Two handles on the same inbox on purpose. `reading` is emptied once the
    // inbox says it has closed, so the select loop stops asking; `inbox` stays
    // whole, because an acknowledgement can still arrive after the last read.
    let inbox = spec.inbox.clone();
    let mut reading = inbox.clone();

    let outcome = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = LineSink { stdin: &mut stdin }
                    .send(serde_json::json!({ "type": "cancel", "id": id }))
                    .await;
                let _ = child.kill().await;
                break Ok(RunOutcome::Cancelled);
            }
            queued = next_queued(reading.clone()), if reading.is_some() => {
                match queued {
                    Some(message) => {
                        // Straight down the same pipe the run travels on. The
                        // sidecar hands it to the SDK's input stream, and the
                        // model reads it without the turn having to end.
                        let line = serde_json::json!({
                            "type": "message",
                            "id": id,
                            "message_id": message.id,
                            "text": message.text,
                        });
                        if (LineSink { stdin: &mut stdin }).send(line).await.is_err() {
                            break Err("sidecar stdin closed while queueing a message".to_string());
                        }
                    }
                    // The inbox shut before the run did. Stop asking, rather
                    // than spinning on a branch that answers instantly.
                    None => reading = None,
                }
            }
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(l)) => l,
                    Ok(None) => {
                        if saw_done {
                            break Ok(RunOutcome::Completed {
                                session_id: session_id.clone(),
                                cost_usd,
                                turns,
                            });
                        }
                        break Err("sidecar closed stdout before result".to_string());
                    }
                    Err(e) => break Err(format!("read sidecar stdout: {e}")),
                };
                let msg: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match msg.get("type").and_then(|t| t.as_str()) {
                    Some("event") => {
                        let ev = msg.get("event").cloned().unwrap_or_default();
                        let kind = ev.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                        match kind {
                            "started" => {
                                session_id = ev
                                    .get("session_id")
                                    .and_then(|s| s.as_str())
                                    .map(String::from);
                                let _ = tx.send(RunEvent::Started {
                                    session_id: session_id.clone().unwrap_or_default(),
                                })
                                .await;
                            }
                            "delta" | "thinking" => {
                                let text = ev
                                    .get("text")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !text.is_empty() {
                                    let _ = tx
                                        .send(if kind == "delta" {
                                            RunEvent::Delta { text }
                                        } else {
                                            RunEvent::Thinking { text }
                                        })
                                        .await;
                                }
                            }
                            "text" => {
                                let text = ev
                                    .get("text")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !text.trim().is_empty() {
                                    let _ = tx.send(RunEvent::Text { text }).await;
                                }
                            }
                            "turns" => {
                                let count = ev.get("count").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
                                let _ = tx.send(RunEvent::Turns { count }).await;
                            }
                            "message_read" => {
                                // The one thing that can honestly retire a
                                // "not read yet" mark: the sidecar wrote it to
                                // the SDK, so the run has it.
                                if let Some(queue_id) =
                                    ev.get("message_id").and_then(|m| m.as_str())
                                {
                                    if let Some(inbox) = &inbox {
                                        inbox.mark_read(queue_id);
                                    }
                                    let _ = tx
                                        .send(RunEvent::UserRead {
                                            queue_id: queue_id.to_string(),
                                        })
                                        .await;
                                }
                            }
                            "commands" => {
                                // Shaped in the sidecar, where the SDK's own
                                // field names are known; anything malformed is
                                // dropped rather than turned into a command
                                // the composer would offer and nothing serves.
                                if let Some(list) = ev.get("commands") {
                                    if let Ok(commands) = serde_json::from_value::<
                                        Vec<harness_ports::SlashCommand>,
                                    >(list.clone())
                                    {
                                        let _ = tx.send(RunEvent::Commands { commands }).await;
                                    }
                                }
                            }
                            "local_output" => {
                                let text = ev
                                    .get("text")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !text.trim().is_empty() {
                                    let _ = tx.send(RunEvent::LocalOutput { text }).await;
                                }
                            }
                            "usage" => {
                                let tokens = |name: &str| {
                                    ev.get(name).and_then(|v| v.as_u64()).unwrap_or(0)
                                };
                                let _ = tx
                                    .send(RunEvent::Usage {
                                        input_tokens: tokens("input_tokens"),
                                        output_tokens: tokens("output_tokens"),
                                        cache_read_tokens: tokens("cache_read_tokens"),
                                        cache_creation_tokens: tokens("cache_creation_tokens"),
                                        model: ev
                                            .get("model")
                                            .and_then(|m| m.as_str())
                                            .map(str::to_string),
                                    })
                                    .await;
                            }
                            "tool_use" => {
                                let tool = ev
                                    .get("tool")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("tool")
                                    .to_string();
                                let summary = ev
                                    .get("summary")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let tool_use_id = ev
                                    .get("tool_use_id")
                                    .and_then(|t| t.as_str())
                                    .map(str::to_string);
                                let parent_tool_use_id = ev
                                    .get("parent_tool_use_id")
                                    .and_then(|t| t.as_str())
                                    .map(str::to_string);
                                let _ =
                                    tx.send(RunEvent::ToolUse { tool, summary, tool_use_id, parent_tool_use_id })
                                        .await;
                            }
                            "tool_result" => {
                                let tool_use_id = ev
                                    .get("tool_use_id")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let ok = ev.get("ok").and_then(|o| o.as_bool()).unwrap_or(true);
                                let summary = ev
                                    .get("summary")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let detail = ev
                                    .get("detail")
                                    .and_then(|d| d.as_str())
                                    .map(str::to_string);
                                let _ = tx
                                    .send(RunEvent::ToolResult { tool_use_id, ok, summary, detail })
                                    .await;
                            }
                            "done" => {
                                saw_done = true;
                                cost_usd = ev.get("cost_usd").and_then(|c| c.as_f64());
                                turns = ev
                                    .get("turns")
                                    .and_then(|t| t.as_u64())
                                    .map(|t| t as u32);
                                if let Some(sid) = ev.get("session_id").and_then(|s| s.as_str()) {
                                    session_id = Some(sid.to_string());
                                }
                                let failure = done_error(&ev);
                                let _ = tx
                                    .send(RunEvent::Done {
                                        session_id: session_id.clone(),
                                        cost_usd,
                                        turns,
                                        result: ev
                                            .get("result")
                                            .and_then(|r| r.as_str())
                                            .map(String::from),
                                        error: failure.clone(),
                                    })
                                    .await;
                                // The result is the end of the run. The sidecar
                                // keeps its stdin open for another command, so
                                // waiting for stdout to close would hang here.
                                let _ = child.kill().await;
                                // An error result must not pass for a finished
                                // run: nothing downstream would know the
                                // difference, and a card would be committed and
                                // reviewed on an answer that never came.
                                break match failure {
                                    Some(message) => Ok(RunOutcome::Failed {
                                        message,
                                        cost_usd,
                                        turns,
                                    }),
                                    None => Ok(RunOutcome::Completed {
                                        session_id: session_id.clone(),
                                        cost_usd,
                                        turns,
                                    }),
                                };
                            }
                            "failed" => {
                                let message = ev
                                    .get("message")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("unknown sidecar failure")
                                    .to_string();
                                break Ok(RunOutcome::Failed {
                                        message,
                                        cost_usd: None,
                                        turns: None,
                                    });
                            }
                            _ => {}
                        }
                    }
                    Some("tool_request") => {
                        // A Relay tool: the shell carries it out and we hand
                        // the answer straight back to the model.
                        let request_id = msg
                            .get("request_id")
                            .and_then(|r| r.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let name = msg
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let input = msg.get("input").cloned().unwrap_or_default();

                        let reply = match &spec.tools {
                            Some(run_tool) => {
                                run_tool(ToolCall {
                                    name: name.clone(),
                                    input,
                                })
                                .await
                            }
                            None => harness_ports::ToolReply::refused(
                                "this run cannot act on Relay",
                            ),
                        };

                        let _ = tx
                            .send(RunEvent::ToolUse {
                                tool: format!("harness:{name}"),
                                summary: reply.text.chars().take(160).collect(),
                                tool_use_id: None,
                                parent_tool_use_id: None,
                            })
                            .await;

                        let response = serde_json::json!({
                            "type": "tool_response",
                            "request_id": request_id,
                            "ok": reply.ok,
                            "text": reply.text,
                        });
                        if (LineSink { stdin: &mut stdin })
                            .send(response)
                            .await
                            .is_err()
                        {
                            break Err("sidecar stdin closed during a tool call".to_string());
                        }
                    }
                    Some("approval_request") => {
                        let request_id = msg
                            .get("request_id")
                            .and_then(|r| r.as_str())
                            .unwrap_or_default()
                            .to_string();
                        let tool = msg
                            .get("tool")
                            .and_then(|t| t.as_str())
                            .unwrap_or("tool")
                            .to_string();
                        let input = msg.get("input").cloned().unwrap_or_default();
                        // The sidecar knows the tool by name and can say what
                        // the call would actually do; the generic key-value
                        // rendering below is the fallback for anything it has
                        // no line for. A sheet that says "the Director wants to
                        // install something" is not a sheet anyone can answer.
                        let summary = msg
                            .get("summary")
                            .and_then(|s| s.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| summarize(&input));

                        let _ = tx
                            .send(RunEvent::ApprovalRequested {
                                request_id: request_id.clone(),
                                tool: tool.clone(),
                                summary: summary.clone(),
                            })
                            .await;

                        let allowed = match &spec.approver {
                            Some(approve) => {
                                approve(ApprovalRequest {
                                    request_id: request_id.clone(),
                                    tool: tool.clone(),
                                    summary: summary.clone(),
                                    input: input.clone(),
                                })
                                .await
                            }
                            // Nobody is listening, so the safe answer is no.
                            None => false,
                        };

                        let _ = tx
                            .send(RunEvent::ApprovalAnswered {
                                request_id: request_id.clone(),
                                allow: allowed,
                            })
                            .await;

                        let response = serde_json::json!({
                            "type": "approval_response",
                            "request_id": request_id,
                            "allow": allowed,
                        });
                        if (LineSink { stdin: &mut stdin })
                            .send(response)
                            .await
                            .is_err()
                        {
                            break Err("sidecar stdin closed during approval".to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    };

    if !cancel.is_cancelled() && !saw_done && outcome.is_ok() {
        drop(stdin);
        let status = child.wait().await.map_err(|e| format!("wait sidecar: {e}"))?;
        if !status.success() {
            return Ok(RunOutcome::Failed {
        message: format!("sidecar exited with {status}"),
        cost_usd: None,
        turns: None,
    });
        }
    }

    outcome
}

impl harness_ports::AgentPort for SidecarAgent {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<RunOutcome, String>> + Send>> {
        let program = self.program.clone();
        let script = self.script.clone();
        // O run manda sobre o porto. Uma conversa constrói o seu porto por
        // perfil e não traz nada no spec; um run de cartão partilha o porto de
        // todos, portanto é o spec que diz de quem ele é. Vazio quer dizer
        // "usa as do porto", que é o que mantém as conversas como estavam.
        let grants = if spec.grants.is_empty() {
            self.grants.clone()
        } else {
            spec.grants.clone()
        };
        let provider = spec.provider.clone();
        Box::pin(async move {
            let mut cmd = Command::new(&program);
            // Set per run, not per process: two agents in the same Relay can be
            // pointed at different endpoints, and one of them being local must
            // not decide where the other one runs.
            if let Some(provider) = &provider {
                for (key, value) in provider.env() {
                    cmd.env(key, value);
                }
            }
            cmd.arg(&script)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                // One sidecar per run, and it outlives its usefulness the
                // moment the result arrives; never leave it running.
                .kill_on_drop(true);
            #[cfg(windows)]
            {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("failed to spawn sidecar ({program} {}): {e}", script.display()))?;
            drive(&mut child, spec, grants, tx, cancel).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{done_error, mcp_json};
    use harness_ports::{Grants, McpGrant, McpTransport};
    use serde_json::json;

    #[test]
    fn granted_servers_travel_and_harness_is_never_one_of_them() {
        let mut env = std::collections::BTreeMap::new();
        env.insert("FIGMA_TOKEN".to_string(), "abc".to_string());
        let grants = Grants {
            skills_dir: Some(std::path::PathBuf::from("/tmp/relay/skills/designer")),
            mcp_servers: vec![
                McpGrant {
                    name: "figma".into(),
                    transport: McpTransport::Stdio {
                        command: "npx".into(),
                        args: vec!["-y".into(), "figma-mcp".into()],
                    },
                    env,
                    ..Default::default()
                },
                McpGrant {
                    name: "docs".into(),
                    transport: McpTransport::Http { url: "https://example.invalid/mcp".into() },
                    ..Default::default()
                },
                // Stored by a hand-edited agents.json, past the app-layer
                // refusal. The wire must not carry it either.
                McpGrant {
                    name: "harness".into(),
                    transport: McpTransport::Stdio { command: "node".into(), args: vec![] },
                    ..Default::default()
                },
            ],
        };
        let wire = mcp_json(&grants);
        let map = wire.as_object().unwrap();
        assert_eq!(map.len(), 2, "harness was dropped");
        assert!(!map.contains_key("harness"));
        assert_eq!(map["figma"]["command"], json!("npx"));
        assert_eq!(map["figma"]["args"], json!(["-y", "figma-mcp"]));
        assert_eq!(map["figma"]["env"]["FIGMA_TOKEN"], json!("abc"));
        assert_eq!(map["docs"]["type"], json!("http"));
    }

    #[test]
    fn an_agent_granted_nothing_sends_nothing() {
        let wire = mcp_json(&Grants::default());
        assert_eq!(wire, json!({}));
        assert!(Grants::default().is_empty());
    }

    #[test]
    fn a_done_event_carries_its_failure_or_nothing() {
        assert_eq!(done_error(&json!({ "kind": "done" })), None);
        assert_eq!(done_error(&json!({ "kind": "done", "error": null })), None);
        assert_eq!(done_error(&json!({ "kind": "done", "error": "   " })), None);
        assert_eq!(
            done_error(&json!({
                "kind": "done",
                "error": "No conversation found with session ID: 0000",
            }))
            .as_deref(),
            Some("No conversation found with session ID: 0000"),
        );
    }
}
