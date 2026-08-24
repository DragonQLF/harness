//! Curated memory: the distilled knowledge that crosses cards and projects.
//!
//! Two files, both written and owned by the operator:
//!
//! - `<repo>/charter.md` — what this project is for, its rules and taste. One
//!   per project, handed to every agent that works on it.
//! - `<appdata>/global.md` — small, always in the prompt: how the operator
//!   likes work done everywhere.
//!
//! The tree of area notes and decision indexes comes later, when the ceiling
//! breaks; these two are the floor. Reading is capped because a prompt is not
//! a filing cabinet: if the file does not fit, it is too long to be followed.

use std::path::Path;

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
