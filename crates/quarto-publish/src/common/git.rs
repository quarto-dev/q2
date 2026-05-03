//! Thin wrappers around the `git` CLI.
//!
//! Native-only: the gh-pages provider shells out to `git` for
//! everything (worktree, push, branch management). WASM providers
//! never enter this module — they don't need git.
//!
//! Helpers return `GitError` rather than `PublishError` so they
//! stay reusable; callers translate to `PublishError::UnableToPublish`
//! at the boundary with provider context.

use std::path::Path;
use std::process::{Command, Stdio};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("git command failed (exit {code}): {stderr}")]
    Failed { code: i32, stderr: String },

    #[error("git command did not exit normally (signal): {stderr}")]
    Signaled { stderr: String },

    #[error("could not spawn git: {0}")]
    Spawn(#[from] std::io::Error),
}

/// Result of a successful `git` invocation.
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub stdout: String,
    pub stderr: String,
}

/// Run `git <args...>` in `cwd`. Captures stdout and stderr.
///
/// On non-zero exit, returns `GitError::Failed` with the captured
/// stderr — providers surface this in their own diagnostics.
pub fn run_git(args: &[&str], cwd: &Path) -> Result<GitOutput, GitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if output.status.success() {
        Ok(GitOutput { stdout, stderr })
    } else if let Some(code) = output.status.code() {
        Err(GitError::Failed { code, stderr })
    } else {
        Err(GitError::Signaled { stderr })
    }
}

/// Run `git <args...>` ignoring non-zero exits — useful for
/// "ls-remote", which signals "not present" via exit code 2.
pub fn run_git_allow_failure(args: &[&str], cwd: &Path) -> std::io::Result<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
}

/// `git --version`. Returns the trimmed version string (e.g.
/// `"git version 2.40.1"`).
pub fn git_version() -> Result<String, GitError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    let out = run_git(&["--version"], &cwd)?;
    Ok(out.stdout.trim().to_string())
}

/// True when a local branch with `name` exists in `cwd`'s repo.
pub fn git_branch_exists(name: &str, cwd: &Path) -> Result<bool, GitError> {
    // `git show-ref --verify --quiet refs/heads/<name>` exits 0 if
    // the ref exists, non-zero otherwise. Use `run_git_allow_failure`
    // because non-zero is informational, not fatal.
    let ref_path = format!("refs/heads/{name}");
    let output = run_git_allow_failure(&["show-ref", "--verify", "--quiet", &ref_path], cwd)?;
    Ok(output.status.success())
}

/// True if both `user.name` and `user.email` are configured (any
/// scope — local, global, or system).
pub fn git_user_identity_configured(cwd: &Path) -> Result<bool, GitError> {
    let name = run_git_allow_failure(&["config", "user.name"], cwd)?;
    let email = run_git_allow_failure(&["config", "user.email"], cwd)?;
    let name_set = name.status.success() && !name.stdout.is_empty();
    let email_set = email.status.success() && !email.stdout.is_empty();
    Ok(name_set && email_set)
}

/// Current branch name, e.g. `"main"` or `"gh-pages"`.
pub fn git_current_branch(cwd: &Path) -> Result<String, GitError> {
    let out = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], cwd)?;
    Ok(out.stdout.trim().to_string())
}

/// True when the working tree has no uncommitted changes against
/// `HEAD`. Untracked files do not count.
pub fn git_dir_is_clean(cwd: &Path) -> Result<bool, GitError> {
    let out = run_git(&["diff", "HEAD"], cwd)?;
    Ok(out.stdout.trim().is_empty())
}

/// True when the repo at `cwd` has a remote named `origin` whose
/// URL is set.
pub fn git_has_origin(cwd: &Path) -> Result<bool, GitError> {
    let out = run_git_allow_failure(&["config", "--get", "remote.origin.url"], cwd)?;
    Ok(out.status.success() && !out.stdout.is_empty())
}

/// Get `remote.origin.url`. Returns `None` if no origin is set.
pub fn git_origin_url(cwd: &Path) -> Result<Option<String>, GitError> {
    let out = run_git_allow_failure(&["config", "--get", "remote.origin.url"], cwd)?;
    if out.status.success() {
        let url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if url.is_empty() {
            Ok(None)
        } else {
            Ok(Some(url))
        }
    } else {
        Ok(None)
    }
}

/// Check whether a remote branch exists on `origin`. Returns:
/// - `Ok(Some(true))` — branch present.
/// - `Ok(Some(false))` — branch absent (clean exit 2 from
///   `git ls-remote --exit-code`).
/// - `Ok(None)` — could not check (network failure, auth failure).
///   Caller decides whether to treat as fatal.
pub fn git_remote_branch_exists(branch: &str, cwd: &Path) -> Result<Option<bool>, GitError> {
    let out = run_git_allow_failure(
        &["ls-remote", "--quiet", "--exit-code", "origin", branch],
        cwd,
    )?;
    if out.status.success() {
        Ok(Some(true))
    } else if out.status.code() == Some(2) {
        // Documented "no matching ref" exit code from
        // `git ls-remote --exit-code`.
        Ok(Some(false))
    } else {
        Ok(None)
    }
}

/// True if a remote named `origin` exists at all.
pub fn git_remote_origin_exists(cwd: &Path) -> Result<bool, GitError> {
    let out = run_git_allow_failure(&["remote", "get-url", "origin"], cwd)?;
    Ok(out.status.success())
}

/// `git rev-parse <ref>` returning the resolved SHA (or None when
/// the ref doesn't exist).
pub fn git_rev_parse(ref_name: &str, cwd: &Path) -> Result<Option<String>, GitError> {
    let out = run_git_allow_failure(&["rev-parse", "--verify", "--quiet", ref_name], cwd)?;
    if out.status.success() {
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if sha.is_empty() {
            Ok(None)
        } else {
            Ok(Some(sha))
        }
    } else {
        Ok(None)
    }
}

/// Run a sequence of git commands, stopping at the first failure.
/// Convenience for provider code that needs to atomically run
/// "checkout, rm, commit, push".
pub fn run_git_seq(commands: &[&[&str]], cwd: &Path) -> Result<(), GitError> {
    for cmd in commands {
        run_git(cmd, cwd)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Initialise an empty git repo in `dir`, configure a user
    /// identity, return the path. Each test gets its own tempdir so
    /// they can run in parallel.
    fn init_repo(dir: &Path) {
        run_git(&["init", "--initial-branch=main"], dir).expect("init");
        run_git(&["config", "user.name", "Test User"], dir).expect("set user.name");
        run_git(&["config", "user.email", "test@example.com"], dir).expect("set user.email");
    }

    /// Make an initial commit so `HEAD` resolves.
    fn make_initial_commit(dir: &Path) {
        fs::write(dir.join("README.md"), "# test\n").unwrap();
        run_git(&["add", "README.md"], dir).expect("add");
        run_git(&["commit", "-m", "initial"], dir).expect("commit");
    }

    #[test]
    fn git_version_returns_a_version_string() {
        let v = git_version().expect("git --version should succeed");
        assert!(v.starts_with("git version "), "got: {v}");
    }

    #[test]
    fn run_git_captures_stdout() {
        let v = run_git(&["--version"], &std::env::current_dir().unwrap()).expect("git --version");
        assert!(v.stdout.contains("git version"), "got: {}", v.stdout);
    }

    #[test]
    fn run_git_returns_failed_on_nonzero_exit() {
        let temp = TempDir::new().unwrap();
        // `git status` outside a repo exits non-zero with a
        // diagnostic on stderr.
        let err = run_git(&["status"], temp.path()).unwrap_err();
        match err {
            GitError::Failed { code, stderr } => {
                assert_ne!(code, 0);
                assert!(stderr.contains("git"), "expected git in stderr: {stderr}");
            }
            other => panic!("expected Failed, got: {other:?}"),
        }
    }

    #[test]
    fn git_branch_exists_true_for_main_after_initial_commit() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        make_initial_commit(temp.path());
        assert!(git_branch_exists("main", temp.path()).unwrap());
        assert!(!git_branch_exists("nonexistent", temp.path()).unwrap());
    }

    #[test]
    fn git_branch_exists_false_for_unborn_main() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        // Before any commit, `refs/heads/main` doesn't exist yet.
        assert!(!git_branch_exists("main", temp.path()).unwrap());
    }

    #[test]
    fn git_user_identity_configured_true_after_init_repo_helper() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        assert!(git_user_identity_configured(temp.path()).unwrap());
    }

    #[test]
    fn git_current_branch_returns_main_after_initial_commit() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        make_initial_commit(temp.path());
        assert_eq!(git_current_branch(temp.path()).unwrap(), "main");
    }

    #[test]
    fn git_dir_is_clean_true_immediately_after_commit() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        make_initial_commit(temp.path());
        assert!(git_dir_is_clean(temp.path()).unwrap());
    }

    #[test]
    fn git_dir_is_clean_false_with_uncommitted_changes() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        make_initial_commit(temp.path());
        fs::write(temp.path().join("README.md"), "# changed\n").unwrap();
        assert!(!git_dir_is_clean(temp.path()).unwrap());
    }

    #[test]
    fn git_has_origin_false_for_fresh_repo() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        assert!(!git_has_origin(temp.path()).unwrap());
        assert_eq!(git_origin_url(temp.path()).unwrap(), None);
    }

    #[test]
    fn git_has_origin_true_after_remote_add() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        run_git(
            &["remote", "add", "origin", "https://example.com/foo.git"],
            temp.path(),
        )
        .unwrap();
        assert!(git_has_origin(temp.path()).unwrap());
        assert_eq!(
            git_origin_url(temp.path()).unwrap().as_deref(),
            Some("https://example.com/foo.git")
        );
    }

    #[test]
    fn git_remote_origin_exists_matches_git_has_origin() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        assert!(!git_remote_origin_exists(temp.path()).unwrap());
        run_git(
            &["remote", "add", "origin", "https://example.com/foo.git"],
            temp.path(),
        )
        .unwrap();
        assert!(git_remote_origin_exists(temp.path()).unwrap());
    }

    #[test]
    fn git_remote_branch_exists_true_for_branch_on_bare_remote() {
        // Set up: bare remote with a `gh-pages` branch.
        let bare = TempDir::new().unwrap();
        run_git(&["init", "--bare"], bare.path()).unwrap();

        // Working clone, init, commit, branch, push.
        let work = TempDir::new().unwrap();
        run_git(&["init", "--initial-branch=main"], work.path()).unwrap();
        run_git(&["config", "user.name", "T"], work.path()).unwrap();
        run_git(&["config", "user.email", "t@e.com"], work.path()).unwrap();
        fs::write(work.path().join("a.txt"), "a").unwrap();
        run_git(&["add", "a.txt"], work.path()).unwrap();
        run_git(&["commit", "-m", "x"], work.path()).unwrap();
        run_git(&["checkout", "--orphan", "gh-pages"], work.path()).unwrap();
        run_git(&["rm", "-rf", "."], work.path()).unwrap();
        fs::write(work.path().join("index.html"), "ok").unwrap();
        run_git(&["add", "index.html"], work.path()).unwrap();
        run_git(&["commit", "-m", "page"], work.path()).unwrap();
        run_git(
            &["remote", "add", "origin", &bare.path().to_string_lossy()],
            work.path(),
        )
        .unwrap();
        run_git(&["push", "origin", "gh-pages"], work.path()).unwrap();

        // Now: a fresh clone should see gh-pages on remote.
        let clone = TempDir::new().unwrap();
        run_git(
            &[
                "clone",
                &bare.path().to_string_lossy(),
                &clone.path().to_string_lossy(),
            ],
            std::env::current_dir().unwrap().as_path(),
        )
        .unwrap();
        assert_eq!(
            git_remote_branch_exists("gh-pages", clone.path()).unwrap(),
            Some(true),
        );
        assert_eq!(
            git_remote_branch_exists("does-not-exist", clone.path()).unwrap(),
            Some(false),
        );
    }

    #[test]
    fn git_rev_parse_returns_sha_after_commit() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        make_initial_commit(temp.path());
        let sha = git_rev_parse("HEAD", temp.path()).unwrap();
        let sha = sha.expect("HEAD should resolve");
        assert_eq!(sha.len(), 40, "expected 40-char SHA, got {sha}");
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn git_rev_parse_returns_none_for_nonexistent_ref() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        make_initial_commit(temp.path());
        assert_eq!(git_rev_parse("refs/heads/nope", temp.path()).unwrap(), None);
    }
}
