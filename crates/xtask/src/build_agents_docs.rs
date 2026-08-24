//! `cargo xtask build-agents-docs` — stage the docs-site llms.txt
//! artifacts for embedding into the `q2` binary (bd-hwop1zii).
//!
//! Steps:
//!
//! 1. Render `docs/` in place with `q2` (via `cargo run --bin q2`),
//!    exactly like CI's docs build. The render's llms-txt pass writes
//!    `docs/_site/llms.txt`, `docs/_site/llms-full.txt`, one `.md`
//!    companion per page, and the ledger
//!    `docs/.quarto/llms-manifest.json`.
//! 2. Copy the ledger-listed artifacts (and only those — a
//!    user-authored resource `.md` in `_site/` is not ours to embed)
//!    into a fresh `agents-docs-dist/` at the workspace root.
//! 3. Write `agents-docs-dist/embed-info.json` recording the staging
//!    checkout's git commit + dirty flag, for `q2 docs llms
//!    --embed-info`.
//!
//! The next `cargo build --bin q2` embeds the staged tree via the
//! `quarto` crate's `build.rs` (`include_dir!`). Like the preview SPA
//! and the MCP bundle, a plain rebuild does NOT re-render the docs —
//! rerun this xtask to refresh the embed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::util::nested_command;

pub fn run() -> Result<()> {
    let root = find_project_root()?;

    // Capture provenance BEFORE rendering: the render writes into
    // `docs/_site` and `docs/.quarto` and can leave generated siblings
    // (e.g. `*_files/`) in the source tree, so a post-render `git
    // status` would report a clean checkout as dirty. What we want to
    // record is the state of the sources the docs were built from.
    let info = embed_info(&root)?;

    println!("━━━ Rendering docs/ (llms.txt artifacts) ━━━");
    render_docs(&root)?;

    println!("━━━ Staging agents-docs-dist/ ━━━");
    let manifest_path = root.join("docs").join(".quarto").join("llms-manifest.json");
    let manifest_json = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "reading {} — did the docs render run its llms-txt pass? \
             (docs/_quarto.yml must set `website.llms-txt: true`)",
            manifest_path.display()
        )
    })?;
    let site_dir = root.join("docs").join("_site");
    let dist_dir = root.join("agents-docs-dist");
    let staged = stage_files(&manifest_json, &site_dir, &dist_dir)?;

    std::fs::write(dist_dir.join("embed-info.json"), &info)
        .with_context(|| format!("writing {}/embed-info.json", dist_dir.display()))?;

    println!(
        "✓ agents-docs-dist/ staged: {staged} artifacts, embed-info {}",
        info.trim()
    );
    println!("  Rebuild to embed: cargo build --bin q2");
    Ok(())
}

/// Render the docs website in place (`docs/_site`), the same render CI
/// runs. `q2` is invoked through a nested cargo so a stale or
/// placeholder-embed binary is fine — the render doesn't consume the
/// embed.
fn render_docs(root: &Path) -> Result<()> {
    let mut cmd = nested_command("cargo");
    cmd.current_dir(root)
        .args(["run", "--bin", "q2", "--", "render", "docs"]);
    let status = cmd
        .status()
        .context("spawning `cargo run --bin q2 -- render docs`")?;
    if !status.success() {
        bail!("`q2 render docs` failed (exit {:?})", status.code());
    }
    Ok(())
}

/// Copy the ledger-listed artifacts from `site_dir` into a fresh
/// `dist_dir`, preserving the directory tree. Returns the number of
/// files staged. The dist is rebuilt from scratch so files removed
/// from the docs can never linger in the embed.
fn stage_files(manifest_json: &str, site_dir: &Path, dist_dir: &Path) -> Result<usize> {
    #[derive(serde::Deserialize)]
    struct LlmsManifest {
        generated: Vec<String>,
    }

    let manifest: LlmsManifest =
        serde_json::from_str(manifest_json).context("parsing llms-manifest.json")?;
    if manifest.generated.is_empty() {
        bail!("llms-manifest.json lists no generated artifacts");
    }
    if !manifest.generated.iter().any(|rel| rel == "llms.txt") {
        bail!("llms-manifest.json does not list llms.txt; refusing to stage");
    }

    if dist_dir.exists() {
        std::fs::remove_dir_all(dist_dir)
            .with_context(|| format!("clearing stale {}", dist_dir.display()))?;
    }
    std::fs::create_dir_all(dist_dir)
        .with_context(|| format!("creating {}", dist_dir.display()))?;

    for rel in &manifest.generated {
        // Ledger paths are output-dir-relative with forward slashes.
        let src = site_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let dest = dist_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::copy(&src, &dest).with_context(|| {
            format!(
                "copying {} — the ledger lists it but the render did not \
                 produce it",
                src.display()
            )
        })?;
    }
    Ok(manifest.generated.len())
}

/// Provenance sidecar for `q2 docs llms --embed-info`: the staging
/// checkout's HEAD commit and whether the working tree was dirty.
fn embed_info(root: &Path) -> Result<String> {
    let commit = git_stdout(root, &["rev-parse", "HEAD"])?;
    let dirty = !git_stdout(root, &["status", "--porcelain"])?.is_empty();
    Ok(format!(
        "{}\n",
        serde_json::json!({ "commit": commit, "dirty": dirty })
    ))
}

fn git_stdout(root: &Path, args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("running git {args:?}"))?;
    if !out.status.success() {
        bail!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn find_project_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("Failed to get current directory")?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let content = std::fs::read_to_string(&cargo_toml)?;
            if content.contains("[workspace]") {
                return Ok(dir);
            }
        }
        if !dir.pop() {
            bail!("Could not find workspace root (Cargo.toml with [workspace])");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::stage_files;

    fn manifest(entries: &[&str]) -> String {
        serde_json::json!({ "version": 1, "generated": entries }).to_string()
    }

    #[test]
    fn stages_listed_files_preserving_tree_and_clearing_stale() {
        let tmp = tempfile::TempDir::new().unwrap();
        let site = tmp.path().join("_site");
        let dist = tmp.path().join("dist");
        std::fs::create_dir_all(site.join("guides")).unwrap();
        std::fs::write(site.join("llms.txt"), "# Index\n").unwrap();
        std::fs::write(site.join("llms-full.txt"), "full\n").unwrap();
        std::fs::write(site.join("guides").join("a.md"), "# A\n").unwrap();
        // An unlisted _site file (user resource) must NOT be staged.
        std::fs::write(site.join("guides").join("stray.md"), "stray\n").unwrap();
        // A stale file from a previous staging must be cleared.
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("removed-page.md"), "old\n").unwrap();

        let n = stage_files(
            &manifest(&["guides/a.md", "llms.txt", "llms-full.txt"]),
            &site,
            &dist,
        )
        .unwrap();

        assert_eq!(n, 3);
        assert_eq!(
            std::fs::read_to_string(dist.join("llms.txt")).unwrap(),
            "# Index\n"
        );
        assert_eq!(
            std::fs::read_to_string(dist.join("guides").join("a.md")).unwrap(),
            "# A\n"
        );
        assert!(dist.join("llms-full.txt").is_file());
        assert!(
            !dist.join("guides").join("stray.md").exists(),
            "unlisted _site files must not be staged"
        );
        assert!(
            !dist.join("removed-page.md").exists(),
            "stale staged files must be cleared"
        );
    }

    #[test]
    fn refuses_manifest_without_llms_txt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let err = stage_files(
            &manifest(&["guides/a.md"]),
            &tmp.path().join("_site"),
            &tmp.path().join("dist"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("llms.txt"), "{err}");
    }

    #[test]
    fn missing_listed_file_is_an_error_naming_the_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let site = tmp.path().join("_site");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("llms.txt"), "# Index\n").unwrap();
        let err = stage_files(
            &manifest(&["llms.txt", "ghost.md"]),
            &site,
            &tmp.path().join("dist"),
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("ghost.md"), "{err:#}");
    }
}
