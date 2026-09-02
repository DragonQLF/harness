//! Curated memory: the distilled knowledge that crosses cards and projects.
//!
//! Two files, both written and owned by the operator, and one thing derived
//! from the event log:
//!
//! - `<repo>/charter.md` — what this project is for, its rules and taste. One
//!   per project, handed to every agent that works on it.
//! - `<appdata>/global.md` — small, always in the prompt: how the operator
//!   likes work done everywhere.
//! - what earlier cards on this board learned — `notes_from`, read out of the
//!   `WorkReported` events themselves.
//!
//! The third one used to be a promotion pass (`curator`) that copied those
//! notes into a tree of files under `memory/areas/` and regenerated an index
//! over them. It was written, tested, registered as a command — and never
//! called by anything, so `areas/` did not exist on any machine while every
//! card paid to rediscover what the last one had already written down.
//!
//! It is derived instead, for reasons better than being shorter. A promotion
//! pass has to be triggered, and memory that has to be triggered is memory
//! that rots the first time nobody triggers it. Files outlive the cards that
//! wrote them, so a card later rejected leaves its notes behind as fact. And a
//! second copy on disk is a second thing that can disagree with the log — the
//! same reason `runstats` and `insights` are pure functions of the log and
//! touch no files at all.
//!
//! Reading is capped because a prompt is not a filing cabinet: if it does not
//! fit, it is too long to be followed.

use std::path::Path;

use harness_domain::{Card, Event, Status};
use harness_ports::StoredEvent;

const CHARTER_MAX_CHARS: usize = 4000;
const GLOBAL_MAX_CHARS: usize = 1500;

/// Read a memory file: missing, empty or whitespace-only is `None`; otherwise
/// the text, hard-capped at `max_chars` on a line boundary so a paragraph is
/// never cut mid-word.
fn read_capped(path: &Path, max_chars: usize) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.chars().count() <= max_chars {
        return Some(text.to_string());
    }
    let cut = text
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    // Back up to the end of the last full line inside the cap.
    let cut = text[..cut]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(cut);
    let mut out = text[..cut].trim_end().to_string();
    out.push_str("\n[truncated]");
    Some(out)
}

/// The project's charter, from wherever it lives. Preferred home is the
/// project's own memory directory in appdata — beside the runs and the
/// transcripts, outside any repository, so two concurrent cards can never
/// conflict over a memory file. A `charter.md` at the repository root (#52's
/// original spot) still counts: operator habit outranks file layout.
pub fn charter_between(appdata_charter: &Path, repo_charter: &Path) -> Option<String> {
    read_capped(appdata_charter, CHARTER_MAX_CHARS)
        .or_else(|| read_capped(repo_charter, CHARTER_MAX_CHARS))
}

/// The project's charter from the repository root — the pre-memory-tree spot.
pub fn charter_for(repo_root: &Path) -> Option<String> {
    read_capped(&repo_root.join("charter.md"), CHARTER_MAX_CHARS)
}

/// The operator's standing notes, small and always in the prompt.
pub fn global_for(data_dir: &Path) -> Option<String> {
    read_capped(&data_dir.join("global.md"), GLOBAL_MAX_CHARS)
}

/// How much of a prompt the project's recorded decisions may take.
///
/// Bigger than the notes budget because a decision is a *rule* — the thing that
/// changes what the agent does next turn — and there are few of them: six files
/// across two projects when this was written.
const DECISIONS_MAX_CHARS: usize = 5000;

/// The standing rules `record_decision` wrote, newest first.
///
/// These were written and never read. `record_decision` created the file and
/// nothing anywhere loaded it back, so a rule the operator dictated reached
/// exactly nobody — including the one he dictated *about this*: "verified work
/// proceeds without asking: approve, merge, start the next card", written after
/// the Director asked him twice in one session for permission it already had.
///
/// A decision the agent cannot read is worse than an unwritten one, because
/// writing it feels like it settled something.
///
/// Newest first and capped, same policy as the notes: forgetting is budget, not
/// judgment. The file name carries the date `record_decision` stamped, so
/// sorting by name is sorting by age.
pub fn decisions_from(dir: &Path) -> Option<String> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    // `read_dir` is in the filesystem's order, which is not an order.
    files.sort();
    files.reverse();

    let mut out = String::new();
    let mut dropped = 0usize;
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if out.chars().count() + text.chars().count() > DECISIONS_MAX_CHARS {
            dropped += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n---\n\n");
        }
        out.push_str(text);
    }
    if out.is_empty() {
        return None;
    }
    if dropped > 0 {
        out.push_str(&format!(
            "\n\n[{dropped} older decisions left out; these are the most recent]"
        ));
    }
    Some(out)
}

/// How much of a run's prompt the board's own memory may take.
///
/// Sized against the material rather than guessed: the whole of Nightfall's
/// thirteen-run history is 80 notes and about 3,800 tokens, so this holds it
/// with room over. When it stops holding, the newest survive — see below.
const NOTES_MAX_CHARS: usize = 6000;

/// What earlier cards on this board learned, for the next card's prompt.
///
/// Read from the log at the moment it is needed, never promoted to files. Two
/// consequences worth naming, because they are the argument for doing it this
/// way:
///
/// - **Only Done cards count.** Notes from a card that was rejected, or that
///   is still running, are not knowledge — they are a guess that has not been
///   accepted yet. Deriving means a card rejected *after* reporting stops
///   contributing the moment it is rejected; a promoted file would have gone
///   on asserting it forever.
/// - **The last report per card wins.** A card that reports twice reports a
///   superset the second time (`c_f50e` did exactly this: nine notes, then
///   twelve containing those nine). Keeping both would spend the budget
///   saying the same thing.
///
/// Newest first, so the budget — when it eventually binds — falls on the
/// oldest. That is the whole of the forgetting policy, and it is deliberate:
/// deciding that one note supersedes another needs judgment, and judgment
/// means a model, and a model in this path is the pass this replaced.
pub fn notes_from(history: &[StoredEvent], cards: &[Card]) -> Option<String> {
    let done: std::collections::HashSet<&str> = cards
        .iter()
        .filter(|c| c.status == Status::Done)
        .map(|c| c.id.as_str())
        .collect();

    // Latest report per card: later sequence numbers overwrite earlier ones,
    // and the log is in sequence order.
    let mut latest: std::collections::HashMap<&str, &Vec<String>> =
        std::collections::HashMap::new();
    let mut order: Vec<(u64, &str)> = Vec::new();
    for stored in history {
        let Event::WorkReported { card_id, notes, .. } = &stored.event else {
            continue;
        };
        let id = card_id.as_str();
        if !done.contains(id) {
            continue;
        }
        latest.insert(id, notes);
        order.retain(|(_, seen)| *seen != id);
        order.push((stored.seq, id));
    }

    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut lines: Vec<String> = Vec::new();
    let mut chars = 0usize;
    let mut dropped = 0usize;
    for (_, id) in order.iter().rev() {
        for note in latest.get(id).into_iter().flat_map(|n| n.iter()) {
            let note = note.trim();
            // The same fact learned twice by two cards is one fact.
            if note.is_empty() || !seen.insert(note) {
                continue;
            }
            let line = format!("- {note}");
            if chars + line.chars().count() > NOTES_MAX_CHARS {
                dropped += 1;
                continue;
            }
            chars += line.chars().count();
            lines.push(line);
        }
    }

    if lines.is_empty() {
        return None;
    }
    // What was left out is said out loud. A prompt that silently holds some of
    // the memory reads exactly like one that holds all of it.
    if dropped > 0 {
        lines.push(format!(
            "[{dropped} older notes left out; these are the most recent]"
        ));
    }
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "harness-memory-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_empty_and_whitespace_are_no_memory() {
        let dir = temp("missing");
        assert!(charter_for(&dir).is_none());
        std::fs::write(dir.join("charter.md"), "   \n\t  ").unwrap();
        assert!(charter_for(&dir).is_none(), "whitespace is not a charter");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_charter_is_read_whole_when_small() {
        let dir = temp("small");
        std::fs::write(dir.join("charter.md"), "\nShip weekly. No dark patterns.\n").unwrap();
        let c = charter_for(&dir).unwrap();
        assert_eq!(c, "Ship weekly. No dark patterns.", "trimmed, whole");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_overlong_file_is_cut_on_a_line_not_mid_sentence() {
        let dir = temp("long");
        let mut long = String::new();
        for i in 0..400 {
            long.push_str(&format!("line {i} of the operator's standing notes\n"));
        }
        std::fs::write(dir.join("global.md"), &long).unwrap();
        let g = global_for(&dir).unwrap();
        assert!(g.starts_with("line 0 "));
        assert!(g.ends_with("[truncated]"));
        assert!(g.chars().count() <= 1500 + "[truncated]\n".len());
        // The cap lands on a line boundary, never inside a word.
        for line in g.lines() {
            if line != "[truncated]" {
                assert!(line.starts_with("line ") || line.is_empty(), "{line}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod notes_tests {
    use super::*;
    use harness_domain::{CardId, Status};
    use harness_ports::StoredEvent;

    fn reported(seq: u64, card: &str, notes: &[&str]) -> StoredEvent {
        StoredEvent {
            seq,
            ts_ms: seq,
            event: Event::WorkReported {
                card_id: CardId::new(card),
                summary: "did the thing".into(),
                notes: notes.iter().map(|n| n.to_string()).collect(),
            },
        }
    }

    fn card(id: &str, status: Status) -> Card {
        Card {
            id: CardId::new(id),
            title: id.into(),
            status,
            current_run: None,
            agent_id: "builder".into(),
            cost_usd: 0.0,
            turns: 0,
            runs: 1,
            last_review: None,
            hunk_verdicts: Vec::new(),
            session_id: None,
            worktree: None,
            branch: None,
            depends_on: Vec::new(),
            budget_paused: false,
            finished_ms: None,
        }
    }

    #[test]
    fn nothing_reported_is_no_memory_rather_than_an_empty_heading() {
        assert!(notes_from(&[], &[]).is_none());
    }

    #[test]
    fn only_cards_that_reached_done_teach_anything() {
        let history = vec![
            reported(1, "c_done", &["Relay commits at report_work"]),
            reported(2, "c_thrown_away", &["the aura should be a starting weapon"]),
        ];
        let cards = vec![card("c_done", Status::Done), card("c_thrown_away", Status::Review)];

        let out = notes_from(&history, &cards).unwrap();
        assert!(out.contains("Relay commits at report_work"));
        assert!(
            !out.contains("starting weapon"),
            "work that was not accepted is not knowledge",
        );
    }

    /// A razão de derivar em vez de promover, num teste: um cartão rejeitado
    /// **depois** de reportar deixa de ensinar no instante em que é rejeitado.
    /// Um ficheiro promovido continuaria a afirmá-lo para sempre.
    #[test]
    fn a_card_that_stops_being_done_stops_teaching() {
        let history = vec![reported(1, "c_x", &["/tmp is path-guarded inside a run"])];
        let done = vec![card("c_x", Status::Done)];
        assert!(notes_from(&history, &done).is_some());

        let sent_back = vec![card("c_x", Status::Review)];
        assert!(
            notes_from(&history, &sent_back).is_none(),
            "o mesmo log, outro estado do quadro, outra memória",
        );
    }

    /// `c_f50e` reportou duas vezes: nove notas, depois doze contendo aquelas
    /// nove. Guardar as duas gastaria o orçamento a dizer o mesmo.
    #[test]
    fn the_last_report_of_a_card_is_the_one_that_counts() {
        let history = vec![
            reported(1, "c_f50e", &["first pass"]),
            reported(2, "c_f50e", &["first pass", "and what it learned after"]),
        ];
        let cards = vec![card("c_f50e", Status::Done)];

        let out = notes_from(&history, &cards).unwrap();
        assert_eq!(out.matches("first pass").count(), 1, "uma vez, não duas");
        assert!(out.contains("and what it learned after"));
    }

    #[test]
    fn the_same_fact_learned_by_two_cards_is_one_line() {
        let history = vec![
            reported(1, "c_a", &["Chrome is not available inside a run"]),
            reported(2, "c_b", &["Chrome is not available inside a run"]),
        ];
        let cards = vec![card("c_a", Status::Done), card("c_b", Status::Done)];
        let out = notes_from(&history, &cards).unwrap();
        assert_eq!(out.lines().count(), 1);
    }

    #[test]
    fn newest_first_and_what_did_not_fit_is_said_out_loud() {
        let old: String = "o".repeat(NOTES_MAX_CHARS);
        let history = vec![
            reported(1, "c_old", &[old.as_str()]),
            reported(2, "c_new", &["the newest thing anyone learned"]),
        ];
        let cards = vec![card("c_old", Status::Done), card("c_new", Status::Done)];

        let out = notes_from(&history, &cards).unwrap();
        assert!(out.starts_with("- the newest thing anyone learned"), "{out}");
        assert!(
            out.contains("1 older notes left out"),
            "um prompt que cala metade da memória lê-se como um que a tem toda: {out}",
        );
    }
}

#[cfg(test)]
mod decisions_tests {
    use super::*;
    use std::path::PathBuf;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "harness-decisions-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_project_with_no_decisions_contributes_nothing() {
        assert!(decisions_from(&dir("empty")).is_none());
        assert!(
            decisions_from(&std::path::Path::new("/does/not/exist")).is_none(),
            "uma pasta que não existe é um projecto sem decisões, não um erro",
        );
    }

    /// O que estava a falhar: o operador ditou uma regra, o `record_decision`
    /// escreveu-a, e nada a lia. Isto é a leitura.
    #[test]
    fn what_was_recorded_is_what_comes_back() {
        let d = dir("read");
        std::fs::write(
            d.join("2026-08-30-verified-work-proceeds-without-asking-01.md"),
            "# Verified work proceeds without asking\n\nVerifying IS the decision.",
        )
        .unwrap();
        let out = decisions_from(&d).unwrap();
        assert!(out.contains("Verifying IS the decision."));
    }

    #[test]
    fn newest_first_and_only_markdown() {
        let d = dir("order");
        std::fs::write(d.join("2026-08-01-old-01.md"), "the older rule").unwrap();
        std::fs::write(d.join("2026-09-01-new-01.md"), "the newer rule").unwrap();
        // O `record_decision` escreve um `curator-state.json` ao lado noutros
        // caminhos; nada que não seja markdown é uma decisão.
        std::fs::write(d.join("notes.json"), "{}").unwrap();

        let out = decisions_from(&d).unwrap();
        assert!(
            out.find("the newer rule").unwrap() < out.find("the older rule").unwrap(),
            "a mais recente lê-se primeiro, que é onde o orçamento morde por último",
        );
        assert!(!out.contains('{'), "só markdown: {out}");
    }

    #[test]
    fn the_budget_drops_the_oldest_and_says_so() {
        let d = dir("budget");
        std::fs::write(d.join("2026-08-01-old-01.md"), "o".repeat(DECISIONS_MAX_CHARS)).unwrap();
        std::fs::write(d.join("2026-09-01-new-01.md"), "the newest rule").unwrap();

        let out = decisions_from(&d).unwrap();
        assert!(out.starts_with("the newest rule"), "{out}");
        assert!(
            out.contains("1 older decisions left out"),
            "um prompt que cala metade das regras lê-se como um que as tem todas: {out}",
        );
    }
}
