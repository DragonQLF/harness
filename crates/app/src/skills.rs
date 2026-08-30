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
                "Find and install an agent-native CLI for professional software — image editors, \
                 3D tools, video, office suites — instead of deciding a program can only be \
                 driven by hand. Read this before saying something cannot be done here."
            }
        }
    }

    /// What the operator is agreeing to, for the screen that offers it.
    pub fn note(self) -> &'static str {
        match self {
            Self::Cli => {
                "Adapted from HKUDS/CLI-Anything (Apache-2.0). Prose, not permission: it grants \
                 nothing on its own, and only pays off on an agent that already has Shell. \
                 Installing a harness changes this machine, and every shell call still asks you."
            }
        }
    }
}

/// The skill's text, kept whole in the binary rather than fetched at run time.
///
/// The prose is adapted from HKUDS/CLI-Anything (Apache-2.0) — the catalogue,
/// the `cli-hub` commands and the matrix workflow are theirs. What Relay adds
/// is the part no general skill could know: where this shell guard stops, and
/// that nobody is watching the terminal.
///
/// Fetching it instead would mean an agent's prompt could change without anyone
/// approving the new words, which is the one thing a grant is supposed to
/// prevent — "show the source" is the whole safety story for prose.
fn body(which: Skill) -> &'static str {
    match which {
        Skill::Cli => concat!(
            "Most professional software has no API you can call, and driving it by hand is not \
             something you can do at all. CLI-Hub is a catalogue of *agent-native* command-line \
             interfaces for that software — image editors, 3D tools, video, audio, office \
             suites, local models — each one a real CLI with machine-readable output. Before \
             concluding that a program cannot be reached from here, look for a harness.\n\n",

            "## Finding one\n\n",
            "```\n",
            "pip install cli-anything-hub     # the package manager itself, once\n",
            "cli-hub list                     # everything in the registry\n",
            "cli-hub search image             # by keyword\n",
            "cli-hub info gimp                # what one actually does\n",
            "cli-hub install gimp             # installs the cli-anything-gimp package\n",
            "```\n\n",
            "`cli-hub` is a thin wrapper around `pip`: installing `gimp` installs the package \
             `cli-anything-gimp`, which puts a `cli-anything-gimp` command on the path. The live \
             catalogue is at https://clianything.cc.\n\n",

            "## Never open a REPL\n\n",
            "These CLIs offer an interactive mode. **You cannot use it.** Nobody is watching your \
             terminal, so a REPL waits for input that never comes and the run is spent on \
             nothing. Always the one-shot form, and always `--json`:\n\n",
            "```\n",
            "cli-anything-gimp --json image resize --width 800 in.png\n",
            "```\n\n",
            "The same goes for anything that pages, prompts or opens an editor: pass `--yes`, \
             `--no-pager`, or pipe to `cat`. Parsing prose written for a person is how a script \
             breaks the day the wording changes; `--json` is there so you do not have to.\n\n",

            "## Install only what the task needs\n\n",
            "A *matrix* is a whole workflow — `video-creation`, say — mapping capabilities to \
             tools. Look before you install, and scope it:\n\n",
            "```\n",
            "cli-hub can \"transcribe audio\"                          # who can do this?\n",
            "cli-hub matrix preflight video-creation --json          # what is already usable\n",
            "cli-hub matrix install video-creation --capability text.transcribe\n",
            "```\n\n",
            "`--dry-run` previews with no side effects. Exit codes are `0` ok, `3` partial, `1` \
             failure, `2` bad usage. Do not bulk-install a fourteen-tool matrix for a \
             one-capability job: every install changes the operator's machine, and they approve \
             each one.\n\n",

            "## What Relay's guard does and does not stop\n\n",
            "The shell guard here is about **paths**, not about installing software. It refuses \
             writes outside your worktree — and with them redirection (`>`, `>>`, `tee`), \
             command substitution (`$(…)`) and `find -exec`. Reading outside your worktree is \
             allowed: `ls`, `cat` and `find` may look anywhere, because looking changes \
             nothing.\n\n",
            "So `pip install` and `cli-hub install` are *not* refused by the guard — they name no \
             absolute path. What stops them being invisible is the permission sheet: every shell \
             call is put to the operator unless a standing rule covers it. Treat an install as \
             something you are asking for, not something you are doing: say what you want and \
             why before you run it. And when you genuinely need to write outside the worktree, \
             ask — do not go looking for a form of the command that slips past.\n\n",

            "## Say what you ran\n\n",
            "A command whose output you acted on belongs in what you report, exactly enough that \
             someone could run it again. \"I checked\" is not a check anybody can repeat.\n\n",

            "## Passing this on\n\n",
            "If another agent keeps stalling on something a CLI would solve, give it this same \
             skill with `install_skill`. It is prose, so it costs that agent nothing but prompt; \
             what it will also need is the `Shell` permission, which is set on its profile and is \
             the part that actually grants reach.\n\n",

            "---\n",
            "Adapted from CLI-Anything by HKUDS (https://github.com/HKUDS/CLI-Anything), \
             Apache-2.0. The catalogue, the commands and the matrix workflow are theirs; the \
             sections on the shell guard and on never opening a REPL are what running inside \
             Relay adds."
        ),
    }
}

pub fn grant(which: Skill, now_ms: u64) -> SkillGrant {
    SkillGrant {
        name: which.id().to_string(),
        description: which.description().to_string(),
        source: "HKUDS/CLI-Anything (Apache-2.0), adapted for Relay".to_string(),
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
        assert!(text.contains("refuses writes outside your worktree"));
        assert!(text.contains("Reading outside your worktree is allowed"));
        for guarded in ["tee", "$(…)", "find -exec"] {
            assert!(text.contains(guarded), "the guard also stops {guarded}");
        }
        // O guardo é de caminhos e não de instalações: dizer o contrário fazia
        // um agente desistir de instalar coisas que na verdade pode instalar.
        assert!(
            text.contains("are *not* refused by the guard"),
            "an install names no absolute path, so the guard does not stop it"
        );
        assert!(text.contains("permission sheet"), "what does stop it being invisible");
    }

    /// A adaptação que mais importa: estes CLIs têm modo interactivo e ninguém
    /// está a ver o terminal. Um REPL fica à espera para sempre e a execução
    /// gasta-se em nada.
    #[test]
    fn it_forbids_the_interactive_mode() {
        let text = body(Skill::Cli);
        assert!(text.contains("Never open a REPL"));
        assert!(text.contains("--json"));
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
