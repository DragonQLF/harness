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
    pub created_ms: u64,
    pub updated_ms: u64,
    pub archived: bool,
    /// How many turns the operator has sent. Only for the list.
    pub messages: u32,
    pub cost_usd: f64,
    /// The last resume was refused by the SDK, so the next message starts a new
    /// session. The transcript is still readable; this is what the UI says out
    /// loud instead of pretending the thread continued.
    pub resume_failed: bool,
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
            messages: 0,
            cost_usd: 0.0,
            resume_failed: false,
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

    pub fn record_cost(&mut self, id: &str, cost_usd: Option<f64>, now_ms: u64) {
        if let Some(entry) = self.get_mut(id) {
            entry.cost_usd += cost_usd.unwrap_or(0.0);
            entry.updated_ms = now_ms;
        }
    }

    /// The native session could not be resumed. Drop it rather than retrying
    /// forever, and remember that we did so the UI can say it plainly.
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
        idx.record_cost("chat_1", Some(0.25), 8);

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

    #[test]
    fn removing_a_project_unpins_its_conversations() {
        let mut idx = index();
        idx.insert(Conversation::new("chat_1", "director", Some("atlas".into()), 1));
        idx.insert(Conversation::new("chat_2", "director", Some("other".into()), 2));
        idx.unpin_project("atlas");
        assert!(idx.get("chat_1").unwrap().project_id.is_none());
        assert_eq!(idx.get("chat_2").unwrap().project_id.as_deref(), Some("other"));
    }
}
