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
//!   3. marks the beads issue `in_progress`,
//!   4. rewrites the `<!-- BEGIN/END WORKTREE CONTEXT -->` block in
//!      `CLAUDE.local.md` so the next Claude Code session knows what
//!      it's working on.
//!
//! Omit `--from` to branch off the current HEAD without changing
//! branches first. `--no-claim` skips the beads status update.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::create_worktree::{
    BeadsMetadata, SectionKind, build_section, derive_slug, fetch_beads_metadata,
    parse_external_ref_to_github_url, repo_root, update_claude_local_md, validate_slug,
};

/// Arguments for `cargo xtask switch-task`.
pub struct Args {
    /// Beads issue ID to switch to (e.g. `bd-yxqt`).
    pub beads_id: String,
    /// Integration branch to switch+pull to before creating the topic
    /// branch. Omit to branch off the current HEAD.
    pub from: Option<String>,
    /// Optional slug override (kebab-case). Default: derived from the
    /// issue title via `derive_slug`.
    pub slug: Option<String>,
    /// Don't mark the issue `in_progress` in beads.
    pub no_claim: bool,
}

pub fn run(args: Args) -> Result<()> {
    let root = repo_root()?;

    let meta = fetch_beads_metadata(&args.beads_id)?;
    let slug = match args.slug.as_deref() {
        Some(s) => {
            validate_slug(s)?;
            s.to_string()
        }
        None => derive_slug(&meta.title)?,
    };
    let branch = format!("beads/{}-{}", args.beads_id, slug);

    if let Some(from) = args.from.as_deref() {
        git_switch_and_pull(from)?;
    }

    git_switch_new_branch(&branch)?;

    if !args.no_claim {
        claim_issue(&args.beads_id)?;
    }

    update_worktree_context(&root, &args.beads_id, &meta)?;

    print_summary(&args.beads_id, &branch, args.from.as_deref());
    Ok(())
}

fn git_switch_and_pull(branch: &str) -> Result<()> {
    eprintln!("→ git switch {branch}");
    let status = Command::new("git")
        .args(["switch", branch])
        .status()
        .context("spawning `git switch`")?;
    if !status.success() {
        bail!("`git switch {branch}` failed — does the branch exist locally?");
    }

    eprintln!("→ git pull --ff-only");
    let status = Command::new("git")
        .args(["pull", "--ff-only"])
        .status()
        .context("spawning `git pull`")?;
    if !status.success() {
        // Non-fast-forward state is something the user should resolve;
        // don't try to be clever here.
        bail!("`git pull --ff-only` failed — resolve divergence and retry");
    }
    Ok(())
}

fn git_switch_new_branch(branch: &str) -> Result<()> {
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
    eprintln!("→ br update {id} --status in_progress");
    let status = Command::new("br")
        .args(["update", id, "--status", "in_progress"])
        .status()
        .context("spawning `br update`")?;
    if !status.success() {
        bail!("`br update {id} --status in_progress` failed");
    }
    Ok(())
}

fn update_worktree_context(root: &Path, id: &str, meta: &BeadsMetadata) -> Result<()> {
    let github_url = parse_external_ref_to_github_url(meta.external_ref.as_deref());
    let kind = SectionKind::Beads {
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
    println!("  beads issue marked in_progress");
    println!();
    println!("Next: implement, commit, then promote with:");
    println!("  git switch {}", from.unwrap_or("<epic-branch>"));
    println!("  git merge --no-ff {branch}");
}
