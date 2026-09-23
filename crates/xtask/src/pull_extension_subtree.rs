//! `cargo xtask pull-extension-subtree` — port of Quarto 1's hidden
//! `pull-git-subtree` dev command (`src/command/dev-call/pull-git-subtree/cmd.ts`).
//!
//! Keeps vendored extension repos in sync under `resources/extension-subtrees/<name>/`
//! via `git subtree`, tracked by the `git-subtree-dir`/`git-subtree-split` trailers
//! `git subtree` writes into the squash commit it creates.

use crate::util::nested_command;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single vendored extension subtree entry (mirrors Q1's `SubtreeConfig`,
/// using Rust field names directly — this is a q2-only dev seam with no
/// need to mirror Q1's camelCase JSON shape).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtreeConfig {
    pub name: String,
    pub prefix: String,
    pub remote_url: String,
    pub remote_branch: String,
}

/// Production subtree table.
///
/// A function rather than a `const` because `SubtreeConfig` owns `String`s,
/// which cannot be built non-empty in a const context.
///
/// `orange-book` (book-projects P2, plan item 80): pinned by upstream tag
/// `0.2.0` = `2b59b76727f22bdcc3522ee56e2437f6abd81c36`, which was
/// `origin/main`'s HEAD on `quarto-ext/orange-book` at vendoring time
/// (2026-09-24) — the durable pin lives in the subtree-add commit's
/// `git-subtree-split` trailer, not in this row; `remote_branch: "main"`
/// just means "re-running `pull-extension-subtree orange-book` later picks
/// up whatever main has moved to," same as every other row.
pub fn subtrees() -> Vec<SubtreeConfig> {
    vec![SubtreeConfig {
        name: "orange-book".to_string(),
        prefix: "resources/extension-subtrees/orange-book".to_string(),
        remote_url: "https://github.com/quarto-ext/orange-book.git".to_string(),
        remote_branch: "main".to_string(),
    }]
}

/// Result of processing a single subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullOutcome {
    /// No prior split found (or the prefix is missing) — ran `subtree add`.
    Added,
    /// New upstream commits found — ran `subtree pull`.
    Pulled,
    /// Upstream has no commits past the last recorded split.
    NoOp,
}

/// CLI arguments for `pull-extension-subtree`.
pub struct Args {
    /// Subtree name to process; `None` or `Some("all")` processes every row.
    pub name: Option<String>,
    /// Dev/test seam: replace the built-in [`subtrees()`] table with the
    /// contents of this JSON file.
    pub table: Option<PathBuf>,
}

pub fn run(args: Args) -> Result<()> {
    let root = subtree_root()?;
    let table = load_table(args.table.as_deref())?;
    let selected = select_subtrees(&table, args.name.as_deref())?;

    let mut had_error = false;
    for config in selected {
        println!("=== Pulling subtree: {} ===", config.name);
        println!("Prefix: {}", config.prefix);
        println!("Remote: {} ({})", config.remote_url, config.remote_branch);
        match pull_subtree(&root, config) {
            Ok(outcome) => println!("-> {}", describe(outcome)),
            Err(e) => {
                eprintln!("Failed to pull subtree {}: {e:#}", config.name);
                had_error = true;
            }
        }
    }

    if had_error {
        bail!("one or more subtree pulls failed");
    }
    Ok(())
}

fn describe(outcome: PullOutcome) -> &'static str {
    match outcome {
        PullOutcome::Added => "added (git subtree add --squash)",
        PullOutcome::Pulled => "pulled new commits (git subtree pull --squash)",
        PullOutcome::NoOp => "no new commits to merge",
    }
}

/// Dev/test seam: operate on a repo other than the real one. Falls back to
/// [`crate::create_worktree::repo_root`] when unset.
fn subtree_root() -> Result<PathBuf> {
    if let Ok(root) = std::env::var("QUARTO_SUBTREE_ROOT") {
        return Ok(PathBuf::from(root));
    }
    crate::create_worktree::repo_root()
}

fn load_table(table_path: Option<&Path>) -> Result<Vec<SubtreeConfig>> {
    match table_path {
        Some(path) => {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("reading subtree table override {}", path.display()))?;
            serde_json::from_str(&content)
                .with_context(|| format!("parsing subtree table override {}", path.display()))
        }
        None => Ok(subtrees()),
    }
}

fn select_subtrees<'a>(
    table: &'a [SubtreeConfig],
    name: Option<&str>,
) -> Result<Vec<&'a SubtreeConfig>> {
    match name {
        None | Some("all") => Ok(table.iter().collect()),
        Some(name) => match table.iter().find(|c| c.name == name) {
            Some(config) => Ok(vec![config]),
            None => {
                let available: Vec<&str> = table.iter().map(|c| c.name.as_str()).collect();
                bail!(
                    "unknown subtree name '{name}'; available: {}",
                    available.join(", ")
                );
            }
        },
    }
}

fn run_git(root: &Path, args: &[&str]) -> Result<String> {
    let output = nested_command("git")
        .current_dir(root)
        .args(args)
        .output()
        .with_context(|| format!("spawning `git {}`", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git {} failed (exit {:?}):\n{stderr}",
            args.join(" "),
            output.status.code()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Find the most recent `git-subtree-split:` commit for `prefix`, by
/// grepping commit bodies for the `git-subtree-dir:` trailer `git subtree`
/// writes into its squash commit. Mirrors Q1's `findLastSplit`.
fn find_last_split(root: &Path, prefix: &str) -> Result<Option<String>> {
    let grep = format!("--grep=git-subtree-dir: {prefix}$");
    let log = run_git(root, &["log", &grep, "-1", "--pretty=%b"])?;
    Ok(log.lines().find_map(|line| {
        line.strip_prefix("git-subtree-split:")
            .and_then(|rest| rest.split_whitespace().next())
            .map(str::to_string)
    }))
}

/// Fetch `config`'s remote and reconcile it into `root` at `config.prefix`:
/// `subtree add` if the prefix is missing or no prior split is recorded,
/// `subtree pull` if upstream has new commits, otherwise a no-op.
pub fn pull_subtree(root: &Path, config: &SubtreeConfig) -> Result<PullOutcome> {
    run_git(root, &["fetch", &config.remote_url, &config.remote_branch])?;
    let fetch_head = run_git(root, &["rev-parse", "FETCH_HEAD"])?;

    let prefix_path = root.join(&config.prefix);
    let last_split = find_last_split(root, &config.prefix)?;

    let Some(last_split) = last_split.filter(|_| prefix_path.is_dir()) else {
        run_git(
            root,
            &[
                "subtree",
                "add",
                "--squash",
                &format!("--prefix={}", config.prefix),
                &config.remote_url,
                &config.remote_branch,
            ],
        )?;
        return Ok(PullOutcome::Added);
    };

    let range = format!("{last_split}..{fetch_head}");
    let has_new_commits = run_git(root, &["log", "--oneline", &range, "-1"])?;
    if has_new_commits.is_empty() {
        return Ok(PullOutcome::NoOp);
    }

    run_git(
        root,
        &[
            "subtree",
            "pull",
            "--squash",
            &format!("--prefix={}", config.prefix),
            &config.remote_url,
            &config.remote_branch,
        ],
    )?;
    Ok(PullOutcome::Pulled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn git_init(dir: &Path, initial_branch: &str) {
        run_git(dir, &["init", "-q", "-b", initial_branch]).unwrap();
        run_git(dir, &["config", "user.email", "xtask-test@example.com"]).unwrap();
        run_git(dir, &["config", "user.name", "xtask test"]).unwrap();
    }

    fn write_file(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn commit_all(dir: &Path, message: &str) {
        run_git(dir, &["add", "-A"]).unwrap();
        run_git(dir, &["commit", "-q", "-m", message]).unwrap();
    }

    /// A remote extension repo with two commits, one of which adds
    /// `_extensions/fake/_extension.yml`.
    fn build_fixture_remote() -> TempDir {
        let dir = TempDir::new().unwrap();
        git_init(dir.path(), "main");
        write_file(
            dir.path(),
            "_extensions/fake/_extension.yml",
            "title: Fake\n",
        );
        commit_all(dir.path(), "add fake extension");
        write_file(dir.path(), "README.md", "fake extension repo\n");
        commit_all(dir.path(), "add readme");
        dir
    }

    /// A consumer repo with an initial commit. `git subtree add` fails with
    /// "ambiguous argument 'HEAD'" on a zero-commit repo (verified
    /// empirically) — an initial commit, even empty, is enough.
    fn build_fixture_consumer() -> TempDir {
        let dir = TempDir::new().unwrap();
        git_init(dir.path(), "main");
        run_git(
            dir.path(),
            &["commit", "-q", "--allow-empty", "-m", "initial commit"],
        )
        .unwrap();
        dir
    }

    fn config_for(remote: &Path, prefix: &str) -> SubtreeConfig {
        SubtreeConfig {
            name: "fake".to_string(),
            prefix: prefix.to_string(),
            remote_url: remote.to_string_lossy().to_string(),
            remote_branch: "main".to_string(),
        }
    }

    #[test]
    fn production_subtrees_table_rows_are_well_formed() {
        let table = subtrees();
        assert!(
            !table.is_empty(),
            "subtrees() should contain the orange-book row (book-projects P2 item 80)"
        );
        for row in &table {
            assert_eq!(
                row.prefix,
                format!("resources/extension-subtrees/{}", row.name),
                "row {:?}: prefix must be resources/extension-subtrees/<name>",
                row.name
            );
            assert!(
                row.remote_url.starts_with("https://"),
                "row {:?}: remote_url must be an https URL",
                row.name
            );
            assert!(
                !row.remote_branch.is_empty(),
                "row {:?}: remote_branch must be set",
                row.name
            );
        }
        let orange_book = table
            .iter()
            .find(|r| r.name == "orange-book")
            .expect("subtrees() must contain the orange-book row");
        assert_eq!(
            orange_book.remote_url,
            "https://github.com/quarto-ext/orange-book.git"
        );
        assert_eq!(orange_book.remote_branch, "main");
    }

    #[test]
    fn pull_subtree_adds_then_noops_then_pulls_new_upstream_commit() {
        let remote = build_fixture_remote();
        let consumer = build_fixture_consumer();
        let config = config_for(remote.path(), "vendor/fake");

        // Initial run: no prior split -> `subtree add --squash`.
        let outcome = pull_subtree(consumer.path(), &config).unwrap();
        assert_eq!(outcome, PullOutcome::Added);
        assert!(
            consumer
                .path()
                .join(&config.prefix)
                .join("_extensions/fake/_extension.yml")
                .is_file(),
            "subtree add should have vendored the remote's tree under the prefix"
        );
        let body = run_git(consumer.path(), &["log", "-1", "--pretty=%b"]).unwrap();
        let trailer_body = run_git(
            consumer.path(),
            &["log", "--all", "-1", "--grep=git-subtree-dir:"],
        )
        .unwrap();
        assert!(
            !trailer_body.is_empty(),
            "expected a commit carrying the git-subtree-dir trailer somewhere in history"
        );
        let _ = body; // merge commit body is empty; the trailer lives on the squash commit.

        // Immediate second run: no new upstream commits -> no-op.
        let outcome = pull_subtree(consumer.path(), &config).unwrap();
        assert_eq!(outcome, PullOutcome::NoOp);

        // New upstream commit -> `subtree pull --squash` picks it up.
        write_file(remote.path(), "CHANGELOG.md", "v2\n");
        commit_all(remote.path(), "add changelog");
        let outcome = pull_subtree(consumer.path(), &config).unwrap();
        assert_eq!(outcome, PullOutcome::Pulled);
        assert!(
            consumer
                .path()
                .join(&config.prefix)
                .join("CHANGELOG.md")
                .is_file(),
            "subtree pull should have picked up the new upstream file"
        );
    }

    #[test]
    fn select_subtrees_errors_on_unknown_name_listing_available_names() {
        let table = vec![
            config_for(Path::new("u1"), "p1"),
            SubtreeConfig {
                name: "beta".to_string(),
                ..config_for(Path::new("u2"), "p2")
            },
        ];
        let err = select_subtrees(&table, Some("gamma")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("gamma"), "message was: {msg}");
        assert!(msg.contains("fake"), "message was: {msg}");
        assert!(msg.contains("beta"), "message was: {msg}");
    }

    #[test]
    fn select_subtrees_processes_every_row_when_name_omitted_or_all() {
        let table = vec![
            config_for(Path::new("u1"), "p1"),
            SubtreeConfig {
                name: "beta".to_string(),
                ..config_for(Path::new("u2"), "p2")
            },
        ];
        assert_eq!(select_subtrees(&table, None).unwrap().len(), 2);
        assert_eq!(select_subtrees(&table, Some("all")).unwrap().len(), 2);
    }

    #[test]
    fn run_honors_subtree_root_env_and_table_override() {
        let remote = build_fixture_remote();
        let consumer = build_fixture_consumer();
        let config = config_for(remote.path(), "vendor/fake");

        let table_dir = TempDir::new().unwrap();
        let table_path = table_dir.path().join("table.json");
        std::fs::write(
            &table_path,
            serde_json::to_string(&[config.clone()]).unwrap(),
        )
        .unwrap();

        // Each nextest test runs in its own process, so mutating the
        // process environment here is safe (no cross-test race).
        unsafe {
            std::env::set_var("QUARTO_SUBTREE_ROOT", consumer.path());
        }
        let result = run(Args {
            name: None,
            table: Some(table_path),
        });
        unsafe {
            std::env::remove_var("QUARTO_SUBTREE_ROOT");
        }
        result.unwrap();

        assert!(
            consumer
                .path()
                .join(&config.prefix)
                .join("_extensions/fake/_extension.yml")
                .is_file()
        );
    }
}
