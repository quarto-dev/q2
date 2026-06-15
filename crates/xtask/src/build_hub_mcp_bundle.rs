//! `cargo xtask build-hub-mcp-bundle` — build the self-contained hub
//! MCP server bundle (`ts-packages/quarto-hub-mcp/dist-bundle/`).
//!
//! The bundle (single `index.mjs` + the platform keyring addon as a
//! mini node_modules + `build-info.json` stamp) is what the `q2`
//! binary embeds for `q2 mcp` (bd-81cfshmw). Like the q2-preview SPA,
//! a plain `cargo build` does NOT refresh it — run this (or
//! `cargo xtask build-all`) after changing quarto-hub-mcp,
//! quarto-sync-client, or quarto-automerge-schema sources, or the
//! next `q2` build silently re-embeds a stale bundle. The esbuild
//! config compiles workspace deps straight from their TypeScript
//! sources (the `source` export condition), so no prior `tsc` builds
//! are needed.
//!
//! Mirrors `build_q2_preview_spa.rs`.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use crate::util::nested_command;

pub fn run() -> Result<()> {
    let project_root = find_project_root()?;
    let pkg_dir = project_root.join("ts-packages/quarto-hub-mcp");
    if !pkg_dir.join("package.json").is_file() {
        bail!(
            "ts-packages/quarto-hub-mcp/package.json not found under {}",
            project_root.display()
        );
    }

    println!("━━━ Building quarto-hub-mcp bundle ━━━");
    let status = nested_command("npm")
        .args(["run", "bundle"])
        .current_dir(&pkg_dir)
        .status()
        .with_context(|| format!("Failed to spawn npm in {}", pkg_dir.display()))?;
    if !status.success() {
        bail!("quarto-hub-mcp bundle build failed");
    }
    println!("✓ ts-packages/quarto-hub-mcp/dist-bundle/ is up to date");
    Ok(())
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
