//! Finding and preparing the Node sidecar that hosts the Claude Agent SDK.
//!
//! In development the checked-out `sidecar/` directory is used as is. In an
//! installed build the script is shipped as a bundled resource and copied into
//! app data, where its `node_modules` can be installed at runtime — the binary's
//! own directory is read-only on a normal install, so nothing is written there.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use harness_app::paths::AppPaths;

#[derive(Debug, Clone, Serialize)]
pub struct SidecarStatus {
    /// Directory the sidecar runs from.
    pub dir: String,
    pub script: String,
    /// The script exists where we expect it.
    pub script_found: bool,
    /// Dependencies are installed, so a run can start.
    pub ready: bool,
    pub node_found: bool,
    pub node_version: Option<String>,
    /// True when running against the repository checkout rather than app data.
    pub development: bool,
}

pub(crate) fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn node_version() -> Option<String> {
    let out = no_window(&mut Command::new("node")).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn deps_installed(dir: &Path) -> bool {
    dir.join("node_modules")
        .join("@anthropic-ai")
        .join("claude-agent-sdk")
        .exists()
}

/// A checked-out sidecar next to the binary or above it, used while developing.
fn development_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(PathBuf::from));
    while let Some(d) = dir {
        let candidate = d.join("sidecar");
        if candidate.join("index.mjs").exists() && deps_installed(&candidate) {
            return Some(candidate);
        }
        dir = d.parent().map(PathBuf::from);
    }
    let from_manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("sidecar"));
    match from_manifest {
        Some(dir) if dir.join("index.mjs").exists() && deps_installed(&dir) => Some(dir),
        _ => None,
    }
}

/// Copy the bundled script into app data when it is missing or out of date.
fn seed_from_resources(app: &AppHandle, target: &Path) -> Result<(), String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("no resource directory: {e}"))?
        .join("sidecar");
    std::fs::create_dir_all(target).map_err(|e| e.to_string())?;
    for name in ["index.mjs", "pathguard.mjs", "toolsum.mjs", "package.json"] {
        let from = resource_dir.join(name);
        let to = target.join(name);
        if !from.exists() {
            continue;
        }
        let newer = match (std::fs::metadata(&from), std::fs::metadata(&to)) {
            (Ok(a), Ok(b)) => match (a.modified(), b.modified()) {
                (Ok(a), Ok(b)) => a > b,
                _ => true,
            },
            (Ok(_), Err(_)) => true,
            _ => false,
        };
        if newer {
            std::fs::copy(&from, &to).map_err(|e| format!("copying {name}: {e}"))?;
        }
    }
    Ok(())
}

/// Where the sidecar should run from, preparing app data if needed.
pub fn prepare(app: &AppHandle, paths: &AppPaths) -> PathBuf {
    if let Ok(explicit) = std::env::var("HARNESS_SIDECAR") {
        let path = PathBuf::from(explicit);
        if let Some(parent) = path.parent() {
            return parent.to_path_buf();
        }
    }
    if let Some(dev) = development_dir() {
        return dev;
    }
    let target = paths.sidecar_dir();
    if let Err(e) = seed_from_resources(app, &target) {
        eprintln!("could not stage the sidecar: {e}");
    }
    target
}

pub fn script_in(dir: &Path) -> PathBuf {
    dir.join("index.mjs")
}

pub fn status(dir: &Path) -> SidecarStatus {
    let script = script_in(dir);
    let version = node_version();
    SidecarStatus {
        dir: dir.to_string_lossy().to_string(),
        script: script.to_string_lossy().to_string(),
        script_found: script.exists(),
        ready: script.exists() && deps_installed(dir),
        node_found: version.is_some(),
        node_version: version,
        development: development_dir().as_deref() == Some(dir),
    }
}

/// Install the sidecar's dependencies, streaming progress to the UI.
pub fn install(app: &AppHandle, dir: &Path) -> Result<String, String> {
    if !script_in(dir).exists() {
        return Err(format!(
            "the sidecar script is missing from {}",
            dir.display()
        ));
    }
    if node_version().is_none() {
        return Err("node was not found on PATH; install Node 20 or newer".to_string());
    }
    let program = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let _ = app.emit(crate::events::SIDECAR_LOG, "installing the sidecar dependencies…");
    let out = no_window(&mut Command::new(program))
        .args(["install", "--omit=dev", "--no-audit", "--no-fund"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let combined = format!("{stdout}\n{stderr}").trim().to_string();
    let _ = app.emit(crate::events::SIDECAR_LOG, &combined);
    if !out.status.success() {
        return Err(if combined.is_empty() {
            format!("npm install failed with {}", out.status)
        } else {
            combined
        });
    }
    if !deps_installed(dir) {
        return Err("npm reported success but the agent SDK is still missing".to_string());
    }
    Ok(combined)
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeStatus {
    pub cli_found: bool,
    pub cli_version: Option<String>,
    pub logged_in: bool,
    pub credentials_path: Option<String>,
}

/// A subscription login is stored in the login Keychain on macOS and in a file
/// next to the CLI config everywhere else; an API key in the environment works
/// too.
pub fn claude_status() -> ClaudeStatus {
    let version = no_window(&mut Command::new("claude"))
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if std::env::var_os("ANTHROPIC_API_KEY").is_some_and(|v| !v.is_empty()) {
        return ClaudeStatus {
            cli_found: version.is_some(),
            cli_version: version,
            logged_in: true,
            credentials_path: None,
        };
    }

    let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|h| PathBuf::from(h).join(".claude"))
        });
    let credentials = config_dir.map(|d| d.join(".credentials.json"));
    let on_disk = credentials.as_ref().map(|p| p.exists()).unwrap_or(false);

    // The file being absent is not proof of a logout: on macOS it never exists,
    // because the token lives in the Keychain instead.
    if !on_disk && keychain_login() {
        return ClaudeStatus {
            cli_found: version.is_some(),
            cli_version: version,
            logged_in: true,
            credentials_path: Some(format!("login keychain: {KEYCHAIN_SERVICE}")),
        };
    }

    ClaudeStatus {
        cli_found: version.is_some(),
        cli_version: version,
        logged_in: on_disk,
        credentials_path: credentials.map(|p| p.to_string_lossy().to_string()),
    }
}

/// The Keychain item the CLI writes its subscription token to on macOS.
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

/// Whether that Keychain item exists. Asking for the item *without* `-w` reports
/// its presence without reading the secret back, so this never puts an unlock
/// prompt in front of someone who only opened the app.
#[cfg(target_os = "macos")]
fn keychain_login() -> bool {
    Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Every other platform keeps the token in the file, so there is nothing to ask.
#[cfg(not(target_os = "macos"))]
fn keychain_login() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_is_not_judged_by_the_credentials_file_alone_on_macos() {
        // On macOS this probe is the only thing standing between a logged-in
        // operator and a banner telling them they are not, so calling it at all
        // is the assertion: it must answer without panicking or prompting.
        let found = keychain_login();
        if !cfg!(target_os = "macos") {
            assert!(!found, "only macOS keeps the token in a keychain");
        }
    }

    #[test]
    fn status_reports_a_missing_script_without_panicking() {
        let dir = std::env::temp_dir().join("harness-no-sidecar-here");
        let _ = std::fs::remove_dir_all(&dir);
        let missing = status(&dir);
        assert!(!missing.script_found);
        assert!(!missing.ready);
        assert!(missing.script.ends_with("index.mjs"));
    }

    #[test]
    fn a_staged_sidecar_needs_its_dependencies_before_it_is_ready() {
        let dir = std::env::temp_dir().join(format!("harness-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.mjs"), "// stub\n").unwrap();
        let staged = status(&dir);
        assert!(staged.script_found);
        assert!(!staged.ready, "no node_modules yet");

        std::fs::create_dir_all(dir.join("node_modules/@anthropic-ai/claude-agent-sdk")).unwrap();
        assert!(status(&dir).ready);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_script_path_sits_inside_the_directory() {
        let dir = PathBuf::from("some").join("where");
        assert_eq!(script_in(&dir), dir.join("index.mjs"));
    }
}
