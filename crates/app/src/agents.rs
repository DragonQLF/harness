//! Agent profiles: the crew the operator configures once and assigns to cards.
//! A profile is turned into a `RunProfile` at the moment a run starts, which is
//! the only place policy meets the engine.

use harness_ports::{Backend, McpGrant, Reviewer, RunProfile, SkillGrant, WorktreeMode};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::settings::Settings;

pub const DIRECTOR_ID: &str = "director";
pub const DEFAULT_WORKER: &str = "builder";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    /// One letter for the avatar.
    pub initial: String,
    /// What it is, in the operator's words.
    pub title: String,
    pub role: String,
    /// The standing instruction handed to every run.
    pub brief: String,
    /// Accent used for this agent across the UI.
    pub tone: String,
    /// Which agent binary carries this profile's runs out — Claude, or Codex
    /// on the ChatGPT plan this machine is logged into. It is not a provider:
    /// see `harness_ports::Backend` for why the two cannot be the same field.
    pub backend: Backend,
    /// The model, in the chosen backend's own names: `opus` | `sonnet` |
    /// `haiku` for Claude, `gpt-5.6-*` for Codex, or None to let the backend
    /// choose. Switching backend leaves this behind, which is why
    /// `normalise_backend` clears a name the new one has never heard of.
    pub model: Option<String>,
    /// Capability names, translated into allowed tools for the run.
    pub permissions: Vec<String>,
    pub budget_usd: Option<f64>,
    pub worktree: WorktreeMode,
    /// Which configured model endpoint this agent runs on. Empty means the
    /// Anthropic login the machine already has.
    pub provider: String,
    pub reviewer: Reviewer,
    /// A paused agent picks up no new work.
    pub paused: bool,
    pub permission_mode: Option<String>,

    // ---- how this profile is used ----
    /// Grouping in the UI: "leadership", "engineering", "growth". Free text.
    pub team: String,
    /// The one project this profile may act on, or `None` for all of them.
    ///
    /// A fence, not a preference. When it is set, board tools default to this
    /// project *and* refuse a call that names another one — which is what
    /// makes "this agent owns that board" a fact rather than a sentence in a
    /// brief. Finished work on a fenced project is also reviewed by whoever
    /// owns it instead of by the Director (see `crate::agents::owner_of`).
    ///
    /// Never set on the Director. He is handed every board's finished work, so
    /// fencing him would leave him unable to act on what he is given — the
    /// failure would look like the review loop being broken rather than like a
    /// setting. `normalise` clears it if a hand-edited file sets it.
    ///
    /// Does nothing on a profile that cannot delegate: those never call a board
    /// tool at all. The Agents screen says so rather than offering a setting
    /// with no effect.
    pub project: Option<String>,
    /// May the operator open a persistent conversation with it?
    pub chat_enabled: bool,
    /// May it be assigned a card to carry out?
    pub tasks_enabled: bool,
    /// How many cards it may work on at once.
    pub max_concurrent: u32,
    /// Named abilities, handed to it as part of the brief. Prose, not
    /// packages: "planning", "typography". Nothing is loaded from these.
    ///
    /// The real thing shares the word and could not share the key: an
    /// `agents.json` in the wild already has `skills: ["planning", "scoping"]`,
    /// and reading that as a list of installed skill packages would have Relay
    /// look for packages the operator never asked for. So the loaded ones live
    /// in `granted_skills` and this stays what it always was.
    pub skills: Vec<String>,
    /// Skills installed for this agent: markdown that enters its prompt,
    /// approved one by one and written to a directory of its own.
    pub granted_skills: Vec<SkillGrant>,
    /// MCP servers this agent may reach. Nothing is inherited: what is not
    /// listed here does not exist for its runs.
    pub mcp_servers: Vec<McpGrant>,
    /// Which profile it answers to.
    pub reports_to: Option<String>,
    /// May it put work on a board and hand it to other agents?
    pub can_delegate: bool,
    /// What a finished piece of work from it should look like.
    pub expected_output: String,
    /// Where it sends anything it cannot resolve.
    pub escalate_to: Option<String>,
    /// How it writes, not what it knows: one of the engine's output styles —
    /// `Concise`, `Explanatory`, `Proactive`, `Learning` — or `None` for the
    /// default. The names are the engine's; Relay ships none of its own.
    ///
    /// It rides in the system prompt, which is read once when a session opens,
    /// so changing it here reaches the *next* conversation rather than the one
    /// on screen. Anything else would be a setting that appears to do nothing.
    pub output_style: Option<String>,
}

impl Default for AgentProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            initial: String::new(),
            title: String::new(),
            role: String::new(),
            brief: String::new(),
            tone: "accent".to_string(),
            backend: Backend::Claude,
            model: None,
            permissions: vec!["Read".into(), "Search".into()],
            budget_usd: None,
            worktree: WorktreeMode::PerCard,
            provider: crate::providers::ANTHROPIC.to_string(),
            reviewer: Reviewer::Director,
            paused: false,
            permission_mode: None,
            team: String::new(),
            // A profile is talkable and workable unless the operator says
            // otherwise, so an older file without these fields behaves exactly
            // as it did before.
            project: None,
            chat_enabled: true,
            tasks_enabled: true,
            max_concurrent: 1,
            skills: Vec::new(),
            granted_skills: Vec::new(),
            mcp_servers: Vec::new(),
            reports_to: None,
            can_delegate: false,
            expected_output: String::new(),
            escalate_to: None,
            // The engine's default. A profile written before this field
            // existed reads as one that never chose, which is what it is.
            output_style: None,
        }
    }
}

/// Every reach an agent can be given, in the crew's own spelling.
///
/// `allowed_tools` below is what each one actually means to a run. This list is
/// the vocabulary: something not in it is not a permission, it is a typo, and a
/// typo silently becomes a tool name the SDK has never heard of.
///
/// The Agents screen reads this same list: `vocabulary::typescript()` writes it
/// into the frontend, so there is no second copy to drift from.
pub const ALL_PERMISSIONS: [&str; 7] =
    ["Read", "Search", "Edit", "Write", "Git", "Web", "Shell"];

impl AgentProfile {
    /// Capabilities the operator ticks map onto concrete tool allowances.
    pub fn allowed_tools(&self) -> Vec<String> {
        let mut tools = Vec::new();
        for permission in &self.permissions {
            match permission.to_ascii_lowercase().as_str() {
                "read" => tools.push("Read".to_string()),
                "search" => {
                    tools.push("Glob".to_string());
                    tools.push("Grep".to_string());
                }
                "edit" => tools.push("Edit".to_string()),
                "write" => tools.push("Write".to_string()),
                "git" => tools.push("Bash(git *)".to_string()),
                "web" => {
                    tools.push("WebSearch".to_string());
                    tools.push("WebFetch".to_string());
                }
                "shell" => tools.push("Bash".to_string()),
                other => tools.push(other.to_string()),
            }
        }
        tools.sort();
        tools.dedup();
        tools
    }

    /// The endpoint this profile's runs actually reach.
    ///
    /// A [`ModelProvider`] is three environment variables that only an agent
    /// speaking the Anthropic Messages protocol reads. Handing one to Codex
    /// would set variables it never looks at and leave the operator believing
    /// their run went somewhere it did not — so a Codex profile resolves to
    /// nothing, whatever endpoint is stored on it.
    ///
    /// Lives here rather than inline in `run_profile` because two other places
    /// build a `RunSpec` straight from a profile, and a rule about where a run
    /// is sent that only holds in one of the three is not a rule.
    pub fn resolved_provider(&self, settings: &Settings) -> Option<harness_ports::ModelProvider> {
        match self.backend {
            Backend::Claude => {
                crate::providers::find(&settings.providers, &self.provider).and_then(|p| p.resolve())
            }
            Backend::Codex => None,
        }
    }

    /// The ceiling in dollars, where dollars are a thing. A ChatGPT plan does
    /// not bill per run, so there is nothing here to cap and a `0.0` would read
    /// as a budget of nothing rather than as no budget at all.
    pub fn resolved_budget(&self) -> Option<f64> {
        self.backend.meters_cost().then_some(self.budget_usd).flatten()
    }

    /// A raiz da aplicação entra aqui porque é onde vivem as pastas de
    /// concessões. Pedi-la torna impossível resolver um perfil e esquecer o
    /// que o agente pode usar: um run sem concessões seria um agente calado
    /// sobre as suas próprias ferramentas, sem nada a dizê-lo.
    pub fn run_profile(&self, settings: &Settings, root: &std::path::Path) -> RunProfile {
        RunProfile {
            agent_id: self.id.clone(),
            backend: self.backend,
            grants: crate::grants::for_profile(root, self),
            provider: self.resolved_provider(settings),
            model: self.model.clone(),
            allowed_tools: Some(self.allowed_tools()),
            permission_mode: Some(
                self.permission_mode
                    .clone()
                    .unwrap_or_else(|| settings.permission_mode.clone()),
            ),
            max_budget_usd: self.resolved_budget(),
            worktree: self.worktree,
            reviewer: if settings.director_reviews_first {
                self.reviewer
            } else if self.reviewer == Reviewer::Director {
                // The operator turned automatic review off; the diff comes to them.
                Reviewer::Human
            } else {
                self.reviewer
            },
            // A hand-edited profile with 0 would otherwise mean "runs nothing".
            max_concurrent: self.max_concurrent.max(1),
            output_style: self.output_style.clone(),
        }
    }

    /// Can the operator open a conversation with this profile right now?
    pub fn can_chat(&self) -> bool {
        self.chat_enabled && !self.paused
    }

    /// Would a card run on this profile edit the operator's own checkout?
    ///
    /// `WorktreeMode::None` is described everywhere as "reads the main
    /// checkout, never writes" — the words are in the Agents screen, in
    /// `vocabulary.rs` and on the enum itself. Nothing enforced them. A `none`
    /// run gets `cwd = repo_root`, and the path guard only refuses writes
    /// *outside* `cwd`, so an agent with Write and no worktree edits the live
    /// tree: no branch, no diff, nothing to approve or reject, and no way back
    /// but git.
    ///
    /// The Director found this from the other side — it wanted to let an agent
    /// write, saw that granting Write to a `none` profile was the wrong shape,
    /// and had no tool to change the worktree. Both halves are fixed: it can
    /// change the worktree now, and this is what makes the sentence true.
    ///
    /// A conversation is deliberately not covered. That is the operator's own
    /// seat, with them watching; it is a card run — unattended, reviewed by a
    /// diff that would not exist — that must not.
    pub fn writes_into_the_live_checkout(&self) -> bool {
        self.worktree == WorktreeMode::None
            && self.permissions.iter().any(|p| {
                matches!(p.trim().to_ascii_lowercase().as_str(), "write" | "edit")
            })
    }

    /// Can this profile be handed a card right now?
    pub fn can_take_work(&self) -> bool {
        self.tasks_enabled && !self.paused
    }

  /// Prompt handed to the agent for a card.
  pub fn prompt_for(&self, card_title: &str, extra: Option<&str>) -> String {
    let mut prompt = String::new();
    prompt.push_str(
      "Relay commits for you — never commit yourself. What it expects from you at the \
       end is one call to the report_work tool: a summary of what changed, and durable \
       notes worth keeping after this card.\n\n",
    );
        if !self.brief.trim().is_empty() {
            prompt.push_str(self.brief.trim());
            prompt.push_str("\n\n");
        }
        if !self.skills.is_empty() {
            prompt.push_str("What you are relied on for: ");
            prompt.push_str(&self.skills.join(", "));
            prompt.push_str("\n\n");
        }
        if !self.expected_output.trim().is_empty() {
            prompt.push_str("What finished work looks like: ");
            prompt.push_str(self.expected_output.trim());
            prompt.push_str("\n\n");
        }
        prompt.push_str("Task: ");
        prompt.push_str(card_title.trim());
        if let Some(extra) = extra {
            if !extra.trim().is_empty() {
                prompt.push_str("\n\nNotes from the operator:\n");
                prompt.push_str(extra.trim());
            }
        }
        prompt
    }
}

/// The crew a fresh install starts with.
pub fn defaults() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: DIRECTOR_ID.into(),
            name: "Director".into(),
            initial: "D".into(),
            title: "Orchestrator".into(),
            role: "Your main assistant: answers, plans, and puts work on boards when you ask for it.".into(),
            // "One run each" was the wrong unit and it showed: it produced a
            // card per file and a review queue nobody could read. The unit is
            // the diff — what the operator can take in at one sitting.
            //
            // The paragraph about time is here because the model has no clock.
            // Days and sprints are borrowed from prose written by people who
            // did, and an estimate in units it cannot observe is a guess
            // wearing a number.
            brief: concat!(
                "Be useful about whatever I bring you. Answer directly when that's what's ",
                "wanted; when I ask for something to be done, put it on a board and hand it ",
                "to the right agent.\n\n",
                "A card is one reviewable diff — not one small change. Several related fixes ",
                "across different files belong on the same card if I can read the diff in one ",
                "sitting. Split only when the work is genuinely independent, when it touches ",
                "something the rest depends on, or when the diff would be too large to review ",
                "at once.\n\n",
                "Don't estimate effort in human time — days, weeks, sprints, \"significant ",
                "work\". You can't measure your own wall-clock time and those units come from ",
                "text written by people. Size by what you can observe: how many files it ",
                "touches, whether it's one pass or several, whether it's reversible, and what ",
                "it could break. If none of that is clear, don't size it."
            )
            .into(),
            tone: "info".into(),
            model: Some("opus".into()),
            // The full set, and the reason is what the operator actually wants
            // from this profile: an assistant that can carry out the small
            // thing itself instead of filing a card for a typo. It is the one
            // profile with no worktree, so what it writes lands in the
            // checkout rather than behind a review — which is the trade being
            // made here, not an oversight. Anything it cannot read in one
            // sitting still belongs on a card, and the brief above says so.
            permissions: ALL_PERMISSIONS.iter().map(|p| p.to_string()).collect(),
            budget_usd: Some(1.5),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Human,
            team: "leadership".into(),
            can_delegate: true,
            // The Director is the one profile that is never handed a card: it
            // plans, delegates and reviews.
            tasks_enabled: false,
            expected_output: "A direct answer, or a plan with the work already on a board.".into(),
            // The one profile the operator reads every word of. `Concise` puts
            // the answer first and stops narrating the walk to it; it does not
            // make the model do less, and it keeps errors and confirmations
            // whole, which are the two things a shorter answer must never
            // shorten. The workers get the default — nobody reads their prose,
            // and their account of themselves is the commit body.
            output_style: Some("Concise".into()),
            ..Default::default()
        },
        AgentProfile {
            id: DEFAULT_WORKER.into(),
            name: "Builder".into(),
            initial: "B".into(),
            title: "Implementer".into(),
            role: "Does the work inside a fresh worktree and commits when it holds up.".into(),
            brief: "Implement one card at a time. Run the tests before committing, keep the diff scoped to the card, and stop and ask rather than widening your own permissions.".into(),
            tone: "accent".into(),
            model: Some("sonnet".into()),
            permissions: vec![
                "Read".into(),
                "Search".into(),
                "Edit".into(),
                "Write".into(),
                "Git".into(),
            ],
            budget_usd: Some(0.75),
            worktree: WorktreeMode::PerCard,
            reviewer: Reviewer::Director,
            ..Default::default()
        },
        AgentProfile {
            id: "scout".into(),
            name: "Scout".into(),
            initial: "S".into(),
            title: "Researcher".into(),
            role: "Reads the repo and answers questions without touching a file.".into(),
            brief: "Answer questions about the codebase with file paths and line numbers. Never edit. Prefer a short answer with citations over a summary.".into(),
            tone: "ok".into(),
            model: Some("haiku".into()),
            permissions: vec!["Read".into(), "Search".into()],
            budget_usd: Some(0.25),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Nobody,
            ..Default::default()
        },
    ]
}

/// Profiles the operator can create from, and nothing more: a template is a
/// starting point in a list, never something Relay turns on by itself. Only
/// the Director is required; every one of these is optional.
pub fn templates() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: DIRECTOR_ID.into(),
            name: "Director".into(),
            title: "Orchestrator".into(),
            role: "Your main assistant: answers, plans, and puts work on boards when you ask for it.".into(),
            brief: "Be useful about whatever I bring you. Answer directly when that is what is wanted; when I ask for something to be done, break it into cards small enough for one run each and hand them to the right agent.".into(),
            tone: "info".into(),
            model: Some("opus".into()),
            output_style: Some("Concise".into()),
            budget_usd: Some(1.5),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Human,
            team: "leadership".into(),
            can_delegate: true,
            tasks_enabled: false,
            expected_output: "A direct answer, or a plan with the work already on a board.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "pm".into(),
            name: "Project PM".into(),
            title: "Project manager".into(),
            role: "Owns the board of one project: what is next, what is blocked, what is done.".into(),
            brief: "Own one project. Keep the board honest: at most two cards ready, nothing vague, every card small enough for one run. Tell me what is blocked and why before I ask.".into(),
            tone: "accent".into(),
            model: Some("sonnet".into()),
            budget_usd: Some(0.5),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Human,
            team: "leadership".into(),
            can_delegate: true,
            tasks_enabled: false,
            skills: vec!["planning".into(), "scoping".into(), "status reporting".into()],
            expected_output: "A short status and the next two cards, named.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "researcher".into(),
            name: "Researcher".into(),
            title: "Researcher".into(),
            role: "Finds out, reads around, and comes back with sources rather than impressions.".into(),
            brief: "Answer with evidence. Cite where each claim came from, a file and line or a source. Say what you could not find rather than filling the gap.".into(),
            tone: "ok".into(),
            model: Some("sonnet".into()),
            permissions: vec!["Read".into(), "Search".into(), "Web".into()],
            budget_usd: Some(0.5),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Nobody,
            team: "research".into(),
            skills: vec!["desk research".into(), "codebase reading".into(), "summarising".into()],
            expected_output: "Findings with sources, and an explicit list of what is still unknown.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "designer".into(),
            name: "Designer".into(),
            title: "Design and frontend".into(),
            role: "Turns intent into interface, and interface into working frontend code.".into(),
            brief: "Design and build the interface. Match the conventions already in the codebase before inventing new ones, keep the diff scoped to the card, and say what you chose and why.".into(),
            tone: "accent".into(),
            model: Some("sonnet".into()),
            permissions: vec!["Read".into(), "Search".into(), "Edit".into(), "Write".into(), "Git".into()],
            budget_usd: Some(1.0),
            team: "product".into(),
            skills: vec!["layout".into(), "typography".into(), "component work".into()],
            expected_output: "A working screen, and a note on what changed visually.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "engineer".into(),
            name: "Senior Engineer".into(),
            title: "Senior engineer".into(),
            role: "Takes the work that needs judgement: architecture, hard bugs, reviews.".into(),
            brief: "Take the work that needs judgement. Read enough of the codebase to be sure, prefer the smallest change that fixes the cause, and run the tests before committing. Push back on a card that is wrong rather than implementing it.".into(),
            tone: "accent".into(),
            model: Some("opus".into()),
            permissions: vec!["Read".into(), "Search".into(), "Edit".into(), "Write".into(), "Git".into()],
            budget_usd: Some(2.0),
            team: "engineering".into(),
            can_delegate: true,
            skills: vec!["architecture".into(), "debugging".into(), "code review".into()],
            expected_output: "A scoped diff with tests, and the reasoning in the commit.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "builder".into(),
            name: "Builder".into(),
            title: "Implementer".into(),
            role: "Does the work inside a fresh worktree and commits when it holds up.".into(),
            brief: "Implement one card at a time. Run the tests before committing, keep the diff scoped to the card, and stop and ask rather than widening your own permissions.".into(),
            tone: "accent".into(),
            model: Some("sonnet".into()),
            permissions: vec!["Read".into(), "Search".into(), "Edit".into(), "Write".into(), "Git".into()],
            budget_usd: Some(0.75),
            team: "engineering".into(),
            expected_output: "One commit that does what the card said.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "editor".into(),
            name: "Editor".into(),
            title: "Editor".into(),
            role: "Writes and cuts prose: copy, docs, anything meant to be read.".into(),
            brief: "Write plainly and cut what is not needed. Keep my voice rather than making everything sound like marketing. Never invent a fact to make a sentence work.".into(),
            tone: "info".into(),
            model: Some("sonnet".into()),
            permissions: vec!["Read".into(), "Search".into(), "Edit".into(), "Write".into(), "Git".into()],
            budget_usd: Some(0.5),
            team: "content".into(),
            skills: vec!["copywriting".into(), "editing".into(), "documentation".into()],
            expected_output: "Text ready to publish, and a note on what you cut.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "seo".into(),
            name: "SEO Specialist".into(),
            title: "SEO".into(),
            role: "Keywords, structure and the technical checks that decide whether a page is found.".into(),
            brief: "Work on what is measurable: titles, structure, internal links, schema, page speed. No keyword stuffing, and no claims about rankings you cannot support.".into(),
            tone: "ok".into(),
            model: Some("sonnet".into()),
            permissions: vec!["Read".into(), "Search".into(), "Edit".into(), "Write".into(), "Web".into(), "Git".into()],
            budget_usd: Some(0.6),
            team: "growth".into(),
            skills: vec!["keyword research".into(), "technical SEO".into(), "schema markup".into()],
            expected_output: "The change, plus what you expect it to move and how you would check.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "ads".into(),
            name: "Ads and Brand Safety".into(),
            title: "Advertising".into(),
            role: "Campaign copy and placement, with the brand rules held to.".into(),
            brief: "Write and review campaign work against the brand rules. Flag anything that could not be defended publicly. Never make a claim we cannot back.".into(),
            tone: "warn".into(),
            model: Some("sonnet".into()),
            permissions: vec!["Read".into(), "Search".into(), "Web".into()],
            budget_usd: Some(0.5),
            worktree: WorktreeMode::None,
            team: "growth".into(),
            skills: vec!["campaign copy".into(), "brand safety review".into()],
            expected_output: "Copy plus an explicit risk note.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "analytics".into(),
            name: "Analytics".into(),
            title: "Analytics".into(),
            role: "Turns numbers into something decidable, and says when they cannot decide it.".into(),
            brief: "Answer with the numbers and their caveats. Show how each figure was arrived at, and say plainly when the data cannot answer the question.".into(),
            tone: "info".into(),
            model: Some("sonnet".into()),
            permissions: vec!["Read".into(), "Search".into()],
            budget_usd: Some(0.5),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Nobody,
            team: "growth".into(),
            skills: vec!["reporting".into(), "funnel analysis".into()],
            expected_output: "The figure, how it was derived, and its caveats.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "finance".into(),
            name: "Finance".into(),
            title: "Finance".into(),
            role: "Costs, pricing and runway, including what this harness is spending.".into(),
            brief: "Be conservative and show the arithmetic. Separate what is known from what is assumed, and name the assumption every time.".into(),
            tone: "ok".into(),
            model: Some("sonnet".into()),
            permissions: vec!["Read".into(), "Search".into()],
            budget_usd: Some(0.4),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Nobody,
            team: "operations".into(),
            skills: vec!["pricing".into(), "budgeting".into(), "cost tracking".into()],
            expected_output: "Numbers with the workings and the assumptions listed.".into(),
            ..Default::default()
        },
        AgentProfile {
            id: "compliance".into(),
            name: "Compliance and Security".into(),
            title: "Compliance and security".into(),
            role: "Reads changes for what could go wrong: permissions, data, obligations.".into(),
            brief: "Look for what could go wrong rather than confirming it looks fine. Be specific about the failure: what input, what result. Never widen a permission to make something work.".into(),
            tone: "bad".into(),
            model: Some("opus".into()),
            permissions: vec!["Read".into(), "Search".into()],
            budget_usd: Some(1.0),
            worktree: WorktreeMode::None,
            reviewer: Reviewer::Human,
            team: "operations".into(),
            skills: vec!["security review".into(), "privacy".into(), "policy".into()],
            expected_output: "Findings with a concrete failure case each, worst first.".into(),
            ..Default::default()
        },
    ]
}

/// One template, ready to be added to the crew under an id nobody is using.
pub fn from_template(template_id: &str, taken: &[String]) -> Option<AgentProfile> {
    let found = templates().into_iter().find(|t| t.id == template_id)?;
    Some(with_free_id(found, taken))
}

/// A new profile the Director asked for, with the crew's conservative defaults.
///
/// Deliberately narrow: a name, what it is for, and where it runs. It cannot
/// grant itself tools — a new agent starts able to read and search, and
/// widening that is the operator's move on the Agents screen. The Director
/// asking for a helper is a reasonable thing to approve; the Director writing
/// itself a shell-capable one is not the same question, and should not arrive
/// wearing the same clothes.
pub fn drafted(
    name: &str,
    title: &str,
    brief: &str,
    taken: &[String],
) -> AgentProfile {
    let name = name.trim();
    with_free_id(
        AgentProfile {
            name: name.to_string(),
            title: title.trim().to_string(),
            brief: brief.trim().to_string(),
            // Everything else is Default: Read and Search only, its own
            // worktree per card, and the Director reading the diff after.
            ..Default::default()
        },
        taken,
    )
}

/// A copy of an existing profile, under its own id.
pub fn duplicate(profile: &AgentProfile, taken: &[String]) -> AgentProfile {
    let mut copy = profile.clone();
    copy.name = format!("{} copy", profile.name.trim());
    copy.id = format!("{}-copy", profile.id.trim());
    with_free_id(copy, taken)
}

fn with_free_id(mut profile: AgentProfile, taken: &[String]) -> AgentProfile {
    let seed = if profile.id.trim().is_empty() {
        profile.name.clone()
    } else {
        profile.id.clone()
    };
    profile.id = crate::projects::unique_id(&seed, taken);
    if profile.initial.trim().is_empty() {
        profile.initial = profile
            .name
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());
    }
    profile
}

/// Fill in anything a hand-edited or older profile file left out.
/// Does this model name belong to this backend?
///
/// The two vocabularies do not overlap and neither side validates the other's:
/// handing `sonnet` to Codex is a thread that starts and then fails on the
/// first turn, and handing `gpt-5.6-terra` to the agent SDK is the same failure
/// mirrored. The check is by prefix rather than by list because a list goes
/// stale the week a model ships — and a *stale* list would refuse a name that
/// works, which is worse than letting an unknown one through.
pub fn model_fits(backend: Backend, model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    if model.is_empty() {
        return true;
    }
    let claude_shaped = model.starts_with("claude")
        || matches!(model.as_str(), "opus" | "sonnet" | "haiku" | "fable")
        || model.starts_with("opus-")
        || model.starts_with("sonnet-")
        || model.starts_with("haiku-");
    let codex_shaped = model.starts_with("gpt-") || model.starts_with("codex-") || model.starts_with("o3");
    match backend {
        // Anything not obviously Codex's is allowed through: a Claude profile
        // may legitimately name a model served by OpenRouter or Ollama, and
        // those names look like nothing in particular.
        Backend::Claude => !codex_shaped,
        Backend::Codex => !claude_shaped,
    }
}

/// A profile that changed backend keeps the model it had, and that model is
/// usually meaningless to the new one. Clearing it means the backend picks its
/// own default, which is the only choice here that cannot fail at run time.
fn normalise_backend(agent: &mut AgentProfile) {
    if let Some(model) = agent.model.clone() {
        if !model_fits(agent.backend, &model) {
            agent.model = None;
        }
    }
    // A stored endpoint on a Codex profile is a setting with no effect. Kept in
    // the file rather than deleted — switching back restores it — but the run
    // profile already refuses to resolve it.
    if agent.backend == Backend::Codex && agent.budget_usd.is_some() {
        agent.budget_usd = None;
    }
}

pub fn normalise(mut agents: Vec<AgentProfile>) -> Vec<AgentProfile> {
    if agents.is_empty() {
        return defaults();
    }
    for agent in &mut agents {
        normalise_backend(agent);
        if agent.id.trim().is_empty() {
            agent.id = crate::paths::sanitize(&agent.name);
        }
        if agent.name.trim().is_empty() {
            agent.name = agent.id.clone();
        }
        if agent.initial.trim().is_empty() {
            agent.initial = agent
                .name
                .chars()
                .next()
                .map(|c| c.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "?".to_string());
        }
    }
    // The brief below was shipped by an older build, so it is ours to correct
    // rather than the operator's to keep: it framed the Director as a board
    // owner and nothing else. A brief the operator has since edited is left
    // exactly as they wrote it.
    for agent in &mut agents {
        if agent.id == DIRECTOR_ID {
            if agent.brief.trim() == "Own the board. Break new intent into cards small enough for one run each, keep at most two cards ready, and review every finished diff against the card before it reaches me." {
                agent.brief = "Be useful about whatever I bring you. Answer directly when that is what is wanted; when I ask for something to be done, break it into cards small enough for one run each and hand them to the right agent.".to_string();
            }
            if agent.role.trim() == "Splits your intent into cards, picks the order and reviews every finished diff before it reaches you." {
                agent.role = "Your main assistant: answers, plans, and puts work on boards when you ask for it.".to_string();
            }
            // Acting on boards IS the Director's job (decision #27). Profiles
            // saved before `can_delegate` existed inherit the struct default
            // of false, which silently muted him into a bystander who then
            // improvises outside the system. The Director ships enabled; an
            // operator who truly wants him blinded removes the profile.
            if !agent.can_delegate {
                agent.can_delegate = true;
            }
            // Saber que a linha de comandos existe é conhecimento, não alcance:
            // ele já tem `Shell` desde sempre, e isto só lhe diz que a tem, o
            // que o guardo recusa, e que a pode passar a outro agente. Entra
            // uma vez e é removível como qualquer outra concessão — mesma
            // disciplina do `can_delegate` acima: vem ligado, e quem não o
            // quiser tira-o no ecrã de Agentes.
            // Ver o doc de `project`: cercá-lo partiria a revisão, que lhe
            // entrega todos os quadros.
            agent.project = None;
            if !agent
                .granted_skills
                .iter()
                .any(|s| s.name == crate::skills::Skill::Cli.id())
            {
                agent
                    .granted_skills
                    .push(crate::skills::grant(crate::skills::Skill::Cli, 0));
            }
        }
    }

    // The Director is not optional: the review loop needs it.
    if !agents.iter().any(|a| a.id == DIRECTOR_ID) {
        let mut with_director = defaults()
            .into_iter()
            .filter(|a| a.id == DIRECTOR_ID)
            .collect::<Vec<_>>();
        with_director.extend(agents);
        return with_director;
    }
    agents
}

/// Who owns a project's board, if anybody does.
///
/// Owning means three things at once, and all three are needed for the review
/// to land: fenced to this project, able to change a board, and able to hold a
/// conversation — because a review runs *as a turn* in that conversation. A
/// profile missing any of them is not an owner, and the work goes to the
/// Director as it always did.
///
/// Ties are broken by id so the answer is the same every time. Two agents
/// fenced to one board is a configuration the operator made; picking
/// arbitrarily would make the same board review differently run to run.
pub fn owner_of<'a>(agents: &'a [AgentProfile], project_id: &str) -> Option<&'a AgentProfile> {
    let mut owners: Vec<&AgentProfile> = agents
        .iter()
        .filter(|a| a.id != DIRECTOR_ID)
        .filter(|a| a.project.as_deref() == Some(project_id))
        .filter(|a| a.can_delegate && a.can_chat())
        .collect();
    owners.sort_by(|a, b| a.id.cmp(&b.id));
    owners.into_iter().next()
}

pub fn find<'a>(agents: &'a [AgentProfile], id: &str) -> Option<&'a AgentProfile> {
    agents
        .iter()
        .find(|a| a.id == id)
        .or_else(|| agents.iter().find(|a| a.id == DEFAULT_WORKER))
        .or_else(|| agents.iter().find(|a| a.id != DIRECTOR_ID))
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_drafted_agent_cannot_arrive_holding_tools() {
        let made = drafted("Scribe", "Note taker", "Writes things down", &[]);
        assert_eq!(made.id, "scribe");
        assert_eq!(made.initial, "S");
        assert_eq!(
            made.permissions,
            vec!["Read".to_string(), "Search".to_string()],
            "a new agent reads and searches; widening that is the operator's move"
        );
        assert_eq!(made.worktree, WorktreeMode::PerCard, "it works in its own checkout");
        assert_eq!(made.reviewer, Reviewer::Director, "and something reads the diff after");
        assert!(!made.paused);

        let second = drafted("Scribe", "", "", &[made.id.clone()]);
        assert_eq!(second.id, "scribe-2", "two of the same name do not collide");
    }

    use super::*;

    #[test]
    fn capabilities_translate_into_tools() {
        let mut agent = AgentProfile {
            permissions: vec!["Read".into(), "Search".into(), "Git".into()],
            ..Default::default()
        };
        assert_eq!(
            agent.allowed_tools(),
            vec![
                "Bash(git *)".to_string(),
                "Glob".to_string(),
                "Grep".to_string(),
                "Read".to_string()
            ]
        );

        agent.permissions = vec!["Read".into(), "Read".into()];
        assert_eq!(agent.allowed_tools(), vec!["Read".to_string()]);
    }

    #[test]
    fn turning_off_automatic_review_sends_diffs_to_the_operator() {
        let agent = AgentProfile {
            reviewer: Reviewer::Director,
            ..Default::default()
        };
        let mut settings = Settings::default();
        assert_eq!(agent.run_profile(&settings, std::path::Path::new("/tmp/relay-test")).reviewer, Reviewer::Director);

        settings.director_reviews_first = false;
        assert_eq!(agent.run_profile(&settings, std::path::Path::new("/tmp/relay-test")).reviewer, Reviewer::Human);

        // An agent nobody reviews stays that way.
        let loose = AgentProfile {
            reviewer: Reviewer::Nobody,
            ..Default::default()
        };
        assert_eq!(loose.run_profile(&settings, std::path::Path::new("/tmp/relay-test")).reviewer, Reviewer::Nobody);
    }

    #[test]
    fn the_prompt_carries_the_brief_and_the_card() {
        let agent = AgentProfile {
            brief: "  Keep it scoped.  ".into(),
            ..Default::default()
        };
        let prompt = agent.prompt_for("Fix the retry", Some("see issue 12"));
        assert!(prompt.contains("Keep it scoped."));
        assert!(prompt.contains("Task: Fix the retry"));
        assert!(prompt.contains("see issue 12"));

        let bare = AgentProfile::default().prompt_for("Do a thing", None);
        assert!(bare.starts_with("Relay commits for you"));
      assert!(bare.ends_with("Task: Do a thing"));
    }

    fn fenced(id: &str, project: Option<&str>, delegate: bool, chat: bool) -> AgentProfile {
        AgentProfile {
            id: id.into(),
            name: id.into(),
            project: project.map(str::to_string),
            can_delegate: delegate,
            chat_enabled: chat,
            ..Default::default()
        }
    }

    /// Dono de um quadro são três coisas ao mesmo tempo, e todas fazem falta:
    /// cercado àquele projecto, capaz de mexer num quadro, e capaz de ter uma
    /// conversa — porque a revisão corre *como um turno* numa. Faltando uma, o
    /// trabalho volta para o Director, que é o que sempre aconteceu.
    #[test]
    fn owning_a_board_takes_all_three_and_never_the_director() {
        let crew = vec![
            fenced("ana", Some("relay"), true, true),
            fenced("mute", Some("relay"), true, false),
            fenced("watcher", Some("relay"), false, true),
            fenced("elsewhere", Some("other"), true, true),
        ];
        assert_eq!(owner_of(&crew, "relay").map(|a| a.id.as_str()), Some("ana"));
        assert!(owner_of(&crew, "nobody-owns-this").is_none());

        for lacking in ["mute", "watcher"] {
            let only = vec![crew.iter().find(|a| a.id == lacking).unwrap().clone()];
            assert!(
                owner_of(&only, "relay").is_none(),
                "{lacking} cannot review, so it does not own the board"
            );
        }

        // Ele recebe todos os quadros; se pudesse ser dono de um, deixava de
        // poder agir sobre o que lhe é entregue dos outros.
        let director = vec![fenced(DIRECTOR_ID, Some("relay"), true, true)];
        assert!(owner_of(&director, "relay").is_none());
    }

    /// Empate resolvido pelo id, e não pela ordem do ficheiro: o mesmo quadro
    /// tem de ser revisto pelo mesmo agente de execução para execução.
    #[test]
    fn two_owners_resolve_the_same_way_every_time() {
        let a = fenced("ana", Some("relay"), true, true);
        let z = fenced("zoe", Some("relay"), true, true);
        assert_eq!(
            owner_of(&[a.clone(), z.clone()], "relay").map(|x| x.id.as_str()),
            owner_of(&[z, a], "relay").map(|x| x.id.as_str()),
        );
    }

    /// Cercar o Director partiria a revisão: continua a receber os quadros
    /// todos e deixaria de poder agir sobre eles. Um ficheiro editado à mão que
    /// o cerque é corrigido ao carregar.
    #[test]
    fn normalise_unfences_the_director() {
        let out = normalise(vec![fenced(DIRECTOR_ID, Some("relay"), true, true)]);
        let director = out.iter().find(|a| a.id == DIRECTOR_ID).unwrap();
        assert_eq!(director.project, None);
    }

    /// E não mexe no cerco de mais ninguém.
    #[test]
    fn normalise_leaves_other_fences_alone() {
        let out = normalise(vec![
            fenced(DIRECTOR_ID, None, true, true),
            fenced("ana", Some("relay"), true, true),
        ]);
        let ana = out.iter().find(|a| a.id == "ana").unwrap();
        assert_eq!(ana.project.as_deref(), Some("relay"));
    }

    #[test]
    fn normalise_fills_gaps_and_keeps_a_director() {
        let normalised = normalise(vec![AgentProfile {
            name: "Odd One".into(),
            ..Default::default()
        }]);
        assert_eq!(normalised.len(), 2);
        assert_eq!(normalised[0].id, DIRECTOR_ID);
        let odd = normalised.iter().find(|a| a.name == "Odd One").unwrap();
        assert_eq!(odd.id, "odd-one");
        assert_eq!(odd.initial, "O");

        assert_eq!(normalise(vec![]).len(), defaults().len());
    }

    #[test]
    fn an_older_profile_file_still_loads_and_behaves_as_before() {
        // Written before any of the new fields existed.
        let raw = r#"[{"id":"builder","name":"Builder","permissions":["Read"]}]"#;
        let loaded: Vec<AgentProfile> = serde_json::from_str(raw).unwrap();
        let agents = normalise(loaded);
        let builder = agents.iter().find(|a| a.id == "builder").unwrap();

        assert!(builder.chat_enabled, "an old profile is talkable by default");
        assert!(builder.tasks_enabled, "and still takes work, as it used to");
        assert_eq!(builder.max_concurrent, 1);
        assert!(!builder.can_delegate);
        assert!(builder.skills.is_empty());
        assert_eq!(builder.team, "");
        assert!(builder.reports_to.is_none());
    }

    #[test]
    fn pausing_stops_both_chat_and_work() {
        let mut agent = AgentProfile::default();
        assert!(agent.can_chat() && agent.can_take_work());

        agent.paused = true;
        assert!(!agent.can_chat());
        assert!(!agent.can_take_work());

        agent.paused = false;
        agent.chat_enabled = false;
        assert!(!agent.can_chat());
        assert!(agent.can_take_work());
    }

    #[test]
    fn templates_are_offered_but_never_installed() {
        let list = templates();
        assert!(list.len() >= 12, "every template in the list");
        for wanted in [
            "director", "pm", "researcher", "designer", "engineer", "builder", "editor", "seo",
            "ads", "analytics", "finance", "compliance",
        ] {
            assert!(list.iter().any(|t| t.id == wanted), "missing template {wanted}");
        }
        // A fresh install gets three profiles, not twelve: templates are a menu.
        assert_eq!(defaults().len(), 3);

        // Every template is complete enough to run as it stands.
        for t in &list {
            assert!(!t.name.trim().is_empty(), "{} has no name", t.id);
            assert!(!t.brief.trim().is_empty(), "{} has no brief", t.id);
            assert!(!t.role.trim().is_empty(), "{} has no role", t.id);
            assert!(!t.paused, "{} would arrive paused", t.id);
        }
    }

    #[test]
    fn the_director_template_delegates_and_takes_no_cards() {
        let director = templates().into_iter().find(|t| t.id == DIRECTOR_ID).unwrap();
        assert!(director.can_delegate);
        assert!(!director.tasks_enabled, "the Director is not handed cards");
        assert!(director.can_chat());
    }

    #[test]
    fn creating_from_a_template_never_collides_with_the_crew() {
        let taken = vec!["builder".to_string(), "director".to_string()];
        let fresh = from_template("builder", &taken).unwrap();
        assert_eq!(fresh.id, "builder-2");
        assert_eq!(fresh.name, "Builder");
        assert!(from_template("nothing-like-this", &taken).is_none());

        let again = from_template("builder", &["builder".into(), "builder-2".into()]).unwrap();
        assert_eq!(again.id, "builder-3");
    }

    #[test]
    fn the_shipped_director_brief_is_generalised_but_an_edited_one_is_kept() {
        // The brief an older build wrote is replaced.
        let stale = normalise(vec![AgentProfile {
            id: DIRECTOR_ID.into(),
            name: "Director".into(),
            brief: "Own the board. Break new intent into cards small enough for one run each, keep at most two cards ready, and review every finished diff against the card before it reaches me.".into(),
            ..Default::default()
        }]);
        let director = stale.iter().find(|a| a.id == DIRECTOR_ID).unwrap();
        assert!(director.brief.starts_with("Be useful about whatever I bring you"));
        assert!(!director.brief.contains("Own the board"));

        // Anything the operator wrote themselves is left alone.
        let mine = normalise(vec![AgentProfile {
            id: DIRECTOR_ID.into(),
            name: "Director".into(),
            brief: "Be blunt with me and skip the pleasantries.".into(),
            ..Default::default()
        }]);
        assert_eq!(
            mine.iter().find(|a| a.id == DIRECTOR_ID).unwrap().brief,
            "Be blunt with me and skip the pleasantries."
        );
    }

    #[test]
    fn duplicating_keeps_the_settings_but_takes_a_new_id() {
        let original = AgentProfile {
            id: "seo".into(),
            name: "SEO Specialist".into(),
            skills: vec!["schema".into()],
            budget_usd: Some(0.6),
            ..Default::default()
        };
        let copy = duplicate(&original, &["seo".to_string()]);
        assert_eq!(copy.id, "seo-copy");
        assert_eq!(copy.name, "SEO Specialist copy");
        assert_eq!(copy.skills, vec!["schema".to_string()]);
        assert_eq!(copy.budget_usd, Some(0.6));
    }

    #[test]
    fn the_prompt_carries_skills_and_expected_output() {
        let agent = AgentProfile {
            brief: "Keep it scoped.".into(),
            skills: vec!["schema markup".into(), "internal links".into()],
            expected_output: "The change and how to check it.".into(),
            ..Default::default()
        };
        let prompt = agent.prompt_for("Fix the titles", None);
        assert!(prompt.contains("Keep it scoped."));
        assert!(prompt.contains("relied on for: schema markup, internal links"));
        assert!(prompt.contains("finished work looks like: The change and how to check it."));
        assert!(prompt.contains("Task: Fix the titles"));
    }

    #[test]
    fn lookup_falls_back_to_the_default_worker() {
        let agents = defaults();
        assert_eq!(find(&agents, "scout").unwrap().id, "scout");
        assert_eq!(find(&agents, "ghost").unwrap().id, DEFAULT_WORKER);
        assert!(find(&[], "anything").is_none());
    }
}

#[cfg(test)]
mod live_checkout_tests {
    use super::*;

    fn profile(worktree: WorktreeMode, permissions: &[&str]) -> AgentProfile {
        AgentProfile {
            id: "a".into(),
            worktree,
            permissions: permissions.iter().map(|p| p.to_string()).collect(),
            ..Default::default()
        }
    }

    /// A frase estava em três sítios e não era verdade em nenhum: um run sem
    /// worktree recebe `cwd = repo_root`, e o guarda de caminhos só recusa
    /// escritas *fora* do `cwd`.
    #[test]
    fn no_worktree_plus_write_is_the_live_tree() {
        assert!(profile(WorktreeMode::None, &["Read", "Write"]).writes_into_the_live_checkout());
        assert!(profile(WorktreeMode::None, &["Edit"]).writes_into_the_live_checkout());
        // A grafia do operador não decide uma propriedade de segurança.
        assert!(profile(WorktreeMode::None, &["write"]).writes_into_the_live_checkout());
    }

    #[test]
    fn a_reader_without_a_worktree_is_exactly_what_it_says_it_is() {
        // O `scout` que existe hoje: lê a checkout viva e não lhe toca.
        assert!(
            !profile(WorktreeMode::None, &["Read", "Search", "Web"])
                .writes_into_the_live_checkout()
        );
    }

    #[test]
    fn a_worktree_is_what_makes_writing_safe() {
        for isolated in [WorktreeMode::PerCard, WorktreeMode::Shared] {
            assert!(
                !profile(isolated, &["Read", "Write", "Edit"]).writes_into_the_live_checkout(),
                "{isolated:?} dá ramo e diff — é para isso que existe",
            );
        }
    }

    /// Nenhum perfil que a Relay instala nasce na combinação recusada, e o
    /// único que a tem hoje (`director`) não pega em cartões. Se um template
    /// novo a criar, isto parte aqui e não na cara do operador.
    #[test]
    fn no_shipped_template_is_born_refused() {
        for template in templates() {
            if template.tasks_enabled {
                assert!(
                    !template.writes_into_the_live_checkout(),
                    "{} pega em cartões e escreveria na checkout viva",
                    template.id,
                );
            }
        }
    }
}
