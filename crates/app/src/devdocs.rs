//! The Director's window onto the harness's own records: `DEBT.md` and
//! `DECISIONS.md`, the two files that say what is designed versus what is done.
//!
//! His posture asks him to distinguish desenhado de feito — but both files live
//! in this repository and he had no way to read them. Baking them into every
//! prompt would tax every turn for conversations that never touch them (and
//! grow the cached prefix as DEBT.md grows), so reading is a tool instead.
//!
//! DECISIONS.md is far past what fits in one reply, so it is read whole only up
//! to a cap; anything more specific comes through `find`, which pulls the
//! numbered sections whose text matches. Code does the finding — never the
//! model guessing at offsets.

use std::path::Path;

/// Which record to open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Doc {
    Debt,
    Decisions,
}

impl Doc {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_lowercase().as_str() {
            "debt" => Some(Self::Debt),
            "decisions" => Some(Self::Decisions),
            _ => None,
        }
    }

    fn file_name(self) -> &'static str {
        match self {
            Self::Debt => "DEBT.md",
            Self::Decisions => "DECISIONS.md",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Debt => "the debt list",
            Self::Decisions => "the decision log",
        }
    }
}

/// How much of a file one reply may carry. A prompt is not a filing cabinet.
const CAP_CHARS: usize = 14_000;

/// Read one of the two documents from `<harness repo>/docs`. With `find`, the
/// sections matching the query are returned instead of the head of the file.
pub fn render(docs_dir: &Path, doc: Doc, find: Option<&str>) -> Result<String, String> {
    let path = docs_dir.join(doc.file_name());
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "{} ({}) could not be read from {}. Harness's own repository is not registered as a \
             project here, so there is nowhere to look it up.",
            doc.title(),
            doc.file_name(),
            docs_dir.display()
        )
    })?;

    match find.map(str::trim).filter(|f| !f.is_empty()) {
        None => Ok(head(text, doc)),
        Some(query) => Ok(sections(&text, doc, query)),
    }
}

fn head(text: String, doc: Doc) -> String {
    if text.chars().count() <= CAP_CHARS {
        return text;
    }
    let cut = text.char_indices().nth(CAP_CHARS).map(|(i, _)| i).unwrap_or(text.len());
    let cut = text[..cut].rfind('\n').map(|i| i + 1).unwrap_or(cut);
    let total = text.chars().count();
    format!(
        "{}\n\n[showing the first {CAP_CHARS} of {total} characters of {} — ask for a section \
         with find (a number like \"75\" or a few words) rather than asking for the rest]",
        &text[..cut],
        doc.file_name(),
    )
}

/// Pull every `###`-headed section whose heading or body contains the query.
/// The decision numbers are stable identifiers everyone cites ("#75"), so a
/// bare number is matched with word boundaries — "#7" must not drag in "#75".
fn sections(text: &str, doc: Doc, query: &str) -> String {
    let needle = query.to_lowercase();

    let matches_of = |body: &str| -> bool {
        // A pure number searches decision ids; words search plainly.
        if needle.chars().all(|c| c.is_ascii_digit()) && !needle.is_empty() {
            body.to_lowercase()
                .split(|c: char| !c.is_ascii_digit())
                .any(|n| n == needle)
        } else {
            body.to_lowercase().contains(&needle)
        }
    };

    let mut picked: Vec<String> = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in text.lines() {
        if line.starts_with("### ") {
            if let Some((heading, body)) = current.take() {
                if matches_of(&format!("{heading}\n{body}")) {
                    picked.push(format!("{heading}\n{body}"));
                }
            }
            current = Some((line.to_string(), String::new()));
        } else if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    if let Some((heading, body)) = current.take() {
        if matches_of(&format!("{heading}\n{body}")) {
            picked.push(format!("{heading}\n{body}"));
        }
    }

    if picked.is_empty() {
        return format!(
            "Nothing in {} matches \"{query}\".",
            doc.file_name()
        );
    }

    let mut out = picked.join("\n");
    if out.chars().count() > CAP_CHARS {
        let cut = out.char_indices().nth(CAP_CHARS).map(|(i, _)| i).unwrap_or(out.len());
        let cut = out[..cut].rfind('\n').map(|i| i + 1).unwrap_or(cut);
        out.truncate(cut);
        out.push_str("\n[truncated — narrow the search]");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "harness-devdocs-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn debt_is_read_whole_and_decisions_are_found_by_number() {
        let dir = temp("find");
        std::fs::write(
            dir.join("DEBT.md"),
            "| Item | Estado |\n|---|---|\n| Curador | Por fazer |",
        )
        .unwrap();
        std::fs::write(
            dir.join("DECISIONS.md"),
            "## Decisões\n\
             ### 74. Pausa por orçamento\nCard.budget_paused existe.\n\
             ### 75. Postura do Director\nSete linhas de postura no prompt.\n\
             ### 76. record_decision\nEscreve em memória.\n",
        )
        .unwrap();

        let debt = render(&dir, Doc::Debt, None).unwrap();
        assert!(debt.contains("Curador"));

        let seventy_five = render(&dir, Doc::Decisions, Some("75")).unwrap();
        assert!(seventy_five.contains("### 75."), "{seventy_five}");
        assert!(seventy_five.contains("postura"));
        assert!(!seventy_five.contains("budget_paused"), "no neighbouring section");
        // A short number must not swallow a longer one that contains it.
        let seven = render(&dir, Doc::Decisions, Some("7")).unwrap();
        assert!(seven.contains("Nothing"), "\"7\" matches nothing exactly: {seven}");

        let words = render(&dir, Doc::Decisions, Some("record_decision")).unwrap();
        assert!(words.contains("### 76."));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_says_why_rather_than_lying() {
        let dir = temp("missing");
        let err = render(&dir, Doc::Debt, None).unwrap_err();
        assert!(err.contains("DEBT.md"));
        assert!(err.contains("not registered as a project"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_overlong_head_is_cut_with_a_pointer_to_find() {
        let dir = temp("cap");
        let mut big = String::new();
        for i in 0..2000 {
            big.push_str(&format!("line {i} padded enough to be worth skipping over entirely\n"));
        }
        std::fs::write(dir.join("DECISIONS.md"), &big).unwrap();

        let head = render(&dir, Doc::Decisions, None).unwrap();
        assert!(head.contains("[showing the first"), "{head}");
        assert!(head.contains("with find"));
        assert!(head.chars().count() < CAP_CHARS + 400);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn doc_names_are_loose_about_case() {
        assert_eq!(Doc::parse("DEBT"), Some(Doc::Debt));
        assert_eq!(Doc::parse(" decisions "), Some(Doc::Decisions));
        assert_eq!(Doc::parse("charters"), None);
    }
}
