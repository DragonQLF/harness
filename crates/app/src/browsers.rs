//! The two browsers an agent can be given.
//!
//! Both are the same MCP server — `chrome-devtools-mcp` — launched two
//! different ways, and the difference between them is the only thing that
//! matters here: **whether what happens in the browser survives the run.**
//!
//! - **Private** passes `--isolated`, which gives Chrome a temporary user data
//!   directory and deletes it when the browser closes. Nothing is remembered,
//!   nothing is signed in, and two agents running at once cannot tread on each
//!   other because each gets its own. This is the one a worker should have.
//!
//! - **Signed in** points Chrome at a directory Relay owns and keeps. Cookies
//!   and logins persist, which is the point — it is what lets the Director
//!   drive a site the operator is actually signed into — and it is also the
//!   whole of its danger, because an agent holding that profile can act as the
//!   operator everywhere that profile is signed in.
//!
//! Two facts about it are worth stating plainly rather than discovering:
//!
//! 1. **Only one browser can use a user data directory at a time.** Chrome
//!    locks it. Two agents granted the signed-in browser and running together
//!    means the second one fails to start its browser — so it is meant for one
//!    agent, and the screen says so.
//! 2. **It is not the operator's own Chrome.** It is a separate profile in
//!    Relay's data directory, empty until somebody signs into something in it.
//!    Whatever it can reach is what was deliberately put there, which is what
//!    makes "sign in to the one site the Director needs" a real answer rather
//!    than a hope. Attaching to a already-running Chrome is possible
//!    (`--browser-url`) and is deliberately not offered as a preset: that one
//!    hands over every session the operator has open.
//!
//! Neither is granted by default. A browser is reach, and reach is granted per
//! agent, on purpose, from the Agents screen.

use harness_ports::{McpGrant, McpTransport};

/// Which of the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Private,
    SignedIn,
}

impl Browser {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "browser" => Some(Self::Private),
            "signed-in-browser" => Some(Self::SignedIn),
            _ => None,
        }
    }

    /// The server name. Tools arrive as `mcp__<name>__<tool>`, so this is also
    /// what the agent sees, and the two must differ or granting both to one
    /// agent would collide.
    pub fn id(self) -> &'static str {
        match self {
            Self::Private => "browser",
            Self::SignedIn => "signed-in-browser",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Private => "Browser (private)",
            Self::SignedIn => "Browser (signed in)",
        }
    }

    /// What the operator is agreeing to. Written for the approval, not for a
    /// catalogue: it says what persists and what that costs.
    pub fn note(self) -> &'static str {
        match self {
            Self::Private => {
                "A fresh Chrome with no history and nobody signed in, thrown away when the run \
                 ends. Safe to give to several agents at once — each gets its own."
            }
            Self::SignedIn => {
                "A Chrome that keeps its cookies between runs, in a profile Relay owns — not your \
                 own Chrome. Whatever you sign into in it, an agent holding it can do as you. \
                 Chrome locks the profile, so give it to one agent, not four."
            }
        }
    }
}

/// The tools these servers actually publish, as of chrome-devtools-mcp 1.7.
///
/// Declared rather than discovered, like every other grant: finding out for
/// real means running the server, and running it is the thing the approval
/// exists to gate. The list is what the operator is told they are approving.
const TOOLS: [&str; 29] = [
    "click",
    "close_page",
    "drag",
    "emulate",
    "evaluate_script",
    "fill",
    "fill_form",
    "get_console_message",
    "get_network_request",
    "handle_dialog",
    "hover",
    "lighthouse_audit",
    "list_console_messages",
    "list_network_requests",
    "list_pages",
    "navigate_page",
    "new_page",
    "performance_analyze_insight",
    "performance_start_trace",
    "performance_stop_trace",
    "press_key",
    "resize_page",
    "select_page",
    "take_heapsnapshot",
    "take_screenshot",
    "take_snapshot",
    "type_text",
    "upload_file",
    "wait_for",
];

/// One browser as a grant, ready to be written onto an agent.
///
/// `profile_dir` is where the signed-in one keeps its cookies; the private one
/// ignores it, because `--isolated` makes Chrome mint and destroy its own.
pub fn grant(which: Browser, profile_dir: &std::path::Path, now_ms: u64) -> McpGrant {
    let mut args = vec![
        "-y".to_string(),
        "chrome-devtools-mcp@latest".to_string(),
    ];
    match which {
        Browser::Private => args.push("--isolated".to_string()),
        Browser::SignedIn => {
            args.push("--user-data-dir".to_string());
            args.push(profile_dir.to_string_lossy().into_owned());
        }
    }
    McpGrant {
        name: which.id().to_string(),
        transport: McpTransport::Stdio {
            command: "npx".to_string(),
            args,
        },
        // Nothing to fill in: this server takes no keys. An empty map is what
        // stops the Agents screen from drawing a password field nobody needs.
        env: Default::default(),
        tools: TOOLS.iter().map(|t| t.to_string()).collect(),
        source: "chrome-devtools-mcp (Chrome DevTools team)".to_string(),
        added_ms: now_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_private_one_keeps_nothing_and_the_other_is_told_where_to_keep_it() {
        let dir = Path::new("/data/browser-profile");
        let private = grant(Browser::Private, dir, 1);
        let signed_in = grant(Browser::SignedIn, dir, 1);

        let args = |g: &McpGrant| match &g.transport {
            McpTransport::Stdio { args, .. } => args.clone(),
            _ => panic!("both browsers are stdio servers"),
        };

        assert!(args(&private).contains(&"--isolated".to_string()));
        assert!(
            !args(&private).iter().any(|a| a.contains("browser-profile")),
            "an isolated browser must not be handed a directory to keep"
        );

        assert!(args(&signed_in).contains(&"/data/browser-profile".to_string()));
        assert!(
            !args(&signed_in).contains(&"--isolated".to_string()),
            "the signed-in one exists to remember; isolating it would empty it every run"
        );
    }

    /// Os dois nomes têm de diferir: as ferramentas chegam como
    /// `mcp__<nome>__<tool>`, e dar os dois ao mesmo agente com o mesmo nome
    /// seria um substituir o outro em silêncio.
    #[test]
    fn the_two_do_not_collide_and_both_pass_the_guard() {
        let dir = Path::new("/data/browser-profile");
        assert_ne!(Browser::Private.id(), Browser::SignedIn.id());
        for which in [Browser::Private, Browser::SignedIn] {
            let g = grant(which, dir, 1);
            assert!(crate::grants::check_mcp(&g).is_ok(), "{}", which.id());
            assert_eq!(Browser::from_id(which.id()), Some(which));
        }
    }
}
