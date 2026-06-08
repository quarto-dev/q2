//! Switch-task command — in-place sub-task transition without spinning a
//! fresh worktree.
//!
//! Companion to `cargo xtask create-worktree`. Where `create-worktree`
//! is the right answer for *parallel* / *investigation* work (you want
//! isolation, you'll pay the npm install + cargo cold-cache cost
//! deliberately), `switch-task` is the right answer for *sequential*
//! implementation work in an epic: most sub-tasks branch off the same
//! integration line, do not need a separate worktree, and benefit from
//! keeping `node_modules/` and `target/` warm across branch switches.
//!
//! Typical use, mid-epic:
//!
//!   # in some worktree (could be the main repo or any .worktrees/*)
//!   cargo xtask switch-task bd-yxqt --from feature/q2-preview-command
//!
//! That:
//!   1. switches the current worktree to `feature/q2-preview-command`
//!      and runs `git pull --ff-only` so it picks up siblings' merges,
//!   2. creates a new topic branch `beads/<id>-<slug>` off the new
//!      tip,
//!   3. marks the braid strand `in_progress`,
//!   4. rewrites the `<!-- BEGIN/END WORKTREE CONTEXT -->` block in
//!      `CLAUDE.local.md` so the next Claude Code session knows what
//!      it's working on.
//!
//! Omit `--from` to branch off the current HEAD without changing
//! branches first. `--no-claim` skips the braid status update.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::create_worktree::{
    IssueMetadata, SectionKind, build_section, derive_slug, fetch_issue_metadata,
    parse_external_ref_to_github_url, update_claude_local_md, validate_slug,
};

/// Find the *current worktree's* root via `git rev-parse --show-toplevel`.
///
/// We deliberately *don't* reuse `create_worktree::repo_root()` here:
/// that walks up to find `Cargo.toml` with `[workspace]`, which for a
/// worktree of a workspace lands on the *main repo* (the `[workspace]`
/// declaration is shared across all worktrees). The CLAUDE.local.md
/// we want to rewrite is per-worktree, so we need the worktree's own
/// toplevel.
fn current_worktree_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("spawning `git rev-parse --show-toplevel`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`git rev-parse --show-toplevel` failed:\n{stderr}");
    }
    let path = std::str::from_utf8(&output.stdout)
        .context("toplevel path is not valid UTF-8")?
        .trim();
    Ok(PathBuf::from(path))
}

/// Arguments for `cargo xtask switch-task`.
pub struct Args {
    /// Braid strand ID to switch to (e.g. `bd-yxqt`).
    pub beads_id: String,
    /// Integration branch to switch+pull to before creating the topic
    /// branch. Omit to branch off the current HEAD.
    pub from: Option<String>,
    /// Optional slug override (kebab-case). Default: derived from the
    /// strand title via `derive_slug`.
    pub slug: Option<String>,
    /// Don't mark the strand `in_progress` in braid.
    pub no_claim: bool,
}

pub fn run(args: Args) -> Result<()> {
    let root = current_worktree_root()?;

    let meta = fetch_issue_metadata(&args.beads_id)?;
    let slug = match args.slug.as_deref() {
        Some(s) => {
            validate_slug(s)?;
            s.to_string()
        }
        None => derive_slug(&meta.title)?,
    };
    let branch = format!("beads/{}-{}", args.beads_id, slug);

    if let Some(from) = args.from.as_deref() {
        // Best-effort fetch so origin/<from> reflects the latest tip.
        // Failure is non-fatal — the user might be offline or `<from>`
        // might be a local-only branch.
        git_fetch_origin(from);
        git_switch_create_from(&branch, from)?;
    } else {
        git_switch_create(&branch)?;
    }

    if !args.no_claim {
        claim_issue(&args.beads_id)?;
    }

    update_worktree_context(&root, &args.beads_id, &meta)?;

    print_summary(&args.beads_id, &branch, args.from.as_deref());
    Ok(())
}

fn git_fetch_origin(branch: &str) {
    eprintln!("→ git fetch origin {branch} (best-effort)");
    let _ = Command::new("git")
        .args(["fetch", "origin", branch])
        .status();
}

/// Create `<branch>` at the tip of `<start_point>` and check it out.
/// Uses `git switch -c` with a start-point, which doesn't require
/// `<start_point>` to be checked out in this worktree — so this is
/// safe when the integration branch is already checked out in another
/// worktree (the common case for the main repo + `.worktrees/*` setup).
fn git_switch_create_from(branch: &str, start_point: &str) -> Result<()> {
    eprintln!("→ git switch -c {branch} {start_point}");
    let status = Command::new("git")
        .args(["switch", "-c", branch, start_point])
        .status()
        .context("spawning `git switch -c <branch> <start>`")?;
    if !status.success() {
        bail!(
            "`git switch -c {branch} {start_point}` failed — branch may \
             already exist; rename or delete it first"
        );
    }
    Ok(())
}

fn git_switch_create(branch: &str) -> Result<()> {
    eprintln!("→ git switch -c {branch}");
    let status = Command::new("git")
        .args(["switch", "-c", branch])
        .status()
        .context("spawning `git switch -c`")?;
    if !status.success() {
        bail!(
            "`git switch -c {branch}` failed — branch may already exist; \
             rename or delete it first"
        );
    }
    Ok(())
}

fn claim_issue(id: &str) -> Result<()> {
    eprintln!("→ braid update {id} --status in_progress");
    let status = Command::new("braid")
        .args(["update", id, "--status", "in_progress"])
        .status()
        .context("spawning `braid update`")?;
    if !status.success() {
        bail!("`braid update {id} --status in_progress` failed");
    }
    Ok(())
}

fn update_worktree_context(root: &Path, id: &str, meta: &IssueMetadata) -> Result<()> {
    let github_url = parse_external_ref_to_github_url(meta.external_ref.as_deref());
    let kind = SectionKind::Braid {
        id: id.to_string(),
        title: meta.title.clone(),
        github_url,
    };
    let section = build_section(&kind);
    let path = root.join("CLAUDE.local.md");
    update_claude_local_md(&path, &section)?;
    eprintln!("→ refreshed worktree context in {}", path.display());
    Ok(())
}

fn print_summary(id: &str, branch: &str, from: Option<&str>) {
    println!();
    println!("✓ Switched to {id} on branch {branch}");
    if let Some(from) = from {
        println!("  (branched off {from} after pull)");
    }
    println!("  braid strand marked in_progress");
    println!();
    println!("Next: implement, commit, then promote with:");
    println!("  git switch {}", from.unwrap_or("<epic-branch>"));
    println!("  git merge --no-ff {branch}");
}
