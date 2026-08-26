use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use harness_ports::{GitError, GitPort, Trailers, WorktreePath};
use ts_rs::TS;
use serde::Serialize;

const TAB: char = '\t';

/// Every git call goes through this. On Windows a plain `Command` flashes a
/// console window for each process, and one screen can run a dozen git commands
/// — so the flag that suppresses it belongs in exactly one place.
fn git_command() -> Command {
    let mut cmd = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Everything git, through the `git` executable. Worktrees live outside the
/// checkout so the repository the operator pointed us at stays clean.
pub struct CliGit {
    repo_root: PathBuf,
    worktrees_root: PathBuf,
    branch_prefix: String,
}

fn sanitize(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "card".to_string()
    } else {
        trimmed
    }
}

impl CliGit {
    /// `worktrees_root` is where per-card checkouts are created; keep it out of
    /// `repo_root` so `git add -A` in a run never sees sibling worktrees.
    pub fn new(repo_root: impl Into<PathBuf>, worktrees_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
            worktrees_root: worktrees_root.into(),
            branch_prefix: "harness".to_string(),
        }
    }

    pub fn with_branch_prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = sanitize(&prefix.into());
        self.branch_prefix = prefix;
        self
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn worktrees_root(&self) -> &Path {
        &self.worktrees_root
    }

    pub fn branch_prefix(&self) -> &str {
        &self.branch_prefix
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> Result<String, GitError> {
        let out = git_command()
            .arg("-c")
            .arg("core.quotepath=false")
            .args(args)
            .current_dir(cwd)
            .output()
            .map_err(|e| GitError::Io(e.to_string()))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            let combined = format!("{stderr}\n{stdout}");
            Err(GitError::Git(combined.trim().to_string()))
        }
    }

    fn rev_parse_head(&self, cwd: &Path) -> Result<String, GitError> {
        self.git(cwd, &["rev-parse", "HEAD"])
    }

    fn branch_for(&self, card_id: &str) -> String {
        format!("{}/{}", self.branch_prefix, sanitize(card_id))
    }

    // ---- read-only project information used by the Project screens ----

    /// Default branch of the repository: whatever HEAD points at, falling back
    /// to main or master if the repo is empty.
    pub fn default_branch(&self) -> String {
        if let Ok(name) = self.git(&self.repo_root, &["symbolic-ref", "--short", "HEAD"]) {
            if !name.is_empty() {
                return name;
            }
        }
        for candidate in ["main", "master"] {
            if self
                .git(&self.repo_root, &["rev-parse", "--verify", candidate])
                .is_ok()
            {
                return candidate.to_string();
            }
        }
        "main".to_string()
    }

    /// The `origin` URL, when there is one. Relay needs no remote: a purely
    /// local repository is a first-class project.
    pub fn remote(&self) -> Option<String> {
        self.git(&self.repo_root, &["remote", "get-url", "origin"])
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Does this repository know who is committing? Without an identity every
    /// agent commit fails, and it fails late — at the end of a run.
    pub fn has_committer_identity(&self) -> bool {
        self.git(&self.repo_root, &["config", "user.email"])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    /// Give this repository a local identity so agent commits can succeed. Only
    /// ever repo-local, and only when there is none to inherit.
    pub fn set_local_identity(&self) -> Result<(), GitError> {
        self.git(&self.repo_root, &["config", "user.name", "Relay"])?;
        self.git(&self.repo_root, &["config", "user.email", "harness@localhost"])?;
        Ok(())
    }

    pub fn head_sha(&self) -> Option<String> {
        self.rev_parse_head(&self.repo_root).ok()
    }

    pub fn commit_count(&self) -> u64 {
        self.git(&self.repo_root, &["rev-list", "--count", "HEAD"])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0)
    }

    pub fn is_repo(path: &Path) -> bool {
        git_command()
            .args(["rev-parse", "--is-inside-work-tree"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn init_repo(path: &Path) -> Result<(), GitError> {
        std::fs::create_dir_all(path).map_err(|e| GitError::Io(e.to_string()))?;
        let git = CliGit::new(path, path.join(".harness-worktrees"));
        git.git(path, &["init", "-b", "main"])?;
        Ok(())
    }

    /// Branch rows for the project detail screen.
    pub fn branches(&self) -> Vec<BranchRow> {
        let default = self.default_branch();
        let raw = self
            .git(
                &self.repo_root,
                &[
                    "for-each-ref",
                    "--sort=-committerdate",
                    "--format=%(refname:short)%09%(committerdate:relative)%09%(objectname:short)",
                    "refs/heads",
                ],
            )
            .unwrap_or_default();
        let merged: Vec<String> = self
            .git(&self.repo_root, &["branch", "--merged", &default, "--format=%(refname:short)"])
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .collect();
        let live: Vec<String> = self
            .worktree_list()
            .into_iter()
            .filter_map(|w| w.branch)
            .collect();

        raw.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let mut parts = line.split('\t');
                let name = parts.next().unwrap_or("").to_string();
                let when = parts.next().unwrap_or("").to_string();
                let sha = parts.next().unwrap_or("").to_string();
                let state = if name == default {
                    BranchState::Default
                } else if live.contains(&name) {
                    BranchState::Live
                } else if merged.contains(&name) {
                    BranchState::Merged
                } else {
                    BranchState::Open
                };
                BranchRow { name, when, sha, state }
            })
            .collect()
    }

    pub fn worktree_list(&self) -> Vec<WorktreeRow> {
        let text = self
            .git(&self.repo_root, &["worktree", "list", "--porcelain"])
            .unwrap_or_default();
        let mut rows = Vec::new();
        let mut cur = WorktreeRow::default();
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("worktree ") {
                cur.path = v.to_string();
            } else if let Some(v) = line.strip_prefix("HEAD ") {
                cur.head = v.chars().take(7).collect();
            } else if let Some(v) = line.strip_prefix("branch ") {
                cur.branch = Some(v.trim_start_matches("refs/heads/").to_string());
            } else if line == "bare" {
                cur.bare = true;
            } else if line == "detached" {
                cur.branch = None;
            } else if line.trim().is_empty() && !cur.path.is_empty() {
                rows.push(std::mem::take(&mut cur));
            }
        }
        if !cur.path.is_empty() {
            rows.push(cur);
        }
        for row in &mut rows {
            row.dirty = self.is_dirty(Path::new(&row.path));
        }
        rows
    }

    pub fn is_dirty(&self, dir: &Path) -> bool {
        self.git(dir, &["status", "--porcelain"])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    /// Shas reachable from the default branch, newest first.
    fn default_branch_shas(&self, limit: usize) -> std::collections::HashSet<String> {
        let default = self.default_branch();
        self.git(
            &self.repo_root,
            &["log", &format!("--max-count={}", limit * 4), "--format=%H", &default],
        )
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
    }

    /// Recent history with the numbers the commit graph needs.
    pub fn recent_commits(&self, limit: usize) -> Vec<CommitRow> {
        let on_default = self.default_branch_shas(limit.max(20));
        let fmt = "%x1e%H%x1f%h%x1f%s%x1f%an%x1f%ar%x1f%at%x1f%P%x1f%D%x1f%(trailers:key=Harness-Card,valueonly)%x1f%(trailers:key=Harness-Agent,valueonly)";
        let raw = self
            .git(
                &self.repo_root,
                &[
                    "log",
                    "--all",
                    &format!("--max-count={limit}"),
                    &format!("--format={fmt}"),
                    "--numstat",
                ],
            )
            .unwrap_or_default();

        raw.split('\u{1e}')
            .filter(|chunk| !chunk.trim().is_empty())
            .map(|chunk| {
                let mut fields = chunk.split('\u{1f}');
                let sha = fields.next().unwrap_or("").trim().to_string();
                let short = fields.next().unwrap_or("").trim().to_string();
                let subject = fields.next().unwrap_or("").trim().to_string();
                let author = fields.next().unwrap_or("").trim().to_string();
                let when = fields.next().unwrap_or("").trim().to_string();
                let at_secs = fields
                    .next()
                    .unwrap_or("")
                    .trim()
                    .parse::<u64>()
                    .unwrap_or(0);
                let parents: Vec<String> = fields
                    .next()
                    .unwrap_or("")
                    .split_whitespace()
                    .map(|s| s.chars().take(7).collect())
                    .collect();
                let refs = fields.next().unwrap_or("").trim().to_string();
                let card = fields.next().unwrap_or("").trim().to_string();
                // The trailing field carries the agent trailer plus the numstat block.
                let tail = fields.next().unwrap_or("");
                let mut lines = tail.lines();
                let agent = lines.next().unwrap_or("").trim().to_string();
                let (mut added, mut removed, mut files) = (0u64, 0u64, 0u64);
                for line in lines {
                    let mut cols = line.split('\t');
                    let a = cols.next().unwrap_or("");
                    let d = cols.next().unwrap_or("");
                    if cols.next().is_none() {
                        continue;
                    }
                    files += 1;
                    added += a.parse::<u64>().unwrap_or(0);
                    removed += d.parse::<u64>().unwrap_or(0);
                }
                CommitRow {
                    on_default: on_default.contains(&sha),
                    sha,
                    short,
                    subject,
                    author,
                    when,
                    at_secs,
                    parents,
                    refs,
                    card: if card.is_empty() { None } else { Some(card) },
                    agent: if agent.is_empty() { None } else { Some(agent) },
                    added,
                    removed,
                    files,
                }
            })
            .collect()
    }

    /// Commits per day for the last `days` days, oldest first.
    pub fn activity(&self, days: usize) -> Vec<u64> {
        let mut buckets = vec![0u64; days.max(1)];
        let since = format!("--since={} days ago", days);
        let raw = self
            .git(&self.repo_root, &["log", "--all", &since, "--format=%at"])
            .unwrap_or_default();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        for line in raw.lines() {
            let at: u64 = match line.trim().parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            if at > now {
                continue;
            }
            let days_ago = ((now - at) / 86_400) as usize;
            if days_ago < buckets.len() {
                let idx = buckets.len() - 1 - days_ago;
                buckets[idx] += 1;
            }
        }
        buckets
    }

    /// Which files a worktree changed against `base`, plus the line counts —
    /// the facts the Director needs to talk about a card without guessing.
    pub fn changed_files(&self, wt: &Path, base: &str) -> (Vec<String>, u64, u64) {
        let range = format!("{base}...HEAD");
        let raw = self
            .git(wt, &["diff", "--numstat", &range])
            .unwrap_or_default();
        let mut files = Vec::new();
        let mut added = 0u64;
        let mut removed = 0u64;
        for line in raw.lines() {
            let mut cols = line.split(TAB);
            let a = cols.next().unwrap_or("").parse::<u64>().unwrap_or(0);
            let d = cols.next().unwrap_or("").parse::<u64>().unwrap_or(0);
            if let Some(path) = cols.next() {
                files.push(path.trim().to_string());
                added += a;
                removed += d;
            }
        }
        // Anything not committed yet counts too: a run that could not commit
        // still has work sitting in the worktree.
        let pending = self.git(wt, &["status", "--porcelain"]).unwrap_or_default();
        for line in pending.lines() {
            let path = line.get(3..).unwrap_or("").trim();
            if !path.is_empty() && !files.iter().any(|f| f == path) {
                files.push(path.to_string());
            }
        }
        (files, added, removed)
    }

    /// The patch a worktree holds against `base`, stat header first, for a
    /// human to read. Long diffs are cut rather than refused: the review screen
    /// wants the shape of the change, not every line of a vendored file.
    pub fn review_patch(&self, wt: &Path, base: &str) -> String {
        let range = format!("{base}...HEAD");
        let stat = self.git(wt, &["diff", "--stat", &range]).unwrap_or_default();
        let committed = self.git(wt, &["diff", &range]).unwrap_or_default();
        // Work the run could not commit is still part of what is being
        // reviewed, so it follows the committed patch instead of vanishing.
        let pending = self.git(wt, &["diff", "HEAD"]).unwrap_or_default();
        let mut patch = committed;
        if !pending.trim().is_empty() {
            patch.push_str("\n--- uncommitted in the worktree ---\n");
            patch.push_str(&pending);
        }
        const CAP: usize = 240_000;
        if patch.chars().count() > CAP {
            let cut: String = patch.chars().take(CAP).collect();
            format!("{stat}\n{cut}\n[diff truncated]")
        } else {
            format!("{stat}\n{patch}")
        }
    }

    /// Lines added plus removed across the last `days` days.
    pub fn changed_lines(&self, days: usize) -> u64 {
        let since = format!("--since={days} days ago");
        let raw = self
            .git(&self.repo_root, &["log", "--all", &since, "--numstat", "--format="])
            .unwrap_or_default();
        raw.lines()
            .filter_map(|line| {
                let mut cols = line.split(TAB);
                let a = cols.next()?.parse::<u64>().ok()?;
                let d = cols.next()?.parse::<u64>().ok()?;
                Some(a + d)
            })
            .sum()
    }

    /// Language split by tracked bytes per extension.
    pub fn languages(&self) -> Vec<LanguageRow> {
        let files = self
            .git(&self.repo_root, &["ls-files"])
            .unwrap_or_default();
        let mut totals: BTreeMap<&'static str, u64> = BTreeMap::new();
        let mut grand = 0u64;
        for rel in files.lines() {
            let rel = rel.trim();
            if rel.is_empty() {
                continue;
            }
            let Some(lang) = language_for(rel) else { continue };
            let size = std::fs::metadata(self.repo_root.join(rel))
                .map(|m| m.len())
                .unwrap_or(0);
            if size == 0 {
                continue;
            }
            *totals.entry(lang).or_insert(0) += size;
            grand += size;
        }
        if grand == 0 {
            return Vec::new();
        }
        let mut rows: Vec<LanguageRow> = totals
            .into_iter()
            .map(|(name, bytes)| LanguageRow {
                name: name.to_string(),
                bytes,
                pct: (bytes as f64 / grand as f64) * 100.0,
            })
            .collect();
        rows.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        rows.truncate(6);
        rows
    }

    pub fn line_count(&self) -> u64 {
        self.languages().iter().map(|l| l.bytes / 40).sum()
    }
}

fn language_for(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => "Rust",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TypeScript",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "c" | "h" => "C",
        "cc" | "cpp" | "hpp" | "cxx" => "C++",
        "cs" => "C#",
        "rb" => "Ruby",
        "php" => "PHP",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" => "PowerShell",
        "sql" => "SQL",
        "css" | "scss" | "sass" | "less" => "CSS",
        "html" | "htm" => "HTML",
        "svelte" => "Svelte",
        "vue" => "Vue",
        "md" | "mdx" => "Markdown",
        "json" | "jsonc" => "JSON",
        "toml" => "TOML",
        "yml" | "yaml" => "YAML",
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BranchState {
    Default,
    Live,
    Merged,
    Open,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct BranchRow {
    pub name: String,
    pub when: String,
    pub sha: String,
    pub state: BranchState,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
pub struct WorktreeRow {
    pub path: String,
    pub head: String,
    pub branch: Option<String>,
    pub bare: bool,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct CommitRow {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub when: String,
    pub at_secs: u64,
    pub parents: Vec<String>,
    pub refs: String,
    pub card: Option<String>,
    pub agent: Option<String>,
    pub added: u64,
    pub removed: u64,
    pub files: u64,
    /// True when this commit is reachable from the default branch, which is
    /// what lets the history draw main as one lane and card branches as another.
    pub on_default: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
pub struct LanguageRow {
    pub name: String,
    pub bytes: u64,
    pub pct: f64,
}

/// Make `dir` usable as a harness project: a git repo with at least one commit.
pub fn ensure_workspace(dir: &Path) -> Result<(), GitError> {
    std::fs::create_dir_all(dir).map_err(|e| GitError::Io(e.to_string()))?;
    let git = CliGit::new(dir, dir.join(".harness-worktrees"));
    if git.git(dir, &["rev-parse", "--is-inside-work-tree"]).is_err() {
        git.git(dir, &["init", "-b", "main"])?;
    }
    if git.git(dir, &["rev-parse", "--verify", "HEAD"]).is_err() {
        let readme = dir.join("README.md");
        if !readme.exists() {
            std::fs::write(&readme, "# workspace\n").map_err(|e| GitError::Io(e.to_string()))?;
        }
        git.git(dir, &["add", "-A"])?;
        // A fresh install may have no committer identity configured.
        let _ = git.git(dir, &["config", "user.name", "Relay"]);
        let _ = git.git(dir, &["config", "user.email", "harness@localhost"]);
        git.git(dir, &["commit", "-m", "chore: workspace bootstrap"])?;
    }
    Ok(())
}

impl GitPort for CliGit {
    fn worktree_path(&self, name: &str) -> PathBuf {
        self.worktrees_root.join(sanitize(name))
    }

    fn create_worktree(&self, card_id: &str, base: &str) -> Result<WorktreePath, GitError> {
        let safe = sanitize(card_id);
        let wt = self.worktrees_root.join(&safe);
        let branch = self.branch_for(card_id);

        std::fs::create_dir_all(&self.worktrees_root).map_err(|e| GitError::Io(e.to_string()))?;
        if wt.exists() {
            let _ = self.git(
                &self.repo_root,
                &["worktree", "remove", "--force", &wt.to_string_lossy()],
            );
            if wt.exists() {
                std::fs::remove_dir_all(&wt).map_err(|e| GitError::Io(e.to_string()))?;
            }
        }
        let _ = self.git(&self.repo_root, &["worktree", "prune"]);
        let _ = self.git(&self.repo_root, &["branch", "-D", &branch]);

        self.git(
            &self.repo_root,
            &["worktree", "add", "-b", &branch, &wt.to_string_lossy(), base],
        )?;
        Ok(WorktreePath(wt))
    }

    fn commit(&self, wt: &WorktreePath, msg: &str, trailers: &Trailers) -> Result<String, GitError> {
        self.git(&wt.0, &["add", "-A"])?;
        let mut args: Vec<String> = vec!["commit".into(), "-m".into(), msg.to_string()];
        for (k, v) in &trailers.0 {
            args.push("--trailer".into());
            args.push(format!("{k}={v}"));
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        match self.git(&wt.0, &arg_refs) {
            Ok(_) => {}
            Err(GitError::Git(stderr)) => {
                if !stderr.contains("nothing to commit") && !stderr.contains("nothing added") {
                    return Err(GitError::Git(stderr));
                }
            }
            Err(e) => return Err(e),
        }
        self.rev_parse_head(&wt.0)
    }

    fn commit_wip(&self, wt: &WorktreePath) -> Result<Option<String>, GitError> {
        let status = self.git(&wt.0, &["status", "--porcelain"])?;
        if status.is_empty() {
            return Ok(None);
        }
        self.commit(wt, "wip: interrupted run", &Trailers::default())?;
        Ok(Some(self.rev_parse_head(&wt.0)?))
    }

    fn remove_worktree(&self, wt: &WorktreePath) -> Result<(), GitError> {
        if wt.0.exists() {
            self.git(
                &self.repo_root,
                &["worktree", "remove", "--force", &wt.0.to_string_lossy()],
            )?;
        }
        self.git(&self.repo_root, &["worktree", "prune"])?;
        Ok(())
    }

    fn diff_summary(&self, wt: &WorktreePath, base: &str) -> Result<String, GitError> {
        let range = format!("{base}...HEAD");
        let stat = self.git(&wt.0, &["diff", "--stat", &range]).unwrap_or_default();
        let patch = self.git(&wt.0, &["diff", &range]).unwrap_or_default();
        if patch.chars().count() > 4000 {
            let cut: String = patch.chars().take(4000).collect();
            Ok(format!("{stat}\n\n{cut}\n[diff truncated]"))
        } else {
            Ok(format!("{stat}\n\n{patch}"))
        }
    }

    fn diff_numstat(&self, wt: &WorktreePath, base: &str) -> Result<(u64, u64), GitError> {
        let range = format!("{base}...HEAD");
        let raw = self
            .git(&wt.0, &["diff", "--numstat", &range])
            .unwrap_or_default();
        let mut added = 0u64;
        let mut removed = 0u64;
        for line in raw.lines() {
            let mut cols = line.split('\t');
            added += cols.next().unwrap_or("").parse::<u64>().unwrap_or(0);
            removed += cols.next().unwrap_or("").parse::<u64>().unwrap_or(0);
        }
        Ok((added, removed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test in this module gets its own directory, guaranteed rather
    /// than hoped for. The name used to be pid plus nanoseconds, which is not
    /// unique: these tests run in parallel, the helper's first act is to delete
    /// that path, and two tests landing in the same nanosecond bucket meant one
    /// wiping the other's repository mid-setup. Windows's finer clock hid it;
    /// a macOS runner failed on `git init` inside a directory that had just
    /// been removed underneath it. A counter cannot collide.
    fn fresh_repo() -> (PathBuf, CliGit) {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "harness-git-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let git = CliGit::new(&repo, dir.join("worktrees"));
        git.git(&repo, &["init", "-b", "main"]).unwrap();
        git.git(&repo, &["config", "user.name", "test"]).unwrap();
        git.git(&repo, &["config", "user.email", "test@test"]).unwrap();
        std::fs::write(repo.join("README.md"), "# t\n").unwrap();
        git.git(&repo, &["add", "-A"]).unwrap();
        git.git(&repo, &["commit", "-m", "init"]).unwrap();
        (dir, git)
    }

    #[test]
    fn worktree_lifecycle_with_commits_and_wip() {
        let (_dir, git) = fresh_repo();

        let wt = git.create_worktree("card 1/abc", "main").unwrap();
        assert!(wt.0.join(".git").exists());
        assert!(
            !wt.0.starts_with(git.repo_root()),
            "worktrees must live outside the checkout, got {:?}",
            wt.0
        );

        std::fs::write(wt.0.join("feature.txt"), "content").unwrap();
        let sha = git
            .commit(
                &wt,
                "feat: do thing",
                &Trailers(vec![("Harness-Card".into(), "c1".into())]),
            )
            .unwrap();
        assert_eq!(sha.len(), 40);

        let body = git.git(&wt.0, &["log", "-1", "--format=%B"]).unwrap();
        assert!(body.contains("Harness-Card: c1"), "body was: {body}");

        assert_eq!(git.commit_wip(&wt).unwrap(), None);

        std::fs::write(wt.0.join("new.txt"), "dirty").unwrap();
        let wip_sha = git.commit_wip(&wt).unwrap().expect("wip commit expected");
        assert_eq!(wip_sha.len(), 40);

        git.remove_worktree(&wt).unwrap();
        assert!(!wt.0.exists());
    }

    #[test]
    fn empty_commit_reports_head_without_error() {
        let (_dir, git) = fresh_repo();
        let wt = git.create_worktree("c2", "main").unwrap();
        let head_before = git.rev_parse_head(&wt.0).unwrap();
        let sha = git.commit(&wt, "no changes", &Trailers::default()).unwrap();
        assert_eq!(sha, head_before);
    }

    #[test]
    fn create_worktree_is_idempotent_for_same_card() {
        let (_dir, git) = fresh_repo();
        let w1 = git.create_worktree("c3", "main").unwrap();
        std::fs::write(w1.0.join("a.txt"), "x").unwrap();
        let w2 = git.create_worktree("c3", "main").unwrap();
        assert_eq!(w1.0, w2.0);
        assert!(!w2.0.join("a.txt").exists());
    }

    #[test]
    fn a_repository_can_be_given_a_local_identity() {
        let (_dir, git) = fresh_repo();
        // `has_committer_identity` deliberately counts an inherited global
        // identity, which is why this cannot assert the absence of one on a
        // developer machine: what matters is that setting one is repo-local and
        // that commits work afterwards.
        git.set_local_identity().unwrap();
        assert!(git.has_committer_identity());
        assert_eq!(
            git.git(git.repo_root(), &["config", "--local", "user.email"]).unwrap(),
            "harness@localhost"
        );

        let wt = git.create_worktree("c-id", "main").unwrap();
        std::fs::write(wt.0.join("a.txt"), "x").unwrap();
        assert_eq!(
            git.commit(&wt, "feat: after identity", &Trailers::default())
                .unwrap()
                .len(),
            40
        );
    }

    #[test]
    fn a_local_repository_has_no_remote() {
        let (_dir, git) = fresh_repo();
        assert_eq!(git.remote(), None, "a local project needs no remote");
    }

    #[test]
    fn diff_numstat_counts_lines_against_base() {
        let (_dir, git) = fresh_repo();
        let wt = git.create_worktree("c4", "main").unwrap();
        std::fs::write(wt.0.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        git.commit(&wt, "feat: three lines", &Trailers::default()).unwrap();
        let (added, removed) = git.diff_numstat(&wt, "main").unwrap();
        assert_eq!((added, removed), (3, 0));
    }

    #[test]
    fn project_information_reads_history_branches_and_languages() {
        let (_dir, git) = fresh_repo();
        std::fs::write(git.repo_root().join("main.rs"), "fn main() {}\n".repeat(20)).unwrap();
        git.git(git.repo_root(), &["add", "-A"]).unwrap();
        git.git(
            git.repo_root(),
            &[
                "commit",
                "-m",
                "feat: add rust",
                "--trailer",
                "Harness-Card=c9",
                "--trailer",
                "Harness-Agent=builder",
            ],
        )
        .unwrap();

        assert_eq!(git.default_branch(), "main");
        assert_eq!(git.commit_count(), 2);

        let commits = git.recent_commits(10);
        assert_eq!(commits.len(), 2, "two commits on main before the side branch");
        let top = &commits[0];
        assert_eq!(top.subject, "feat: add rust");
        assert_eq!(top.card.as_deref(), Some("c9"));
        assert_eq!(top.agent.as_deref(), Some("builder"));
        assert_eq!(top.added, 20);
        assert_eq!(top.files, 1);
        assert!(top.on_default, "a commit on main must be marked as such");
        assert!(git.changed_lines(7) >= 20);

        let wt = git.create_worktree("c5", "main").unwrap();
        let branches = git.branches();
        assert!(branches.iter().any(|b| b.state == BranchState::Default && b.name == "main"));
        assert!(branches
            .iter()
            .any(|b| b.name == "harness/c5" && b.state == BranchState::Live));

        // A commit on a card branch is not on the default branch.
        std::fs::write(wt.0.join("side.txt"), "x
").unwrap();
        git.commit(&wt, "feat: side work", &Trailers::default()).unwrap();
        let side = git
            .recent_commits(10)
            .into_iter()
            .find(|c| c.subject == "feat: side work")
            .expect("the side commit");
        assert!(!side.on_default);

        let langs = git.languages();
        assert!(langs.iter().any(|l| l.name == "Rust"));
        assert!(langs.iter().map(|l| l.pct).sum::<f64>() > 99.0);

        let trees = git.worktree_list();
        assert_eq!(trees.len(), 2);
        assert!(trees.iter().any(|t| t.branch.as_deref() == Some("harness/c5")));

        std::fs::write(wt.0.join("dirty.txt"), "x").unwrap();
        assert!(git.is_dirty(&wt.0));

        let week = git.activity(7);
        assert_eq!(week.len(), 7);
        assert_eq!(week.iter().sum::<u64>(), 3, "two on main plus the side commit");
    }
}
