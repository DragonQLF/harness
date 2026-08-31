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

pub mod queue;

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

/// A finished run the Director is meant to read.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewRequest {
    /// Which board this card is on. Carried because who reads the diff can
    /// depend on it: a project with an owner is reviewed by that owner.
    pub project_id: String,
    pub card_id: String,
    pub run_id: String,
    /// The card's own words. The engine has them; whoever runs the review
    /// would otherwise have to go and ask the board for them again.
    pub title: String,
    /// Where the work is, so the diff can be read against the base branch.
    pub worktree: String,
}

/// Who reads a finished diff when the reviewer is the Director.
///
/// The engine used to answer this itself, by spawning a second, headless
/// Director with no session, no inbox and `dontAsk` — a stranger that read the
/// diff and moved the card while the Director the operator was talking to knew
/// nothing about it. It was the same *role* run by a different *instance*, and
/// from the operator's chair that is a ghost: work vanished from Review with
/// nobody visibly doing anything.
///
/// So the engine stops answering it. It states that a review is wanted and
/// hands over the facts; **where** that review happens — which conversation,
/// whose session, whether anybody is watching — is the shell's business, and
/// the engine still learns nothing about conversations (#19).
///
/// `false` means nobody took it. The card then waits for the operator, which
/// is the honest outcome: better a card sitting in Review than a card moved by
/// something the operator cannot see.
pub type ReviewHook =
    Arc<dyn Fn(ReviewRequest) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;

/// Something a working agent wants the Director to know now.
#[derive(Debug, Clone, Serialize)]
pub struct AgentMessage {
    /// Which board the card is on, so this reaches whoever is in charge of it.
    pub project_id: String,
    pub card_id: String,
    /// Which agent is speaking, so the Director is not left guessing which of
    /// four builders just said this.
    pub agent_id: String,
    pub text: String,
}

/// How a worker reaches the Director mid-run.
///
/// The other half of `EngineHandle::message_run`, and it needs a hook for the
/// same reason the review does: the message ends up in a *conversation*, and
/// the engine does not know conversations exist (#19). It hands over who is
/// speaking and what they said; the shell decides where that lands.
///
/// The reply is what the agent is told — the Director's answer does not come
/// back this way. A worker that stops to wait for one would be blocking on a
/// person, and the whole point of the queue is that neither side waits.
pub type MessageHook =
    Arc<dyn Fn(AgentMessage) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync>;

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
/// [`queue::Queue`], beside it, where cargo can test the ordering.
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

/// Where a run's socket lives, and why it is not simply where we would like it.
///
/// A unix domain socket is bound by writing its path into a `sockaddr_un`,
/// whose `sun_path` is a **fixed 104-byte array** on macOS and BSD (108 on
/// Linux). Past that, `listen()` fails with `EINVAL` — not a truncation, not a
/// warning: the socket is never created at all.
///
/// Relay walked straight into it. The obvious path — app data, plus the run key
/// as the file name — comes to 124 bytes on an ordinary macOS account:
///
/// ```text
/// /Users/<name>/Library/Application Support/com.harness.app/run-sockets/chat-chat_<32 hex>.sock
/// ```
///
/// so **every** run failed with "sidecar never served", whether it was resuming
/// or starting fresh, and the reattachment of decision #111 was dead on macOS
/// from the day it shipped. It presented as lost sessions, which is what sent
/// the search somewhere else entirely.
///
/// Two things fix it and both are needed. The file name becomes a short digest
/// of the run key rather than the key itself, which is what brings an ordinary
/// account back under the limit. And if the path *still* does not fit — a long
/// home directory, an app data root somewhere deep — the socket moves to a
/// short directory of our own under `/tmp` instead of failing. A socket is a
/// meeting point, not a record: nothing is kept in it, so moving it costs
/// nothing. What guards it is unchanged, and is not the location — whoever
/// connects checks the run key before adopting anything (#111).
#[cfg(test)]
mod pricing_tests {
    use super::*;

    /// The 27× over-report, as a rule. A run only has a price when Anthropic
    /// actually billed it; the SDK's figure is otherwise an invoice from the
    /// wrong tables, and a plan has no per-run figure at all.
    #[test]
    fn only_a_run_anthropic_actually_billed_has_a_price() {
        let mut spec = RunSpec::new("x", std::path::PathBuf::from("/tmp"));
        assert!(spec.prices_in_dollars(), "the Claude login, billed per token");

        // The GLM case: the SDK still reports `total_cost_usd`, priced against
        // Anthropic, for a run that went somewhere else entirely.
        spec.provider = Some(ModelProvider {
            base_url: "https://openrouter.ai/api/v1".into(),
            auth_token: "k".into(),
        });
        assert!(!spec.prices_in_dollars());

        // And a plan has no per-run price to report, endpoint or no endpoint.
        let mut codex = RunSpec::new("x", std::path::PathBuf::from("/tmp"));
        codex.backend = Backend::Codex;
        assert!(!codex.prices_in_dollars());
    }
}

pub mod sockets {
    use std::path::{Path, PathBuf};

    /// Usable bytes for a socket path, kept under the smallest real limit with
    /// room for the trailing NUL. macOS is 104 and Linux 108; 100 is under both
    /// and is not worth splitting per platform.
    pub const MAX_PATH: usize = 100;

    /// A stable short name for a run key.
    ///
    /// FNV-1a, 64 bits, written out by hand: this needs to be the same string
    /// on both sides of a restart, and `DefaultHasher` explicitly does not
    /// promise that across builds. A collision would mean two runs meeting on
    /// one socket, which is exactly what the run key check on connect exists to
    /// refuse — so the cost of one is a refusal, not a mixed-up conversation.
    pub fn file_name(run_key: &str) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in run_key.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}.sock")
    }

    /// Somewhere short enough that no home directory can push it over.
    ///
    /// Per-user so two accounts on one machine cannot collide, and created by
    /// the caller with the same permissions app data has.
    pub fn fallback_dir() -> PathBuf {
        #[cfg(unix)]
        let user = std::env::var("UID")
            .ok()
            .or_else(|| std::env::var("USER").ok().map(|u| {
                let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
                for byte in u.as_bytes() {
                    hash ^= *byte as u64;
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
                format!("{:08x}", hash as u32)
            }))
            .unwrap_or_else(|| "0".to_string());
        #[cfg(not(unix))]
        let user = "0".to_string();
        PathBuf::from(format!("/tmp/relay-{user}"))
    }

    /// The socket for this run: in `preferred` when it fits, and somewhere
    /// short when it does not.
    pub fn path_for(preferred: &Path, run_key: &str) -> PathBuf {
        let name = file_name(run_key);
        let wanted = preferred.join(&name);
        if wanted.as_os_str().len() <= MAX_PATH {
            return wanted;
        }
        fallback_dir().join(name)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The bug itself, pinned. This exact path is what an ordinary macOS
        /// account produced, and it is 21 bytes over what `bind` accepts.
        #[test]
        fn the_path_that_broke_every_run_now_fits() {
            let dir = Path::new(
                "/Users/fernandopinto/Library/Application Support/com.harness.app/run-sockets",
            );
            let key = "chat-chat_3624cc14c96b4bb38468c7ae8fc19ca2";
            let old = dir.join(format!("{key}.sock"));
            assert_eq!(old.as_os_str().len(), 124, "what shipped");
            assert!(old.as_os_str().len() > MAX_PATH, "and it could never bind");

            let new = path_for(dir, key);
            assert!(
                new.as_os_str().len() <= MAX_PATH,
                "{} is still {} bytes",
                new.display(),
                new.as_os_str().len()
            );
            assert!(new.starts_with(dir), "and it stays in app data on this account");
        }

        /// A root deep enough that even the short name does not help gets a
        /// socket somewhere else rather than a run that cannot start.
        #[test]
        fn a_root_too_deep_moves_the_socket_instead_of_failing() {
            let deep = PathBuf::from(format!("/Users/{}/run-sockets", "d".repeat(90)));
            let path = path_for(&deep, "chat-x");
            assert!(!path.starts_with(&deep));
            assert!(path.as_os_str().len() <= MAX_PATH);
            assert!(path.to_string_lossy().starts_with("/tmp/relay-"));
        }

        /// The name has to survive a restart: it is how a returning Relay finds
        /// the run it left behind.
        #[test]
        fn the_same_key_always_names_the_same_socket() {
            assert_eq!(file_name("chat-abc"), file_name("chat-abc"));
            assert_ne!(file_name("chat-abc"), file_name("chat-abd"));
            assert_eq!(file_name("chat-abc").len(), 21, "16 hex and an extension");
        }
    }
}

/// Which agent this profile actually runs on.
///
/// Not a provider. A [`ModelProvider`] is an endpoint that speaks the Anthropic
/// Messages protocol, so pointing an agent at Ollama or OpenRouter is three
/// environment variables and nothing else changes. Codex speaks nothing of the
/// sort: it is a second agent binary with its own protocol, its own sandbox and
/// its own login. So it is chosen *here*, next to the worktree mode, and the
/// port is picked from it — which is why `providers.rs` still refuses to list
/// it as an endpoint.
///
/// `Claude` is the default and stays the default: a stored profile written
/// before this existed is a Claude profile, and reading it as anything else
/// would move somebody's crew onto another vendor on upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Claude,
    Codex,
}

impl Default for Backend {
    fn default() -> Self {
        Self::Claude
    }
}

impl Backend {
    /// The name stored in `agents.json` and shown in the UI's own words.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// A backend named by something that was hand-edited or came from an older
    /// build. Unknown falls back to Claude rather than failing the load: a
    /// typo in one field must not cost the operator their whole crew.
    pub fn parse(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "codex" => Self::Codex,
            _ => Self::Claude,
        }
    }

    /// Does this backend bill per token, so a cost and a budget mean something?
    ///
    /// Codex runs on a ChatGPT plan: there is no per-run price to report and no
    /// budget to enforce. Whatever reads this must show its emptiness rather
    /// than a zero, which would read as "this run was free".
    pub fn meters_cost(&self) -> bool {
        matches!(self, Self::Claude)
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
    /// Which agent binary carries this out.
    pub backend: Backend,
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
    /// Which agent binary runs this. The port that reads it is one switch over
    /// several adapters, so a run carries its own answer rather than the app
    /// holding a mode.
    pub backend: Backend,
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
    /// Quem é este trabalho, de forma estável: a conversa ou o cartão a que
    /// pertence. É por ela que o sidecar é encontrado outra vez depois de a
    /// Relay reiniciar — e é ela que se confere ao ligar, para não se apanhar
    /// o run de outro agente que por acaso estivesse no mesmo sítio.
    ///
    /// Vazia quer dizer "sem reatação": o run vive preso a esta Relay, como
    /// sempre viveu.
    pub run_key: Option<String>,
    /// Por onde ia quem se liga. O sidecar reenvia o que veio depois disto e
    /// mais nada.
    ///
    /// Vazio quer dizer "só o que vier a seguir", que é a omissão segura: uma
    /// Relay que reiniciou não sabe por onde ia, e pedir tudo outra vez punha a
    /// conversa no ecrã com as falas repetidas. O atraso não se perde — está na
    /// sessão em disco —, só não volta a ser desenhado.
    pub from_seq: Option<u64>,
    /// Ligar-se a trabalho que já anda, sem mandar nenhum.
    ///
    /// É o arranque: a Relay reabre e vai ver se algum turno continuou sem ela.
    /// Não havendo, isto acaba em silêncio — sem levantar nada e sem escrever
    /// nada na conversa, porque não houve turno nenhum.
    pub attach_only: bool,
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
    /// Is a dollar figure from this run a **price**, or an invoice for work
    /// that was never billed that way?
    ///
    /// The agent SDK reports `total_cost_usd` against Anthropic's tables and
    /// has no idea the run was pointed somewhere else — `ANTHROPIC_BASE_URL` is
    /// what redirects it, and the SDK never sees the bill. So a run through
    /// OpenRouter, Ollama or anything else comes back priced as if Anthropic
    /// had served it. Measured on a real card: Relay reported **$18.26** for
    /// work that cost **$0.67**, a 27× over-report, and the operator was
    /// choosing models on that number.
    ///
    /// Codex is the same problem in its other form: a ChatGPT plan has no
    /// per-run price at all, so any figure would be invented rather than
    /// merely wrong.
    ///
    /// One rule for both: a cost is real only when the run actually billed
    /// against Anthropic. Everywhere else the honest answer is *nothing* —
    /// which the screen shows as an em-dash rather than as `$0.00`, because a
    /// zero claims the work was free (`CLAUDE.md`: nothing on screen is
    /// decorative).
    pub fn prices_in_dollars(&self) -> bool {
        self.backend.meters_cost() && self.provider.is_none()
    }

    pub fn new(prompt: impl Into<String>, cwd: PathBuf) -> Self {
        Self {
            prompt: prompt.into(),
            cwd,
            backend: Backend::default(),
            provider: None,
            model: None,
            allowed_tools: None,
            max_budget_usd: None,
            permission_mode: None,
            approver: None,
            resume_session: None,
            run_key: None,
            from_seq: None,
            attach_only: false,
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

/// Trabalho que o agente deixou a correr por baixo da resposta: um comando
/// posto em fundo, um subagente. Não é um recibo de uma chamada — é uma coisa
/// que continua depois de o turno acabar de falar.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
pub struct BackgroundTask {
    pub task_id: String,
    /// `shell`, `subagent`, … — a etiqueta que o motor lhe dá.
    pub task_type: String,
    pub description: String,
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
    /// A finished stretch of reasoning, kept.
    ///
    /// The relationship to `Thinking` is the one `Text` has to `Delta`: the
    /// slices make the screen feel live and are thrown away, and this is the
    /// record. Without it the reasoning existed only while somebody was
    /// watching — reload the conversation and the model appeared to have
    /// thought nothing, which is the opposite of what it did.
    ///
    /// Sealed per stretch rather than per turn: a turn thinks, acts, and thinks
    /// again, and folding those into one block would put reasoning next to work
    /// it happened after.
    Thought {
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
        /// Lines this call adds and removes, when the call itself says so — an
        /// `Edit` carries both versions of the stretch, so the count is exact
        /// and costs no disk read. Absent, not zero, for a tool that does not
        /// touch lines: the group header then shows no number instead of
        /// claiming nothing changed.
        #[serde(default)]
        added: Option<u32>,
        #[serde(default)]
        removed: Option<u32>,
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
    /// Todo o trabalho de fundo vivo neste momento, sempre que o conjunto muda.
    ///
    /// Nível, não aresta: cada um destes traz o conjunto inteiro e substitui o
    /// anterior, que é o que impede um sinal perdido de deixar um indicador a
    /// girar para sempre. Efémero pela mesma razão que os `Commands` — é
    /// por-processo e nada é emitido ao arrancar, portanto uma linha guardada
    /// só serviria para ressuscitar tarefas que já não existem.
    BackgroundTasks {
        tasks: Vec<BackgroundTask>,
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
                | RunEvent::BackgroundTasks { .. }
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
