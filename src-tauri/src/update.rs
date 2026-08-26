//! Installing a mirror build, and surviving it.
//!
//! The dance, in full:
//!
//! 1. install: rename the running exe to `<stem>.previous<ext>` (a running
//!    exe cannot be overwritten on Windows, but it can be renamed), copy the
//!    new binary into place, write the "startup in progress" marker, relaunch;
//! 2. the new process runs `setup`; when it completes, the marker is cleared;
//! 3. if a later start finds the marker still there, the previous launch
//!    never got healthy: restore the saved binary over the current one and
//!    say why. A start that fails twice rolls back by itself.
//!
//! Everything here is plain file arithmetic on paths handed in by the caller,
//! so the round-trip is testable without launching anything.

use std::path::{Path, PathBuf};

/// One artefact waiting in `updates/<card-id>/`: a manifest plus the binary
/// it vouches for.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PendingUpdate {
    pub card_id: String,
    pub commit_sha: String,
    pub built_at_ms: u64,
    /// Absolute path to the parked binary.
    pub binary: PathBuf,
    /// `card` when an agent's run produced it, `build` when it is simply a
    /// newer binary sitting in the repository's target directory.
    pub kind: String,
}

fn marker_default_name() -> &'static str {
    "update-in-progress.json"
}

/// Every card directory under `updates/` whose manifest parses and whose
/// binary is actually there. A manifest without its binary is a broken
/// promise and is skipped, not shown.
pub fn list_pending(updates_dir: &Path) -> Vec<PendingUpdate> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(updates_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(dir.join("manifest.json")) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        let (Some(card_id), Some(sha)) = (
            value.get("card_id").and_then(|v| v.as_str()),
            value.get("commit_sha").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let Some(binary) = value.get("binary").and_then(|v| v.as_str()) else {
            continue;
        };
        let binary = dir.join(binary);
        if !binary.is_file() {
            continue;
        }
        out.push(PendingUpdate {
            kind: "card".to_string(),
            card_id: card_id.to_string(),
            commit_sha: sha.to_string(),
            built_at_ms: value
                .get("built_at_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            binary,
        });
    }
    out.sort_by(|a, b| b.built_at_ms.cmp(&a.built_at_ms));
    out
}

/// The rename-and-copy swap. The current exe moves aside (legal while it
/// runs), the incoming binary takes its place, and the marker goes down
/// *before* anything launches — so a launch that never gets healthy is
/// detectable.
pub fn swap(
    exe: &Path,
    incoming: &Path,
    backup: &Path,
    marker: &Path,
    info: &serde_json::Value,
) -> Result<(), String> {
    std::fs::rename(exe, backup).map_err(|e| format!("could not set the old binary aside: {e}"))?;
    if let Err(e) = std::fs::copy(incoming, exe) {
        // Put the original back; leaving no app at all is not an option.
        let restore = std::fs::rename(backup, exe);
        return Err(format!(
            "could not put the new binary in place ({e});{}",
            restore.map(|_| " the previous one is back".to_string()).unwrap_or_else(|_| " AND the previous one could not be restored — it is at the .previous path".to_string())
        ));
    }
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(marker, info.to_string())
        .map_err(|e| format!("could not write the startup marker: {e}"))?;
    Ok(())
}

/// Called at the very top of startup. `Some(reason)` means the last update
/// never got healthy and has been rolled back — the caller must say so out
/// loud. `None` means business as usual. Either way a healthy boot clears
/// the marker afterwards (`mark_healthy`).
pub fn recover_if_needed(marker: &Path, exe: &Path, backup: &Path) -> Option<String> {
    let Ok(raw) = std::fs::read_to_string(marker) else {
        return None;
    };
    let info: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    let card = info
        .get("card_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let reason = if !backup.is_file() {
        // Nothing to roll back to: leave the new binary, but say what happened.
        std::fs::remove_file(marker).ok();
        format!(
            "the update from card {card} never started cleanly, and the previous \
             binary is missing; staying on the current one"
        )
    } else {
        match std::fs::copy(backup, exe) {
            Ok(_) => format!(
                "the update from card {card} did not start cleanly; rolled back to \
                 the previous version"
            ),
            Err(e) => format!(
                "the update from card {card} did not start cleanly, and the rollback \
                 failed too: {e}. The previous binary is kept at {}",
                backup.display()
            ),
        }
    };
    Some(reason)
}

/// A completed, healthy setup clears the marker. Only ever called after the
/// shell is fully up.
pub fn mark_healthy(marker: &Path) {
    let _ = std::fs::remove_file(marker);
}

/// Where the marker lives, given an appdata root. It must sit outside the
/// exe directory: a rollback replaces files there.
pub fn default_marker(appdata_root: &Path) -> PathBuf {
    appdata_root.join(marker_default_name())
}

/// Where the previous binary is parked: beside the running one, same name
/// with `.previous` before the extension.
pub fn previous_binary_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("relay.exe"));
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "relay".to_string());
    let ext = exe
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    exe.with_file_name(format!("{stem}.previous{ext}"))
}

/// Launch the freshly installed binary as its own process. The caller exits
/// right after; the marker decides whether this launch was healthy.
pub fn relaunch(exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        return std::process::Command::new(exe)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not relaunch: {e}"));
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(exe)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not relaunch: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "harness-update-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn pending_updates_are_listed_newest_first_and_lies_are_skipped() {
        let root = scratch("pending");
        // Good card.
        let good = root.join("c_good");
        std::fs::create_dir_all(&good).unwrap();
        write(&good.join("harness.exe"), "new-binary");
        write(
            &good.join("manifest.json"),
            r#"{"card_id":"c_good","commit_sha":"abc","built_at_ms":10,"binary":"harness.exe"}"#,
        );
        // A manifest promising a binary that is not there.
        let liar = root.join("c_liar");
        std::fs::create_dir_all(&liar).unwrap();
        write(
            &liar.join("manifest.json"),
            r#"{"card_id":"c_liar","commit_sha":"bad","built_at_ms":20,"binary":"gone.exe"}"#,
        );

        let pending = list_pending(&root);
        assert_eq!(pending.len(), 1, "the broken promise is not shown");
        assert_eq!(pending[0].card_id, "c_good");
        assert!(pending[0].binary.ends_with("harness.exe"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The whole life of an update, minus the relaunch: swap puts the new
    /// binary in place and leaves the old one recoverable; recover puts the
    /// old one back and explains itself; a healthy boot clears the marker.
    #[test]
    fn swap_then_recover_roundtrips_and_a_healthy_boot_clears_the_marker() {
        let root = scratch("roundtrip");
        let exe = root.join("app").join("harness.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        write(&exe, "OLD-APP");
        let incoming = root.join("updates").join("c_up").join("harness.exe");
        std::fs::create_dir_all(incoming.parent().unwrap()).unwrap();
        write(&incoming, "NEW-APP");
        let backup = root.join("app").join("harness.previous.exe");
        let marker = default_marker(&root);

        swap(
            &exe,
            &incoming,
            &backup,
            &marker,
            &serde_json::json!({"card_id": "c_up"}),
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "NEW-APP");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "OLD-APP");
        assert!(marker.is_file(), "the marker waits for a healthy setup");

        // The next start finds the marker: rollback, with a reason that names
        // the card.
        let reason = recover_if_needed(&marker, &exe, &backup).expect("a rollback happened");
        assert!(reason.contains("c_up"), "{reason}");
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "OLD-APP");

        // And a healthy boot after any of this leaves no trace.
        mark_healthy(&marker);
        assert!(!marker.exists());
        assert!(recover_if_needed(&marker, &exe, &backup).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_install_puts_the_old_binary_back() {
        let root = scratch("failed-swap");
        let exe = root.join("app").join("harness.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        write(&exe, "OLD-APP");
        let backup = root.join("app").join("harness.previous.exe");
        let marker = default_marker(&root);

        // An incoming "binary" that is a directory: the copy must fail.
        let incoming = root.join("incoming");
        std::fs::create_dir_all(&incoming).unwrap();

        let result = swap(&exe, &incoming, &backup, &marker, &serde_json::json!({}));
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(&exe).unwrap(),
            "OLD-APP",
            "the operator is never left without an app"
        );
        assert!(!marker.exists(), "no marker for an install that never was");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_marker_without_a_backup_says_so_instead_of_bricking() {
        let root = scratch("nobackup");
        let exe = root.join("app").join("harness.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        write(&exe, "NEW-APP");
        let marker = default_marker(&root);
        write(
            &marker,
            r#"{"card_id":"c_ghost"}"#,
        );

        let reason = recover_if_needed(&marker, &exe, &root.join("missing.previous.exe"))
            .expect("the situation is explained");
        assert!(reason.contains("previous binary is missing"), "{reason}");
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "NEW-APP", "staying up");
        assert!(!marker.exists(), "the marker is spent either way");
        let _ = std::fs::remove_dir_all(&root);
    }




}
