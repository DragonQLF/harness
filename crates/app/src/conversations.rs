//! The conversation index: which chats exist, and which Claude session each
//! one continues.
//!
//! Deliberately *not* a transcript store. The words live where every other run
//! transcript already lives — one JSONL per conversation behind `RunLogPort`
//! (decision #11) — and this index only says which file belongs to which chat
//! and which native Claude session to resume. Two copies of the same words is
//! the thing to avoid; the index is the pointer, the run log is the record.
//!
//! Pure: no I/O, no clock. The shell hands in ids and timestamps so the whole
//! module is testable without a window.

use harness_ports::{RunEvent, RunLogLine};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One chat: a Relay conversation bound to a native Claude session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Conversation {
    /// Relay's own id, and the run-log file name. Prefixed `chat_` so it can
    /// never be mistaken for a card id (`c_…`).
    pub id: String,
    /// The Claude Agent SDK session this continues. `None` before the first
    /// answer, or after a resume was refused.
    pub session_id: Option<String>,
    /// Which agent profile is speaking: the Director, or a specialist.
    pub profile_id: String,
    /// Optionally pinned to a project, which decides what code it may read.
    pub project_id: Option<String>,
    pub title: String,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
    pub archived: bool,
    /// A versão do Relay que esta conversa viu da última vez.
    ///
    /// Existe para uma coisa só: uma sessão retomada não sabe que o binário
    /// mudou por baixo dela. O Director percebeu uma actualização porque lhe
    /// apareceram ferramentas novas na lista — deduziu-a pelo efeito, que é a
    /// pior maneira de saber uma coisa que alguém lhe podia ter dito. Guardar
    /// a última vista é o que permite dizê-lo **uma vez**, em vez de repetir a
    /// versão a cada turno num ramo que existe para não repetir nada.
    #[serde(default)]
    pub seen_version: Option<String>,
    /// How many turns the operator has sent. Only for the list.
    pub messages: u32,
    pub cost_usd: f64,
    /// The last resume was refused by the SDK, so the next message starts a new
    /// session. The transcript is still readable; this is what the UI says out
    /// loud instead of pretending the thread continued.
    pub resume_failed: bool,
    /// Is `cost_usd` the whole story?
    ///
    /// False once any turn ran somewhere that does not bill in dollars — a
    /// custom endpoint, or Codex on a plan. The total is then a sum over *some*
    /// of the turns, which is not a spend, and the screen shows an em-dash
    /// rather than a number that reads as complete. See
    /// `RunSpec::prices_in_dollars` for why the figure would otherwise be
    /// wrong rather than merely partial.
    #[serde(default = "yes")]
    pub priced: bool,
}

/// `serde(default)` for a bool that must default to **true**: a conversation
/// written before this field existed ran on the Claude login, which is priced.
fn yes() -> bool {
    true
}

impl Default for Conversation {
    fn default() -> Self {
        Self {
            id: String::new(),
            session_id: None,
            profile_id: crate::agents::DIRECTOR_ID.to_string(),
            project_id: None,
            title: String::new(),
            created_ms: 0,
            updated_ms: 0,
            archived: false,
            seen_version: None,
            messages: 0,
            cost_usd: 0.0,
            resume_failed: false,
            priced: true,
        }
    }
}

/// Placeholder title until the first message names the conversation.
pub const UNTITLED: &str = "New conversation";

impl Conversation {
    pub fn new(
        id: impl Into<String>,
        profile_id: impl Into<String>,
        project_id: Option<String>,
        now_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            profile_id: profile_id.into(),
            project_id,
            title: UNTITLED.to_string(),
            created_ms: now_ms,
            updated_ms: now_ms,
            ..Default::default()
        }
    }

    /// Is this conversation continuing a native session, or starting one?
    pub fn resumes(&self) -> Option<&str> {
        self.session_id.as_deref().filter(|s| !s.trim().is_empty())
    }
}

/// Did the **session** fail, or did something else fail on the way to it?
///
/// The difference decides whether a conversation keeps its `session_id`, and
/// getting it wrong is not symmetric. Dropping a good pointer throws away the
/// model's memory of a thread that is still sitting on disk — fourteen
/// megabytes of it, in the case that prompted this — and nothing puts it back.
/// Keeping a stale one costs a single failed turn and a message saying so.
///
/// So this answers "yes" only when the failure names the session itself, and
/// **defaults to no**. Relay used to do the opposite: any failure during a
/// resume cleared the pointer, which meant a socket that could not bind — a
/// process problem, nothing to do with the session — permanently detached a
/// conversation from its own history. See `harness_ports::sockets` for the bug
/// that made that path fire on every macOS run.
pub fn session_was_lost(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    // The transport, the process, the socket, the binary: all of these fail
    // with the session perfectly intact behind them.
    const NOT_THE_SESSION: [&str; 8] = [
        "never served",
        "socket",
        "sidecar",
        "failed to spawn",
        "no such file",
        "did not answer",
        "refusing to adopt",
        "connection refused",
    ];
    if NOT_THE_SESSION.iter().any(|s| message.contains(s)) {
        return false;
    }
    const THE_SESSION: [&str; 4] = [
        "no conversation found with session",
        "session not found",
        "session does not exist",
        "no such session",
    ];
    THE_SESSION.iter().any(|s| message.contains(s))
}

/// A first message, shortened into something recognisable in a list.
pub fn title_from(message: &str) -> String {
    const MAX: usize = 48;
    let line = message
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .trim();
    if line.is_empty() {
        return UNTITLED.to_string();
    }
    let mut out = String::new();
    for word in line.split_whitespace() {
        if !out.is_empty() && out.chars().count() + 1 + word.chars().count() > MAX {
            out.push('…');
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// What a thread has spent, read back off its own transcript.
///
/// Every field is `Option` where the answer can genuinely be missing, because
/// a transcript written before usage was recorded has no tokens in it and no
/// amount of arithmetic will invent them. `None` is what makes the screen show
/// an em-dash instead of a plausible zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct ConversationTotals {
    /// Every token the thread has spent, prompt and answer and cache alike.
    pub tokens: Option<u64>,
    /// Carried here so the card is one response rather than two. `None` when
    /// the thread ran, wholly or partly, somewhere that does not bill in
    /// dollars — a sum over some of the turns is not a spend, and a number
    /// here would be read as one.
    pub spend_usd: Option<f64>,
    /// Tool invocations over the whole transcript, not just what is on screen.
    pub tool_calls: u32,
    /// What the model was holding on its last turn: the prompt it was handed,
    /// plus what it wrote. This is the context in use, not the running total.
    pub context_tokens: Option<u64>,
    /// The window that context sits in. `None` when the model is unknown.
    pub context_window: Option<u64>,
    /// `context_tokens` as a percentage of `context_window`, 0–100.
    pub context_pct: Option<f64>,
    /// The model the last recorded turn ran on.
    pub model: Option<String>,
}

/// Context window per model, in tokens.
///
/// Deliberately a table and not `catalog.rs`: the catalogue is fetched from
/// models.dev over the network and only cached once the operator has opened
/// the Agents screen, so reading a thread on a fresh install would find
/// nothing. These are the models a Claude login serves. Anything not listed
/// returns `None` and the screen says so rather than dividing by a guess.
///
/// Matched by prefix because the SDK reports dated ids (`claude-opus-4-5-…`),
/// longest-lived families first so `claude-opus-4-5` is not read as
/// `claude-opus-4`.
const CONTEXT_WINDOWS: &[(&str, u64)] = &[
    // The million-token generations.
    ("claude-fable-5", 1_000_000),
    ("claude-mythos-5", 1_000_000),
    ("claude-opus-5", 1_000_000),
    ("claude-opus-4-8", 1_000_000),
    ("claude-opus-4-7", 1_000_000),
    ("claude-opus-4-6", 1_000_000),
    ("claude-sonnet-5", 1_000_000),
    ("claude-sonnet-4-6", 1_000_000),
    // Everything before them held 200k.
    ("claude-haiku-4-5", 200_000),
    ("claude-haiku-3", 200_000),
    ("claude-opus-4-5", 200_000),
    ("claude-opus-4-1", 200_000),
    ("claude-opus-4", 200_000),
    ("claude-sonnet-4-5", 200_000),
    ("claude-sonnet-4", 200_000),
    ("claude-3", 200_000),
    // The bare aliases a profile can carry when the transcript never recorded
    // a resolved id. They point at whatever the login currently serves.
    ("fable", 1_000_000),
    ("opus", 1_000_000),
    ("sonnet", 1_000_000),
    ("haiku", 200_000),
];

/// How much room this model has, or `None` when we have never heard of it.
pub fn context_window(model: &str) -> Option<u64> {
    let id = model.trim().to_lowercase();
    if id.is_empty() {
        return None;
    }
    // The 1M-context beta rides on the id itself, and it outranks the family.
    if id.contains("[1m]") {
        return Some(1_000_000);
    }
    CONTEXT_WINDOWS
        .iter()
        .find(|(needle, _)| id.starts_with(needle))
        .map(|(_, window)| *window)
}

/// Add up a stored transcript. `cost_usd` comes from the index, which is where
/// spend is already accounted; everything else is read from the lines.
pub fn totals(
    lines: &[RunLogLine],
    cost_usd: f64,
    priced: bool,
    fallback_model: Option<&str>,
) -> ConversationTotals {
    let mut spent = 0u64;
    let mut saw_usage = false;
    let mut tool_calls = 0u32;
    let mut context_tokens = None;
    let mut model = None;

    for line in lines {
        match &line.event {
            RunEvent::ToolUse { .. } => tool_calls = tool_calls.saturating_add(1),
            RunEvent::Usage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                model: turn_model,
                subagent,
            } => {
                saw_usage = true;
                let prompt = input_tokens
                    .saturating_add(*cache_read_tokens)
                    .saturating_add(*cache_creation_tokens);
                // Spend is spend, whoever spent it: a subagent's tokens are on
                // the same bill.
                spent = spent
                    .saturating_add(prompt)
                    .saturating_add(*output_tokens);
                // The context is not. A subagent carries its own window, so
                // reading the newest turn without asking whose it was made the
                // gauge report the child's context as this session's — and it
                // reads *lower*, which is the direction that hides a session
                // about to run out of room. The newest turn of *ours* wins.
                if !subagent {
                    context_tokens = Some(prompt.saturating_add(*output_tokens));
                    if let Some(name) = turn_model {
                        model = Some(name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    let model = model.or_else(|| fallback_model.map(str::to_string));
    let window = model.as_deref().and_then(context_window);
    let context_pct = match (context_tokens, window) {
        (Some(used), Some(window)) if window > 0 => {
            Some(((used as f64 / window as f64) * 100.0).min(100.0))
        }
        _ => None,
    };

    ConversationTotals {
        tokens: saw_usage.then_some(spent),
        spend_usd: priced.then_some(cost_usd),
        tool_calls,
        context_tokens,
        context_window: window,
        context_pct,
        model,
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversationIndex {
    pub conversations: Vec<Conversation>,
    /// Reopened on the next start, so the app comes back where it was left.
    pub last_selected: Option<String>,
}

impl ConversationIndex {
    /// Newest first, archived ones left out unless asked for.
    pub fn list(&self, include_archived: bool) -> Vec<Conversation> {
        let mut out: Vec<Conversation> = self
            .conversations
            .iter()
            .filter(|c| include_archived || !c.archived)
            .cloned()
            .collect();
        out.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms));
        out
    }

    pub fn get(&self, id: &str) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut Conversation> {
        self.conversations.iter_mut().find(|c| c.id == id)
    }

    pub fn insert(&mut self, conversation: Conversation) -> Conversation {
        let id = conversation.id.clone();
        self.conversations.retain(|c| c.id != id);
        self.conversations.push(conversation);
        self.last_selected = Some(id.clone());
        self.get(&id).cloned().expect("just inserted")
    }

    /// The chat to reopen on start: the last selected one if it is still here
    /// and not archived, else the most recent for that profile.
    pub fn resume_target(&self, profile_id: &str) -> Option<&Conversation> {
        if let Some(id) = &self.last_selected {
            if let Some(found) = self.get(id).filter(|c| !c.archived) {
                return Some(found);
            }
        }
        self.conversations
            .iter()
            .filter(|c| !c.archived && c.profile_id == profile_id)
            .max_by_key(|c| c.updated_ms)
    }

    pub fn select(&mut self, id: &str) -> Result<(), String> {
        if self.get(id).is_none() {
            return Err(format!("no conversation {id}"));
        }
        self.last_selected = Some(id.to_string());
        Ok(())
    }

    pub fn rename(&mut self, id: &str, title: &str, now_ms: u64) -> Result<Conversation, String> {
        let clean = title.trim();
        if clean.is_empty() {
            return Err("a conversation needs a name".to_string());
        }
        let clean = clean.to_string();
        let entry = self.get_mut(id).ok_or_else(|| format!("no conversation {id}"))?;
        entry.title = clean;
        entry.updated_ms = now_ms;
        Ok(entry.clone())
    }

    pub fn set_archived(
        &mut self,
        id: &str,
        archived: bool,
        now_ms: u64,
    ) -> Result<Conversation, String> {
        let entry = self.get_mut(id).ok_or_else(|| format!("no conversation {id}"))?;
        entry.archived = archived;
        entry.updated_ms = now_ms;
        let cloned = entry.clone();
        // An archived chat should not be the one that reopens on start.
        if archived && self.last_selected.as_deref() == Some(id) {
            self.last_selected = None;
        }
        Ok(cloned)
    }

    /// Forget a conversation. The caller deletes its transcript file; the index
    /// hands back what it removed so the caller knows what to delete.
    pub fn remove(&mut self, id: &str) -> Result<Conversation, String> {
        let at = self
            .conversations
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| format!("no conversation {id}"))?;
        let gone = self.conversations.remove(at);
        if self.last_selected.as_deref() == Some(id) {
            self.last_selected = None;
        }
        Ok(gone)
    }

    /// The operator sent a message: name the conversation if it is still
    /// untitled, and count the turn.
    pub fn record_message(
        &mut self,
        id: &str,
        message: &str,
        now_ms: u64,
    ) -> Result<Conversation, String> {
        let entry = self.get_mut(id).ok_or_else(|| format!("no conversation {id}"))?;
        if entry.title.trim().is_empty() || entry.title == UNTITLED {
            entry.title = title_from(message);
        }
        entry.messages = entry.messages.saturating_add(1);
        entry.updated_ms = now_ms;
        Ok(entry.clone())
    }

    /// Save the session the SDK just handed back. This is the whole point of
    /// the index: without it the next message starts a stranger.
    pub fn record_session(
        &mut self,
        id: &str,
        session_id: &str,
        now_ms: u64,
    ) -> Result<bool, String> {
        let session = session_id.trim();
        if session.is_empty() {
            return Ok(false);
        }
        let entry = self.get_mut(id).ok_or_else(|| format!("no conversation {id}"))?;
        entry.resume_failed = false;
        entry.updated_ms = now_ms;
        if entry.session_id.as_deref() == Some(session) {
            return Ok(false);
        }
        entry.session_id = Some(session.to_string());
        Ok(true)
    }

    /// `priced` is whether this turn *could* have a price, which is not the
    /// same question as whether one arrived. A cancelled Anthropic turn reports
    /// no cost and is still priced work; a turn on a custom endpoint reports a
    /// number that is not a price at all. Only the second makes the total
    /// incomplete, so the caller says which it was rather than this guessing
    /// from a missing figure.
    pub fn record_cost(&mut self, id: &str, cost_usd: Option<f64>, priced: bool, now_ms: u64) {
        if let Some(entry) = self.get_mut(id) {
            entry.cost_usd += cost_usd.unwrap_or(0.0);
            entry.priced &= priced;
            entry.updated_ms = now_ms;
        }
    }

    /// The native session could not be resumed. Drop it rather than retrying
    /// forever, and remember that we did so the UI can say it plainly.
    /// Regista que esta conversa já foi informada desta versão.
    pub fn record_version(&mut self, id: &str, version: &str) {
        if let Some(entry) = self.get_mut(id) {
            entry.seen_version = Some(version.to_string());
        }
    }

    pub fn record_resume_failure(&mut self, id: &str, now_ms: u64) -> Option<Conversation> {
        let entry = self.get_mut(id)?;
        entry.session_id = None;
        entry.resume_failed = true;
        entry.updated_ms = now_ms;
        Some(entry.clone())
    }

    /// Conversations belonging to a project that is being removed lose their
    /// pin, rather than pointing at a project that is gone.
    pub fn unpin_project(&mut self, project_id: &str) {
        for entry in &mut self.conversations {
            if entry.project_id.as_deref() == Some(project_id) {
                entry.project_id = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> ConversationIndex {
        ConversationIndex::default()
    }

    #[test]
    fn a_conversation_remembers_the_session_it_continues() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", None, 10));
        assert!(idx.get("chat_1").unwrap().resumes().is_none());

        assert!(idx.record_session("chat_1", "sess-abc", 20).unwrap());
        assert_eq!(idx.get("chat_1").unwrap().resumes(), Some("sess-abc"));
        // The same session again is not a change worth persisting.
        assert!(!idx.record_session("chat_1", "sess-abc", 30).unwrap());
        // Blank ids are ignored rather than clearing a good session.
        assert!(!idx.record_session("chat_1", "   ", 40).unwrap());
        assert_eq!(idx.get("chat_1").unwrap().resumes(), Some("sess-abc"));
    }

    #[test]
    fn separate_conversations_keep_separate_sessions() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_a", "director", None, 1));
        idx.insert(Conversation::new("chat_b", "researcher", None, 2));
        idx.record_session("chat_a", "sess-a", 3).unwrap();
        idx.record_session("chat_b", "sess-b", 4).unwrap();

        assert_eq!(idx.get("chat_a").unwrap().resumes(), Some("sess-a"));
        assert_eq!(idx.get("chat_b").unwrap().resumes(), Some("sess-b"));
        assert_eq!(idx.get("chat_a").unwrap().profile_id, "director");
        assert_eq!(idx.get("chat_b").unwrap().profile_id, "researcher");
    }

    #[test]
    fn a_new_chat_starts_with_no_session_to_resume() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_old", "director", None, 1));
        idx.record_session("chat_old", "sess-old", 2).unwrap();

        // New Chat is a new row, so there is nothing to resume: the SDK mints a
        // fresh session and the old one is left untouched.
        let fresh = idx.insert(Conversation::new("chat_new", "director", None, 3));
        assert!(fresh.resumes().is_none());
        assert_eq!(idx.get("chat_old").unwrap().resumes(), Some("sess-old"));
        assert_eq!(idx.last_selected.as_deref(), Some("chat_new"));
    }

    #[test]
    fn the_project_pin_and_everything_else_survives_a_round_trip() {
        let mut idx = index();
        idx.insert(Conversation {
            project_id: Some("atlas".into()),
            ..Conversation::new("chat_1", "director", Some("atlas".into()), 5)
        });
        idx.record_session("chat_1", "sess-1", 6).unwrap();
        idx.record_message("chat_1", "how is the board looking?", 7).unwrap();
        idx.record_cost("chat_1", Some(0.25), true, 8);

        let raw = serde_json::to_string(&idx).unwrap();
        let back: ConversationIndex = serde_json::from_str(&raw).unwrap();
        let one = back.get("chat_1").unwrap();
        assert_eq!(one.project_id.as_deref(), Some("atlas"));
        assert_eq!(one.session_id.as_deref(), Some("sess-1"));
        assert_eq!(one.title, "how is the board looking?");
        assert_eq!(one.messages, 1);
        assert_eq!(one.cost_usd, 0.25);
        assert_eq!(back.last_selected.as_deref(), Some("chat_1"));
    }

    #[test]
    fn an_older_index_file_still_loads() {
        // Written before `resume_failed` and `cost_usd` existed.
        let raw = r#"{"conversations":[{"id":"chat_1","profile_id":"director","title":"Old"}]}"#;
        let idx: ConversationIndex = serde_json::from_str(raw).unwrap();
        let one = idx.get("chat_1").unwrap();
        assert_eq!(one.title, "Old");
        assert!(!one.resume_failed);
        assert_eq!(one.cost_usd, 0.0);
        assert!(one.session_id.is_none());
    }

    #[test]
    fn the_first_message_names_the_conversation_and_later_ones_do_not() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", None, 1));
        assert_eq!(idx.get("chat_1").unwrap().title, UNTITLED);

        idx.record_message("chat_1", "  Help me plan a website studio  ", 2).unwrap();
        assert_eq!(idx.get("chat_1").unwrap().title, "Help me plan a website studio");

        idx.record_message("chat_1", "and now something else", 3).unwrap();
        assert_eq!(idx.get("chat_1").unwrap().title, "Help me plan a website studio");
        assert_eq!(idx.get("chat_1").unwrap().messages, 2);

        // A rename sticks, and is not overwritten by the next message.
        idx.rename("chat_1", "  Studio plan  ", 4).unwrap();
        assert_eq!(idx.get("chat_1").unwrap().title, "Studio plan");
        idx.record_message("chat_1", "more", 5).unwrap();
        assert_eq!(idx.get("chat_1").unwrap().title, "Studio plan");
        assert!(idx.rename("chat_1", "   ", 6).is_err());
    }

    #[test]
    fn long_first_messages_are_shortened_on_a_word_boundary() {
        assert_eq!(title_from(""), UNTITLED);
        assert_eq!(title_from("\n\n  hello  "), "hello");
        let long = title_from(
            "I would like to understand how the whole approval pipeline works end to end please",
        );
        assert!(long.ends_with('…'), "{long}");
        assert!(long.chars().count() <= 49, "{long}");
        assert!(long.starts_with("I would like to understand"));
    }

    #[test]
    fn listing_is_newest_first_and_hides_archived() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", None, 100));
        idx.insert(Conversation::new("chat_2", "director", None, 200));
        idx.insert(Conversation::new("chat_3", "director", None, 300));

        let ids: Vec<String> = idx.list(false).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["chat_3", "chat_2", "chat_1"]);

        idx.set_archived("chat_3", true, 400).unwrap();
        let ids: Vec<String> = idx.list(false).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["chat_2", "chat_1"]);
        assert_eq!(idx.list(true).len(), 3);

        // Archiving the selected chat clears the selection.
        assert!(idx.last_selected.is_none());
        idx.set_archived("chat_3", false, 500).unwrap();
        assert_eq!(idx.list(false).len(), 3);
    }

    #[test]
    fn the_last_selected_conversation_is_what_reopens() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", None, 100));
        idx.insert(Conversation::new("chat_2", "director", None, 200));
        idx.select("chat_1").unwrap();
        assert_eq!(idx.resume_target("director").unwrap().id, "chat_1");

        // Selection gone: fall back to the newest chat for that profile.
        idx.remove("chat_1").unwrap();
        assert_eq!(idx.resume_target("director").unwrap().id, "chat_2");
        // A profile with no conversations has nothing to reopen.
        assert!(idx.resume_target("researcher").is_none());
        assert!(idx.select("chat_1").is_err());
    }

    #[test]
    fn deleting_hands_back_the_row_so_its_transcript_can_go_too() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", None, 1));
        let gone = idx.remove("chat_1").unwrap();
        assert_eq!(gone.id, "chat_1");
        assert!(idx.get("chat_1").is_none());
        assert!(idx.remove("chat_1").is_err());
    }

    #[test]
    fn a_refused_resume_is_recorded_rather_than_retried() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", None, 1));
        idx.record_session("chat_1", "sess-gone", 2).unwrap();

        let after = idx.record_resume_failure("chat_1", 3).unwrap();
        assert!(after.resume_failed);
        assert!(after.resumes().is_none(), "a dead session is not offered again");

        // The next answer clears the flag.
        idx.record_session("chat_1", "sess-new", 4).unwrap();
        assert!(!idx.get("chat_1").unwrap().resume_failed);
        assert_eq!(idx.get("chat_1").unwrap().resumes(), Some("sess-new"));
    }

    fn line(ts_ms: u64, event: RunEvent) -> RunLogLine {
        RunLogLine { ts_ms, event }
    }

    fn usage(input: u64, output: u64, cache_read: u64, model: &str) -> RunEvent {
        RunEvent::Usage {
            input_tokens: input,
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_creation_tokens: 0,
            model: Some(model.to_string()),
            subagent: false,
        }
    }

    /// O mesmo turno, gasto por um subagente.
    fn child_usage(input: u64, output: u64, cache_read: u64, model: &str) -> RunEvent {
        match usage(input, output, cache_read, model) {
            RunEvent::Usage { input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, model, .. } =>
                RunEvent::Usage { input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, model, subagent: true },
            other => other,
        }
    }

    #[test]
    fn a_thread_adds_up_every_turn_it_recorded() {
        let lines = vec![
            line(1, RunEvent::UserMessage { text: "go".into() }),
            line(2, usage(1_000, 200, 0, "claude-opus-4-5-20251101")),
            line(3, RunEvent::Text { text: "on it".into(), parent_tool_use_id: None }),
            line(4, usage(1_500, 300, 4_000, "claude-opus-4-5-20251101")),
        ];
        let t = totals(&lines, 0.42, true, None);
        // Both turns, prompt and answer and cache alike.
        assert_eq!(t.tokens, Some(1_200 + 5_800));
        assert_eq!(t.spend_usd, Some(0.42));
        // Context is the last turn only, not the running total.
        assert_eq!(t.context_tokens, Some(5_800));
        assert_eq!(t.context_window, Some(200_000));
        assert!((t.context_pct.unwrap() - 2.9).abs() < 1e-9);
        assert_eq!(t.model.as_deref(), Some("claude-opus-4-5-20251101"));
    }

    #[test]
    fn tool_calls_count_the_whole_transcript_not_the_answered_ones() {
        let lines = vec![
            line(
                1,
                RunEvent::ToolUse {
                    tool: "create_card".into(),
                    summary: String::new(),
                    tool_use_id: Some("t1".into()),
                    parent_tool_use_id: None,
                    added: None,
                    removed: None,
                },
            ),
            line(
                2,
                RunEvent::ToolResult {
                    tool_use_id: "t1".into(),
                    ok: true,
                    summary: String::new(),
                    detail: None,
                },
            ),
            // Still in flight when the log was written: it was still a call.
            line(
                3,
                RunEvent::ToolUse {
                    tool: "read_diff".into(),
                    summary: String::new(),
                    tool_use_id: Some("t2".into()),
                    parent_tool_use_id: None,
                    added: None,
                    removed: None,
                },
            ),
        ];
        assert_eq!(totals(&lines, 0.0, true, None).tool_calls, 2);
    }

    #[test]
    fn a_transcript_written_before_usage_existed_reports_nothing_rather_than_zero() {
        let lines = vec![
            line(1, RunEvent::UserMessage { text: "hello".into() }),
            line(2, RunEvent::Text { text: "hi".into(), parent_tool_use_id: None }),
        ];
        let t = totals(&lines, 1.25, true, Some("sonnet"));
        assert_eq!(t.tokens, None, "no usage lines means no honest total");
        assert_eq!(t.context_tokens, None);
        assert_eq!(t.context_pct, None, "a window with nothing in it is not 0%");
        // Spend is accounted elsewhere, so it survives an empty transcript.
        assert_eq!(t.spend_usd, Some(1.25));
    }

    #[test]
    fn an_unknown_model_leaves_the_context_blank_instead_of_guessing() {
        let lines = vec![line(1, usage(100, 10, 0, "qwen3.5:latest"))];
        let t = totals(&lines, 0.0, true, None);
        assert_eq!(t.tokens, Some(110), "tokens are still countable");
        assert_eq!(t.context_window, None);
        assert_eq!(t.context_pct, None);
    }

    #[test]
    fn the_window_table_reads_dated_ids_and_stops_at_the_right_family() {
        assert_eq!(context_window("claude-opus-4-5-20251101"), Some(200_000));
        assert_eq!(context_window("claude-opus-4-8"), Some(1_000_000));
        assert_eq!(context_window("claude-sonnet-4-5-20250929"), Some(200_000));
        assert_eq!(context_window("claude-sonnet-4-6"), Some(1_000_000));
        assert_eq!(context_window("claude-haiku-4-5-20251001"), Some(200_000));
        // The 1M beta is on the id itself and outranks the family.
        assert_eq!(context_window("claude-sonnet-4-5[1m]"), Some(1_000_000));
        // Bare aliases the login resolves for us.
        assert_eq!(context_window("opus"), Some(1_000_000));
        assert_eq!(context_window("haiku"), Some(200_000));
        assert_eq!(context_window("   "), None);
        assert_eq!(context_window("llama4"), None);
    }

    /// Regressão: o indicador de contexto lia o do subagente.
    ///
    /// Os turnos de um subagente chegam ao mesmo fluxo do run que o mandou
    /// trabalhar, intercalados com os dele. Como o último turno ganhava sem se
    /// perguntar de quem era, uma chamada `Task` fazia a leitura saltar
    /// 34967 → 8544 → 34967 — e o salto é para *baixo*, que é o lado que
    /// esconde uma sessão prestes a ficar sem espaço.
    #[test]
    fn a_subagents_turn_is_spend_but_never_this_sessions_context() {
        let lines = vec![
            line(1, usage(0, 0, 34_967, "claude-opus-5")),
            // O filho, com o contexto pequeno dele.
            line(2, child_usage(0, 0, 8_544, "claude-haiku-4-5")),
            line(3, child_usage(0, 0, 8_544, "claude-haiku-4-5")),
        ];
        let t = totals(&lines, 0.0, true, None);

        assert_eq!(
            t.context_tokens,
            Some(34_967),
            "o contexto é o do último turno *desta* sessão",
        );
        assert_eq!(
            t.tokens,
            Some(34_967 + 8_544 + 8_544),
            "o gasto do subagente está na mesma conta",
        );
        assert_eq!(
            t.model.as_deref(),
            Some("claude-opus-5"),
            "e o modelo é o desta sessão, não o que o filho por acaso usou",
        );
    }

    #[test]
    fn the_last_turn_names_the_model_even_when_earlier_ones_did_not() {
        let lines = vec![
            line(
                1,
                RunEvent::Usage {
                    input_tokens: 10,
                    output_tokens: 1,
                    cache_read_tokens: 0,
                    cache_creation_tokens: 0,
                    model: None,
                    subagent: false,
                },
            ),
            line(2, usage(20, 2, 0, "claude-sonnet-4-6")),
        ];
        let t = totals(&lines, 0.0, true, Some("opus"));
        assert_eq!(t.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(t.tokens, Some(33));
    }

    #[test]
    fn the_profile_model_stands_in_when_the_transcript_never_named_one() {
        let lines = vec![line(
            1,
            RunEvent::Usage {
                input_tokens: 100_000,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                model: None,
                subagent: false,
            },
        )];
        let t = totals(&lines, 0.0, true, Some("haiku"));
        assert_eq!(t.context_window, Some(200_000));
        assert_eq!(t.context_pct, Some(50.0));
    }

    #[test]
    fn removing_a_project_unpins_its_conversations() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", Some("atlas".into()), 1));
        idx.insert(Conversation::new("chat_2", "director", Some("other".into()), 2));
        idx.unpin_project("atlas");
        assert!(idx.get("chat_1").unwrap().project_id.is_none());
        assert_eq!(idx.get("chat_2").unwrap().project_id.as_deref(), Some("other"));
    }
    /// O caso do GLM: a Relay dizia $18.26 por trabalho que custou $0.67,
    /// porque o SDK factura contra as tabelas da Anthropic seja para onde for
    /// que o `ANTHROPIC_BASE_URL` mandou o run. Suprimido o número na origem,
    /// o que fica aqui é a diferença entre um total e um total *parcial* — e um
    /// parcial não se mostra como se fosse a conta toda.
    #[test]
    fn a_thread_that_ran_somewhere_unpriced_reports_no_spend_at_all() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", None, 1));

        // Dois turnos na Anthropic: o total é um total.
        idx.record_cost("chat_1", Some(0.25), true, 2);
        idx.record_cost("chat_1", Some(0.25), true, 3);
        let priced = idx.get("chat_1").unwrap();
        assert!(priced.priced);
        assert_eq!(totals(&[], priced.cost_usd, priced.priced, None).spend_usd, Some(0.5));

        // Um turno num endpoint que não factura em dólares contamina o total:
        // a partir daqui a soma é sobre *alguns* turnos, o que não é um gasto.
        idx.record_cost("chat_1", None, false, 4);
        let mixed = idx.get("chat_1").unwrap();
        assert!(!mixed.priced);
        assert_eq!(mixed.cost_usd, 0.5, "o que se sabia continua somado");
        assert_eq!(
            totals(&[], mixed.cost_usd, mixed.priced, None).spend_usd,
            None,
            "mas o ecrã não pode dizer 0,50 como se fosse a conta toda"
        );

        // E não se recupera: um turno pago a seguir não volta a tornar o total
        // completo, porque o buraco continua lá.
        idx.record_cost("chat_1", Some(1.0), true, 5);
        assert!(!idx.get("chat_1").unwrap().priced);
    }

    /// A regra que decide se uma conversa fica ou não sem a sua memória. É
    /// assimétrica de propósito: um ponteiro bom deitado fora não se recupera,
    /// um ponteiro velho custa um turno.
    #[test]
    fn only_the_session_failing_costs_the_session() {
        // O que partiu de facto: um socket que não liga não diz nada sobre a
        // sessão, que está inteira no disco.
        for transport in [
            "sidecar never served on /Users/x/Library/Application Support/a.sock",
            "failed to spawn 'node': No such file or directory",
            "sidecar did not answer the attach",
            "that socket is serving \"card-1\", not \"chat-2\" — refusing to adopt it",
        ] {
            assert!(!session_was_lost(transport), "{transport}");
        }

        for gone in [
            "No conversation found with session ID f105a514",
            "session not found",
        ] {
            assert!(session_was_lost(gone), "{gone}");
        }

        // O desconhecido guarda-se. É o lado barato de errar.
        assert!(!session_was_lost("something nobody has seen before"));
        assert!(!session_was_lost(""));
    }

}
