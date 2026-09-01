use std::pin::Pin;

use harness_ports::{AgentPort, RunEvent, RunOutcome, RunSpec};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub struct ClaudeCliAgent {
    program: String,
}

impl ClaudeCliAgent {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

fn summarize_input(input: &Value) -> String {
    match input {
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .take(3)
                .map(|(k, v)| {
                    let rendered = match v {
                        Value::String(s) => truncate(s, 80),
                        other => truncate(&other.to_string(), 80),
                    };
                    format!("{k}: {rendered}")
                })
                .collect();
            parts.join(" | ")
        }
        other => truncate(&other.to_string(), 120),
    }
}

async fn emit(tx: &mpsc::Sender<RunEvent>, ev: RunEvent) {
    let _ = tx.send(ev).await;
}

async fn pump_lines(
    child: &mut Child,
    tx: mpsc::Sender<RunEvent>,
    cancel: CancellationToken,
    // Is a dollar figure from this run a price at all? See
    // `RunSpec::prices_in_dollars`.
    priced: bool,
) -> Result<Option<(Option<String>, Option<f64>, Option<String>, Option<String>)>, String> {
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "claude stdout unavailable".to_string())?;
    let mut lines = BufReader::new(&mut stdout).lines();
    let mut session_id: Option<String> = None;
    let mut cost_usd: Option<f64> = None;
    let mut turns: Option<u32> = None;
    let mut failure: Option<String> = None;
    let mut done_seen = false;
    let mut final_result: Option<String> = None;
    // Assistant message ids already accounted for. See the `assistant` arm.
    let mut counted_messages: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Ok(None);
            }
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(l)) => l,
                    Ok(None) => break,
                    Err(e) => return Err(format!("read stdout: {e}")),
                };
                let v: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("system") => {
                        if v.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                            if let Some(id) = v.get("session_id").and_then(|s| s.as_str()) {
                                session_id = Some(id.to_string());
                                emit(&tx, RunEvent::Started { session_id: id.to_string() }).await;
                            }
                        }
                    }
                    Some("assistant") => {
                        // The same per-turn usage the sidecar reports, so a
                        // conversation run through the command line accounts
                        // for itself too — and with the same guard, because the
                        // CLI repeats a message once per content block and each
                        // repeat carries the whole message's usage. Counted as
                        // they arrive, a turn with three tool calls is billed
                        // three times.
                        let message_id = v
                            .pointer("/message/id")
                            .and_then(|m| m.as_str())
                            .map(str::to_string);
                        let first_sight = match &message_id {
                            // No id is not "already seen": an output that does
                            // not number its messages would lose every turn.
                            None => true,
                            Some(id) => counted_messages.insert(id.clone()),
                        };
                        if let (true, Some(usage)) = (first_sight, v.pointer("/message/usage")) {
                            let tokens = |name: &str| {
                                usage.get(name).and_then(|t| t.as_u64()).unwrap_or(0)
                            };
                            emit(
                                &tx,
                                RunEvent::Usage {
                                    input_tokens: tokens("input_tokens"),
                                    output_tokens: tokens("output_tokens"),
                                    cache_read_tokens: tokens("cache_read_input_tokens"),
                                    cache_creation_tokens: tokens("cache_creation_input_tokens"),
                                    model: v
                                        .pointer("/message/model")
                                        .and_then(|m| m.as_str())
                                        .map(str::to_string),
                                    subagent: v
                                        .get("parent_tool_use_id")
                                        .is_some_and(|p| !p.is_null()),
                                },
                            )
                            .await;
                        }
                        let content = v
                            .pointer("/message/content")
                            .and_then(|c| c.as_array())
                            .cloned()
                            .unwrap_or_default();
                        for item in content {
                            match item.get("type").and_then(|t| t.as_str()) {
                                Some("text") => {
                                    let text = item
                                        .get("text")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    if !text.trim().is_empty() {
                                        emit(&tx, RunEvent::Text { text, parent_tool_use_id: None }).await;
                                    }
                                }
                                Some("tool_use") => {
                                    let tool = item
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("tool")
                                        .to_string();
                                    let summary = item
                                        .get("input")
                                        .map(summarize_input)
                                        .unwrap_or_default();
                                    emit(&tx, RunEvent::ToolUse { tool, summary, tool_use_id: None, parent_tool_use_id: None, added: None, removed: None }).await;
                                }
                                _ => {}
                            }
                        }
                    }
                    Some("result") => {
                        done_seen = true;
                        // Só quando é um preço. Ver `RunSpec::prices_in_dollars`:
                        // o SDK factura contra as tabelas da Anthropic seja
                        // qual for o endpoint a que o run foi.
                        if let Some(c) = v
                            .get("total_cost_usd")
                            .and_then(|c| c.as_f64())
                            .filter(|_| priced)
                        {
                            cost_usd = Some(c);
                        }
                        if let Some(t) = v.get("num_turns").and_then(|t| t.as_u64()) {
                            turns = Some(t as u32);
                        }
                        if v.get("result").and_then(|r| r.as_str()).is_some() {
                            final_result =
                                Some(v.get("result").unwrap().as_str().unwrap().to_string());
                        }
                        let subtype = v.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
                        if subtype != "success" && failure.is_none() {
                            failure = Some(
                                v.get("result")
                                    .or_else(|| v.get("error"))
                                    .and_then(|r| r.as_str())
                                    .unwrap_or(subtype)
                                    .to_string(),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let status = child.wait().await.map_err(|e| format!("wait claude: {e}"))?;
    if !done_seen && !status.success() {
        failure = Some(format!("claude exited with {status}"));
    }

    if let Some(msg) = failure {
        return Ok(Some((session_id, cost_usd, final_result, Some(msg))));
    }
    emit(
        &tx,
        RunEvent::Done {
            session_id: session_id.clone(),
            cost_usd,
            turns,
            result: final_result.clone(),
            // The command line adapter reports a failure as a non-zero exit,
            // not as an error result, so there is nothing to carry here.
            error: None,
        },
    )
    .await;
    Ok(Some((session_id, cost_usd, final_result, None)))
}

impl AgentPort for ClaudeCliAgent {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<RunOutcome, String>> + Send>> {
        let program = self.program.clone();
        Box::pin(async move {
            let mut cmd = tokio::process::Command::new(&program);
            // The command line reads the same variables the SDK does, so the
            // fallback adapter reaches the same endpoints.
            if let Some(provider) = &spec.provider {
                for (key, value) in provider.env() {
                    cmd.env(key, value);
                }
            }
            cmd.arg("-p")
                .arg(&spec.prompt)
                .arg("--output-format")
                .arg("stream-json")
                .arg("--verbose")
                .arg(
                    "--permission-mode",
                )
                .arg(spec.permission_mode.as_deref().unwrap_or("acceptEdits"));
            if let Some(tools) = &spec.allowed_tools {
                if !tools.is_empty() {
                    cmd.arg("--allowedTools").args(tools);
                }
            }
            if let Some(model) = &spec.model {
                cmd.arg("--model").arg(model);
            }
            if let Some(session) = &spec.resume_session {
                cmd.arg("--resume").arg(session);
            }
            if let Some(budget) = spec.max_budget_usd {
                cmd.arg("--max-budget-usd").arg(budget.to_string());
            }
            cmd.current_dir(&spec.cwd);
            #[cfg(windows)]
            {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            cmd.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());

            let mut child = cmd
                .spawn()
                .map_err(|e| format!("failed to spawn '{program}': {e}"))?;

            match pump_lines(&mut child, tx, cancel, spec.prices_in_dollars()).await? {
                None => Ok(RunOutcome::Cancelled),
                Some((_sid, cost, _result, None)) => Ok(RunOutcome::completed(_sid, cost)),
                Some((_sid, _cost, _result, Some(msg))) => Ok(RunOutcome::Failed {
                    message: msg,
                    cost_usd: _cost,
                    turns: None,
                }),
            }
        })
    }
}
