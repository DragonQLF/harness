use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;

use harness_ports::{
    ApprovalRequest, Grants, Inbox, McpTransport, QueuedMessage, RunEvent, RunOutcome, RunSpec,
    ToolCall,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Command;
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
    /// Onde vivem os sockets dos runs destacados. Sem isto o porto continua a
    /// falar por canos, que é o que os testes e uma Relay antiga fazem.
    runs_dir: Option<PathBuf>,
}

impl SidecarAgent {
    pub fn new(program: impl Into<String>, script: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            script: script.into(),
            grants: Grants::default(),
            runs_dir: None,
        }
    }

    pub fn with_runs_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.runs_dir = Some(dir.into());
        self
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

/// Por onde se fala com o sidecar. Um cano ou um socket — o protocolo é o
/// mesmo, e é isso que deixa todo o `drive` de baixo intacto.
type Writer = Box<dyn AsyncWrite + Unpin + Send>;
type Reader = Box<dyn AsyncRead + Unpin + Send>;

struct LineSink<'a> {
    out: &'a mut Writer,
}

impl LineSink<'_> {
    async fn send(&mut self, value: serde_json::Value) -> Result<(), String> {
        let line = format!("{value}\n");
        self.out
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("sidecar write failed: {e}"))
    }
}

/// Conduz um run que já está do outro lado do fio, seja ele qual for.
///
/// `fresh` diz se este é o princípio ou uma reatação. No princípio manda-se o
/// `run`; numa reatação não — o trabalho já vai a meio, e mandá-lo outra vez
/// pedia ao modelo que refizesse o que já fez.
async fn drive(
    mut stdin: Writer,
    mut lines: tokio::io::Lines<BufReader<Reader>>,
    fresh: bool,
    progress: Option<PathBuf>,
    spec: RunSpec,
    grants: Grants,
    tx: mpsc::Sender<RunEvent>,
    cancel: CancellationToken,
) -> Result<RunOutcome, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let priced = spec.prices_in_dollars();
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
    if fresh {
        LineSink { out: &mut stdin }.send(run_msg).await?;
    }

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
                let _ = LineSink { out: &mut stdin }
                    .send(serde_json::json!({ "type": "cancel", "id": id }))
                    .await;
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
                        if (LineSink { out: &mut stdin }).send(line).await.is_err() {
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
                        // Por onde vamos, para uma Relay futura poder pedir só
                        // o que lhe falta. Guardado aqui e não do lado da Relay
                        // porque é aqui que o número existe — atravessá-lo até
                        // lá obrigava a mudar a forma de todos os eventos, e o
                        // que se ganhava com isso era exactamente isto.
                        //
                        // Só o que fica escrito na transcrição conta. Os
                        // efémeros — os tokens — não se guardam, e apontar para
                        // um deles fazia a reatação saltar o texto assente que
                        // veio antes.
                        if let (Some(mark), Some(seq)) =
                            (&progress, msg.get("seq").and_then(|s| s.as_u64()))
                        {
                            if !EPHEMERAL.contains(&kind) {
                                let _ = std::fs::write(mark, seq.to_string());
                            }
                        }
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
                            "background_tasks" => {
                                // Nível: o conjunto inteiro de cada vez. Uma
                                // carga malformada não passa por "já não há
                                // nada a correr" — cai fora e o ecrã fica com
                                // o último conjunto bom, que é o menos
                                // enganador dos dois erros.
                                if let Some(list) = ev.get("tasks") {
                                    if let Ok(tasks) = serde_json::from_value::<
                                        Vec<harness_ports::BackgroundTask>,
                                    >(list.clone())
                                    {
                                        let _ =
                                            tx.send(RunEvent::BackgroundTasks { tasks }).await;
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
                                    tx.send(RunEvent::ToolUse {
                                        tool,
                                        summary,
                                        tool_use_id,
                                        parent_tool_use_id,
                                        added: ev.get("added").and_then(|v| v.as_u64()).map(|n| n as u32),
                                        removed: ev.get("removed").and_then(|v| v.as_u64()).map(|n| n as u32),
                                    })
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
                                // Um número só é um preço quando o run foi
                                // mesmo facturado pela Anthropic. Ver
                                // `RunSpec::prices_in_dollars`: o SDK não sabe
                                // para onde o `ANTHROPIC_BASE_URL` o mandou, e
                                // factura na mesma contra as tabelas dela.
                                cost_usd = ev
                                    .get("cost_usd")
                                    .and_then(|c| c.as_f64())
                                    .filter(|_| priced);
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
                                // O `done` é o fim do run. Quem desliga o
                                // sidecar é o `run` lá em baixo, que é quem
                                // sabe se ele é nosso ou se ficou destacado a
                                // servir outra coisa.
                                //
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
                                added: None,
                                removed: None,
                            })
                            .await;

                        let response = serde_json::json!({
                            "type": "tool_response",
                            "request_id": request_id,
                            "ok": reply.ok,
                            "text": reply.text,
                        });
                        if (LineSink { out: &mut stdin })
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
                        if (LineSink { out: &mut stdin })
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

    // Quem espera pelo processo é o `run`, que é o único que sabe se há
    // processo nosso para esperar: numa reatação o sidecar é de outra Relay que
    // já morreu, e não há `Child` nenhum deste lado.
    let _ = saw_done;
    drop(stdin);
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
        let runs_dir = self.runs_dir.clone();
        // Só em unix. Em Windows não há socket de domínio nesta pilha, e o que
        // aconteceria era pior do que não haver reatação: levantava-se o sidecar
        // com `--serve`, esperava-se por uma porta que nunca abria, e o turno
        // falhava em vez de correr. Lá continua-se pelos canos, como sempre.
        // O nome é um resumo da chave e não a chave: um `sun_path` tem 104
        // bytes e o caminho óbvio passava-os numa conta de macOS qualquer.
        // Ver `harness_ports::sockets`.
        #[cfg(unix)]
        let socket = runs_dir
            .as_ref()
            .zip(spec.run_key.as_ref())
            .map(|(dir, key)| harness_ports::sockets::path_for(dir, key));
        #[cfg(not(unix))]
        let socket: Option<PathBuf> = {
            let _ = &runs_dir;
            None
        };
        #[cfg(unix)]
        let from_seq = spec.from_seq;
        #[cfg(unix)]
        let progress = runs_dir
            .as_ref()
            .zip(spec.run_key.as_ref())
            .map(|(dir, key)| dir.join(format!("{key}.seq")));
        // Em Windows nada disto tem uso: sem socket não há marca por onde
        // retomar, e o `drive` recebe `None`.
        #[cfg(not(unix))]
        let _ = &runs_dir;
        Box::pin(async move {
            // Há trabalho a andar deste lado? Um socket que atende é um run
            // vivo, e a resposta certa a um run vivo é ligar-se a ele — não
            // levantar um segundo que lhe iria disputar a sessão. É a diferença
            // entre um agente que sobrevive a um reinício e um que não.
            //
            // Em Windows o `socket` é sempre `None`, mas isso não chega: o que
            // está aqui dentro tem de *compilar* lá, e o `UnixStream` nem
            // existe nessa plataforma. Daí o `cfg` no bloco e não só no valor.
            #[cfg(unix)]
            if let Some(path) = &socket {
                if let Ok(stream) = tokio::net::UnixStream::connect(path).await {
                    let (read, write) = stream.into_split();
                    let mut out: Writer = Box::new(write);
                    let key = spec.run_key.clone().unwrap_or_default();
                    // Onde a transcrição desta conversa ficou. Sem marca
                    // pede-se só o que vier a seguir, que é a omissão segura;
                    // com marca recupera-se o que se perdeu enquanto a Relay
                    // esteve fora, sem repetir o que já lá está.
                    let resume_at = from_seq.or_else(|| {
                        progress
                            .as_ref()
                            .and_then(|p| std::fs::read_to_string(p).ok())
                            .and_then(|raw| raw.trim().parse::<u64>().ok())
                    });
                    let (lines, running) =
                        handshake(&mut out, Box::new(read), &key, resume_at).await?;
                    // Vivo quer dizer "a meio": manda-se-lhe o `run` outra vez
                    // pedia ao modelo que refizesse o que já fez. Parado quer
                    // dizer que o sidecar sobreviveu ao turno mas não tem
                    // trabalho — nesse caso este é o turno novo dele.
                    if spec.attach_only && !running {
                        // Atendeu, mas não tem trabalho a andar: não há nada a
                        // retomar, e mandar-lhe um turno vazio era inventar uma
                        // conversa que ninguém pediu.
                        return Ok(RunOutcome::Completed {
                            session_id: None,
                            cost_usd: None,
                            turns: None,
                        });
                    }
                    return drive(out, lines, !running, progress, spec, grants, tx, cancel).await;
                }
                // Um socket que não atende é de um processo que já morreu. O
                // ficheiro fica para trás e faria a próxima tentativa bater na
                // mesma porta fechada. A marca vai com ele: os números do
                // sidecar seguinte começam do zero, e uma marca velha só podia
                // apontar para um sítio que já não quer dizer nada.
                let _ = std::fs::remove_file(path);
                if let Some(mark) = &progress {
                    let _ = std::fs::remove_file(mark);
                }
            }

            if spec.attach_only {
                // Ninguém atendeu: não havia turno nenhum a continuar. Isto é o
                // caso normal de um arranque, e não é um erro.
                return Ok(RunOutcome::Completed {
                    session_id: None,
                    cost_usd: None,
                    turns: None,
                });
            }

            let mut cmd = Command::new(&program);
            // Set per run, not per process: two agents in the same Relay can be
            // pointed at different endpoints, and one of them being local must
            // not decide where the other one runs.
            if let Some(provider) = &provider {
                for (key, value) in provider.env() {
                    cmd.env(key, value);
                }
            }
            cmd.arg(&script);
            if let Some(path) = &socket {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                    // O recuo do `sockets::path_for` vai parar a `/tmp`, que é
                    // de toda a gente. Quem guarda o socket é a conferência da
                    // chave ao ligar (#111), mas uma pasta que só o dono abre
                    // é a diferença entre uma conferência e uma porta.
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(
                            dir,
                            std::fs::Permissions::from_mode(0o700),
                        );
                    }
                }
                cmd.arg("--serve").arg(path);
                if let Some(key) = &spec.run_key {
                    cmd.arg("--key").arg(key);
                }
            }
            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            // Destacado, quando serve num socket: é esse o ponto. Um sidecar
            // que morre com a Relay leva o turno com ele, e foi isso que fez um
            // reinício custar o trabalho de uma tarde. Sem socket continua
            // preso à Relay como sempre esteve.
            if socket.is_none() {
                cmd.kill_on_drop(true);
            }
            // O sidecar lidera um grupo só dele, e o CLI que ele levanta
            // herda-o. Sem isto o grupo dele é o da própria Relay — matá-lo
            // pelo grupo matava a aplicação.
            #[cfg(unix)]
            cmd.process_group(0);
            #[cfg(windows)]
            {
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let mut child = cmd
                .spawn()
                .map_err(|e| format!("failed to spawn sidecar ({program} {}): {e}", script.display()))?;
            // Guardado agora: depois do `kill` o handle já não sabe o pid, e é
            // o pid que dá o grupo.
            #[cfg(unix)]
            let group = child.id();

            #[cfg(unix)]
            if let Some(path) = &socket {
                let stream = await_socket(path).await?;
                let (read, write) = stream.into_split();
                let mut out: Writer = Box::new(write);
                let key = spec.run_key.clone().unwrap_or_default();
                // Run acabado de nascer: não há atraso possível.
                let (lines, _) = handshake(&mut out, Box::new(read), &key, Some(0)).await?;
                let outcome = drive(out, lines, true, progress, spec, grants, tx, cancel).await;
                // Destacado de propósito: não se mata. Ou acabou — e ele
                // desliga-se sozinho — ou a Relay é que se foi, e o que fica de
                // pé é precisamente o que uma Relay nova vai reencontrar.
                let _ = &mut child;
                return outcome;
            }

            let stdin = child.stdin.take().ok_or("sidecar stdin unavailable")?;
            let stdout = child.stdout.take().ok_or("sidecar stdout unavailable")?;
            let outcome = drive(
                Box::new(stdin),
                BufReader::new(Box::new(stdout) as Reader).lines(),
                true,
                None,
                spec,
                grants,
                tx,
                cancel,
            )
            .await;

            // Sem socket, nada deste run lhe sobrevive. O `child.kill()` manda
            // um SIGKILL ao node e mais nada: o CLI do Claude é neto, ficava
            // órfão, e órfão continuava a segurar a sessão (#108).
            #[cfg(unix)]
            if let Some(pid) = group {
                unsafe { libc::killpg(pid as libc::pid_t, libc::SIGKILL) };
            }
            let _ = child.kill().await;
            outcome
        })
    }
}

/// O que a Relay nunca escreve na transcrição, e que por isso não serve de
/// marca: são os tokens da escrita ao vivo, e o texto assente que os sucede é
/// que fica. Apontar a reatação para um deles saltava esse texto.
///
/// Espelha o `RunEvent::is_ephemeral`; as duas listas descrevem a mesma decisão
/// vista de lados diferentes do cano.
const EPHEMERAL: [&str; 5] = ["delta", "thinking", "turns", "commands", "background_tasks"];

/// Diz quem somos e confere quem atendeu.
///
/// Um socket é um sítio, não uma identidade. Sem esta conferência, uma Relay
/// que se ligasse a um caminho reaproveitado adoptava o run de *outro* agente —
/// um cartão a compilar em vez da conversa do Director — e passava a escrever
/// os eventos dele na conversa errada, a responder-lhe às aprovações e a
/// mandar-lhe mensagens que não eram para ele. Melhor recusar e levantar um run
/// novo do que herdar trabalho alheio.
/// Atendeu quem procurávamos?
///
/// Separado para poder ser exercitado sem levantar processos: é uma decisão de
/// três linhas com um `SIGKILL` e uma conversa alheia do outro lado.
#[cfg_attr(not(unix), allow(dead_code))]
fn same_run(greeting: &serde_json::Value, expect_key: &str) -> Result<(), String> {
    let theirs = greeting.get("run_key").and_then(|k| k.as_str()).unwrap_or("");
    if theirs != expect_key {
        return Err(format!(
            "that socket is serving {theirs:?}, not {expect_key:?} — refusing to adopt it"
        ));
    }
    Ok(())
}

#[cfg(unix)]
async fn handshake(
    out: &mut Writer,
    read: Reader,
    expect_key: &str,
    from_seq: Option<u64>,
) -> Result<(tokio::io::Lines<BufReader<Reader>>, bool), String> {
    LineSink { out }
        .send(serde_json::json!({ "type": "attach", "from_seq": from_seq }))
        .await?;
    let mut lines = BufReader::new(read).lines();
    let greeting = match lines.next_line().await {
        Ok(Some(line)) => line,
        _ => return Err("sidecar did not answer the attach".to_string()),
    };
    let greeting: serde_json::Value =
        serde_json::from_str(&greeting).map_err(|e| format!("bad attach answer: {e}"))?;
    same_run(&greeting, expect_key)?;
    let running = greeting
        .get("running")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);
    Ok((lines, running))
}

/// O socket aparece um instante depois do processo. Espera-se por ele em vez de
/// se adivinhar um tempo: uma máquina cansada demora mais, e um `sleep` fixo ou
/// falha nela ou faz toda a gente esperar por ela.
#[cfg(unix)]
async fn await_socket(path: &std::path::Path) -> Result<tokio::net::UnixStream, String> {
    for _ in 0..200 {
        if let Ok(stream) = tokio::net::UnixStream::connect(path).await {
            return Ok(stream);
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    Err(format!("sidecar never served on {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{done_error, mcp_json, same_run, EPHEMERAL};
    use harness_ports::{Grants, McpGrant, McpTransport, RunEvent};
    use serde_json::json;

    /// O contrato entre as duas linguagens. O sidecar manda `task_type` e
    /// `description` sempre presentes — nunca `undefined` — porque isto
    /// desserializa para `String`: um campo em falta faria a carga inteira cair
    /// fora, e o ecrã ficaria com o conjunto anterior a dizer que ainda corre.
    #[test]
    fn the_sidecars_background_tasks_deserialise_as_sent() {
        let sent = json!([
            { "task_id": "t1", "task_type": "shell", "description": "sleep 400" },
            { "task_id": "t2", "task_type": "", "description": "" },
        ]);
        let tasks: Vec<harness_ports::BackgroundTask> =
            serde_json::from_value(sent).expect("o que o sidecar manda tem de entrar");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].task_id, "t1");
        assert_eq!(tasks[0].task_type, "shell");
        assert_eq!(tasks[1].description, "");

        // E o vazio é um conjunto, não um erro: é ele que desliga o indicador.
        let none: Vec<harness_ports::BackgroundTask> =
            serde_json::from_value(json!([])).expect("o vazio é uma resposta");
        assert!(none.is_empty());
    }

    /// Um socket é um sítio, não uma identidade.
    ///
    /// Sem esta conferência, uma Relay que se ligasse a um caminho reaproveitado
    /// adoptava o run de outro agente — um cartão a compilar em vez da conversa
    /// do Director — e passava a escrever-lhe os eventos na conversa errada e a
    /// responder-lhe às aprovações em nome dela.
    #[test]
    fn a_socket_serving_someone_else_is_refused() {
        assert!(same_run(&json!({ "run_key": "chat-c1" }), "chat-c1").is_ok());
        // O caso que interessa: um cartão a atender onde se procurava a conversa.
        let wrong = same_run(&json!({ "run_key": "card-c_e530" }), "chat-c1").unwrap_err();
        assert!(wrong.contains("card-c_e530") && wrong.contains("chat-c1"));
        // Um sidecar velho, que não se identifica, também não serve: adoptar às
        // cegas é o que isto existe para impedir.
        assert!(same_run(&json!({}), "chat-c1").is_err());
        assert!(same_run(&json!({ "run_key": null }), "chat-c1").is_err());
    }

    /// As duas listas descrevem a mesma decisão de lados diferentes do cano, e
    /// separá-las é como se estragam em silêncio: um evento que a Relay passasse
    /// a guardar sem sair daqui fazia a marca da reatação apontar para trás
    /// dele, e o texto voltava repetido; ao contrário, saltava-o.
    #[test]
    fn the_ephemeral_list_matches_what_relay_refuses_to_keep() {
        let all = [
            RunEvent::Delta { text: String::new() },
            RunEvent::Thinking { text: String::new() },
            RunEvent::Turns { count: 0 },
            RunEvent::Commands { commands: vec![] },
            RunEvent::BackgroundTasks { tasks: vec![] },
            RunEvent::Text { text: String::new() },
            RunEvent::Notice { text: String::new() },
            RunEvent::LocalOutput { text: String::new() },
            RunEvent::Failed { message: String::new() },
        ];
        for event in all {
            let kind = serde_json::to_value(&event).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string();
            assert_eq!(
                EPHEMERAL.contains(&kind.as_str()),
                event.is_ephemeral(),
                "{kind} discorda entre a marca e a transcrição",
            );
        }
    }

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

#[cfg(all(test, unix))]
mod process_group_tests {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    fn alive(pid: u32) -> bool {
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }

    /// Regressão (#108): matar o sidecar não pode deixar de pé o que ele
    /// levantou. Antes disto o SIGKILL ia só ao node, o CLI do Claude ficava
    /// órfão a segurar a sessão, e as mensagens seguintes iam parar à fila
    /// dele. O `sh` faz aqui de sidecar e o `sleep` de CLI — o que se guarda é
    /// a mecânica: grupo próprio à nascença, grupo inteiro à morte.
    #[tokio::test]
    async fn killing_the_sidecar_takes_the_cli_with_it() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("sleep 60 & echo $!; sleep 60")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        cmd.process_group(0);
        let mut child = cmd.spawn().expect("spawn");
        let group = child.id().expect("pid");

        let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
        let grandchild: u32 = lines
            .next_line()
            .await
            .expect("read")
            .expect("pid line")
            .trim()
            .parse()
            .expect("pid");
        assert!(alive(grandchild), "o neto devia estar de pé antes de matarmos");

        unsafe { libc::killpg(group as libc::pid_t, libc::SIGKILL) };
        let _ = child.kill().await;

        // O SIGKILL não é síncrono; dá-se-lhe uma janela curta antes de exigir.
        for _ in 0..40 {
            if !alive(grandchild) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        unsafe { libc::kill(grandchild as libc::pid_t, libc::SIGKILL) };
        panic!("o neto sobreviveu ao sidecar: é a avaria do #108 de volta");
    }
}
