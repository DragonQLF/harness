//! Skills, MCP servers and tools granted to one agent — the declaration, its
//! rules, and how it reaches disk.
//!
//! ## Why a declaration and not a command
//!
//! The operator says "install this skill on the Designer". The model searches,
//! reads the documentation, and produces a **declaration**: a name, a source, a
//! target agent, and what it brings. It never produces a script and never runs
//! one. Relay installs from the declaration.
//!
//! The reason is concrete rather than ceremonial: the model reads web pages to
//! find out how something installs, and a page that says "also add this server"
//! becomes an instruction the moment its output is executed. When what comes
//! out is a declaration the operator reviews, the injection is visible on the
//! approval sheet. When what comes out is a command, it is not.
//!
//! It is the pattern this codebase already uses three times: `report_work`
//! tells and the engine commits; the Analyst interprets and the code counts;
//! the Curator promotes mechanically and the model judges.
//!
//! ## The isolation stays, a list is added on top
//!
//! `settingSources: []` and `strictMcpConfig: true` are not removed — without
//! them the operator's own account connectors load and the model starts talking
//! about authorising Linear or Notion (decision #26). Each agent carries only
//! what it was given; there is still no inheritance.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use harness_ports::{Grants, McpGrant, McpTransport, SkillGrant};

use crate::agents::AgentProfile;

/// The plugin name every Relay-owned skill directory carries. Constant on
/// purpose: only one such directory is ever loaded into a run, so the names the
/// model sees (`relay:<skill>`) do not shift with the agent.
pub const PLUGIN_NAME: &str = "relay";

/// Longest a skill body may be. A skill is prose that enters another agent's
/// prompt on every turn; an unbounded one is an unbounded bill.
pub const MAX_BODY: usize = 20_000;

/// A name that is safe as a single path segment and legal as a skill name.
/// Deliberately narrower than `paths::sanitize`, which repairs; this refuses,
/// because a repaired name is a name the operator did not approve.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// What a grant would give, in one line, for the approval sheet.
///
/// The sheet shows the **declaration, not the intention**: "Install MCP X on
/// the Designer — grants the tools A, B, C", never "the Director wants to
/// install something".
pub fn describe_skill(agent: &str, grant: &SkillGrant) -> String {
    format!(
        "install the skill \"{}\" on {agent} — from {}; {} characters of instructions enter its prompt",
        grant.name,
        if grant.source.trim().is_empty() {
            "an unnamed source"
        } else {
            grant.source.trim()
        },
        grant.body.chars().count(),
    )
}

pub fn describe_mcp(agent: &str, grant: &McpGrant) -> String {
    let reach = match &grant.transport {
        McpTransport::Stdio { command, args } => {
            let mut line = command.clone();
            if !args.is_empty() {
                line.push(' ');
                line.push_str(&args.join(" "));
            }
            format!("runs `{}` on this machine", line.trim())
        }
        McpTransport::Http { url } => format!("talks to {url}"),
        McpTransport::Sse { url } => format!("talks to {url}"),
    };
    format!(
        "add the MCP server \"{}\" to {agent} — {reach}; grants the tools {}",
        grant.name,
        if grant.tools.is_empty() {
            "(none declared)".to_string()
        } else {
            grant.tools.join(", ")
        },
    )
}

/// Why a grant was refused outright, before anyone was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The caller is granting itself privilege. Not an approval the operator
    /// can give: an agent that hands itself tools has stopped having limits.
    SelfElevation { kind: &'static str },
    BadName(String),
    TooLong,
    Empty(&'static str),
    UnknownAgent(String),
    /// `harness` is Relay's own in-process server; a grant by that name would
    /// shadow the board tools the Director answers with.
    ReservedName,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::SelfElevation { kind } => write!(
                f,
                "refused: you cannot give yourself {kind}. Granting your own privilege is not \
                 an approval the operator can be asked for — it would leave nothing to approve \
                 next time. Ask them to do it on the Agents screen, or name a different agent."
            ),
            Refusal::BadName(name) => write!(
                f,
                "\"{name}\" is not a usable name: lowercase letters, digits and hyphens only, \
                 starting with a letter or digit, at most 64 characters"
            ),
            Refusal::TooLong => write!(
                f,
                "the instructions are longer than {MAX_BODY} characters; a skill enters the \
                 agent's prompt on every turn, so shorten it or point at a document instead"
            ),
            Refusal::Empty(what) => write!(f, "the declaration needs {what}"),
            Refusal::UnknownAgent(id) => write!(f, "there is no agent called {id}"),
            Refusal::ReservedName => write!(
                f,
                "\"harness\" is Relay's own tool server; a grant by that name would hide the \
                 board tools. Pick another name."
            ),
        }
    }
}

/// The one rule that is a refusal rather than a hard approval: nobody grants
/// themselves privilege.
///
/// Skills are exempt. A skill is markdown that enters a prompt — the same class
/// of thing as `record_decision`, which agents already write for themselves —
/// and it still passes the operator's approval. Tools and MCP servers are not:
/// an MCP server is arbitrary code holding that agent's permissions, so
/// granting oneself a server is granting oneself tools with an extra step.
pub fn refuse_self_elevation(caller: &str, target: &str, kind: &'static str) -> Option<Refusal> {
    if caller.eq_ignore_ascii_case(target) {
        Some(Refusal::SelfElevation { kind })
    } else {
        None
    }
}

/// Which of the grant tools are privilege, and therefore may never be aimed at
/// whoever is calling them.
///
/// It lives here rather than in the tool handler so the policy is one list that
/// can be read and tested, instead of a guard remembered at each call site — a
/// tool added later without the guard is the failure this shape prevents.
pub fn self_elevation_guard(tool: &str, caller: &str, target: &str) -> Option<Refusal> {
    let kind = match tool {
        "grant_agent_tools" => "tools",
        // An MCP server is arbitrary code holding that agent's permissions:
        // granting oneself a server is granting oneself tools with one extra
        // step, so the same refusal applies.
        "add_mcp_server" => "an MCP server",
        // Skills are not privilege. A skill is markdown that enters a prompt —
        // the same class of thing as `record_decision`, which agents already
        // write for themselves — and it still passes the operator's approval.
        _ => return None,
    };
    refuse_self_elevation(caller, target, kind)
}

pub fn check_skill(grant: &SkillGrant) -> Result<(), Refusal> {
    if !valid_name(&grant.name) {
        return Err(Refusal::BadName(grant.name.clone()));
    }
    if grant.description.trim().is_empty() {
        return Err(Refusal::Empty("a description saying when to use the skill"));
    }
    if grant.body.trim().is_empty() {
        return Err(Refusal::Empty("the instructions themselves"));
    }
    if grant.body.chars().count() > MAX_BODY {
        return Err(Refusal::TooLong);
    }
    Ok(())
}

pub fn check_mcp(grant: &McpGrant) -> Result<(), Refusal> {
    if !valid_name(&grant.name) {
        return Err(Refusal::BadName(grant.name.clone()));
    }
    if grant.name == "harness" {
        return Err(Refusal::ReservedName);
    }
    match &grant.transport {
        McpTransport::Stdio { command, .. } if command.trim().is_empty() => {
            Err(Refusal::Empty("a command to run"))
        }
        McpTransport::Http { url } | McpTransport::Sse { url } if url.trim().is_empty() => {
            Err(Refusal::Empty("a URL to talk to"))
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// On disk
// ---------------------------------------------------------------------------

/// The directory holding one agent's granted skills, laid out as a local
/// plugin the Agent SDK can be pointed at:
///
/// ```text
/// <appdata>/skills/<agent>/
///   .claude-plugin/plugin.json
///   skills/<name>/SKILL.md
/// ```
pub fn agent_skills_dir(root: &Path, agent_id: &str) -> PathBuf {
    root.join("skills").join(crate::paths::sanitize(agent_id))
}

/// Write one agent's granted skills to disk, and remove anything there that is
/// no longer granted.
///
/// Rewriting the whole directory from the profile — rather than adding and
/// deleting as calls arrive — is what makes `agents.json` the single truth. A
/// skill the operator removed on the Agents screen disappears from disk on the
/// next write, with no second bookkeeping to drift.
pub fn materialise(root: &Path, profile: &AgentProfile) -> Result<Option<PathBuf>, String> {
    let dir = agent_skills_dir(root, &profile.id);
    if profile.granted_skills.is_empty() {
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        return Ok(None);
    }

    let manifest = dir.join(".claude-plugin");
    std::fs::create_dir_all(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(
        manifest.join("plugin.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": PLUGIN_NAME,
            "version": "1.0.0",
            "description": format!("skills granted to {} by the operator", profile.name),
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let skills = dir.join("skills");
    std::fs::create_dir_all(&skills).map_err(|e| e.to_string())?;

    let mut kept: Vec<String> = Vec::new();
    for grant in &profile.granted_skills {
        // A grant that would not pass the gate is not written, rather than
        // written and hoped over: `../` in a name is a path, not a skill.
        if check_skill(grant).is_err() {
            continue;
        }
        let target = skills.join(&grant.name);
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        std::fs::write(target.join("SKILL.md"), skill_markdown(grant)).map_err(|e| e.to_string())?;
        kept.push(grant.name.clone());
    }

    // Anything on disk that the profile no longer names.
    if let Ok(entries) = std::fs::read_dir(&skills) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !kept.contains(&name) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }

    if kept.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        return Ok(None);
    }
    Ok(Some(dir))
}

/// The SKILL.md a grant becomes. Relay writes the frontmatter itself, from the
/// declared name and description, so a body that carries its own frontmatter
/// cannot rename the skill into something the operator did not approve.
pub fn skill_markdown(grant: &SkillGrant) -> String {
    let description = grant.description.trim().replace('\n', " ");
    let body = strip_frontmatter(&grant.body);
    format!(
        "---\nname: {}\ndescription: {}\n---\n\n{}\n",
        grant.name,
        description,
        body.trim()
    )
}

/// Drop a leading `---` block, if the declared body brought one.
fn strip_frontmatter(body: &str) -> &str {
    let trimmed = body.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return body;
    };
    let Some(end) = rest.find("\n---") else {
        return body;
    };
    rest[end + 4..].trim_start_matches(['\r', '\n'])
}

/// What this profile's runs may load. Resolved at the moment a run starts,
/// like every other piece of policy (decision #12).
pub fn for_profile(root: &Path, profile: &AgentProfile) -> Grants {
    let dir = agent_skills_dir(root, &profile.id);
    Grants {
        skills_dir: dir
            .join("skills")
            .read_dir()
            .ok()
            .and_then(|mut d| d.next().map(|_| dir)),
        mcp_servers: profile
            .mcp_servers
            .iter()
            .filter(|m| check_mcp(m).is_ok())
            .cloned()
            .collect(),
    }
}

/// The environment names a granted server still needs a value for. Shown on
/// the Agents screen: a server that cannot authenticate fails at connect time,
/// and a failure there reads like a broken app rather than a missing key.
pub fn missing_env(grant: &McpGrant) -> Vec<String> {
    grant
        .env
        .iter()
        .filter(|(_, v)| v.trim().is_empty())
        .map(|(k, _)| k.clone())
        .collect()
}

/// Merge a grant into a profile's list, replacing one of the same name.
/// Re-granting is how a declaration is corrected; refusing the second would
/// punish the model for fixing itself (the same call decision #58 made).
pub fn upsert_skill(list: &mut Vec<SkillGrant>, grant: SkillGrant) -> bool {
    match list.iter().position(|g| g.name == grant.name) {
        Some(at) => {
            let replaced = list[at] != grant;
            list[at] = grant;
            replaced
        }
        None => {
            list.push(grant);
            true
        }
    }
}

pub fn upsert_mcp(list: &mut Vec<McpGrant>, grant: McpGrant) -> bool {
    match list.iter().position(|g| g.name == grant.name) {
        Some(at) => {
            let replaced = list[at] != grant;
            list[at] = grant;
            replaced
        }
        None => {
            list.push(grant);
            true
        }
    }
}

/// Environment variables an operator has filled in, ready for the server.
pub fn env_of(grant: &McpGrant) -> BTreeMap<String, String> {
    grant
        .env
        .iter()
        .filter(|(_, v)| !v.trim().is_empty())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents;

    fn skill(name: &str) -> SkillGrant {
        SkillGrant {
            name: name.to_string(),
            description: "when asked to do the thing".into(),
            source: "https://example.invalid/skill".into(),
            body: "Do the thing carefully.".into(),
            added_ms: 1,
        }
    }

    fn profile(id: &str) -> AgentProfile {
        let mut p = agents::defaults()
            .into_iter()
            .find(|a| a.id == agents::DEFAULT_WORKER)
            .unwrap();
        p.id = id.to_string();
        p
    }

    #[test]
    fn a_name_that_is_a_path_is_refused_not_repaired() {
        assert!(valid_name("figma-export"));
        assert!(!valid_name("../escape"));
        assert!(!valid_name("Figma"));
        assert!(!valid_name(""));
        assert!(!valid_name("-leading"));
        assert!(!valid_name(&"a".repeat(65)));
        assert!(matches!(
            check_skill(&SkillGrant { name: "../etc".into(), ..skill("x") }),
            Err(Refusal::BadName(_))
        ));
    }

    #[test]
    fn nobody_grants_themselves_privilege() {
        assert!(refuse_self_elevation("director", "builder", "tools").is_none());
        let refusal = refuse_self_elevation("director", "Director", "tools").unwrap();
        // Readable, and it says why rather than "not allowed".
        assert!(refusal.to_string().contains("cannot give yourself tools"));
        assert!(refusal.to_string().contains("Agents screen"));
    }

    #[test]
    fn the_director_cannot_hand_itself_tools_or_a_server() {
        // The whole point: this is a refusal, not a hard approval. There is no
        // answer the operator can give that makes it safe, because giving it
        // once removes what would ask again.
        assert!(self_elevation_guard("grant_agent_tools", "director", "director").is_some());
        assert!(self_elevation_guard("add_mcp_server", "director", "director").is_some());
        // Case is not a way around it — ids are matched, not spelled.
        assert!(self_elevation_guard("grant_agent_tools", "director", "DIRECTOR").is_some());
        // Aimed at anyone else, it is an ordinary approval.
        assert!(self_elevation_guard("grant_agent_tools", "director", "builder").is_none());
        assert!(self_elevation_guard("add_mcp_server", "director", "designer").is_none());
        // And it is not the Director's rule — it is everyone's. A delegating
        // specialist is held to exactly the same line.
        assert!(self_elevation_guard("grant_agent_tools", "designer", "designer").is_some());
        // A skill to itself is prose in its own prompt, and still approved.
        assert!(self_elevation_guard("install_skill", "director", "director").is_none());
    }

    #[test]
    fn the_approval_sheet_names_what_is_granted() {
        let line = describe_mcp(
            "Designer",
            &McpGrant {
                name: "figma".into(),
                transport: McpTransport::Stdio {
                    command: "npx".into(),
                    args: vec!["-y".into(), "figma-mcp".into()],
                },
                tools: vec!["get_file".into(), "export_frame".into()],
                source: "https://example.invalid/figma".into(),
                ..Default::default()
            },
        );
        assert!(line.contains("Designer"));
        assert!(line.contains("grants the tools get_file, export_frame"));
        assert!(line.contains("npx -y figma-mcp"));

        let line = describe_skill("Designer", &skill("colour-audit"));
        assert!(line.contains("colour-audit"));
        assert!(line.contains("example.invalid"));
    }

    #[test]
    fn a_server_may_not_be_called_harness() {
        assert_eq!(
            check_mcp(&McpGrant {
                name: "harness".into(),
                transport: McpTransport::Stdio { command: "node".into(), args: vec![] },
                ..Default::default()
            }),
            Err(Refusal::ReservedName)
        );
    }

    #[test]
    fn relay_writes_the_frontmatter_so_a_body_cannot_rename_the_skill() {
        let mut grant = skill("colour-audit");
        grant.body = "---\nname: something-else\ndescription: sneaky\n---\n\nReal body.".into();
        let md = skill_markdown(&grant);
        assert!(md.starts_with("---\nname: colour-audit\n"));
        assert!(md.contains("Real body."));
        assert!(!md.contains("something-else"));
    }

    #[test]
    fn two_agents_get_two_directories_and_neither_holds_the_others() {
        let root = std::env::temp_dir().join(format!("relay-grants-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let mut designer = profile("designer");
        designer.granted_skills = vec![skill("figma-export"), skill("colour-audit")];
        let mut builder = profile("builder");
        builder.granted_skills = vec![skill("rustfmt-house-style")];

        let d = materialise(&root, &designer).unwrap().unwrap();
        let b = materialise(&root, &builder).unwrap().unwrap();
        assert_ne!(d, b);

        let names = |dir: &Path| {
            let mut v: Vec<String> = std::fs::read_dir(dir.join("skills"))
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            v.sort();
            v
        };
        assert_eq!(names(&d), ["colour-audit", "figma-export"]);
        assert_eq!(names(&b), ["rustfmt-house-style"]);

        // And what a run is told to load is exactly its own directory.
        assert_eq!(for_profile(&root, &designer).skills_dir.as_deref(), Some(d.as_path()));
        assert_eq!(for_profile(&root, &builder).skills_dir.as_deref(), Some(b.as_path()));

        // A revoked skill leaves disk on the next write; an emptied list takes
        // the directory with it.
        designer.granted_skills = vec![skill("figma-export")];
        materialise(&root, &designer).unwrap();
        assert_eq!(names(&d), ["figma-export"]);
        designer.granted_skills.clear();
        assert_eq!(materialise(&root, &designer).unwrap(), None);
        assert!(!d.exists());
        assert!(for_profile(&root, &designer).skills_dir.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_declaration_can_be_corrected_and_the_last_one_wins() {
        let mut list = vec![skill("a")];
        let mut better = skill("a");
        better.description = "sharper".into();
        assert!(upsert_skill(&mut list, better));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].description, "sharper");
    }

    #[test]
    fn a_server_missing_its_key_says_which_one() {
        let mut grant = McpGrant {
            name: "linear".into(),
            transport: McpTransport::Http { url: "https://example.invalid".into() },
            ..Default::default()
        };
        grant.env.insert("LINEAR_API_KEY".into(), String::new());
        grant.env.insert("LINEAR_TEAM".into(), "core".into());
        assert_eq!(missing_env(&grant), ["LINEAR_API_KEY"]);
        assert_eq!(env_of(&grant).keys().collect::<Vec<_>>(), ["LINEAR_TEAM"]);
    }
}
