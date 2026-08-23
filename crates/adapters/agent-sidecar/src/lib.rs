use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;

use harness_ports::{ApprovalRequest, RunEvent, RunOutcome, RunSpec, ToolCall};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct SidecarAgent {
    program: String,
    script: PathBuf,
}

impl SidecarAgent {
    pub fn new(program: impl Into<String>, script: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            script: script.into(),
        }
    }
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
            // Whether this run may act on Harness itself.
            "harness_tools": spec.tools.is_some(),
            "thinking_tokens": spec.thinking_tokens,
        }
    });
    LineSink { stdin: &mut stdin }.send(run_msg).await?;

    let mut lines = BufReader::new(stdout).lines();
    let mut session_id: Option<String> = None;
    let mut cost_usd: Option<f64> = None;
    let mut turns: Option<u32> = None;
    let mut saw_done = false;

    let outcome = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = LineSink { stdin: &mut stdin }
                    .send(serde_json::json!({ "type": "cancel", "id": id }))
                    .await;
                let _ = child.kill().await;
                break Ok(RunOutcome::Cancelled);
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
                                let _ = tx.send(RunEvent::ToolUse { tool, summary }).await;
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
                                let _ = tx
                                    .send(RunEvent::Done {
                                        session_id: session_id.clone(),
                                        cost_usd,
                                        turns,
                                        result: ev
                                            .get("result")
                                            .and_then(|r| r.as_str())
                                            .map(String::from),
                                    })
                                    .await;
                                // The result is the end of the run. The sidecar
                                // keeps its stdin open for another command, so
                                // waiting for stdout to close would hang here.
                                let _ = child.kill().await;
                                break Ok(RunOutcome::Completed {
                                    session_id: session_id.clone(),
                                    cost_usd,
                                    turns,
                                });
                            }
                            "failed" => {
                                let message = ev
                                    .get("message")
                                    .and_then(|m| m.as_str())
                                    .unwrap_or("unknown sidecar failure")
                                    .to_string();
                                break Ok(RunOutcome::Failed(message));
                            }
                            _ => {}
                        }
                    }
                    Some("tool_request") => {
                        // A Harness tool: the shell carries it out and we hand
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
                                "this run cannot act on Harness",
                            ),
                        };

                        let _ = tx
                            .send(RunEvent::ToolUse {
                                tool: format!("harness:{name}"),
                                summary: reply.text.chars().take(160).collect(),
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
                        let summary = summarize(&input);

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
            return Ok(RunOutcome::Failed(format!("sidecar exited with {status}")));
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
        Box::pin(async move {
            let mut cmd = Command::new(&program);
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
            drive(&mut child, spec, tx, cancel).await
        })
    }
}
