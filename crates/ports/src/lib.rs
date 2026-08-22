use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use harness_domain::Event;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub type JsonValue = serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub seq: u64,
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
    fn append_event(&self, e: &Event) -> Result<StoredEvent, StoreError>;
    fn read_all(&self) -> Result<Vec<StoredEvent>, StoreError>;
}

pub trait ClockPort: Send + Sync {
    fn now_millis(&self) -> u64;
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
    fn create_worktree(&self, card_id: &str, base: &str) -> Result<WorktreePath, GitError>;
    fn commit(&self, wt: &WorktreePath, msg: &str, trailers: &Trailers) -> Result<String, GitError>;
    fn commit_wip(&self, wt: &WorktreePath) -> Result<Option<String>, GitError>;
    fn remove_worktree(&self, wt: &WorktreePath) -> Result<(), GitError>;
    fn diff_summary(&self, wt: &WorktreePath, base: &str) -> Result<String, GitError>;
}

pub type Approver = Arc<
    dyn Fn(String, JsonValue) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync,
>;

#[derive(Clone)]
pub struct RunSpec {
    pub prompt: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub max_budget_usd: Option<f64>,
    pub permission_mode: Option<String>,
    pub approver: Option<Approver>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        session_id: String,
    },
    Text {
        text: String,
    },
    ToolUse {
        tool: String,
        summary: String,
    },
    Done {
        session_id: Option<String>,
        cost_usd: Option<f64>,
        result: Option<String>,
    },
    Failed {
        message: String,
    },
    ApprovalRequested {
        request_id: String,
        tool: String,
        summary: String,
    },
}

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Completed {
        session_id: Option<String>,
        cost_usd: Option<f64>,
    },
    Cancelled,
    Failed(String),
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
