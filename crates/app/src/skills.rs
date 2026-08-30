//! Skills Relay ships, ready to be granted with one click.
//!
//! A skill is prose that enters an agent's prompt. It grants no reach on its
//! own — `permissions` does that — so what a skill actually changes is whether
//! an agent *thinks to use* what it already holds, and whether it uses it well.
//! That is the whole of the safety story too: there is nothing here to sandbox,
//! only something to read, which is why the body is kept whole and shown.
//!
//! These are presets, not a catalogue. Anything else comes in through
//! `install_skill`, written by the Director and approved by the operator.

use harness_ports::SkillGrant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    /// Using the machine's own command-line tools.
    Cli,
}

impl Skill {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "cli" => Some(Self::Cli),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Cli => "cli",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Cli => "Command line",
        }
    }

    /// One line telling the model when to reach for it.
    pub fn description(self) -> &'static str {
        match self {
            Self::Cli => {
                "Reach for the machine's own command-line tools instead of asking for one to be \
                 built. Read this before deciding something cannot be done."
            }
        }
    }

    /// What the operator is agreeing to, for the screen that offers it.
    pub fn note(self) -> &'static str {
        match self {
            Self::Cli => {
                "Prose, not permission: it grants nothing on its own. It only pays off on an \
                 agent that already has Shell, and it tells that agent where the shell guard \
                 stops."
            }
        }
    }
}

/// The skill's text. Written here rather than fetched: a skill that steers an
/// agent is something the operator must be able to read in full, and a URL is
/// not something you can read.
fn body(which: Skill) -> &'static str {
    match which {
        Skill::Cli => concat!(
            "This machine has a shell, and most things you might want already exist on it as a \
             command. Before concluding that something cannot be done, or asking for a tool to \
             be built, check whether one is already installed.\n\n",

            "## Look before you decide\n\n",
            "`command -v <name>` says whether something exists. `<name> --help` says what it \
             does. Both are cheap, and both are better than guessing at flags — a flag \
             remembered from training is how a command silently does the wrong thing.\n\n",

            "## Run commands that can be run unattended\n\n",
            "Nothing is watching the terminal, so a command that waits for input waits forever \
             and its run is spent on nothing. Prefer the non-interactive form: pass `--yes` or \
             `-y` where the tool offers it, `--no-pager` or pipe to `cat` for anything that \
             would page, and never open an editor, a REPL or a `less`. Where a tool has a \
             machine-readable mode — `--json`, `--porcelain`, `--format` — use it: parsing \
             prose meant for a person is how a script breaks on the day the wording changes.\n\n",

            "## Where the guard stops\n\n",
            "Relay guards the shell by path, and it distinguishes reading from writing. \
             Reading outside your worktree is allowed: `ls`, `cat`, `find` and the like can \
             look anywhere, because looking changes nothing. Writing outside your worktree is \
             refused — and so is anything that could write, which includes redirection \
             (`>`, `>>`, `tee`), command substitution (`$(…)`) and `find -exec`. That is a \
             deliberate line, not a bug to work around: if you genuinely need to write \
             somewhere else, say so and ask, rather than looking for a form of the command \
             that slips past.\n\n",

            "## Say what you ran\n\n",
            "A command whose output you acted on belongs in what you report, in enough detail \
             that someone else could run it again. \"I checked\" is not a check anybody can \
             repeat.\n\n",

            "## Passing this on\n\n",
            "If another agent keeps failing at something the command line would solve, you can \
             give it this same skill with `install_skill`. It is prose, so it costs that agent \
             nothing but prompt; what it will also need is the `Shell` permission, which is set \
             on its profile and is the part that actually grants reach."
        ),
    }
}

pub fn grant(which: Skill, now_ms: u64) -> SkillGrant {
    SkillGrant {
        name: which.id().to_string(),
        description: which.description().to_string(),
        source: "written into Relay".to_string(),
        body: body(which).to_string(),
        added_ms: now_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_skill_is_grantable_and_names_itself_consistently() {
        let g = grant(Skill::Cli, 1);
        assert_eq!(g.name, Skill::Cli.id());
        assert_eq!(Skill::from_id(&g.name), Some(Skill::Cli));
        assert!(!g.body.trim().is_empty());
    }

    /// O corpo diz onde o guardo pára, e tem de dizer a verdade sobre ele: um
    /// agente que leia aqui que pode escrever fora da worktree vai bater numa
    /// recusa e não saber porquê. Isto prende as duas metades da regra —
    /// `sidecar/pathguard.mjs` isenta leituras e continua a guardar escritas.
    #[test]
    fn it_states_the_shell_guard_as_it_actually_is() {
        let text = body(Skill::Cli);
        assert!(text.contains("Reading outside your worktree is allowed"));
        assert!(text.contains("Writing outside your worktree is refused"));
        for guarded in ["tee", "$(…)", "find -exec"] {
            assert!(text.contains(guarded), "the guard also stops {guarded}");
        }
    }

    /// A razão de o Director o ter: para saber que existe e o poder passar.
    #[test]
    fn it_tells_the_holder_how_to_pass_it_on() {
        let text = body(Skill::Cli);
        assert!(text.contains("install_skill"));
        assert!(
            text.contains("Shell"),
            "passing the prose on without the permission grants nothing"
        );
    }
}
