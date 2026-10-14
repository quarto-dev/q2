//! `cargo xtask build-engine-host-bundle` — build the committed
//! engine-host-node.mjs bundle (`ts-packages/quarto-engine-host-node/dist/`).
//!
//! The bundle is a single ESM file that Node runs as the TS engine host
//! harness. It is committed to the repo and embedded into the `quarto-core`
//! Rust crate via `include_str!` in `engine/ts_process.rs` (Plan 1b).
//!
//! A plain `cargo build` does NOT refresh the bundle — run this (or
//! `cargo xtask build-all`) after changing `quarto-engine-host-node`,
//! `@quarto/api`, or `@quarto/types` sources, or the next build silently
//! re-embeds a stale bundle.
//!
//! The esbuild config uses `platform:'node'` + `format:'esm'` +
//! `target:'node24'` + `external:['node:*']`. The committed bytes are deterministic across rebuilds at
//! the same git commit (volatile `builtAt` is written only to
//! `dist/build-info.json`, not into the bundle itself).
//!
//! Mirrors `build_hub_mcp_bundle.rs`.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use crate::util::nested_command;

pub fn run() -> Result<()> {
    let project_root = find_project_root()?;
    let pkg_dir = project_root.join("ts-packages/quarto-engine-host-node");
    if !pkg_dir.join("package.json").is_file() {
        bail!(
            "ts-packages/quarto-engine-host-node/package.json not found under {}",
            project_root.display()
        );
    }

    println!("━━━ Building engine-host-node bundle ━━━");
    let status = nested_command("npm")
        .args(["run", "bundle"])
        .current_dir(&pkg_dir)
        .status()
        .with_context(|| format!("Failed to spawn npm in {}", pkg_dir.display()))?;
    if !status.success() {
        bail!("engine-host-node bundle build failed");
    }
    println!("✓ ts-packages/quarto-engine-host-node/dist/ is up to date");
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
