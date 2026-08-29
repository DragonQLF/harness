//! The Code screen's three reads: a worktree's tree, one file, and the diff as
//! hunks. Git runs here; what any of it means is decided in
//! `harness_app::code`, which is where the tests are.

use std::path::PathBuf;
use std::sync::Arc;

use harness_app::code::{self, FileText, Hunk, TreeEntry};
use harness_domain::{Actor, CardId, Command};
use harness_git_cli::CliGit;
use tauri::State;

use crate::workspace::Workspace;

type Shared<'a> = State<'a, Arc<Workspace>>;

/// Which checkout a card is being read in.
///
/// A card that has run has a worktree of its own and that is the truth the
/// operator is reviewing. A card with no session, or no card at all, means the
/// project's own checkout — the screen still browses, it just browses what is
/// on the branch.
async fn checkout(
    ws: &Shared<'_>,
    project_id: &str,
    card_id: Option<&str>,
) -> Result<(Arc<CliGit>, PathBuf, String), String> {
    let runtime = ws.runtime(project_id).await?;
    let base = runtime.project.base_branch.clone();
    let mut dir = PathBuf::from(&runtime.project.path);
    if let Some(card_id) = card_id {
        let snap = runtime.engine.snapshot().await?;
        if let Some(session) = snap.sessions.iter().find(|s| s.card_id.as_str() == card_id) {
            dir = PathBuf::from(&session.worktree);
        }
    }
    Ok((Arc::clone(&runtime.git), dir, base))
}

/// Every file in the card's worktree, with the ones it has changed marked.
#[tauri::command]
pub async fn list_tree(
    project_id: String,
    card_id: Option<String>,
    ws: Shared<'_>,
) -> Result<Vec<TreeEntry>, String> {
    let (git, dir, _) = checkout(&ws, &project_id, card_id.as_deref()).await?;
    tauri::async_runtime::spawn_blocking(move || {
        let head = git.head_at(&dir);
        let tracked = code::TREE_CACHE.tracked(&dir, &head, || git.ls_files(&dir));
        code::build_tree(&tracked, &git.status_porcelain(&dir))
    })
    .await
    .map_err(|e| e.to_string())
}

/// One file, read-only. `rev` reads it out of a commit instead of off disk, so
/// the pane can put HEAD beside the working copy.
#[tauri::command]
pub async fn read_worktree_file(
    project_id: String,
    card_id: Option<String>,
    path: String,
    rev: Option<String>,
    ws: Shared<'_>,
) -> Result<FileText, String> {
    let safe = code::safe_relative(&path)
        .ok_or_else(|| format!("{path} is not a path inside the worktree"))?;
    let (git, dir, _) = checkout(&ws, &project_id, card_id.as_deref()).await?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(rev) = rev {
            let bytes = git.show_file(&dir, &rev, &safe).map_err(|e| e.to_string())?;
            let size = bytes.len() as u64;
            return Ok(code::file_text(&safe, size, &bytes));
        }
        let full = dir.join(&safe);
        let size = std::fs::metadata(&full).map_err(|e| e.to_string())?.len();
        // Read the cap and no more: a file this pane refuses to render is
        // still a file that could be a gigabyte of build output.
        let bytes = read_capped(&full, code::MAX_FILE_BYTES as usize).map_err(|e| e.to_string())?;
        Ok(code::file_text(&safe, size, &bytes))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn read_capped(path: &std::path::Path, cap: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    std::fs::File::open(path)?
        .take(cap as u64)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

/// What the card changed, as `@@` blocks. `path` narrows it to one file.
#[tauri::command]
pub async fn diff_hunks(
    project_id: String,
    card_id: String,
    path: Option<String>,
    ws: Shared<'_>,
) -> Result<Vec<Hunk>, String> {
    let (git, dir, base) = checkout(&ws, &project_id, Some(&card_id)).await?;
    let only = match path.as_deref() {
        Some(raw) => Some(
            code::safe_relative(raw)
                .ok_or_else(|| format!("{raw} is not a path inside the worktree"))?,
        ),
        None => None,
    };
    tauri::async_runtime::spawn_blocking(move || {
        let raw = git.unified_diff(&dir, &base, only.as_deref());
        code::parse_hunks(&raw, only.as_deref())
    })
    .await
    .map_err(|e| e.to_string())
}

/// Decide one block of a card's diff.
///
/// The block is named the way the panel shows it — a file and the `@@` header
/// git wrote — and the diff itself is re-read here rather than taken from the
/// window: the domain has to know how many blocks exist before it can say the
/// review is finished, and the only honest source for that is the worktree as
/// it stands now. What the verdict then *means* for the card — nothing yet,
/// an approval, a send-back, or an approval with the rest carried onto a
/// follow-up card — is decided in `harness_domain`, not here.
#[tauri::command]
pub async fn review_hunk(
    project_id: String,
    card_id: String,
    file: String,
    header: String,
    approved: bool,
    reason: Option<String>,
    ws: Shared<'_>,
) -> Result<u64, String> {
    let runtime = ws.runtime(&project_id).await?;
    let (git, dir, base) = checkout(&ws, &project_id, Some(&card_id)).await?;
    let diff: Vec<harness_domain::HunkRef> = tauri::async_runtime::spawn_blocking(move || {
        code::parse_hunks(&git.unified_diff(&dir, &base, None), None)
            .iter()
            .map(Hunk::as_ref)
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;

    let hunk = diff
        .iter()
        .find(|h| h.file == file && h.header == header)
        .cloned()
        .ok_or_else(|| format!("{file} {header} is not in this card's diff any more"))?;

    runtime
        .engine
        .execute(Command::ReviewHunk {
            card_id: CardId::new(card_id),
            hunk,
            approved,
            by: Actor::Human,
            reason: reason.unwrap_or_default(),
            diff,
            // Minted here, once, and carried into the event. The domain has no
            // randomness of its own and a replay must land on the same id.
            follow_up: CardId::new(format!("c_{}", super::board::short_id())),
        })
        .await
}
