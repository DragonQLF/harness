//! Project registry and the read-only git information behind the Project
//! screens.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use harness_app::checks::{read_checks, run_check, CheckRow};
use harness_app::insights::ProjectStats;
use harness_app::paths;
use harness_app::projects::{FolderInfo, Project};
use harness_git_cli::{BranchRow, CommitRow, LanguageRow, WorktreeRow};
use serde::Serialize;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

/// A project plus the live numbers the list and switcher show.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectView {
    #[serde(flatten)]
    pub project: Project,
    pub exists: bool,
    pub stats: ProjectStats,
}

#[tauri::command]
pub async fn projects_list(
    tz_offset_minutes: Option<i64>,
    ws: Shared<'_>,
) -> Result<Vec<ProjectView>, String> {
    let mut out = Vec::new();
    for project in ws.projects() {
        let exists = Path::new(&project.path).is_dir();
        let stats = if exists {
            match crate::commands::board::project_stats(
                project.id.clone(),
                tz_offset_minutes,
                ws.clone(),
            )
            .await
            {
                Ok(stats) => stats,
                Err(_) => ProjectStats::default(),
            }
        } else {
            ProjectStats::default()
        };
        out.push(ProjectView {
            project,
            exists,
            stats,
        });
    }
    Ok(out)
}

/// Native folder picker, so adding a project does not mean typing a path.
#[tauri::command]
pub async fn project_pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Pick a git repository")
        .pick_folder(move |picked| {
            let _ = tx.send(picked.map(|p| p.to_string()));
        });
    rx.await.map_err(|_| "the folder picker closed".to_string())
}

/// What a folder is, before adopting it: a repository, an empty directory we
/// can initialise, or files we would need explicit permission to touch.
#[tauri::command]
pub async fn project_inspect(path: String, ws: Shared<'_>) -> Result<FolderInfo, String> {
    Ok(ws.inspect_folder(&path))
}

#[tauri::command]
pub async fn project_add(
    path: String,
    name: Option<String>,
    init: Option<bool>,
    ws: Shared<'_>,
) -> Result<Project, String> {
    let project = ws.add_project(&path, name, init.unwrap_or(false))?;
    // Bring the engine up now so the board is ready when the UI switches.
    ws.runtime(&project.id)?;
    Ok(project)
}

/// Start a repository from nothing: `<parent>/<name>`, initialised and adopted.
#[tauri::command]
pub async fn project_create(
    parent: String,
    name: String,
    ws: Shared<'_>,
) -> Result<Project, String> {
    let project = ws.create_project(&parent, &name)?;
    ws.runtime(&project.id)?;
    Ok(project)
}

/// Point mirror mode at Relay's own source, fetching it if it is not here.
///
/// Not a toggle on a project the operator picked. Everything the mode does is
/// specific to this repository — `read_docs` reads these decisions, an accepted
/// proposal becomes a card in this code, the post-run build compiles this app —
/// so aiming it anywhere else is not a preference, it is a mistake with no
/// symptom until the Director cites a stranger's DEBT.md.
///
/// Order matters: an already-registered clone, then the checkout this binary
/// was built from, then a fresh clone. The middle one is the whole reason to
/// look before cloning — a developer running `tauri dev` already has the source
/// open, and a second copy would leave agents editing the one they are not.
#[tauri::command]
pub async fn mirror_setup(ws: Shared<'_>) -> Result<Project, String> {
    let ws = Arc::clone(&ws);
    ensure_mirror(&ws)
}

/// The same work, callable from anywhere the operator can ask for it: the
/// Projects screen, the chat's project picker, or the Director when told to
/// start working on the app. Registering a project should not be a thing the
/// operator has to know to do first.
pub fn ensure_mirror(ws: &Workspace) -> Result<Project, String> {
    use harness_app::mirror::{self, Source};

    let remotes: Vec<(String, Option<String>)> = ws
        .projects()
        .into_iter()
        .map(|p| {
            let remote = harness_git_cli::CliGit::new(
                std::path::Path::new(&p.path),
                ws.paths.project_worktrees(&p.id),
            )
            .remote();
            (p.id, remote)
        })
        .collect();

    let project = match mirror::locate(&remotes, ws.paths.root()) {
        Source::Registered(id) => ws
            .project(&id)
            .ok_or_else(|| format!("project {id} went away while we looked at it"))?,
        Source::OnDisk(path) => ws.add_project(&path.to_string_lossy(), Some("Relay".into()), false)?,
        Source::Clone(into) => {
            if into.exists() {
                // A previous attempt that got as far as the disk. Adopt it
                // rather than refusing, or the operator can never retry.
                ws.add_project(&into.to_string_lossy(), Some("Relay".into()), false)?
            } else {
                let out = std::process::Command::new("git")
                    .args(["clone", mirror::REPO_URL])
                    .arg(&into)
                    .output()
                    .map_err(|e| format!("could not run git: {e}"))?;
                if !out.status.success() {
                    let why = String::from_utf8_lossy(&out.stderr);
                    let why = why.trim();
                    // Leave nothing half-cloned behind to confuse the retry.
                    let _ = std::fs::remove_dir_all(&into);
                    return Err(format!("could not clone Relay's source: {why}"));
                }
                ws.add_project(&into.to_string_lossy(), Some("Relay".into()), false)?
            }
        }
    };

    let mirrored = Project { mirror: true, ..project };
    ws.update_project(mirrored)
}

#[tauri::command]
pub async fn project_update(project: Project, ws: Shared<'_>) -> Result<Project, String> {
    ws.update_project(project)
}

#[tauri::command]
pub async fn project_remove(
    project_id: String,
    delete_data: bool,
    ws: Shared<'_>,
) -> Result<(), String> {
    ws.remove_project(&project_id, delete_data)
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetail {
    pub project: Project,
    pub head: Option<String>,
    pub default_branch: String,
    /// `origin`, when the repository has one. A local-only project has none,
    /// and needs none.
    pub remote: Option<String>,
    pub commit_count: u64,
    pub line_count: u64,
    pub branches: Vec<BranchRow>,
    pub languages: Vec<LanguageRow>,
    pub commits: Vec<CommitRow>,
    /// Commits per day for the last seven days, oldest first.
    pub week_commits: Vec<u64>,
    /// Lines added plus removed in the last seven days.
    pub week_lines: u64,
    pub worktrees: Vec<WorktreeRow>,
    pub checks: Vec<CheckRow>,
}

/// Everything the project screen needs, in one round trip. All of it is git
/// reads, so it runs on the blocking pool.
#[tauri::command]
pub async fn project_detail(
    project_id: String,
    commit_limit: Option<usize>,
    ws: Shared<'_>,
) -> Result<ProjectDetail, String> {
    let runtime = ws.runtime(&project_id)?;
    let git = Arc::clone(&runtime.git);
    let project = runtime.project.clone();
    let checks_file = ws.paths.checks_file(&project_id);
    let limit = commit_limit.unwrap_or(12);

    tauri::async_runtime::spawn_blocking(move || {
        let checks = read_checks(&checks_file, Path::new(&project.path));
        ProjectDetail {
            head: git.head_sha(),
            default_branch: git.default_branch(),
            remote: git.remote(),
            commit_count: git.commit_count(),
            line_count: git.line_count(),
            branches: git.branches(),
            languages: git.languages(),
            commits: git.recent_commits(limit),
            week_commits: git.activity(7),
            week_lines: git.changed_lines(7),
            worktrees: git.worktree_list(),
            checks,
            project,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn worktrees(project_id: String, ws: Shared<'_>) -> Result<Vec<WorktreeRow>, String> {
    let git = Arc::clone(&ws.runtime(&project_id)?.git);
    tauri::async_runtime::spawn_blocking(move || git.worktree_list())
        .await
        .map_err(|e| e.to_string())
}

/// Drop a worktree the operator is done with. The branch is left alone.
#[tauri::command]
pub async fn remove_worktree(
    project_id: String,
    path: String,
    ws: Shared<'_>,
) -> Result<(), String> {
    let runtime = ws.runtime(&project_id)?;
    let expected = ws.paths.project_worktrees(&project_id);
    let target = PathBuf::from(&path);
    // `starts_with` matches component by component, so `<expected>/../../x`
    // passes it while pointing somewhere else entirely. Resolve both sides
    // before comparing, and refuse anything that cannot be resolved.
    if !inside(&expected, &target) {
        return Err(format!(
            "{} is not a worktree Relay created",
            target.display()
        ));
    }
    let git = Arc::clone(&runtime.git);
    tauri::async_runtime::spawn_blocking(move || {
        harness_ports::GitPort::remove_worktree(git.as_ref(), &harness_ports::WorktreePath(target))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Is `target` genuinely inside `root`? Both are canonicalised first, so `..`
/// cannot walk out of the directory we mean to confine this to.
fn inside(root: &std::path::Path, target: &std::path::Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(target) = target.canonicalize() else {
        return false;
    };
    target.starts_with(&root)
}

/// Show a path in the operator's file manager.
#[tauri::command]
pub async fn reveal_path(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("{path} is gone"));
    }
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program)
        .arg(&target)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open the file manager: {e}"))
}


// ---- project checks ----

#[tauri::command]
pub async fn project_checks(project_id: String, ws: Shared<'_>) -> Result<Vec<CheckRow>, String> {
    let project = ws
        .project(&project_id)
        .ok_or_else(|| format!("unknown project {project_id}"))?;
    Ok(read_checks(
        &ws.paths.checks_file(&project_id),
        Path::new(&project.path),
    ))
}

#[tauri::command]
pub async fn project_set_checks(
    project_id: String,
    checks: Vec<CheckRow>,
    ws: Shared<'_>,
) -> Result<Vec<CheckRow>, String> {
    paths::write_json(&ws.paths.checks_file(&project_id), &checks)?;
    Ok(checks)
}

/// Run the configured checks and remember how they went.
#[tauri::command]
pub async fn project_run_checks(
    project_id: String,
    ws: Shared<'_>,
) -> Result<Vec<CheckRow>, String> {
    let project = ws
        .project(&project_id)
        .ok_or_else(|| format!("unknown project {project_id}"))?;
    let file = ws.paths.checks_file(&project_id);
    let root = PathBuf::from(&project.path);
    let checks = read_checks(&file, &root);

    let ran = tauri::async_runtime::spawn_blocking(move || {
        checks
            .into_iter()
            .map(|check| run_check(&root, check))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| e.to_string())?;

    paths::write_json(&file, &ran)?;
    Ok(ran)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_dot_cannot_walk_out_of_the_worktree_root() {
        let base = std::env::temp_dir().join(format!("harness-inside-{}", std::process::id()));
        let root = base.join("worktrees");
        let elsewhere = base.join("elsewhere");
        std::fs::create_dir_all(root.join("card")).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();

        assert!(inside(&root, &root.join("card")));
        // Component-wise matching used to accept this: it starts with the root
        // components but resolves outside it.
        assert!(!inside(&root, &root.join("..").join("elsewhere")));
        assert!(!inside(&root, &elsewhere));
        // A path that does not exist cannot be confirmed, so it is refused.
        assert!(!inside(&root, &root.join("never-created")));

        let _ = std::fs::remove_dir_all(&base);
    }
}
