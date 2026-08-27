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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
