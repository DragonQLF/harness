//! What `node` and `claude` are worth depends entirely on being able to find
//! them, and a window opened from Finder cannot.
//!
//! A GUI launch on macOS inherits launchd's PATH — `/usr/bin:/bin:/usr/sbin:
//! /sbin` — not the one the operator's shell spends its startup building.
//! Homebrew, nvm, volta and `~/.local/bin` are all outside it, so a machine
//! with Node installed and working in a terminal still reports no Node here.
//! Ask the login shell what it would have, and take that.

use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

/// Delimits the answer, because an interactive shell prints whatever the
/// operator's rc files feel like printing and it all lands on the same stdout.
const MARK: &str = "__RELAY_PATH__";

/// A shell that will not answer must not be allowed to hold the window shut.
const PATIENCE: Duration = Duration::from_secs(3);

/// Places a Node or a Claude is commonly installed, used only if asking fails.
const LIKELY: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

/// Merge what the shell reported into what we already have, keeping the
/// existing entries first and dropping repeats. Order is the whole meaning of
/// a PATH, so nothing already there moves.
fn merge(current: &str, found: &str) -> String {
    let mut out: Vec<&str> = current.split(':').filter(|p| !p.is_empty()).collect();
    for part in found.split(':').filter(|p| !p.is_empty()) {
        if !out.contains(&part) {
            out.push(part);
        }
    }
    out.join(":")
}

/// Ask the login shell for its PATH. `-ilc` so the rc files that set up nvm and
/// friends actually run; the marker is what makes their output survivable.
fn ask_login_shell() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    let script = format!("printf '{MARK}%s{MARK}' \"$PATH\"");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let out = Command::new(&shell).args(["-ilc", &script]).output();
        let _ = tx.send(out.ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()));
    });
    // A shell wedged on a prompt or a slow rc file is a hang the operator would
    // read as a broken app, so it is given a deadline rather than forever.
    let text = rx.recv_timeout(PATIENCE).ok()??;
    let mut parts = text.split(MARK);
    parts.next()?;
    let path = parts.next()?.trim().to_string();
    (!path.is_empty()).then_some(path)
}

/// Adopt the operator's real PATH, once, before anything goes looking for a
/// binary. Safe to call when it is already right: merging changes nothing.
pub fn adopt() {
    let current = std::env::var("PATH").unwrap_or_default();
    let found = ask_login_shell().unwrap_or_else(|| {
        // The shell said nothing useful. The common locations are a poorer
        // answer than its own, but a better one than the launchd default.
        LIKELY
            .iter()
            .filter(|d| std::path::Path::new(d).is_dir())
            .copied()
            .collect::<Vec<_>>()
            .join(":")
    });
    let merged = merge(&current, &found);
    if merged != current {
        std::env::set_var("PATH", merged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_already_there_keeps_its_place_and_is_not_repeated() {
        let got = merge("/usr/bin:/bin", "/opt/homebrew/bin:/usr/bin:/usr/local/bin");
        assert_eq!(got, "/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin");
    }

    #[test]
    fn a_shell_that_says_nothing_leaves_the_path_alone() {
        assert_eq!(merge("/usr/bin:/bin", ""), "/usr/bin:/bin");
    }

    #[test]
    fn empty_segments_do_not_become_the_current_directory() {
        // A stray colon means "here", which is not somewhere we want searched.
        assert_eq!(merge("/usr/bin::", ":/opt/homebrew/bin:"), "/usr/bin:/opt/homebrew/bin");
    }
}
