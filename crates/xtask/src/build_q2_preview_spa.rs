//! `cargo xtask build-q2-preview-spa` — build just the q2-preview SPA.
//!
//! Faster iteration than `cargo xtask build-all` when only the SPA
//! source has changed. The `quarto-preview` crate's `include_dir!`
//! picks up the freshly built `q2-preview-spa/dist/` on the next
//! Rust compile.
//!
//! Mirrors `build_trace_viewer.rs` for the trace viewer.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use crate::util::nested_command;

pub fn run() -> Result<()> {
    let project_root = find_project_root()?;
    let spa_dir = project_root.join("q2-preview-spa");
    if !spa_dir.join("package.json").is_file() {
        bail!(
            "q2-preview-spa/package.json not found under {}. The SPA was scaffolded in bd-hfjj Phase 6.",
            project_root.display()
        );
    }

    println!("━━━ Building q2-preview-spa ━━━");
    let status = nested_command("npm")
        .args(["run", "build"])
        .current_dir(&spa_dir)
        .status()
        .with_context(|| format!("Failed to spawn npm in {}", spa_dir.display()))?;
    if !status.success() {
        bail!("q2-preview-spa build failed");
    }
    println!("✓ q2-preview-spa/dist/ is up to date");
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
