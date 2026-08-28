//! Relay's own source, and how it finds it.
//!
//! Mirror mode is not a flag a project can be given. It is one specific
//! repository — the one this binary was built from — and everything the mode
//! does only makes sense against that repository: `read_docs` reads *these*
//! decisions, an accepted proposal becomes a card in *this* code, and the
//! post-run build compiles *this* app. Pointed anywhere else, each of those
//! is quietly wrong: the Director would read a stranger's DEBT.md, file cards
//! into someone's website, and try to `pnpm tauri build` a repository that has
//! no Tauri in it.
//!
//! So the operator does not nominate a project. They ask for mirror mode, and
//! Relay goes and gets its own source:
//!
//! 1. a project already registered whose remote is this repository;
//! 2. the checkout this binary was built from, when there is one — a developer
//!    running `tauri dev` already has the source on disk and cloning a second
//!    copy would leave them editing the wrong one;
//! 3. otherwise, clone it into app data.
//!
//! Step 2 is why this is worth doing carefully. The wrong answer is not "no
//! mirror project" — it is a mirror project pointing at a stale copy of the
//! code the operator is actually working on.

/// Where Relay's source lives. Public, so the clone needs no credentials.
pub const REPO_URL: &str = "https://github.com/DragonQLF/harness.git";

/// The folder a clone lands in, under app data.
pub const CLONE_DIR: &str = "relay-source";

/// What the operator is told, and what has to happen next.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Already registered as a project; nothing to do but mark it.
    Registered(String),
    /// A checkout on disk that is not a project yet.
    OnDisk(std::path::PathBuf),
    /// Nothing local: it has to be cloned to this path.
    Clone(std::path::PathBuf),
}

/// Does this remote point at Relay's own repository?
///
/// Compared by owner and name rather than by string: the same repository is
/// reachable as https, ssh, with or without `.git`, and a mirror project that
/// depends on which form was typed is a mirror project that silently is not one.
pub fn is_relay_remote(remote: &str) -> bool {
    fn slug(url: &str) -> Option<String> {
        let url = url.trim().trim_end_matches('/');
        let url = url.strip_suffix(".git").unwrap_or(url);
        let tail = url
            .rsplit_once("github.com")
            .map(|(_, tail)| tail.trim_start_matches([':', '/']))?;
        let mut parts = tail.split('/');
        let owner = parts.next()?;
        let name = parts.next()?;
        Some(format!("{}/{}", owner.to_lowercase(), name.to_lowercase()))
    }
    match (slug(remote), slug(REPO_URL)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// The checkout this binary was built from, when it is still there.
///
/// Only meaningful while developing: an installed build's manifest path points
/// at whatever machine produced it, so the directory is checked for a `.git`
/// before it is believed.
pub fn built_from() -> Option<std::path::PathBuf> {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.parent()?.parent()?;
    (root.join(".git").exists() && root.join("src-tauri").exists()).then(|| root.to_path_buf())
}

/// Where mirror mode should point, given what is already registered.
///
/// `remotes` is each project's id and its origin, because a project is
/// recognised by what it points at rather than by what it is called.
pub fn locate(
    remotes: &[(String, Option<String>)],
    appdata: &std::path::Path,
) -> Source {
    if let Some((id, _)) = remotes
        .iter()
        .find(|(_, remote)| remote.as_deref().is_some_and(is_relay_remote))
    {
        return Source::Registered(id.clone());
    }
    if let Some(local) = built_from() {
        return Source::OnDisk(local);
    }
    Source::Clone(appdata.join(CLONE_DIR))
}

/// The last commit of the mirror repository Relay saw. Written to app data so
/// the comparison survives a restart; a file that is missing means "first
/// look", which reports nothing.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Watch {
    #[serde(default)]
    pub sha: String,
    #[serde(default)]
    pub checked_ms: u64,
}

/// The commit to compare against, or `None` when there is nothing to compare.
///
/// Two cases answer `None`, and they must both stay silent. The first look —
/// no sha recorded — records where the repository stands and says nothing:
/// dumping a whole history the first time Relay opens its eyes is noise, not a
/// signal. And an unmoved head is simply calm.
pub fn base_to_compare(known: &Watch, head: &str) -> Option<String> {
    let known = known.sha.trim();
    if known.is_empty() || known == head.trim() {
        return None;
    }
    Some(known.to_string())
}

/// What happened to Relay's own source while nobody on the board was looking.
///
/// The board only knows what came through it. When someone works on the Relay
/// repository without Relay — the operator in an editor, an infrastructure
/// agent, a migration — cards describing work that is already done sit in
/// Ready, DEBT.md stops matching the code, and the Director meets behaviour
/// that contradicts what he believes with no way to find out why.
///
/// Detection is deliberately shallow: how many, which files, since when. What
/// the commits *mean* is the operator's to judge.
#[derive(Debug, Clone, PartialEq)]
pub struct OutsideWork {
    pub commits: usize,
    /// Distinct paths touched, sorted, capped by [`FILES_NAMED`].
    pub files: Vec<String>,
    /// How many distinct paths there were before the cap.
    pub files_total: usize,
    /// The oldest of them, in milliseconds. Zero when git said nothing.
    pub since_ms: u64,
}

/// A warning is a pointer, not a changelog. Past this many paths the list stops
/// helping and starts being the diff.
pub const FILES_NAMED: usize = 12;

/// Fold the commits git found into the one fact worth reporting. `None` when
/// nothing came in outside the board — which is the normal case, and must not
/// produce an empty warning.
pub fn outside_work(commits: &[(u64, Vec<String>)]) -> Option<OutsideWork> {
    if commits.is_empty() {
        return None;
    }
    let mut files: Vec<String> = commits
        .iter()
        .flat_map(|(_, f)| f.iter().cloned())
        .collect();
    files.sort();
    files.dedup();
    let files_total = files.len();
    files.truncate(FILES_NAMED);
    let since_ms = commits
        .iter()
        .map(|(ts, _)| *ts)
        .filter(|ts| *ts > 0)
        .min()
        .unwrap_or(0);
    Some(OutsideWork {
        commits: commits.len(),
        files,
        files_total,
        since_ms,
    })
}

/// The warning, in words: how many commits, which files, since when.
///
/// Written for the Director to receive and for the operator to read, and it
/// says what he may do about it — which is to flag, never to act. Closing a
/// card or rewriting a document on this evidence would be deciding on the
/// operator's behalf from a file list (#79: the inbox proposes, never creates).
pub fn describe(work: &OutsideWork, now_ms: u64) -> String {
    let mut out = format!(
        "{} commit{} reached Relay's own repository without a card behind it",
        work.commits,
        if work.commits == 1 { "" } else { "s" }
    );
    match age(work.since_ms, now_ms) {
        Some(when) => out.push_str(&format!(", the oldest {when}")),
        None => out.push_str(" (git did not say when)"),
    }
    out.push_str(". Files touched: ");
    if work.files.is_empty() {
        out.push_str("none that git named");
    } else {
        out.push_str(&work.files.join(", "));
        if work.files_total > work.files.len() {
            out.push_str(&format!(
                " (and {} more)",
                work.files_total - work.files.len()
            ));
        }
    }
    out.push_str(
        ". That is work the board never saw, so cards may describe things already done and \
         DEBT.md may no longer match the code. Say which open cards and which documents are \
         worth re-reading because of it, and stop there: do not close a card, do not move \
         anything, do not rewrite a document. The operator decides.",
    );
    out
}

/// "2 days ago", "3 hours ago". `None` when there is no usable timestamp.
fn age(then_ms: u64, now_ms: u64) -> Option<String> {
    if then_ms == 0 || now_ms < then_ms {
        return None;
    }
    let secs = (now_ms - then_ms) / 1000;
    Some(match secs {
        0..=90 => "just now".to_string(),
        91..=5399 => format!("{} minutes ago", secs / 60),
        5400..=172_799 => format!("{} hours ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn nothing_outside_the_board_is_not_a_warning() {
        assert_eq!(outside_work(&[]), None, "silence must not become a warning");
    }

    /// The first look records where the repository stands and reports nothing.
    /// Otherwise every fresh install would open with a history dump.
    #[test]
    fn the_first_look_records_and_says_nothing() {
        assert_eq!(base_to_compare(&Watch::default(), "abc123"), None);
        let known = Watch { sha: "abc123".into(), checked_ms: NOW };
        assert_eq!(base_to_compare(&known, "abc123"), None, "nothing moved");
        assert_eq!(
            base_to_compare(&known, "def456"),
            Some("abc123".to_string()),
            "it moved: compare from where we left off"
        );
    }

    /// The warning has to carry all three: how many, which files, since when.
    #[test]
    fn the_warning_says_how_many_which_files_and_since_when() {
        let two_days = NOW - 2 * 86_400_000;
        let work = outside_work(&[
            (NOW - 3_600_000, vec!["src/App.tsx".into(), "tailwind.config.js".into()]),
            (two_days, vec!["src/App.tsx".into(), "docs/DEBT.md".into()]),
        ])
        .expect("two commits outside the board");
        assert_eq!(work.commits, 2);
        assert_eq!(work.since_ms, two_days, "the oldest of them");
        assert_eq!(
            work.files,
            vec![
                "docs/DEBT.md".to_string(),
                "src/App.tsx".to_string(),
                "tailwind.config.js".to_string()
            ],
            "distinct paths, sorted"
        );

        let said = describe(&work, NOW);
        assert!(said.contains("2 commits"), "{said}");
        assert!(said.contains("2 days ago"), "{said}");
        assert!(said.contains("docs/DEBT.md"), "{said}");
        // And what he may do with it: flag, never act.
        assert!(said.contains("do not close a card"), "{said}");
        assert!(said.contains("The operator decides"), "{said}");
    }

    #[test]
    fn a_long_file_list_is_capped_but_the_count_is_not() {
        let files: Vec<String> = (0..30).map(|n| format!("src/f{n:02}.tsx")).collect();
        let work = outside_work(&[(NOW - 60_000, files)]).unwrap();
        assert_eq!(work.files.len(), FILES_NAMED);
        assert_eq!(work.files_total, 30);
        assert!(describe(&work, NOW).contains("and 18 more"));
    }

    #[test]
    fn a_commit_with_no_usable_timestamp_still_reports_the_rest() {
        let work = outside_work(&[(0, vec!["Cargo.toml".into()])]).unwrap();
        assert_eq!(work.since_ms, 0);
        let said = describe(&work, NOW);
        assert!(said.contains("1 commit reached"), "{said}");
        assert!(said.contains("git did not say when"), "{said}");
    }

    #[test]
    fn the_same_repository_is_recognised_however_it_was_typed() {
        for form in [
            "https://github.com/DragonQLF/harness.git",
            "https://github.com/DragonQLF/harness",
            "git@github.com:DragonQLF/harness.git",
            "https://github.com/dragonqlf/HARNESS/",
        ] {
            assert!(is_relay_remote(form), "{form} is the same repository");
        }
        for other in [
            "https://github.com/DragonQLF/something-else.git",
            "https://github.com/someone/harness.git",
            "https://gitlab.com/DragonQLF/harness.git",
            "",
        ] {
            assert!(!is_relay_remote(other), "{other} is not");
        }
    }

    #[test]
    fn an_already_registered_clone_is_used_rather_than_a_second_one() {
        let registered = vec![
            ("site".to_string(), Some("https://github.com/x/site.git".to_string())),
            ("relay".to_string(), Some("git@github.com:DragonQLF/harness.git".to_string())),
        ];
        assert_eq!(
            locate(&registered, Path::new("C:/appdata")),
            Source::Registered("relay".to_string()),
            "cloning a second copy would leave the operator editing the wrong one"
        );
    }

    #[test]
    fn with_nothing_registered_it_falls_back_to_a_clone() {
        // `built_from` answers on a developer machine and not in an installed
        // build, so only the shape of the fallback is asserted here.
        let nothing: Vec<(String, Option<String>)> = vec![
            ("site".to_string(), None),
            ("other".to_string(), Some("https://github.com/x/y".to_string())),
        ];
        match locate(&nothing, Path::new("C:/appdata")) {
            Source::OnDisk(path) => assert!(path.join(".git").exists()),
            Source::Clone(path) => assert!(path.ends_with(CLONE_DIR)),
            Source::Registered(_) => panic!("nothing registered points at Relay"),
        }
    }
}
