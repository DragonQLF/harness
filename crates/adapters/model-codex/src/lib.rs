//! `AgentPort` over the Codex CLI, spoken to as its **app server**.
//!
//! Codex offers three ways in and only one of them is a whole agent:
//!
//! - `codex exec --json` prints four kinds of JSONL and then exits. No deltas,
//!   no way to answer an approval, no way to hand it a tool. A card would get
//!   worked, but the operator would watch a spinner and the permission sheet
//!   would never open.
//! - Driving the TUI through a pseudo-terminal and reading the screen. This is
//!   what other orchestrators do, and it is why they document
//!   `approval_policy = "never"`: a boxed prompt on a screen is not an event
//!   you can answer.
//! - `codex app-server`: bidirectional JSON-RPC 2.0 over stdio, which is what
//!   OpenAI's own VS Code extension and desktop app speak. Deltas, reasoning,
//!   per-turn usage, thread resume, and — the reason this is the only usable
//!   one — approvals arriving as **inbound requests** we answer.
//!
//! So this adapter is a JSON-RPC client. It is marked experimental upstream
//! (`codex app-server --help` says so, and the docs repeat it), which is a real
//! cost: the schema drifts between Codex versions. `docs/DECISIONS.md` carries
//! that as a known cost rather than a surprise — the alternative was an agent
//! that cannot be interrupted or asked.
//!
//! **Auth is not ours to hold.** Nothing here reads a key. The app server uses
//! whatever `codex login` left in `CODEX_HOME`, which on a ChatGPT plan is a
//! subscription rather than metered tokens. That is also why there is no
//! `cost_usd` anywhere below: a subscription turn has no dollar figure, and
//! inventing one would be a decorative number (`CLAUDE.md`). What exists
//! instead is [`plan_usage`] — the percentage of the plan's windows spent.

use std::collections::BTreeMap;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;

use harness_ports::{
    AgentPort, ApprovalRequest, McpTransport, RunEvent, RunOutcome, RunSpec,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// What Relay calls itself when it introduces itself to the app server. It
/// lands in the user agent string Codex sends upstream, so it is a name and not
/// a placeholder.
const CLIENT_NAME: &str = "relay";
const CLIENT_TITLE: &str = "Relay";

/// How long to wait for the handshake before deciding the binary is not going
/// to answer. A missing `codex` fails at spawn; a `codex` that is being
/// installed, or is waiting on a login, hangs — and a run that hangs on the
/// board is worse than one that says why it did not start.
const HANDSHAKE_TIMEOUT_SECS: u64 = 30;

pub struct CodexAgent {
    program: String,
    /// The `CODEX_HOME` runs start from. `None` means the operator's own, which
    /// is the fallback rather than the intent: see [`prepare_home`].
    home: Option<std::path::PathBuf>,
}

impl CodexAgent {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            home: None,
        }
    }

    /// Isolate runs in a directory of Relay's, linking the operator's login
    /// into it. Silently keeps the real home when there is no login to link —
    /// an unauthenticated Codex should say so itself rather than be hidden
    /// behind an empty directory Relay made.
    pub fn with_home(mut self, dir: &Path) -> Self {
        self.home = prepare_home(dir);
        self
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

/// Codex's sandbox and approval policy, derived from the permission mode the
/// rest of Relay already speaks.
///
/// The two systems meet here rather than anywhere else. Codex sandboxes by
/// *directory* — a `workspace-write` run edits inside its worktree without
/// asking, and anything outside it becomes an approval request. That happens to
/// be exactly what `acceptEdits` means, so the mapping is a translation and not
/// a policy decision. `danger-full-access` is never chosen: no permission mode
/// Relay has asks for a run with no sandbox at all, and reaching for it because
/// a mode is unknown would silently widen what an agent may do.
fn sandbox_and_approval(mode: Option<&str>) -> (&'static str, &'static str, Option<&'static str>) {
    match mode.unwrap_or("acceptEdits") {
        // Nothing is meant to change, so the sandbox — not a promise in the
        // prompt — is what stops it.
        "plan" => ("read-only", "never", None),
        // A classifier decides. Codex has its own; using it keeps the operator
        // out of a loop they asked to be out of.
        "auto" => ("workspace-write", "on-request", Some("auto_review")),
        // "Deny if not pre-approved": never escalate, so an escape fails
        // instead of arriving on somebody's screen.
        "dontAsk" | "bypassPermissions" => ("workspace-write", "never", None),
        // `default` and `acceptEdits` both land here: edits inside the
        // worktree go through, escapes are asked about.
        _ => ("workspace-write", "on-request", None),
    }
}

/// The granted MCP servers, as `-c` config overrides.
///
/// Decision #26 for Codex, and it took two goes. `-c mcp_servers={}` does
/// **not** clear the operator's servers — the override merges into the loaded
/// config rather than replacing it, so a run started that way still brought up
/// every connector in `~/.codex/config.toml` (measured: four of them announced
/// themselves as `starting` on the first probe). What does work is
/// [`prepare_home`]: a `CODEX_HOME` of Relay's own, where there is nothing to
/// merge with. These overrides then *add* what this one agent was granted, on
/// top of nothing.
///
/// Written as dotted paths rather than as one inline table because the value of
/// a `-c` is parsed as TOML: a JSON object is not a TOML inline table, so
/// `env={"K":"V"}` fails to parse while `env.K="V"` is exactly right. Strings
/// and arrays happen to be spelled the same in both, so `json!` serves for
/// those.
fn mcp_overrides(spec: &RunSpec) -> Vec<String> {
    let mut out = Vec::new();
    for grant in &spec.grants.mcp_servers {
        let name = grant.name.trim();
        // A name with a dot in it would land in the wrong place in the config
        // tree — `mcp_servers.a.b.command` is a server `a` with a table `b`.
        if name.is_empty() || name.contains('.') {
            continue;
        }
        match &grant.transport {
            McpTransport::Stdio { command, args } => {
                if command.trim().is_empty() {
                    continue;
                }
                out.push(format!("mcp_servers.{name}.command={}", json!(command)));
                if !args.is_empty() {
                    out.push(format!("mcp_servers.{name}.args={}", json!(args)));
                }
            }
            McpTransport::Http { url } | McpTransport::Sse { url } => {
                if url.trim().is_empty() {
                    continue;
                }
                out.push(format!("mcp_servers.{name}.url={}", json!(url)));
            }
        }
        for (key, value) in &grant.env {
            if key.trim().is_empty() || key.contains('.') {
                continue;
            }
            out.push(format!("mcp_servers.{name}.env.{key}={}", json!(value)));
        }
    }
    out
}

/// A `CODEX_HOME` that belongs to Relay, holding the operator's login and
/// nothing else.
///
/// This is the isolation. Codex reads its connectors, its skills and its
/// defaults out of `CODEX_HOME`; point it at a directory Relay owns and a run
/// starts from nothing, exactly as decision #26 requires. Auth is the one thing
/// that must come through, so `auth.json` is **linked** rather than copied: the
/// token refreshes, and a copy would be a login that silently goes stale.
///
/// Returns `None` when the link cannot be made — an operator who has never run
/// `codex login` has no file to link, and there is nothing to isolate anyway.
/// The run then uses the real home and Codex says for itself that it is not
/// logged in, which is the error worth showing.
pub fn prepare_home(dir: &Path) -> Option<std::path::PathBuf> {
    let source = real_home()?.join("auth.json");
    if !source.exists() {
        return None;
    }
    std::fs::create_dir_all(dir).ok()?;
    // Written every time: it is ours, it is three lines, and a stale one from
    // an older Relay is not worth reasoning about.
    std::fs::write(
        dir.join("config.toml"),
        "# Written by Relay. Codex runs started from here see this file and\n         # nothing from the operator's own ~/.codex — see decision #26.\n",
    )
    .ok()?;

    let link = dir.join("auth.json");
    if std::fs::symlink_metadata(&link).is_ok() {
        // Already there. Only a link that still points at the operator's file
        // is worth keeping; anything else is replaced.
        if std::fs::read_link(&link).map(|t| t == source).unwrap_or(false) {
            return Some(dir.to_path_buf());
        }
        let _ = std::fs::remove_file(&link);
    }
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&source, &link).is_ok();
    #[cfg(windows)]
    // Symlinks need a privilege ordinary accounts do not have; a hard link
    // needs none and shares the same bytes, which is what matters — the file is
    // rewritten in place on refresh.
    let linked = std::fs::hard_link(&source, &link).is_ok();
    linked.then(|| dir.to_path_buf())
}

/// Where Codex keeps its own state, respecting an operator who has moved it.
fn real_home() -> Option<std::path::PathBuf> {
    if let Ok(set) = std::env::var("CODEX_HOME") {
        if !set.trim().is_empty() {
            return Some(std::path::PathBuf::from(set));
        }
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|h| std::path::PathBuf::from(h).join(".codex"))
}

/// How an approval is answered. Two shapes, because the protocol has two: a
/// command or a patch takes a yes/no, and a permissions request takes back the
/// profile that ends up granted.
enum Decide {
    Decision,
    Grant(Value),
}

/// Why a run stopped, when it stopped before the turn did.
enum Stop {
    Cancelled,
    Failed(String),
}

/// One JSON-RPC conversation with one app server process.
///
/// Deliberately single-threaded over one reader: the protocol here is
/// sequential — handshake, thread, turn — and the one place it is not is an
/// inbound approval, which we want to block the read anyway. Codex is waiting
/// for our answer; reading ahead would only let us process events from a turn
/// that is paused on us.
struct Session {
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    tx: mpsc::Sender<RunEvent>,
    next_id: u64,
    thread_id: Option<String>,
    turn_id: Option<String>,
    turns: u32,
    /// Ids of items we have already announced as a `ToolUse`, so the matching
    /// `item/completed` can be reported as its result instead of as a second
    /// call.
    open_items: BTreeMap<String, String>,
    /// What each open item is *doing*, by id — the command line, the files.
    ///
    /// An approval request names the item and often not the work: it carries an
    /// `itemId` and a reason, and the command itself was in the `item/started`
    /// that announced it. Without this the sheet says "Bash" and nothing else,
    /// which is asking somebody to authorise a thing that is not named.
    commands: BTreeMap<String, String>,
    /// Images this turn saved, in the order Codex produced them.
    ///
    /// They also go into the transcript as markdown, which is what makes them
    /// visible. This list is for the caller that wants the *file* — the
    /// `generate_image` tool, which has to hand a path back to another agent
    /// and must not do it by parsing its own markdown back apart.
    images: Vec<String>,
}

impl Session {
    async fn emit(&self, ev: RunEvent) {
        let _ = self.tx.send(ev).await;
    }

    async fn write(&mut self, msg: Value) -> Result<(), Stop> {
        let mut line = serde_json::to_string(&msg).map_err(|e| Stop::Failed(e.to_string()))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| Stop::Failed(format!("write to codex: {e}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| Stop::Failed(format!("flush codex: {e}")))
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), Stop> {
        self.write(json!({ "method": method, "params": params })).await
    }

    /// Send a request and return its id. The answer is collected by
    /// [`Self::pump`], which is also what forwards everything that arrives
    /// while we wait.
    async fn send(&mut self, method: &str, params: Value) -> Result<u64, Stop> {
        self.next_id += 1;
        let id = self.next_id;
        self.write(json!({ "id": id, "method": method, "params": params }))
            .await?;
        Ok(id)
    }

    /// Read until the thing we are waiting for arrives, turning everything else
    /// into `RunEvent`s on the way and answering anything Codex asks us.
    ///
    /// `until_turn_done` is the whole reason this takes a flag. `turn/start`
    /// answers **immediately**, with `status: "inProgress"` and an empty item
    /// list — it acknowledges the turn rather than reporting it. Treating that
    /// answer as the end is a run that reports success before the model has
    /// said a word, which is exactly what the first build of this adapter did.
    /// The end of a turn is the `turn/completed` notification.
    async fn pump(
        &mut self,
        id: u64,
        until_turn_done: bool,
        spec: &RunSpec,
        cancel: &CancellationToken,
    ) -> Result<Value, Stop> {
        loop {
            let line = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    self.interrupt().await;
                    return Err(Stop::Cancelled);
                }
                queued = next_queued(spec) => {
                    if let Some(msg) = queued {
                        self.steer(spec, msg).await?;
                    }
                    continue;
                }
                line = self.lines.next_line() => line,
            };
            let line = match line {
                Ok(Some(l)) => l,
                // stdout closed with the answer still outstanding: the process
                // died. Whatever it wrote to stderr is gone by design (we do
                // not pipe it), so say the shape of the failure.
                Ok(None) => {
                    return Err(Stop::Failed(
                        "the codex app server closed before answering".to_string(),
                    ))
                }
                Err(e) => return Err(Stop::Failed(format!("read from codex: {e}"))),
            };
            let msg: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                // A non-JSON line is not ours to interpret. Skipping is right:
                // the protocol is JSONL and anything else on the stream is
                // noise from something further down.
                Err(_) => continue,
            };

            if let Some(err) = msg.get("error") {
                if msg.get("id").and_then(Value::as_u64) == Some(id) {
                    let text = err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("codex refused the request");
                    return Err(Stop::Failed(text.to_string()));
                }
                continue;
            }
            if msg.get("id").and_then(Value::as_u64) == Some(id) && msg.get("method").is_none() {
                let result = msg.get("result").cloned().unwrap_or(Value::Null);
                if !until_turn_done {
                    return Ok(result);
                }
                // The acknowledgement. Keep the turn id from it — a steer sent
                // before the first `turn/started` notification needs it — and
                // go on reading.
                if let Some(turn) = result.pointer("/turn/id").and_then(Value::as_str) {
                    self.turn_id = Some(turn.to_string());
                }
                continue;
            }
            // An inbound request: an approval, almost always.
            if let (Some(req_id), Some(method)) = (msg.get("id"), msg.get("method").and_then(Value::as_str)) {
                let req_id = req_id.clone();
                let method = method.to_string();
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                self.answer_request(req_id, &method, params, spec).await?;
                continue;
            }
            if let Some(method) = msg.get("method").and_then(Value::as_str) {
                let method = method.to_string();
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                if until_turn_done && method == "turn/completed" {
                    return Ok(params);
                }
                self.on_notification(&method, params).await;
            }
        }
    }

    /// Hand an operator message to the turn that is already running.
    ///
    /// `turn/steer` is refused unless `expectedTurnId` matches what is live,
    /// which is the right shape: a message typed at a turn that has since ended
    /// belongs to the next one, not to this one. When there is no live turn yet
    /// the message is left in the queue — `mark_read` is only called once Codex
    /// has actually taken it.
    async fn steer(&mut self, spec: &RunSpec, msg: harness_ports::QueuedMessage) -> Result<(), Stop> {
        let (Some(thread), Some(turn)) = (self.thread_id.clone(), self.turn_id.clone()) else {
            return Ok(());
        };
        self.emit(RunEvent::UserQueued {
            queue_id: msg.id.clone(),
            text: msg.text.clone(),
        })
        .await;
        let id = self
            .send(
                "turn/steer",
                json!({
                    "threadId": thread,
                    "expectedTurnId": turn,
                    "input": [{ "type": "text", "text": msg.text }],
                }),
            )
            .await?;
        // Not pumped: the acknowledgement is one line and waiting for it here
        // would nest a pump inside a pump. It lands in the next read and is
        // discarded there, which is all an acknowledgement is worth.
        let _ = id;
        if let Some(inbox) = &spec.inbox {
            inbox.mark_read(&msg.id);
        }
        self.emit(RunEvent::UserRead { queue_id: msg.id }).await;
        Ok(())
    }

    /// Answer something Codex asked us. Approvals go to the operator through
    /// the same `Approver` the sidecar uses, so a Codex run and a Claude run
    /// raise the same sheet.
    async fn answer_request(
        &mut self,
        req_id: Value,
        method: &str,
        params: Value,
        spec: &RunSpec,
    ) -> Result<(), Stop> {
        // Os nomes são os do **fio**, não os dos tipos do esquema. Foi assim
        // que a primeira versão disto falhou: derivei-os de
        // `CommandExecutionRequestApprovalParams` e escrevi
        // `commandExecutionRequestApproval`, quando o que o Codex manda é
        // `item/commandExecution/requestApproval`. Medido — o pedido chegava,
        // não batia com nada, respondia-se-lhe "não implementado" e a aprovação
        // nunca chegava a quem a devia ver. Os antigos ficam como sinónimos:
        // custam uma linha e cobrem uma versão anterior do protocolo.
        let decision = match method {
            "item/commandExecution/requestApproval"
            | "execCommandApproval"
            | "thread/shellCommand" => {
                // O comando não vem sempre no pedido — o que vem sempre é o
                // `itemId`, e o comando ficou no `item/started` que o anunciou.
                // Sem isto a folha dizia "Bash" e mais nada, que é pedir
                // autorização para uma coisa que não se nomeia.
                let command = params
                    .get("command")
                    .map(|c| match c {
                        Value::Array(parts) => parts
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" "),
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .or_else(|| {
                        params
                            .get("itemId")
                            .and_then(Value::as_str)
                            .and_then(|id| self.commands.get(id).cloned())
                    })
                    .unwrap_or_default();
                let reason = params.get("reason").and_then(Value::as_str).unwrap_or("");
                let summary = if reason.is_empty() {
                    truncate(&command, 200)
                } else {
                    format!("{} — {}", truncate(&command, 160), truncate(reason, 120))
                };
                Some(("Bash", summary, Decide::Decision))
            }
            "item/fileChange/requestApproval" | "applyPatchApproval" => {
                let files: Vec<String> = params
                    .get("fileChanges")
                    .and_then(Value::as_object)
                    .map(|m| m.keys().cloned().collect())
                    .unwrap_or_default();
                let summary = if files.is_empty() {
                    params
                        .get("itemId")
                        .and_then(Value::as_str)
                        .and_then(|id| self.commands.get(id).cloned())
                        .or_else(|| {
                            params.get("reason").and_then(Value::as_str).map(str::to_string)
                        })
                        .unwrap_or_else(|| "a write outside the worktree".to_string())
                } else {
                    truncate(&files.join(", "), 200)
                };
                Some(("Edit", summary, Decide::Decision))
            }
            "item/permissions/requestApproval" | "permissionsRequestApproval" => {
                let summary = params
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        format!(
                            "extra permissions: {}",
                            truncate(
                                &params.get("permissions").map(|p| p.to_string()).unwrap_or_default(),
                                140
                            )
                        )
                    });
                // Esta não se responde com um sim/não: responde-se com o que
                // fica concedido. Negar é conceder nada; conceder é devolver
                // exactamente o que foi pedido, e não mais — inventar um perfil
                // aqui era alargar a permissão em nome de quem carregou no
                // botão.
                Some((
                    "Permissions",
                    truncate(&summary, 200),
                    Decide::Grant(params.get("permissions").cloned().unwrap_or(Value::Null)),
                ))
            }
            _ => None,
        };

        let Some((tool, summary, answer)) = decision else {
            // Anything else Codex asks for is answered with a JSON-RPC error
            // rather than silence. Silence stalls the turn forever; an error is
            // something Codex can fall back from.
            return self
                .write(json!({
                    "id": req_id,
                    "error": { "code": -32601, "message": format!("relay does not implement {method}") }
                }))
                .await;
        };

        let request_id = format!(
            "codex-{}",
            req_id
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| req_id.to_string())
        );
        self.emit(RunEvent::ApprovalRequested {
            request_id: request_id.clone(),
            tool: tool.to_string(),
            summary: summary.clone(),
        })
        .await;

        let allow = match &spec.approver {
            Some(approver) => {
                approver(ApprovalRequest {
                    request_id: request_id.clone(),
                    tool: tool.to_string(),
                    summary,
                    input: params.clone(),
                })
                .await
            }
            // No approver means nobody is watching this run. Denying is the
            // honest default: approving on an agent's own authority is what
            // decision #95 refused.
            None => false,
        };
        self.emit(RunEvent::ApprovalAnswered {
            request_id,
            allow,
        })
        .await;

        let result = match answer {
            Decide::Decision => json!({ "decision": if allow { "accept" } else { "decline" } }),
            // Um perfil com os dois campos a nulo é "nada concedido", que é a
            // recusa escrita na linguagem que este pedido fala.
            Decide::Grant(asked) => json!({
                "permissions": if allow { asked } else { json!({ "fileSystem": null, "network": null }) },
                "scope": "turn",
            }),
        };
        self.write(json!({ "id": req_id, "result": result })).await
    }

    /// Ask the live turn to stop. Best effort by design: we are on our way out
    /// and the process is killed next, so a refusal here changes nothing.
    async fn interrupt(&mut self) {
        if let (Some(thread), Some(turn)) = (self.thread_id.clone(), self.turn_id.clone()) {
            let _ = self
                .send("turn/interrupt", json!({ "threadId": thread, "turnId": turn }))
                .await;
        }
    }

    /// Codex's stream, in Relay's vocabulary.
    async fn on_notification(&mut self, method: &str, params: Value) {
        match method {
            "thread/started" => {
                // `params.thread.id`, not `params.threadId`. Every other
                // notification in the protocol uses the flat spelling, which is
                // exactly why this one was read wrong.
                if let Some(id) = params
                    .pointer("/thread/id")
                    .or_else(|| params.get("threadId"))
                    .and_then(Value::as_str)
                {
                    let already = self.thread_id.as_deref() == Some(id);
                    self.thread_id = Some(id.to_string());
                    if !already {
                        self.emit(RunEvent::Started {
                            session_id: id.to_string(),
                        })
                        .await;
                    }
                }
            }
            "turn/started" => {
                if let Some(turn) = params.pointer("/turn/id").and_then(Value::as_str) {
                    self.turn_id = Some(turn.to_string());
                } else if let Some(turn) = params.get("turnId").and_then(Value::as_str) {
                    self.turn_id = Some(turn.to_string());
                }
                self.turns += 1;
                self.emit(RunEvent::Turns { count: self.turns }).await;
            }
            "item/agentMessage/delta" => {
                if let Some(text) = params.get("delta").and_then(Value::as_str) {
                    self.emit(RunEvent::Delta {
                        text: text.to_string(),
                    })
                    .await;
                }
            }
            "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
                if let Some(text) = params.get("delta").and_then(Value::as_str) {
                    self.emit(RunEvent::Thinking {
                        text: text.to_string(),
                    })
                    .await;
                }
            }
            "thread/tokenUsage/updated" => {
                let last = params.pointer("/tokenUsage/last");
                if let Some(u) = last {
                    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    self.emit(RunEvent::Usage {
                        input_tokens: n("inputTokens"),
                        output_tokens: n("outputTokens"),
                        cache_read_tokens: n("cachedInputTokens"),
                        cache_creation_tokens: n("cacheWriteInputTokens"),
                        model: None,
                    })
                    .await;
                }
            }
            "item/started" => {
                if let Some(item) = params.get("item") {
                    self.on_item_started(item).await;
                }
            }
            "item/completed" => {
                if let Some(item) = params.get("item") {
                    self.on_item_completed(item).await;
                }
            }
            "model/rerouted" => {
                let from = params.get("fromModel").and_then(Value::as_str).unwrap_or("?");
                let to = params.get("toModel").and_then(Value::as_str).unwrap_or("?");
                self.emit(RunEvent::Notice {
                    text: format!("Codex moved this turn from {from} to {to}."),
                })
                .await;
            }
            "thread/compacted" => {
                self.emit(RunEvent::Notice {
                    text: "Codex compacted the thread — earlier turns are now a summary."
                        .to_string(),
                })
                .await;
            }
            _ => {}
        }
    }

    async fn on_item_started(&mut self, item: &Value) {
        let Some(kind) = item.get("type").and_then(Value::as_str) else {
            return;
        };
        let Some(id) = item.get("id").and_then(Value::as_str) else {
            return;
        };
        let (tool, summary) = match kind {
            "commandExecution" => (
                "Bash",
                truncate(
                    item.get("command").and_then(Value::as_str).unwrap_or(""),
                    200,
                ),
            ),
            "fileChange" => (
                "Edit",
                item.get("changes")
                    .and_then(Value::as_array)
                    .map(|c| format!("{} file(s)", c.len()))
                    .unwrap_or_default(),
            ),
            "mcpToolCall" => {
                let server = item.get("server").and_then(Value::as_str).unwrap_or("");
                let name = item.get("tool").and_then(Value::as_str).unwrap_or("");
                self.open_items
                    .insert(id.to_string(), format!("mcp__{server}__{name}"));
                self.emit(RunEvent::ToolUse {
                    tool: format!("mcp__{server}__{name}"),
                    summary: truncate(&item.get("arguments").map(|a| a.to_string()).unwrap_or_default(), 200),
                    tool_use_id: Some(id.to_string()),
                    parent_tool_use_id: None,
                    // O Codex não diz quantas linhas um `fileChange` mexe nesta
                    // notificação, portanto não se diz.
                    added: None,
                    removed: None,
                })
                .await;
                return;
            }
            "webSearch" => (
                "WebSearch",
                truncate(item.get("query").and_then(Value::as_str).unwrap_or(""), 160),
            ),
            "imageGeneration" => (
                "GenerateImage",
                truncate(
                    item.get("revisedPrompt").and_then(Value::as_str).unwrap_or("generating an image"),
                    160,
                ),
            ),
            // Reasoning and agent messages are not tool calls; their text
            // arrives as deltas and as the completed item.
            _ => return,
        };
        self.open_items.insert(id.to_string(), tool.to_string());
        if !summary.is_empty() {
            self.commands.insert(id.to_string(), summary.clone());
        }
        self.emit(RunEvent::ToolUse {
            tool: tool.to_string(),
            summary,
            tool_use_id: Some(id.to_string()),
            parent_tool_use_id: None,
                    added: None,
                    removed: None,
        })
        .await;
    }

    async fn on_item_completed(&mut self, item: &Value) {
        let Some(kind) = item.get("type").and_then(Value::as_str) else {
            return;
        };
        let id = item.get("id").and_then(Value::as_str).unwrap_or_default();

        if kind == "agentMessage" {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    self.emit(RunEvent::Text {
                        text: text.to_string(),
                    })
                    .await;
                }
            }
            return;
        }

        // A generated image is the one tool result that is worth *seeing*
        // rather than reading. Codex hands back an absolute path; it goes into
        // the transcript as markdown so the picture is still there when the
        // conversation is read back off disk, rather than only while the run is
        // live.
        if kind == "imageGeneration" {
            let path = item.get("savedPath").and_then(Value::as_str);
            let ok = item.get("status").and_then(Value::as_str) != Some("failed") && path.is_some();
            if let Some(path) = path {
                self.images.push(path.to_string());
                let alt = item
                    .get("revisedPrompt")
                    .and_then(Value::as_str)
                    .unwrap_or("generated image");
                self.emit(RunEvent::Text {
                    // `<...>` à volta do caminho: um destino de markdown com
                    // espaços — e "Application Support" tem um — não se escreve
                    // em `](...)` nu, que o analisador corta no espaço ou
                    // percent-codifica. Os angulares dizem "isto é um destino
                    // só, do princípio ao fim".
                    text: format!("![{}](<{}>)", alt.replace(']', ""), path),
                })
                .await;
            }
            if !id.is_empty() {
                self.emit(RunEvent::ToolResult {
                    tool_use_id: id.to_string(),
                    ok,
                    summary: path.unwrap_or("image generation failed").to_string(),
                    detail: None,
                })
                .await;
            }
            self.open_items.remove(id);
            return;
        }

        if id.is_empty() || !self.open_items.contains_key(id) {
            return;
        }
        self.open_items.remove(id);
        self.commands.remove(id);

        let (ok, summary, detail) = match kind {
            "commandExecution" => {
                let code = item.get("exitCode").and_then(Value::as_i64);
                let out = item
                    .get("aggregatedOutput")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                (
                    code == Some(0),
                    match code {
                        Some(0) => truncate(out.trim(), 200),
                        Some(c) => format!("exit {c}: {}", truncate(out.trim(), 180)),
                        None => truncate(out.trim(), 200),
                    },
                    (!out.is_empty()).then(|| truncate(out, 8_000)),
                )
            }
            "fileChange" => {
                let status = item.get("status").and_then(Value::as_str).unwrap_or("");
                let files: Vec<String> = item
                    .get("changes")
                    .and_then(Value::as_array)
                    .map(|cs| {
                        cs.iter()
                            .filter_map(|c| c.get("path").and_then(Value::as_str))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                (
                    status != "failed",
                    truncate(&files.join(", "), 200),
                    None,
                )
            }
            "mcpToolCall" => {
                let failed = item.get("error").map(|e| !e.is_null()).unwrap_or(false);
                let text = item
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| item.get("result").map(|r| truncate(&r.to_string(), 200)))
                    .unwrap_or_default();
                (!failed, text, None)
            }
            _ => (true, String::new(), None),
        };
        self.emit(RunEvent::ToolResult {
            tool_use_id: id.to_string(),
            ok,
            summary,
            detail,
        })
        .await;
    }
}

/// The inbox, as a future that never resolves when there is no inbox.
///
/// `select!` needs every branch to be a future; a run without an operator
/// typing at it — every card run — must simply never take this branch.
async fn next_queued(spec: &RunSpec) -> Option<harness_ports::QueuedMessage> {
    match &spec.inbox {
        Some(inbox) => std::sync::Arc::clone(inbox).next().await,
        None => std::future::pending().await,
    }
}

/// Spawn one app server: Relay's home, plus whatever this run was granted.
fn spawn(
    program: &str,
    cwd: &Path,
    home: Option<&Path>,
    overrides: &[String],
) -> Result<Child, String> {
    let mut cmd = Command::new(program);
    cmd.arg("app-server");
    for value in overrides {
        cmd.arg("-c").arg(value);
    }
    if let Some(home) = home {
        cmd.env("CODEX_HOME", home);
    }
    cmd.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Codex writes progress chatter to stderr. Inheriting it would put it
        // in Relay's own console; piping it without draining it would fill a
        // pipe buffer and wedge the process.
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .map_err(|e| format!("failed to spawn '{program} app-server': {e}"))
}

impl AgentPort for CodexAgent {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<RunOutcome, String>> + Send>> {
        let program = self.program.clone();
        let home = self.home.clone();
        Box::pin(async move {
            // Nothing to do and nothing to say: attaching to work that
            // continued without us is the sidecar's trick, and it needs a
            // socket the app server does not have. A Codex run lives and dies
            // with this process, which the caller learns by getting no events.
            if spec.attach_only {
                return Ok(RunOutcome::completed(spec.resume_session.clone(), None));
            }

            let mut child = spawn(&program, &spec.cwd, home.as_deref(), &mcp_overrides(&spec))?;
            let stdin = child.stdin.take().ok_or("codex stdin unavailable")?;
            let stdout = child.stdout.take().ok_or("codex stdout unavailable")?;
            let mut session = Session {
                stdin,
                lines: BufReader::new(stdout).lines(),
                tx: tx.clone(),
                next_id: 0,
                thread_id: None,
                turn_id: None,
                turns: 0,
                open_items: BTreeMap::new(),
                commands: BTreeMap::new(),
                images: Vec::new(),
            };

            let outcome = drive(&mut session, &spec, &cancel).await;
            // The process is ours and the turn is over either way: a server
            // left running would hold the thread's rollout lock and refuse the
            // next resume of the same session.
            let _ = child.kill().await;

            match outcome {
                Ok((session_id, turns, result, error)) => {
                    if let Some(message) = error {
                        let _ = tx
                            .send(RunEvent::Done {
                                session_id: session_id.clone(),
                                cost_usd: None,
                                turns: Some(turns),
                                result: result.clone(),
                                error: Some(message.clone()),
                            })
                            .await;
                        return Ok(RunOutcome::Failed {
                            message,
                            cost_usd: None,
                            turns: Some(turns),
                        });
                    }
                    let _ = tx
                        .send(RunEvent::Done {
                            session_id: session_id.clone(),
                            // A subscription turn has no price. See the module
                            // note: an invented figure is worse than a blank.
                            cost_usd: None,
                            turns: Some(turns),
                            result,
                            error: None,
                        })
                        .await;
                    Ok(RunOutcome::Completed {
                        session_id,
                        cost_usd: None,
                        turns: Some(turns),
                    })
                }
                Err(Stop::Cancelled) => Ok(RunOutcome::Cancelled),
                Err(Stop::Failed(message)) => {
                    let _ = tx
                        .send(RunEvent::Failed {
                            message: message.clone(),
                        })
                        .await;
                    Ok(RunOutcome::Failed {
                        message,
                        cost_usd: None,
                        turns: Some(session_turns(&session)),
                    })
                }
            }
        })
    }
}

fn session_turns(session: &Session) -> u32 {
    session.turns
}

/// Handshake, thread, turn. Returns the session id, how many turns happened,
/// the final message, and an error when the turn itself failed.
async fn drive(
    session: &mut Session,
    spec: &RunSpec,
    cancel: &CancellationToken,
) -> Result<(Option<String>, u32, Option<String>, Option<String>), Stop> {
    let hello = session
        .send(
            "initialize",
            json!({
                "clientInfo": {
                    "name": CLIENT_NAME,
                    "title": CLIENT_TITLE,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    // Command output arrives twice — as a delta stream and as
                    // the completed item. Relay shows the item, so the deltas
                    // are thousands of lines nobody reads.
                    "optOutNotificationMethods": [
                        "item/commandExecution/outputDelta",
                        "item/fileChange/outputDelta",
                    ],
                },
            }),
        )
        .await?;
    match tokio::time::timeout(
        std::time::Duration::from_secs(HANDSHAKE_TIMEOUT_SECS),
        session.pump(hello, false, spec, cancel),
    )
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            return Err(Stop::Failed(format!(
                "the codex app server did not answer in {HANDSHAKE_TIMEOUT_SECS}s — is `codex login` done?"
            )))
        }
    };
    session.notify("initialized", json!({})).await?;

    let (sandbox, approval, reviewer) = sandbox_and_approval(spec.permission_mode.as_deref());
    let mut params = json!({
        "cwd": spec.cwd.to_string_lossy(),
        "sandbox": sandbox,
        "approvalPolicy": approval,
    });
    if let Some(reviewer) = reviewer {
        params["approvalsReviewer"] = json!(reviewer);
    }
    if let Some(model) = &spec.model {
        params["model"] = json!(model);
    }

    let (method, params) = match &spec.resume_session {
        Some(id) if !id.is_empty() => {
            params["threadId"] = json!(id);
            ("thread/resume", params)
        }
        _ => ("thread/start", params),
    };
    let started = session.send(method, params).await?;
    let thread = session.pump(started, false, spec, cancel).await?;
    let thread_id = thread
        .get("threadId")
        .or_else(|| thread.pointer("/thread/id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(id) = &thread_id {
        if session.thread_id.is_none() {
            session.thread_id = Some(id.clone());
            session
                .emit(RunEvent::Started {
                    session_id: id.clone(),
                })
                .await;
        }
    }

    let mut turn = json!({
        "threadId": session.thread_id.clone().unwrap_or_default(),
        "input": [{ "type": "text", "text": spec.prompt }],
    });
    if let Some(effort) = &spec.effort {
        turn["effort"] = json!(effort);
    }
    if let Some(model) = &spec.model {
        turn["model"] = json!(model);
    }
    let turn_id = session.send("turn/start", turn).await?;
    let done = session.pump(turn_id, true, spec, cancel).await?;

    let status = done.pointer("/turn/status").and_then(Value::as_str);
    let error = done
        .pointer("/turn/error/message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            (status == Some("failed")).then(|| "the codex turn failed".to_string())
        });
    let result = done
        .pointer("/turn/items")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .rev()
                .find(|i| i.get("type").and_then(Value::as_str) == Some("agentMessage"))
                .and_then(|i| i.get("text").and_then(Value::as_str))
                .map(str::to_string)
        });

    Ok((
        session.thread_id.clone().or(thread_id),
        session.turns.max(1),
        result,
        error,
    ))
}

// ---- one-shot questions, asked without starting a run ----

/// What the plan has left, as Codex reports it.
///
/// This is the honest replacement for a cost: a subscription run has no dollar
/// figure, but it does have a share of a rolling window, and that is a real
/// number that came from the provider rather than from arithmetic here.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PlanUsage {
    /// `plus`, `pro`, `team`, … or empty when Codex does not say.
    pub plan: String,
    /// Percent of the short window spent, and when it resets (unix seconds).
    pub primary_percent: u32,
    pub primary_resets_at: Option<u64>,
    pub primary_window_mins: Option<u64>,
    pub secondary_percent: u32,
    pub secondary_resets_at: Option<u64>,
    pub secondary_window_mins: Option<u64>,
    /// Codex says the limit is already reached, and why.
    pub reached: Option<String>,
}

/// One model Codex offers, as its own catalogue names it.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CodexModel {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_effort: String,
    pub efforts: Vec<String>,
}

/// Ask the app server one question and shut it down again.
///
/// Used for the things that are not runs: what the plan has left, what models
/// exist. A short-lived process is right for both — neither answer is worth
/// keeping a Codex alive between screens, and a stale one is worse than a
/// fresh round trip.
async fn write_line(stdin: &mut ChildStdin, msg: Value) -> Result<(), String> {
    let mut line = serde_json::to_string(&msg).map_err(|e| e.to_string())?;
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

async fn ask(
    program: &str,
    home: Option<&Path>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut child = spawn(program, &cwd, home, &[])?;
    let mut stdin = child.stdin.take().ok_or("codex stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("codex stdout unavailable")?;
    let mut lines = BufReader::new(stdout).lines();

    let answer = async {
        write_line(
            &mut stdin,
            json!({
                "id": 1,
                "method": "initialize",
                "params": {
                    "clientInfo": { "name": CLIENT_NAME, "title": CLIENT_TITLE, "version": env!("CARGO_PKG_VERSION") }
                }
            }),
        )
        .await?;
        // Wait for the handshake before asking: the server refuses everything
        // else until it has answered.
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("id").and_then(Value::as_u64) == Some(1) {
                break;
            }
        }
        write_line(&mut stdin, json!({ "method": "initialized", "params": {} })).await?;
        write_line(&mut stdin, json!({ "id": 2, "method": method, "params": params })).await?;
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if msg.get("id").and_then(Value::as_u64) != Some(2) {
                continue;
            }
            if let Some(err) = msg.get("error").and_then(|e| e.get("message")).and_then(Value::as_str) {
                return Err(err.to_string());
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
        Err(format!("codex closed without answering {method}"))
    };

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(HANDSHAKE_TIMEOUT_SECS),
        answer,
    )
    .await
    .unwrap_or_else(|_| Err(format!("codex did not answer {method} in time")));
    let _ = child.kill().await;
    result
}

/// Generate an image and return where it was saved.
///
/// This is the one thing in Codex that a *Claude* agent wants: OpenAI's image
/// model, reached through the plan this machine is already logged into rather
/// than through an `OPENAI_API_KEY` nobody has. Codex ships it as a built-in
/// tool — its own skill file says the built-in path "does not require
/// `OPENAI_API_KEY`", and the CLI fallback that does is deliberately not what
/// this uses.
///
/// One thread, one turn, thrown away after. Not a conversation: an image is a
/// tool call, and keeping a thread alive between calls would mean the second
/// image is generated in the context of the first.
///
/// `cwd` is where Codex is allowed to write. The generated file lands under
/// `CODEX_HOME` regardless — that is where the built-in tool puts it — so the
/// path that comes back is outside `cwd` and is the caller's to copy if it
/// wants the asset in a repository.
pub async fn generate_image(
    program: &str,
    home: Option<&Path>,
    prompt: &str,
    cwd: &Path,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("an image needs a description".to_string());
    }
    let mut child = spawn(program, cwd, home, &[])?;
    let stdin = child.stdin.take().ok_or("codex stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("codex stdout unavailable")?;
    // The events go nowhere: this is a tool call, not a run the operator is
    // watching. A generous buffer rather than a drain task — one turn cannot
    // outrun it, and a full channel would only slow the pump.
    let (tx, _rx) = mpsc::channel::<RunEvent>(512);
    let mut session = Session {
        stdin,
        lines: BufReader::new(stdout).lines(),
        tx,
        next_id: 0,
        thread_id: None,
        turn_id: None,
        turns: 0,
        open_items: BTreeMap::new(),
        commands: BTreeMap::new(),
        images: Vec::new(),
    };

    let mut spec = RunSpec::new(
        format!(
            "Use your built-in image generation tool to produce exactly one image, then stop. \
             Do not write any file into the working directory, do not explain what you did, \
             and do not ask a follow-up question. The image: {}",
            prompt.trim()
        ),
        cwd.to_path_buf(),
    );
    // Never escalate. There is nobody to ask: this call already went through
    // Relay's approval sheet as `generate_image`, and a second prompt from
    // inside it would arrive with no context the operator could read.
    spec.permission_mode = Some("dontAsk".to_string());

    let outcome = drive(&mut session, &spec, &CancellationToken::new()).await;
    let _ = child.kill().await;

    match outcome {
        Ok((_, _, _, Some(error))) => Err(error),
        Ok(_) => session
            .images
            .into_iter()
            .next()
            .ok_or_else(|| "Codex finished the turn without saving an image".to_string()),
        Err(Stop::Cancelled) => Err("cancelled".to_string()),
        Err(Stop::Failed(message)) => Err(message),
    }
}

pub async fn plan_usage(program: &str, home: Option<&Path>) -> Result<PlanUsage, String> {
    let raw = ask(program, home, "account/rateLimits/read", json!({})).await?;
    let limits = raw.get("rateLimits").unwrap_or(&Value::Null);
    let window = |key: &str, field: &str| {
        limits
            .get(key)
            .and_then(|w| w.get(field))
            .and_then(Value::as_u64)
    };
    Ok(PlanUsage {
        plan: limits
            .get("planType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        primary_percent: window("primary", "usedPercent").unwrap_or(0) as u32,
        primary_resets_at: window("primary", "resetsAt"),
        primary_window_mins: window("primary", "windowDurationMins"),
        secondary_percent: window("secondary", "usedPercent").unwrap_or(0) as u32,
        secondary_resets_at: window("secondary", "resetsAt"),
        secondary_window_mins: window("secondary", "windowDurationMins"),
        reached: limits
            .get("rateLimitReachedType")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub async fn models(program: &str, home: Option<&Path>) -> Result<Vec<CodexModel>, String> {
    let raw = ask(program, home, "model/list", json!({})).await?;
    let Some(data) = raw.get("data").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    Ok(data
        .iter()
        .filter(|m| m.get("hidden").and_then(Value::as_bool) != Some(true))
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str)?.to_string();
            Some(CodexModel {
                name: m
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                description: m
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                default_effort: m
                    .get("defaultReasoningEffort")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                efforts: m
                    .get("supportedReasoningEfforts")
                    .and_then(Value::as_array)
                    .map(|es| {
                        es.iter()
                            .filter_map(|e| {
                                e.as_str()
                                    .map(str::to_string)
                                    .or_else(|| e.get("effort").and_then(Value::as_str).map(str::to_string))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                id,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mapping that decides how much an agent may do. Written down because
    /// getting it wrong widens permissions silently — the run works, and only
    /// the sandbox knows it was the wrong one.
    #[test]
    fn a_planning_run_cannot_write_and_a_normal_one_asks_before_leaving_its_worktree() {
        assert_eq!(sandbox_and_approval(Some("plan")).0, "read-only");
        assert_eq!(sandbox_and_approval(Some("acceptEdits")), ("workspace-write", "on-request", None));
        assert_eq!(sandbox_and_approval(Some("default")), ("workspace-write", "on-request", None));
        assert_eq!(sandbox_and_approval(Some("auto")).2, Some("auto_review"));
        assert_eq!(sandbox_and_approval(Some("dontAsk")).1, "never");
        assert_eq!(sandbox_and_approval(Some("bypassPermissions")).1, "never");
        // The unknown mode is the one that matters: it must not be the
        // permissive one.
        assert_eq!(sandbox_and_approval(Some("something-new")).0, "workspace-write");
        assert_eq!(sandbox_and_approval(None).0, "workspace-write");
        for mode in ["plan", "auto", "dontAsk", "bypassPermissions", "default", "acceptEdits", "?"] {
            assert_ne!(
                sandbox_and_approval(Some(mode)).0,
                "danger-full-access",
                "no permission mode asks for an unsandboxed run"
            );
        }
    }

    /// Decision #26 for Codex: a run reaches the servers Relay granted and
    /// nothing the operator happens to have in `~/.codex/config.toml`.
    ///
    /// The isolation itself is the `CODEX_HOME` — measured, because the obvious
    /// `-c mcp_servers={}` does not do it. What is checked here is the other
    /// half: that what Relay *adds* on top is spelled the way a `-c` is parsed.
    #[test]
    fn granted_servers_are_written_as_toml_paths_and_broken_ones_are_dropped() {
        let spec = RunSpec::new("x", std::path::PathBuf::from("/tmp"));
        assert!(mcp_overrides(&spec).is_empty(), "nothing granted, nothing added");

        let mut spec = RunSpec::new("x", std::path::PathBuf::from("/tmp"));
        spec.grants.mcp_servers = vec![
            harness_ports::McpGrant {
                name: "docs".into(),
                transport: McpTransport::Stdio {
                    command: "node".into(),
                    args: vec!["server.mjs".into()],
                },
                env: [("TOKEN".to_string(), "abc".to_string())].into_iter().collect(),
                ..Default::default()
            },
            // Nameless, commandless, and dotted names are dropped rather than
            // written as broken config: Codex refuses the whole run on one, and
            // a dot would silently nest the server inside another.
            harness_ports::McpGrant::default(),
            harness_ports::McpGrant {
                name: "a.b".into(),
                transport: McpTransport::Stdio {
                    command: "node".into(),
                    args: vec![],
                },
                ..Default::default()
            },
        ];
        let out = mcp_overrides(&spec);
        assert_eq!(
            out,
            vec![
                r#"mcp_servers.docs.command="node""#.to_string(),
                r#"mcp_servers.docs.args=["server.mjs"]"#.to_string(),
                r#"mcp_servers.docs.env.TOKEN="abc""#.to_string(),
            ],
            "an env table is dotted keys, not JSON — TOML does not read {{\"K\":\"V\"}}"
        );
    }
}
