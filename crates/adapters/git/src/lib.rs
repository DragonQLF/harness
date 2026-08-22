use std::path::{Path, PathBuf};
use std::process::Command;

use harness_ports::{GitError, GitPort, Trailers, WorktreePath};

pub struct CliGit {
    repo_root: PathBuf,
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

impl CliGit {
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> Result<String, GitError> {
        let out = Command::new("git")
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
}

pub fn ensure_workspace(dir: &Path) -> Result<(), GitError> {
    std::fs::create_dir_all(dir).map_err(|e| GitError::Io(e.to_string()))?;
    let git = CliGit::new(dir);
    let inside = git.git(dir, &["rev-parse", "--is-inside-work-tree"]);
    if inside.is_err() {
        git.git(dir, &["init", "-b", "main"])?;
    }
    let has_commit = git.git(dir, &["rev-parse", "--verify", "HEAD"]).is_ok();
    if !has_commit {
        let readme = dir.join("README.md");
        if !readme.exists() {
            std::fs::write(&readme, "# workspace\n")
                .map_err(|e| GitError::Io(e.to_string()))?;
        }
        git.git(dir, &["add", "-A"])?;
        git.git(dir, &["commit", "-m", "chore: workspace bootstrap"])?;
    }
    Ok(())
}

impl GitPort for CliGit {
    fn create_worktree(&self, card_id: &str, base: &str) -> Result<WorktreePath, GitError> {
        let safe = sanitize(card_id);
        let wt = self
            .repo_root
            .join(".harness")
            .join("worktrees")
            .join(&safe);
        let branch = format!("harness/{safe}");

        if wt.exists() {
            let _ = self.git(
                &self.repo_root,
                &["worktree", "remove", "--force", &wt.to_string_lossy()],
            );
        }
        let _ = self.git(&self.repo_root, &["branch", "-D", &branch]);

        self.git(
            &self.repo_root,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &wt.to_string_lossy(),
                base,
            ],
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
                let combined = format!("{stderr}");
                if !combined.contains("nothing to commit") {
                    return Err(GitError::Git(combined));
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_repo() -> (PathBuf, CliGit) {
        let dir = std::env::temp_dir().join(format!(
            "harness-git-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = CliGit::new(&dir);
        git.git(&dir, &["init", "-b", "main"]).unwrap();
        git.git(&dir, &["config", "user.name", "test"]).unwrap();
        git.git(&dir, &["config", "user.email", "test@test"]).unwrap();
        std::fs::write(dir.join("README.md"), "# t\n").unwrap();
        git.git(&dir, &["add", "-A"]).unwrap();
        git.git(&dir, &["commit", "-m", "init"]).unwrap();
        (dir, git)
    }

    #[test]
    fn worktree_lifecycle_with_commits_and_wip() {
        let (_dir, git) = fresh_repo();

        let wt = git.create_worktree("card 1/abc", "main").unwrap();
        assert!(wt.0.join(".git").exists());

        std::fs::write(wt.0.join("feature.txt"), "content").unwrap();
        let sha = git
            .commit(&wt, "feat: do thing", &Trailers(vec![("Harness-Card".into(), "c1".into())]))
            .unwrap();
        assert_eq!(sha.len(), 40);

        let body = git
            .git(&wt.0, &["log", "-1", "--format=%B"])
            .unwrap();
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
}
