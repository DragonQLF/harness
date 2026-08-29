//! What the Code screen reads: the file tree of a worktree, one file's text,
//! and a diff broken into hunks.
//!
//! Git prints; this decides. Every function here is pure over what git wrote
//! to stdout, so the parsing is tested without a repository on disk and the
//! Tauri command stays a shell that runs the process.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use ts_rs::TS;

/// A file's text is refused past this, because the pane renders every line
/// into the DOM and a megabyte of source is already 20k rows.
pub const MAX_FILE_BYTES: u64 = 1_048_576;

/// One row of the file tree. The path is full and relative to the worktree;
/// depth is the number of separators in it, so the tree ships flat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct TreeEntry {
    pub path: String,
    /// `dir` or `file`.
    pub kind: String,
    /// Changed against HEAD, or untracked. A directory carries it when
    /// anything under it does: a folder the operator has collapsed must still
    /// say that something moved inside it.
    pub dirty: bool,
}

/// One file, as the read-only pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct FileText {
    pub path: String,
    pub text: String,
    /// The grammar id the highlighter will want, or `text`.
    pub lang: String,
    /// Bytes on disk, reported even when the text was refused.
    pub size: u64,
    /// A NUL byte near the front. The text is empty and the pane says so
    /// rather than printing the bytes.
    pub binary: bool,
    /// Cut at `MAX_FILE_BYTES`.
    pub truncated: bool,
}

/// One line inside a hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct HunkLine {
    /// `+`, `-` or a space.
    pub sign: String,
    pub text: String,
    /// Where this line sits in the file before the change; absent on an
    /// addition.
    pub old_line: Option<u32>,
    /// Where it sits after; absent on a removal. This is what the source pane
    /// matches its gutter against.
    pub new_line: Option<u32>,
}

/// One `@@` block of a unified diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct Hunk {
    /// Path relative to the worktree.
    pub file: String,
    /// The `@@ … @@` line, verbatim, which is also this hunk's identity when
    /// an approval names it.
    pub header: String,
    /// The grammar id for `file`, or `text`. Carried here so the review panel
    /// can highlight a block without a second read of the file it came from.
    pub lang: String,
    /// What git names after the second `@@`: the enclosing function, when it
    /// could work it out. Empty otherwise.
    pub symbol: String,
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<HunkLine>,
    pub added: u32,
    pub removed: u32,
}

impl Hunk {
    /// The domain's name for this block.
    ///
    /// The screen decides hunks, and the log records which one — so the two
    /// halves have to agree on identity. They agree on git's own: the file and
    /// the `@@` header, converted here rather than assembled at each call site.
    pub fn as_ref(&self) -> harness_domain::HunkRef {
        harness_domain::HunkRef {
            file: self.file.clone(),
            header: self.header.clone(),
            new_start: self.new_start,
            // How much of the file after the change this block covers: every
            // line the hunk keeps or adds, which is what the reader sees.
            new_lines: self.lines.iter().filter(|l| l.new_line.is_some()).count() as u32,
        }
    }
}

// ---- the tree --------------------------------------------------------------

/// Split `git status --porcelain` into the paths it names.
///
/// Porcelain v1 is `XY <path>`, and a rename is `XY <old> -> <new>` — the new
/// name is the one that exists. An untracked directory arrives with a trailing
/// slash and stands for everything under it.
fn status_paths(status: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for line in status.lines() {
        if line.len() < 4 {
            continue;
        }
        let path = &line[3..];
        let path = path.rsplit(" -> ").next().unwrap_or(path);
        let path = path.trim().trim_matches('"');
        if !path.is_empty() {
            out.insert(path.to_string());
        }
    }
    out
}

/// Sort key: parents before children, and directories before files at every
/// level. The rank byte is what puts a folder above its siblings — it is 0 for
/// any segment that has something under it, and 1 only for a file's last one.
fn sort_key(path: &str, is_dir: bool) -> Vec<(u8, String)> {
    let parts: Vec<&str> = path.split('/').collect();
    let last = parts.len() - 1;
    parts
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let rank = if i == last && !is_dir { 1 } else { 0 };
            (rank, (*seg).to_string())
        })
        .collect()
}

/// The tree of a worktree: everything git tracks, plus anything untracked that
/// is sitting in it, with the directories that hold them.
///
/// Untracked files are in on purpose — a file an agent just wrote is the one
/// the operator most wants to open, and `ls-files` alone would not show it.
pub fn build_tree(tracked: &[String], status: &str) -> Vec<TreeEntry> {
    let changed = status_paths(status);

    let mut files: HashSet<String> = tracked.iter().cloned().collect();
    let mut dirs: HashSet<String> = HashSet::new();
    for path in &changed {
        if let Some(dir) = path.strip_suffix('/') {
            dirs.insert(dir.to_string());
        } else {
            files.insert(path.clone());
        }
    }

    let seeds: Vec<String> = files.iter().chain(dirs.iter()).cloned().collect();
    for path in &seeds {
        let mut cut = path.as_str();
        while let Some(at) = cut.rfind('/') {
            cut = &cut[..at];
            dirs.insert(cut.to_string());
        }
    }

    // A path is dirty when status named it, or named a directory above it.
    let dirty_prefixes: Vec<String> = changed
        .iter()
        .filter(|p| p.ends_with('/'))
        .cloned()
        .collect();
    let is_dirty = |path: &str| {
        changed.contains(path) || dirty_prefixes.iter().any(|p| path.starts_with(p.as_str()))
    };

    let mut entries: Vec<TreeEntry> = files
        .iter()
        .map(|path| TreeEntry {
            path: path.clone(),
            kind: "file".to_string(),
            dirty: is_dirty(path),
        })
        .collect();

    // A directory inherits the state of what it holds, so a collapsed folder
    // still carries the marker of the change inside it.
    let dirty_dirs: HashSet<String> = entries
        .iter()
        .filter(|e| e.dirty)
        .flat_map(|e| {
            let mut acc = Vec::new();
            let mut cut = e.path.as_str();
            while let Some(at) = cut.rfind('/') {
                cut = &cut[..at];
                acc.push(cut.to_string());
            }
            acc
        })
        .collect();

    entries.extend(dirs.iter().map(|path| TreeEntry {
        path: path.clone(),
        kind: "dir".to_string(),
        dirty: dirty_dirs.contains(path) || is_dirty(path),
    }));

    entries.sort_by(|a, b| sort_key(&a.path, a.kind == "dir").cmp(&sort_key(&b.path, b.kind == "dir")));
    entries
}

// ---- one file --------------------------------------------------------------

/// Refuse anything that could climb out of the worktree. The path arrives from
/// the window, and the window is allowed to read the worktree, not the disk.
pub fn safe_relative(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() || path.starts_with('/') || path.starts_with('\\') {
        return None;
    }
    // A Windows drive letter is absolute too, without a leading separator.
    if path.chars().nth(1) == Some(':') {
        return None;
    }
    let cleaned: Vec<&str> = path.split(['/', '\\']).collect();
    if cleaned.iter().any(|seg| *seg == ".." || seg.is_empty()) {
        return None;
    }
    Some(cleaned.join("/"))
}

/// The grammar id for a path, in the vocabulary a web highlighter uses.
pub fn language_for(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let by_name = match name {
        "Dockerfile" => Some("dockerfile"),
        "Makefile" | "makefile" => Some("makefile"),
        "Cargo.lock" => Some("toml"),
        _ => None,
    };
    if let Some(lang) = by_name {
        return lang.to_string();
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let lang = match ext {
        "rs" => "rust",
        "ts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "json" => "json",
        "toml" => "toml",
        "yml" | "yaml" => "yaml",
        "md" | "markdown" => "markdown",
        "css" => "css",
        "scss" => "scss",
        "html" | "htm" => "html",
        "sh" | "bash" | "zsh" => "shell",
        "py" => "python",
        "go" => "go",
        "sql" => "sql",
        "svg" | "xml" => "xml",
        _ => "text",
    };
    lang.to_string()
}

/// Bytes that are not text. Git's own rule: a NUL in the first block.
fn looks_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8_000)];
    head.contains(&0)
}

/// What the pane is told about a file. `size` is the size on disk, which is
/// not `bytes.len()` once the read was capped.
pub fn file_text(path: &str, size: u64, bytes: &[u8]) -> FileText {
    let lang = language_for(path);
    if looks_binary(bytes) {
        return FileText {
            path: path.to_string(),
            text: String::new(),
            lang,
            size,
            binary: true,
            truncated: false,
        };
    }
    let cap = MAX_FILE_BYTES as usize;
    let truncated = bytes.len() > cap || size > MAX_FILE_BYTES;
    let slice = &bytes[..bytes.len().min(cap)];
    FileText {
        path: path.to_string(),
        text: String::from_utf8_lossy(slice).into_owned(),
        lang,
        size,
        binary: false,
        truncated,
    }
}

// ---- hunks -----------------------------------------------------------------

/// `@@ -12,7 +12,9 @@ pub fn resolve_profile(` → the two starts and the symbol.
fn parse_header(line: &str) -> Option<(u32, u32, String)> {
    let rest = line.strip_prefix("@@ ")?;
    let (ranges, tail) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(' ')?;
    let start = |r: &str| -> Option<u32> {
        let digits = r.trim_start_matches(['-', '+']);
        let head = digits.split(',').next()?;
        head.parse().ok()
    };
    Some((start(old)?, start(new)?, tail.trim().to_string()))
}

/// Every `@@` block of a unified diff, in the order git wrote them.
///
/// `only` narrows to one file without a second git call, which is what the
/// source pane wants: it already has the whole card's diff in hand.
pub fn parse_hunks(diff: &str, only: Option<&str>) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    let mut file = String::new();
    let mut open: Option<Hunk> = None;
    let mut old_at = 0u32;
    let mut new_at = 0u32;

    let close = |open: &mut Option<Hunk>, out: &mut Vec<Hunk>, only: Option<&str>| {
        if let Some(hunk) = open.take() {
            if only.is_none_or(|p| p == hunk.file) {
                out.push(hunk);
            }
        }
    };

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            close(&mut open, &mut out, only);
            // `a/path b/path`; the second name is the one that exists after.
            file = rest
                .rsplit_once(" b/")
                .map(|(_, b)| b.to_string())
                .unwrap_or_default();
            continue;
        }
        if let Some(rest) = line.strip_prefix("+++ b/") {
            file = rest.to_string();
            continue;
        }
        if line.starts_with("@@ ") {
            close(&mut open, &mut out, only);
            if let Some((old_start, new_start, symbol)) = parse_header(line) {
                old_at = old_start;
                new_at = new_start;
                open = Some(Hunk {
                    lang: language_for(&file),
                    file: file.clone(),
                    header: line.to_string(),
                    symbol,
                    old_start,
                    new_start,
                    lines: Vec::new(),
                    added: 0,
                    removed: 0,
                });
            }
            continue;
        }
        let Some(hunk) = open.as_mut() else { continue };
        if line.starts_with("\\ No newline") {
            continue;
        }
        let (sign, text) = match line.chars().next() {
            Some('+') => ("+", &line[1..]),
            Some('-') => ("-", &line[1..]),
            Some(' ') => (" ", &line[1..]),
            // A bare empty line inside a hunk is an empty context line: git
            // drops the trailing space some versions would have written.
            None => (" ", ""),
            // Anything else ends the block — the stat footer, a `similarity`
            // header, the start of the next file.
            _ => {
                close(&mut open, &mut out, only);
                continue;
            }
        };
        let (old_line, new_line) = match sign {
            "+" => {
                hunk.added += 1;
                let at = new_at;
                new_at += 1;
                (None, Some(at))
            }
            "-" => {
                hunk.removed += 1;
                let at = old_at;
                old_at += 1;
                (Some(at), None)
            }
            _ => {
                let old = old_at;
                let new = new_at;
                old_at += 1;
                new_at += 1;
                (Some(old), Some(new))
            }
        };
        hunk.lines.push(HunkLine {
            sign: sign.to_string(),
            text: text.to_string(),
            old_line,
            new_line,
        });
    }
    close(&mut open, &mut out, only);
    out
}

// ---- the tracked-file cache ------------------------------------------------

/// The tracked-file list of a worktree, remembered against the commit it was
/// read at.
///
/// `git ls-files` walks the whole index and is the slow half of building a
/// tree; `git status` is the fast half and is never cached, because it is the
/// half that changes between two keystrokes. What moves the tracked set is a
/// commit, so the sha is the key: while HEAD holds still the list is reused,
/// and the moment a run commits it is read again.
#[derive(Default)]
pub struct TreeCache {
    inner: Mutex<HashMap<PathBuf, (String, Vec<String>)>>,
}

impl TreeCache {
    pub fn tracked(
        &self,
        worktree: &Path,
        head: &str,
        read: impl FnOnce() -> Vec<String>,
    ) -> Vec<String> {
        if let Ok(map) = self.inner.lock() {
            if let Some((at, files)) = map.get(worktree) {
                if at == head && !head.is_empty() {
                    return files.clone();
                }
            }
        }
        let files = read();
        if let Ok(mut map) = self.inner.lock() {
            map.insert(worktree.to_path_buf(), (head.to_string(), files.clone()));
        }
        files
    }

    /// Drop a worktree, for when it is removed from disk.
    pub fn forget(&self, worktree: &Path) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(worktree);
        }
    }
}

pub static TREE_CACHE: LazyLock<TreeCache> = LazyLock::new(TreeCache::default);

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(entries: &[TreeEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn tree_holds_folders_above_files_and_parents_above_children() {
        let tracked = vec![
            "Cargo.toml".to_string(),
            "src/main.rs".to_string(),
            "src/lib/util.rs".to_string(),
        ];
        let tree = build_tree(&tracked, "");
        assert_eq!(
            paths(&tree),
            vec!["src", "src/lib", "src/lib/util.rs", "src/main.rs", "Cargo.toml"]
        );
    }

    #[test]
    fn untracked_files_are_in_the_tree_and_marked() {
        let tracked = vec!["src/main.rs".to_string()];
        let tree = build_tree(&tracked, "?? src/new.rs\n M src/main.rs\n");
        let dirty: Vec<&str> = tree
            .iter()
            .filter(|e| e.dirty)
            .map(|e| e.path.as_str())
            .collect();
        assert!(paths(&tree).contains(&"src/new.rs"));
        // The folder carries what is under it.
        assert_eq!(dirty, vec!["src", "src/main.rs", "src/new.rs"]);
    }

    #[test]
    fn a_rename_is_filed_under_its_new_name() {
        let tree = build_tree(&["a.rs".to_string()], "R  a.rs -> b.rs\n");
        assert!(tree.iter().any(|e| e.path == "b.rs" && e.dirty));
    }

    #[test]
    fn an_untracked_directory_marks_everything_under_it() {
        let tracked = vec!["out/one.rs".to_string()];
        let tree = build_tree(&tracked, "?? out/\n");
        assert!(tree.iter().any(|e| e.path == "out/one.rs" && e.dirty));
        assert!(tree.iter().any(|e| e.path == "out" && e.dirty));
    }

    #[test]
    fn paths_that_climb_out_are_refused() {
        assert_eq!(safe_relative("src/main.rs").as_deref(), Some("src/main.rs"));
        assert_eq!(safe_relative("src\\main.rs").as_deref(), Some("src/main.rs"));
        assert!(safe_relative("../secrets").is_none());
        assert!(safe_relative("/etc/passwd").is_none());
        assert!(safe_relative("C:/Windows").is_none());
        assert!(safe_relative("src//main.rs").is_none());
        assert!(safe_relative("  ").is_none());
    }

    #[test]
    fn binary_is_a_stub_and_long_files_say_they_were_cut() {
        let stub = file_text("logo.png", 12, &[0x89, 0x50, 0x00, 0x0d]);
        assert!(stub.binary);
        assert!(stub.text.is_empty());
        assert_eq!(stub.size, 12);

        let long = vec![b'x'; MAX_FILE_BYTES as usize + 10];
        let cut = file_text("big.rs", long.len() as u64, &long);
        assert!(cut.truncated);
        assert_eq!(cut.text.len(), MAX_FILE_BYTES as usize);
        assert_eq!(cut.lang, "rust");
    }

    #[test]
    fn languages_come_off_the_name() {
        assert_eq!(language_for("crates/app/src/code.rs"), "rust");
        assert_eq!(language_for("src/views/Code.tsx"), "tsx");
        assert_eq!(language_for("Dockerfile"), "dockerfile");
        assert_eq!(language_for("notes"), "text");
    }

    /// A diff as git prints one. Written line by line rather than as one
    /// literal because the leading space of a context line is the marker, and
    /// a `\` continuation in a Rust string would eat it.
    fn diff() -> String {
        [
            "diff --git a/src/policy.rs b/src/policy.rs",
            "index 1111111..2222222 100644",
            "--- a/src/policy.rs",
            "+++ b/src/policy.rs",
            "@@ -14,6 +14,8 @@ pub fn resolve_profile(agent: &AgentProfile) -> RunProfile {",
            "   RunProfile {",
            "     model,",
            "-    budget,",
            "+    budget: budget.clamp(MIN_RUN_BUDGET, caps.per_run_cap),",
            "+    allow_push: false,",
            "     review,",
            "   }",
            "diff --git a/README.md b/README.md",
            "--- a/README.md",
            "+++ b/README.md",
            "@@ -1 +1,2 @@",
            " # Relay",
            "+A harness.",
            "",
        ]
        .join("\n")
    }

    #[test]
    fn hunks_carry_their_file_symbol_and_line_numbers() {
        let diff = diff();
        let hunks = parse_hunks(&diff, None);
        assert_eq!(hunks.len(), 2);

        let first = &hunks[0];
        assert_eq!(first.file, "src/policy.rs");
        assert_eq!(
            first.symbol,
            "pub fn resolve_profile(agent: &AgentProfile) -> RunProfile {"
        );
        assert_eq!(first.old_start, 14);
        assert_eq!(first.new_start, 14);
        assert_eq!(first.added, 2);
        assert_eq!(first.removed, 1);

        // Context counts on both sides; an addition only on the new one.
        let added: Vec<u32> = first
            .lines
            .iter()
            .filter(|l| l.sign == "+")
            .filter_map(|l| l.new_line)
            .collect();
        assert_eq!(added, vec![16, 17]);
        let removed = first.lines.iter().find(|l| l.sign == "-").unwrap();
        assert_eq!(removed.old_line, Some(16));
        assert_eq!(removed.new_line, None);
        // The last context line of the block lands after both additions.
        let last = first.lines.last().unwrap();
        assert_eq!(last.sign, " ");
        assert_eq!(last.new_line, Some(19));
    }

    #[test]
    fn a_hunk_carries_its_grammar_and_its_domain_identity() {
        let hunks = parse_hunks(&diff(), None);
        assert_eq!(hunks[0].lang, "rust");
        assert_eq!(hunks[1].lang, "markdown");

        let named = hunks[0].as_ref();
        assert_eq!(named.file, "src/policy.rs");
        assert_eq!(named.header, hunks[0].header);
        assert_eq!(named.new_start, 14);
        // Six lines exist in the new file: four context, two added.
        assert_eq!(named.new_lines, 6);
    }

    #[test]
    fn one_file_can_be_asked_for() {
        let hunks = parse_hunks(&diff(), Some("README.md"));
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file, "README.md");
        assert_eq!(hunks[0].added, 1);
    }

    #[test]
    fn a_diff_with_nothing_in_it_is_no_hunks() {
        assert!(parse_hunks("", None).is_empty());
        assert!(parse_hunks("diff --git a/x b/x\n", None).is_empty());
    }

    #[test]
    fn the_cache_reads_once_per_commit() {
        let cache = TreeCache::default();
        let dir = PathBuf::from("/tmp/wt");
        let first = cache.tracked(&dir, "abc", || vec!["a.rs".to_string()]);
        let second = cache.tracked(&dir, "abc", || panic!("read twice at the same sha"));
        assert_eq!(first, second);
        // A commit moved: the list is read again.
        let third = cache.tracked(&dir, "def", || vec!["a.rs".to_string(), "b.rs".to_string()]);
        assert_eq!(third.len(), 2);
    }

    #[test]
    fn an_unborn_head_is_never_cached() {
        let cache = TreeCache::default();
        let dir = PathBuf::from("/tmp/empty");
        cache.tracked(&dir, "", Vec::new);
        let again = cache.tracked(&dir, "", || vec!["a.rs".to_string()]);
        assert_eq!(again, vec!["a.rs".to_string()]);
    }
}
