//! `cargo xtask build-ts-bundle` — build the committed ts-bundle.mjs driver
//! (`ts-packages/quarto-ts-bundle/dist/`).
//!
//! The driver is a single ESM file (rollup, its plugins and rollup's wasm)
//! that `q2 call build-ts-extension` pipes to `node -`. It is committed to the
//! repo and embedded into the `quarto-core` crate via `include_str!` in
//! `extension/build.rs`.
//!
//! A plain `cargo build` does NOT refresh the bundle — run this (or
//! `cargo xtask build-all`) after changing `quarto-ts-bundle`, or the next
//! build silently re-embeds a stale driver. CI rebuilds it and fails on a diff.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use crate::util::nested_command;

pub fn run() -> Result<()> {
    let project_root = find_project_root()?;
    let pkg_dir = project_root.join("ts-packages/quarto-ts-bundle");
    if !pkg_dir.join("package.json").is_file() {
        bail!(
            "ts-packages/quarto-ts-bundle/package.json not found under {}",
            project_root.display()
        );
    }

    println!("━━━ Building ts-bundle driver ━━━");
    let status = nested_command("npm")
        .args(["run", "bundle"])
        .current_dir(&pkg_dir)
        .status()
        .with_context(|| format!("Failed to spawn npm in {}", pkg_dir.display()))?;
    if !status.success() {
        bail!("ts-bundle driver build failed");
    }
    println!("✓ ts-packages/quarto-ts-bundle/dist/ is up to date");
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
