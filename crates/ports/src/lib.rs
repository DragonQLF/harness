use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use harness_domain::Event;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub type JsonValue = serde_json::Value;

/// One event as it sits in the log: sequence number, wall clock, payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredEvent {
    pub seq: u64,
    #[serde(default)]
    pub ts_ms: u64,
    pub event: Event,
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Serde(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "store io error: {e}"),
            StoreError::Serde(msg) => write!(f, "store serialization error: {msg}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub trait StorePort: Send + Sync {
    fn append_event(&self, e: &Event, ts_ms: u64) -> Result<StoredEvent, StoreError>;
    fn read_all(&self) -> Result<Vec<StoredEvent>, StoreError>;
    /// Replace the log's contents with exactly these events — the shape
    /// compaction takes: everything so far folded into one snapshot, and the
    /// log restarted from it. Atomic, or not at all. The default refuses, for
    /// stores that cannot rewrite themselves.
    fn compact(&self, keep: &[StoredEvent]) -> Result<(), StoreError> {
        let _ = keep;
        Err(StoreError::Serde("this store cannot compact".to_string()))
    }
}

pub trait ClockPort: Send + Sync {
    fn now_millis(&self) -> u64;
}

/// Append-only transcript of a single run, kept next to the event log so the
/// Sessions view survives a restart.
pub trait RunLogPort: Send + Sync {
    fn append(&self, run_id: &str, line: &RunLogLine) -> Result<(), StoreError>;
    fn read(&self, run_id: &str) -> Result<Vec<RunLogLine>, StoreError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunLogLine {
    pub ts_ms: u64,
    #[serde(flatten)]
    pub event: RunEvent,
}

#[derive(Debug, Clone)]
pub struct WorktreePath(pub PathBuf);

#[derive(Debug, Clone, Default)]
pub struct Trailers(pub Vec<(String, String)>);

#[derive(Debug)]
pub enum GitError {
    Io(String),
    Git(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::Io(msg) => write!(f, "git io error: {msg}"),
            GitError::Git(msg) => write!(f, "git error: {msg}"),
        }
    }
}

impl std::error::Error for GitError {}

impl From<std::io::Error> for GitError {
    fn from(e: std::io::Error) -> Self {
        GitError::Io(e.to_string())
    }
}

pub trait GitPort: Send + Sync {
    /// Where a worktree by this name would live, whether or not it exists yet.
    /// Asking is the only way to reuse one after a restart: `create_worktree`
    /// destroys and recreates, and that deletes the branch its commits are on.
    fn worktree_path(&self, name: &str) -> PathBuf;
    fn create_worktree(&self, card_id: &str, base: &str) -> Result<WorktreePath, GitError>;
    fn commit(&self, wt: &WorktreePath, msg: &str, trailers: &Trailers) -> Result<String, GitError>;
    fn commit_wip(&self, wt: &WorktreePath) -> Result<Option<String>, GitError>;
    fn remove_worktree(&self, wt: &WorktreePath) -> Result<(), GitError>;
    fn diff_summary(&self, wt: &WorktreePath, base: &str) -> Result<String, GitError>;
    /// Lines added / removed on this worktree against `base`.
    fn diff_numstat(&self, wt: &WorktreePath, base: &str) -> Result<(u64, u64), GitError>;
}

/// A tool call the agent cannot make on its own authority.
#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    /// Identifier minted by the agent adapter; the UI answers with this exact id.
    pub request_id: String,
    pub tool: String,
    pub summary: String,
    pub input: JsonValue,
}

pub type Approver =
    Arc<dyn Fn(ApprovalRequest) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// A tool the agent calls that Relay itself implements — moving a card,
/// opening a screen, reading a diff. The adapter forwards the call; the shell
/// carries it out. Like any other tool the agent does not already hold, it goes
/// through the approval flow first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub input: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolReply {
    pub ok: bool,
    /// What the agent is told: the result, or why it could not be done.
    pub text: String,
}

impl ToolReply {
    pub fn ok(text: impl Into<String>) -> Self {
        Self { ok: true, text: text.into() }
    }

    pub fn refused(text: impl Into<String>) -> Self {
        Self { ok: false, text: text.into() }
    }
}

pub type ToolRunner =
    Arc<dyn Fn(ToolCall) -> Pin<Box<dyn Future<Output = ToolReply> + Send>> + Send + Sync>;

/// Something the operator said while a turn was already running.
///
/// The id is minted when it is accepted and comes back when the model has it.
/// Without it the screen would have to guess which of two queued lines the run
/// just took, and a guess is exactly what a "not read yet" mark cannot be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub id: String,
    pub text: String,
}

/// Where a live turn goes looking for what was typed while it worked.
///
/// A port and not a channel because the queue has to answer two questions the
/// adapter cannot: what is still undelivered when the run ends, and whether a
/// message arrived too late to be delivered at all. The implementation is
/// `harness_app::chatqueue`, where cargo can test the ordering.
pub trait InboxPort: Send + Sync {
    /// The next message for the run. Resolves to `None` once the inbox is
    /// closed, and must be cancel-safe: the adapter races it against the
    /// sidecar's own output, so a dropped future may not swallow a message.
    ///
    /// Takes the `Arc` so the future can own it. Borrowing would need a
    /// lifetime on `RunSpec`, and the alternative — handing out a `'static`
    /// reference — is a transmute nobody should have to trust.
    fn next(self: Arc<Self>) -> BoxFut<Option<QueuedMessage>>;
    /// The run says the model has this one now.
    fn mark_read(&self, id: &str);
}

pub type Inbox = Arc<dyn InboxPort>;

/// A skill granted to one agent: the markdown that enters its prompt, kept
/// whole so the operator can read exactly what they are approving.
///
/// Two fields exist only to be shown: `source` says where the text came from,
/// and `body` is the text itself. A skill is prose that steers another agent,
/// so "show the source" is the whole of its safety — there is nothing to
/// sandbox, only something to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct SkillGrant {
    /// Directory name under the agent's plugin, and the name the model sees.
    pub name: String,
    /// One line telling the model when to reach for it.
    pub description: String,
    /// Where this text came from: a URL, a package, or "written here".
    pub source: String,
    /// The SKILL.md body, without the frontmatter Relay writes itself.
    pub body: String,
    #[ts(type = "number")]
    pub added_ms: u64,
}

impl Default for SkillGrant {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            source: String::new(),
            body: String::new(),
            added_ms: 0,
        }
    }
}

/// How Relay reaches a granted MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpTransport {
    /// A process on this machine, spoken to over stdio.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Http {
        url: String,
    },
    Sse {
        url: String,
    },
}

impl Default for McpTransport {
    fn default() -> Self {
        Self::Stdio {
            command: String::new(),
            args: Vec::new(),
        }
    }
}

/// An MCP server granted to one agent.
///
/// `tools` is the declaration, not a discovery: it is what the operator was
/// told this server grants when they approved it. Relay cannot learn the real
/// list without connecting, and connecting runs the server's code — which is
/// the thing the approval exists to gate. So the list is declared, reviewed,
/// and then checked against reality once the run has already been allowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct McpGrant {
    /// Server name; tools arrive as `mcp__<name>__<tool>`.
    pub name: String,
    pub transport: McpTransport,
    /// Environment the server needs. Written by the operator on the Agents
    /// screen, never by a model: a conversation is written to disk, so a key
    /// asked for in one is a key on disk (same reason as `add_endpoint`).
    pub env: std::collections::BTreeMap<String, String>,
    /// The tools this server was declared to grant.
    pub tools: Vec<String>,
    /// Where the declaration came from: a URL, a package, a registry entry.
    pub source: String,
    #[ts(type = "number")]
    pub added_ms: u64,
}

impl Default for McpGrant {
    fn default() -> Self {
        Self {
            name: String::new(),
            transport: McpTransport::default(),
            env: std::collections::BTreeMap::new(),
            tools: Vec::new(),
            source: String::new(),
            added_ms: 0,
        }
    }
}

/// What one run is allowed to load beyond Relay's own wiring: a directory of
/// skills that belongs to this agent alone, and the MCP servers it was granted.
///
/// The isolation is the **directory**, not a filter. The SDK's `skills` option
/// says so itself — "a context filter, not a sandbox: unlisted skills are
/// hidden from the model's listing … but their files remain on disk and are
/// reachable via Read/Bash". So each agent gets its own directory holding
/// exactly what it was granted, and nothing else is ever on the path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grants {
    /// A plugin directory owned by Relay, or `None` when this agent has no
    /// skills. Never a directory of the operator's, and never one inside the
    /// repository being worked on.
    pub skills_dir: Option<PathBuf>,
    pub mcp_servers: Vec<McpGrant>,
}

impl Grants {
    pub fn is_empty(&self) -> bool {
        self.skills_dir.is_none() && self.mcp_servers.is_empty()
    }
}

/// Where an agent does its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeMode {
    /// A fresh branch and worktree per card.
    PerCard,
    /// One long-lived shared branch for the whole project.
    Shared,
    /// Reads the main checkout, never writes.
    None,
}

impl Default for WorktreeMode {
    fn default() -> Self {
        Self::PerCard
    }
}

/// An endpoint that speaks the Anthropic Messages protocol, and the token it
/// wants. Ollama serves one on localhost and OpenRouter serves one over the
/// wire, so "run this agent on a local model" and "run it on someone else's
/// model" are the same three environment variables to whatever we spawn — the
/// agent SDK and the CLI both read them.
///
/// The empty `api_key` is not an oversight. A key left in the environment wins
/// over the base URL, so a machine that has ever exported ANTHROPIC_API_KEY
/// would silently keep talking to Anthropic while the operator believed they
/// were running locally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ModelProvider {
    pub base_url: String,
    pub auth_token: String,
}

impl ModelProvider {
    /// The environment a spawned agent needs to reach this provider.
    pub fn env(&self) -> [(&'static str, String); 3] {
        [
            ("ANTHROPIC_BASE_URL", self.base_url.clone()),
            ("ANTHROPIC_AUTH_TOKEN", self.auth_token.clone()),
            ("ANTHROPIC_API_KEY", String::new()),
        ]
    }
}

/// Everything the engine needs to know about the agent it is about to run.
/// Resolved from the stored agent profile before the run starts, so the engine
/// itself carries no policy.
#[derive(Debug, Clone)]
pub struct RunProfile {
    pub agent_id: String,
    /// Where this agent's model actually lives. `None` is the ordinary case:
    /// the Claude subscription or API key already in the environment.
    pub provider: Option<ModelProvider>,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub permission_mode: Option<String>,
    pub max_budget_usd: Option<f64>,
    pub worktree: WorktreeMode,
    /// Who reads the diff when the run finishes.
    pub reviewer: Reviewer,
    /// How many cards this agent may work on at once. The engine refuses a
    /// start that would exceed it.
    pub max_concurrent: u32,
    /// The skills and MCP servers this agent was granted. They travel with the
    /// profile because a run is the only place that knows which agent it is:
    /// the engine holds one shared port for every run, so a port that carried
    /// them would hand the same ones to everybody.
    pub grants: Grants,
    /// The engine output style this agent's runs answer in. See
    /// `RunSpec::output_style` for why it only binds a fresh session.
    pub output_style: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Reviewer {
    /// The Director reads the diff and approves or sends it back.
    Director,
    /// Every finished run lands in the human review queue.
    Human,
    /// Finished runs go straight to Done.
    Nobody,
}

impl Default for Reviewer {
    fn default() -> Self {
        Self::Director
    }
}

#[derive(Clone)]
pub struct RunSpec {
    pub prompt: String,
    /// Set when this run should talk to something other than Anthropic.
    pub provider: Option<ModelProvider>,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub max_budget_usd: Option<f64>,
    pub permission_mode: Option<String>,
    pub approver: Option<Approver>,
    /// Session to resume instead of starting fresh.
    pub resume_session: Option<String>,
    /// Relay's own tools, when this run is allowed to act on the app.
    pub tools: Option<ToolRunner>,
    /// What the operator says *during* this run. Absent for a card run: only a
    /// conversation has somebody typing at it while it works.
    pub inbox: Option<Inbox>,
    /// Room for the model to reason before answering. Without it there is no
    /// thinking to stream.
    pub thinking_tokens: Option<u32>,
    /// May this run spawn subagents of its own? A run's children may never
    /// spawn: fan-out is capped at one level, enforced in the sidecar's
    /// `canUseTool`.
    pub subagents: bool,
    /// Does this run carry the `report_work` tool — the agent's account of
    /// its own work? The summary becomes the commit body; the engine still
    /// owns the commit. Absent, nothing breaks: the generic body is used and
    /// a Notice says the agent did not report.
    pub report_work: bool,
    /// What this particular run was granted. Empty is the ordinary case and
    /// means "whatever the port was built with" — which is how a conversation
    /// still works, since it builds a port per profile.
    pub grants: Grants,
    /// Which of the engine's output styles this run answers in — `Concise`,
    /// `Explanatory`, and the rest of what `available_output_styles` lists.
    /// `None` is the engine's default.
    ///
    /// It is part of the system prompt, which is read once when the session
    /// opens. A resumed run therefore answers in the style it was born with,
    /// whatever is set here — so this only ever decides how a *new* session
    /// sounds.
    pub output_style: Option<String>,
    /// How hard to think on this turn: `low`, `medium`, `high`, `xhigh`,
    /// `max`. Unlike the style, this binds the request rather than the system
    /// prompt — so the operator can change it mid-conversation and the very
    /// next message goes out at the new level, with no new session.
    ///
    /// It rides here, per run, precisely so it can be changed at any point.
    /// What it is *set to* persists in the composer until changed; what this
    /// field carries is only what was chosen when this one turn was sent.
    ///
    /// Not every model takes every level; the engine downgrades silently to
    /// what the chosen model supports. Relay does not second-guess that — a
    /// list narrowed here would go stale the moment a model gains a level.
    pub effort: Option<String>,
}

impl RunSpec {
    pub fn new(prompt: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            prompt: prompt.into(),
            cwd,
            provider: None,
            model: None,
            allowed_tools: None,
            max_budget_usd: None,
            permission_mode: None,
            approver: None,
            resume_session: None,
            tools: None,
            inbox: None,
            thinking_tokens: None,
            subagents: false,
            report_work: false,
            grants: Grants::default(),
            output_style: None,
            effort: None,
        }
    }
}

/// One thing `/` can mean in a session: the engine's own commands and whatever
/// the granted skills brought with them. Relay never writes this list — it is
/// asked for per session, because what a skill offers depends on what that
/// agent was granted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct SlashCommand {
    /// Without the leading slash.
    pub name: String,
    pub description: String,
    /// What comes after the name, when the command takes anything.
    #[serde(default)]
    pub argument_hint: Option<String>,
    /// Other names that land on this same command.
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        session_id: String,
    },
    /// What the operator said. Only conversations have these: it is what makes
    /// a chat transcript readable on its own, without a second store holding
    /// the other half of the exchange.
    UserMessage {
        text: String,
    },
    /// The operator said something while the turn was still running. Written
    /// the moment it is accepted, and *only* that: it says the message exists
    /// and is waiting, never that the model has seen it. A transcript that
    /// stops here is one where it never arrived — which is what an operator
    /// who closed Relay mid-turn needs to be able to read.
    UserQueued {
        queue_id: String,
        text: String,
    },
    /// The same message, handed to the run. From here it is an ordinary thing
    /// the operator said, and the two lines fold into one bubble.
    UserRead {
        queue_id: String,
    },
    Text {
        text: String,
    },
    /// A slice of the answer as it is written. Ephemeral: shown live, never
    /// written to the run log — the `Text` event that follows is the record.
    Delta {
        text: String,
    },
    /// A slice of the model's reasoning, same rules as `Delta`.
    Thinking {
        text: String,
    },
    /// Interim progress while the run is alive: how many model turns have
    /// happened so far. Ephemeral like deltas - the total lands on `Done`.
    Turns {
        count: u32,
    },
    /// A tool call the agent does not hold by default.
    ToolUse {
        tool: String,
        summary: String,
        /// Links the call to its result: the id the model minted, and the
        /// parent call when this one runs inside a subagent. Absent in logs
        /// written before results were tracked.
        #[serde(default)]
        tool_use_id: Option<String>,
        #[serde(default)]
        parent_tool_use_id: Option<String>,
    },
    /// What actually happened, matched to the call by id. Persisted like the
    /// ToolUse — a failed Bash that reads as a clean one is #41's shape again,
    /// now in the transcript layer.
    ToolResult {
        tool_use_id: String,
        ok: bool,
        summary: String,
        /// Full output, capped: the transcript keeps it, the UI shows it on
        /// expand instead of dumping it inline (#28's reason).
        #[serde(default)]
        detail: Option<String>,
    },
    /// What one model turn spent. Written to the log rather than shown live,
    /// because the thread's accounting is read back off disk: a token count
    /// that only ever existed in memory is one the next read cannot have.
    ///
    /// `input_tokens` is the prompt the model was handed *this turn*, so the
    /// last one of these is also how full its context window is — which is why
    /// the model that spent them travels with the numbers.
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        #[serde(default)]
        cache_read_tokens: u64,
        #[serde(default)]
        cache_creation_tokens: u64,
        #[serde(default)]
        model: Option<String>,
    },
    Done {
        session_id: Option<String>,
        cost_usd: Option<f64>,
        #[serde(default)]
        turns: Option<u32>,
        result: Option<String>,
        /// Set when the run ended in an error result rather than an answer. It
        /// arrives on the same message as a success, so without this a failed
        /// run reads as a completed one.
        #[serde(default)]
        error: Option<String>,
    },
    Failed {
        message: String,
    },
    ApprovalRequested {
        request_id: String,
        tool: String,
        summary: String,
    },
    ApprovalAnswered {
        request_id: String,
        allow: bool,
    },
    /// A note from the harness itself rather than the model (verdicts, cancels).
    Notice {
        text: String,
    },
    /// What `/` can mean, for this session. Ephemeral: it belongs to the live
    /// session and is asked for again on the next one, so writing it to the
    /// transcript would only preserve an answer that has since expired.
    Commands {
        commands: Vec<SlashCommand>,
    },
    /// A command the engine answered by itself, with no model turn behind it —
    /// `/usage`, `/context`. Kept, because for those it is the whole reply:
    /// dropping it leaves a message in the thread that was never answered.
    LocalOutput {
        text: String,
    },
}

impl RunEvent {
    /// Deltas exist to make the UI feel live; keeping thousands of them in the
    /// transcript would bury the record they add up to.
    pub fn is_ephemeral(&self) -> bool {
        matches!(
            self,
            RunEvent::Delta { .. }
                | RunEvent::Thinking { .. }
                | RunEvent::Turns { .. }
                | RunEvent::Commands { .. }
        )
    }
}

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Completed {
        session_id: Option<String>,
        cost_usd: Option<f64>,
        turns: Option<u32>,
    },
    Cancelled,
    /// A failure still spent money and turns — a budget cut after seventeen
    /// rounds is real spend. Carried so the card sums it either way (#41's
    /// cousin: a cost that vanishes because the outcome shape changed).
    Failed {
        message: String,
        cost_usd: Option<f64>,
        turns: Option<u32>,
    },
}

impl RunOutcome {
    pub fn completed(session_id: Option<String>, cost_usd: Option<f64>) -> Self {
        Self::Completed {
            session_id,
            cost_usd,
            turns: None,
        }
    }
}

type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub trait AgentPort: Send + Sync {
    fn run(
        &self,
        spec: RunSpec,
        tx: mpsc::Sender<RunEvent>,
        cancel: CancellationToken,
    ) -> BoxFut<Result<RunOutcome, String>>;
}
