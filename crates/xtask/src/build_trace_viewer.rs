//! `cargo xtask build-trace-viewer` — build just the trace-viewer SPA.
//!
//! Faster iteration than `cargo xtask build-all` when only the SPA needs
//! rebuilding. The `quarto-trace-server` crate's `include_dir!` picks up
//! the freshly built `trace-viewer/dist/` on the next Rust compile.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

use crate::util::nested_command;

pub fn run() -> Result<()> {
    let project_root = find_project_root()?;
    let trace_viewer_dir = project_root.join("trace-viewer");
    if !trace_viewer_dir.join("package.json").is_file() {
        bail!(
            "trace-viewer/package.json not found under {}. The SPA is introduced in Phase 4.3.",
            project_root.display()
        );
    }

    println!("━━━ Building trace-viewer SPA ━━━");
    let status = nested_command("npm")
        .args(["run", "build"])
        .current_dir(&trace_viewer_dir)
        .status()
        .with_context(|| format!("Failed to spawn npm in {}", trace_viewer_dir.display()))?;
    if !status.success() {
        bail!("trace-viewer build failed");
    }
    println!("✓ trace-viewer/dist/ is up to date");
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
