//! `cargo xtask braid-snapshot` — write a **backup-only** `braid export`
//! snapshot to `.braid/snapshot.jsonl` at the repo root.
//!
//! The braid skein (an automerge CRDT) is the single source of truth. This
//! snapshot exists so issues stay greppable in PRs, diffable in git history,
//! and recoverable — nothing more.
//!
//! **BACKUP ONLY / STRICTLY ONE-DIRECTIONAL.** The snapshot flows
//! automerge → file. It is **never** an import or sync source back into the
//! skein. Never run `braid import .braid/snapshot.jsonl`. On a git conflict,
//! do not hand-merge — regenerate with `cargo xtask braid-snapshot` (the live
//! skein is authoritative; the file is a photograph). See CLAUDE.md
//! § Snapshot backup policy.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::create_worktree::repo_root;

/// Path to the committed backup snapshot, relative to the repo root.
/// Deliberately under `.braid/` (a committed directory) — distinct from the
/// gitignored `.braid.toml` secret file at the repo root.
pub fn snapshot_path(root: &Path) -> PathBuf {
    root.join(".braid").join("snapshot.jsonl")
}

pub fn run() -> Result<()> {
    let root = repo_root()?;
    let out_path = snapshot_path(&root);

    let output = Command::new("braid")
        .arg("export")
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "braid is required \u{2014} install from https://github.com/cscheid/braid; see project README"
                )
            } else {
                anyhow::Error::new(e).context("spawning `braid export`")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("braid export failed:\n{stderr}");
    }

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(&out_path, &output.stdout)
        .with_context(|| format!("writing {}", out_path.display()))?;

    // `braid export` writes one strand per line; count lines for a friendly
    // confirmation (trailing newline means the last line is empty, so count
    // newline bytes — that equals the strand count for well-formed JSONL).
    let strands = output.stdout.iter().filter(|&&b| b == b'\n').count();
    println!("wrote {} ({strands} strands)", out_path.display());
    println!(
        "backup only \u{2014} never `braid import` this file; on conflict, regenerate with `cargo xtask braid-snapshot`"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_path_is_under_dot_braid_at_root() {
        let root = Path::new("/repo");
        let p = snapshot_path(root);
        assert!(p.ends_with(".braid/snapshot.jsonl"), "got {}", p.display());
        assert_eq!(p, PathBuf::from("/repo/.braid/snapshot.jsonl"));
    }
}
