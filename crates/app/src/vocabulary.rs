//! The words the app uses for its own concepts, and the one place they live.
//!
//! A status, a checkout mode, a reviewer and a permission each have an id the
//! backend serialises and a label the operator reads. Both halves used to be
//! typed out again in `src/lib/types.ts`, which is fine until the day they
//! disagree — and the day they disagree, the screen offers a choice the engine
//! refuses, or names a state it does not have. Neither failure looks like a
//! typo; both look like the app being broken.
//!
//! So this module owns them and writes the TypeScript. Two things make that
//! worth more than a comment asking people to keep two lists in step:
//!
//! - **The ids are not typed here either.** They come from serialising the
//!   actual enum, so an id in the frontend is by construction the id the
//!   backend parses. Renaming a variant changes both halves or neither.
//! - **The labels are here**, next to the code that gives them meaning, rather
//!   than in a file the backend never reads.
//!
//! What is deliberately *not* here: colour. `TONE` and `STATUS_TONE` map to CSS
//! variables, and a Rust crate has no business knowing what `var(--accent)`
//! resolves to. The frontend keeps those.

use harness_domain::Status;
use harness_ports::{Reviewer, WorktreeMode};
use serde::Serialize;

use crate::agents::ALL_PERMISSIONS;

/// One option the operator picks from: what it is called on the wire, what it
/// is called on screen, and the sentence under it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    pub id: String,
    pub name: String,
    pub hint: String,
}

/// The id the backend actually serialises, not a hand-typed guess at it.
fn id_of<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .expect("these enums serialise as plain strings")
}

fn choice<T: Serialize>(value: &T, name: &str, hint: &str) -> Choice {
    Choice { id: id_of(value), name: name.to_string(), hint: hint.to_string() }
}

/// Board columns, left to right. The order is the workflow, so it belongs with
/// the state machine rather than with the screen that draws it.
pub fn statuses() -> Vec<Choice> {
    vec![
        choice(&Status::Backlog, "Later", "Parked, not queued"),
        choice(&Status::Ready, "Ready", "Waiting for an agent to pick it up"),
        choice(&Status::Running, "Working", "An agent is on it now"),
        choice(&Status::Review, "Review", "Finished, waiting to be read"),
        choice(&Status::Done, "Done", "Approved and merged"),
    ]
}

/// Every column, in the order the board draws them.
const COLUMNS: [Status; 5] = [
    Status::Backlog,
    Status::Ready,
    Status::Running,
    Status::Review,
    Status::Done,
];

/// Which column a card may move to, from where.
///
/// This is `Status::LEGAL_MOVES` — the state machine itself — and it is here
/// for the same reason the ids are: the board decides which drop targets to
/// offer and which drags to refuse, and it was deciding that from a table
/// typed out again in `Board.tsx`. Two copies of a transition table do not
/// fail like a typo. They fail as a column that refuses a card the engine
/// would have accepted, or offers one it will reject — with no error anywhere,
/// because the frontend swallows the move it thinks is illegal before the
/// backend ever hears about it.
pub fn legal_moves() -> Vec<(String, Vec<String>)> {
    COLUMNS
        .iter()
        .map(|from| {
            let to = COLUMNS
                .iter()
                .filter(|dest| from.can_move_to(**dest))
                .map(id_of)
                .collect();
            (id_of(from), to)
        })
        .collect()
}

pub fn worktree_modes() -> Vec<Choice> {
    vec![
        choice(&WorktreeMode::PerCard, "Per card", "A fresh branch and checkout for every card"),
        choice(&WorktreeMode::Shared, "Shared", "One long-lived branch for the project"),
        choice(&WorktreeMode::None, "None", "Reads the main checkout, never writes"),
    ]
}

pub fn reviewers() -> Vec<Choice> {
    vec![
        choice(
            &Reviewer::Director,
            "Director",
            "The Director reads the diff first and only sends you what passes.",
        ),
        choice(&Reviewer::Human, "You", "Every finished run lands in your review queue."),
        choice(&Reviewer::Nobody, "Nobody", "Finished runs go straight to Done."),
    ]
}

/// The models the Claude login offers. Any other endpoint publishes its own
/// list, which is why this one is short and the picker exists.
pub fn models() -> Vec<Choice> {
    vec![
        Choice { id: "opus".into(), name: "Opus".into(), hint: "Deepest reasoning, highest cost".into() },
        Choice { id: "sonnet".into(), name: "Sonnet".into(), hint: "The everyday worker".into() },
        Choice { id: "haiku".into(), name: "Haiku".into(), hint: "Fast and cheap, for lookups".into() },
    ]
}

fn render(name: &str, doc: &str, choices: &[Choice]) -> String {
    let rows: Vec<String> = choices
        .iter()
        .map(|c| {
            format!(
                "  {{ id: {}, name: {}, hint: {} }},",
                json(&c.id),
                json(&c.name),
                json(&c.hint)
            )
        })
        .collect();
    format!(
        "/** {doc} */\nexport const {name}: Choice[] = [\n{}\n];\n\n",
        rows.join("\n")
    )
}

/// A TypeScript string literal, escaped by serde rather than by hand.
fn json(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// The generated module. Written by `pnpm codegen` beside the ts-rs types.
pub fn typescript() -> String {
    let mut out = String::from(
        "// Generated by crates/app/src/vocabulary.rs — do not edit.\n\
         //\n\
         // Ids come from serialising the Rust enums, so a value here is by\n\
         // construction one the backend parses. Run `pnpm codegen` after\n\
         // changing them.\n\n\
         export interface Choice {\n  id: string;\n  name: string;\n  hint: string;\n}\n\n",
    );
    out.push_str(&render(
        "STATUSES",
        "Board columns, left to right. The order is the workflow.",
        &statuses(),
    ));
    out.push_str(&render("WORKTREE_MODES", "Where an agent does its work.", &worktree_modes()));
    out.push_str(&render("REVIEWERS", "Who reads the diff when a run finishes.", &reviewers()));
    out.push_str(&render("MODELS", "What the Claude login offers.", &models()));

    let moves: Vec<String> = legal_moves()
        .into_iter()
        .map(|(from, to)| {
            let dests: Vec<String> = to.iter().map(|d| json(d)).collect();
            format!("  {}: [{}],", json(&from), dests.join(", "))
        })
        .collect();
    out.push_str(&format!(
        "/** Which column a card may move to, from where — `Status::LEGAL_MOVES`\n \
         *  itself, not a copy of it. A move outside this table is an override,\n \
         *  and an override needs a reason. */\nexport const LEGAL_MOVES: Record<string, string[]> = {{\n{}\n}};\n\n",
        moves.join("\n")
    ));

    let shells: Vec<String> = crate::allow::SHELL_TOOLS.iter().map(|t| json(t)).collect();
    out.push_str(&format!(
        "/** As ferramentas que dão uma linha de comandos. Uma regra sobre uma\n \
         *  destas sem comando é irrestrita, e o `allow.rs` revoga-a — o ecrã diz\n \
         *  que está revogada, e dizia-o a partir da mesma lista escrita outra\n \
         *  vez. Uma regra de segurança em duas cópias falha calada: acrescenta-se\n \
         *  uma shell nova de um lado e o outro continua a chamar-lhe válida. */\nexport const SHELL_TOOLS: string[] = [{}];\n\n",
        shells.join(", ")
    ));

    let permissions: Vec<String> = ALL_PERMISSIONS.iter().map(|p| json(p)).collect();
    out.push_str(&format!(
        "/** Every reach an agent can hold. `allowed_tools` in the backend is what\n \
         *  each one means to a run. */\nexport const ALL_PERMISSIONS: string[] = [{}];\n",
        permissions.join(", ")
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_what_the_backend_serialises() {
        // The point of the whole module: not that the strings match today, but
        // that they cannot stop matching. Renaming a variant moves both halves.
        assert_eq!(id_of(&Status::Backlog), "backlog");
        assert_eq!(id_of(&Status::Running), "running");
        assert_eq!(id_of(&WorktreeMode::PerCard), "per_card");
        assert_eq!(id_of(&Reviewer::Nobody), "nobody");

        let ids: Vec<String> = statuses().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["backlog", "ready", "running", "review", "done"], "left to right");
    }

    /// A tabela que o quadro desenha é a máquina de estados, não uma cópia
    /// dela: acrescentar uma transição no domínio tem de chegar ao ecrã sem
    /// ninguém ir lá escrevê-la outra vez.
    #[test]
    fn the_moves_offered_are_the_moves_the_engine_accepts() {
        for (from, to) in legal_moves() {
            for dest in &to {
                assert!(
                    Status::LEGAL_MOVES.iter().any(|(f, d)| id_of(f) == from && id_of(d) == *dest),
                    "{from} -> {dest} is not a legal move"
                );
            }
        }
        let offered: usize = legal_moves().iter().map(|(_, to)| to.len()).sum();
        assert_eq!(offered, Status::LEGAL_MOVES.len(), "every legal move is offered, and no other");
        assert_eq!(
            legal_moves().into_iter().find(|(from, _)| from == "done").map(|(_, to)| to),
            Some(vec![]),
            "nothing leaves Done"
        );
    }

    #[test]
    fn the_generated_module_is_valid_and_complete() {
        let ts = typescript();
        for expected in [
            "export const STATUSES",
            "export const WORKTREE_MODES",
            "export const REVIEWERS",
            "export const MODELS",
            "export const ALL_PERMISSIONS",
            "do not edit",
        ] {
            assert!(ts.contains(expected), "missing {expected}");
        }
        assert!(ts.contains(r#"{ id: "per_card", name: "Per card""#));
        // Copy with an apostrophe or a quote must not break the file.
        assert!(!ts.contains("\n\"\n"), "strings are escaped by serde, not by hand");
    }
}
