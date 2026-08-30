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
/// why an interactive mode is unusable from inside a tool call.
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

            "## How you interact with one\n\n",
            "Turn by turn, not keystroke by keystroke. A shell call is one command and its \
             output: you read what came back and decide the next command. These harnesses are \
             built for exactly that — they keep their state in a project or session file, so a \
             run of one-shot calls behaves like a session:\n\n",
            "```\n",
            "cli-anything-blender --json --project scene.blend-cli.json object add --type cube\n",
            "cli-anything-blender --json --project scene.blend-cli.json render --out out.png\n",
            "```\n\n",
            "You can also feed a command its input, as long as you decide it up front: pipes and \
             heredocs are ordinary commands and work fine (`echo \"...\" | cli-anything-x`).\n\n",
            "**What you cannot do is hold an interactive prompt open.** These CLIs offer a REPL, \
             and it is not for you — not because nobody is there (the operator sees every command \
             you run and can speak to you mid-run) but because there is no keystroke channel to a \
             process already waiting at a prompt. It would sit there until the call is cut off. \
             Use the one-shot form, and always `--json`:\n\n",
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
            "It is about **paths**, and only paths. Your command is split on `|`, `&&`, `;` and \
             newlines, and each part is judged on its own:\n\n",
            "- A part that is a plain read — `ls`, `cat`, `head`, `tail`, `find`, `grep`, `wc`, \
             `stat` — may name any path at all, anywhere. Looking changes nothing.\n",
            "- Every other part is scanned for absolute paths, and one that lands outside your \
             worktree is refused. A part loses the read exemption if it redirects with `>`, \
             substitutes with `$(…)` or backticks, starts with `VAR=`, or is a `find` that \
             executes.\n\n",
            "The consequence worth holding on to: **a command that names no path outside your \
             worktree passes, whatever it does.** `pip install`, `cli-hub install`, a pipeline, a \
             heredoc — none of them is refused here. What keeps them from being invisible is the \
             permission sheet: every shell call goes to the operator unless a standing rule \
             covers it. So treat an install as something you are asking for — say what you want \
             and why before running it. And when you genuinely need to write outside the \
             worktree, ask; do not go hunting for a spelling that slips past.\n\n",
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
        // Os dois lados da regra, e a consequência que dela se tira.
        assert!(text.contains("It is about **paths**, and only paths"));
        assert!(text.contains("may name any path at all, anywhere"));
        assert!(text.contains("lands outside your worktree is refused"));
        assert!(
            text.contains("names no path outside your worktree passes, whatever it does"),
            "a command with no outside path is not refused, and saying otherwise makes an agent \
             give up on installs it can actually do"
        );
        // As formas que perdem a isenção de leitura. Não são proibidas — são
        // julgadas pelos caminhos que trouxerem, e dizer \"proibidas\" era o
        // exagero anterior.
        for loses in ["`>`", "$(…)", "VAR=", "`find` that"] {
            assert!(text.contains(loses), "{loses} loses the read exemption");
        }
        assert!(text.contains("permission sheet"), "what does keep an install visible");
        // Os leitores isentos, nomeados: um agente que não saiba quais são
        // evita comandos que pode correr.
        for reader in ["`ls`", "`cat`", "`grep`", "`stat`"] {
            assert!(text.contains(reader), "{reader} is exempt and should be named");
        }
    }

    /// A adaptação que mais importa, e a razão tem de estar certa: não é que
    /// não esteja lá ninguém — está, e é o ponto todo desta app. É que uma
    /// chamada de shell é um comando e a sua saída, sem canal para escrever
    /// num processo à espera. Dar a razão errada convidava o agente a
    /// raciocinar para lá da regra: "mas o operador está a ver, logo posso".
    #[test]
    fn it_describes_the_interaction_model_and_why_a_repl_is_not_it() {
        let text = body(Skill::Cli);
        // Interagir é possível — turno a turno, com o estado no ficheiro de
        // projecto. Dizer só "não uses o REPL" deixava o agente sem saber
        // como *é* que se faz.
        assert!(text.contains("Turn by turn, not keystroke by keystroke"));
        assert!(text.contains("keep their state in a project or session file"));
        assert!(text.contains("pipes and heredocs"), "input decided up front works");
        assert!(text.contains("--json"));
        // E a razão do REPL tem de continuar a ser a certa.
        assert!(
            text.contains("not because nobody is there"),
            "the operator is watching; that is not why a REPL fails"
        );
        assert!(text.contains("no keystroke channel"));
    }
}
