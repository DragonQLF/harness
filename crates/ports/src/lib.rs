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

/// A tool the agent calls that Harness itself implements — moving a card,
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

/// Everything the engine needs to know about the agent it is about to run.
/// Resolved from the stored agent profile before the run starts, so the engine
/// itself carries no policy.
#[derive(Debug, Clone)]
pub struct RunProfile {
    pub agent_id: String,
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
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub max_budget_usd: Option<f64>,
    pub permission_mode: Option<String>,
    pub approver: Option<Approver>,
    /// Session to resume instead of starting fresh.
    pub resume_session: Option<String>,
    /// Harness's own tools, when this run is allowed to act on the app.
    pub tools: Option<ToolRunner>,
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
}

impl RunSpec {
    pub fn new(prompt: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            prompt: prompt.into(),
            cwd,
            model: None,
            allowed_tools: None,
            max_budget_usd: None,
            permission_mode: None,
            approver: None,
            resume_session: None,
            tools: None,
            thinking_tokens: None,
            subagents: false,
            report_work: false,
        }
    }
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
}

impl RunEvent {
    /// Deltas exist to make the UI feel live; keeping thousands of them in the
    /// transcript would bury the record they add up to.
    pub fn is_ephemeral(&self) -> bool {
        matches!(self, RunEvent::Delta { .. } | RunEvent::Thinking { .. })
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
    Failed(String),
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
