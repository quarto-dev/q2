//! `cargo xtask build-hub-client-embed` — build the hub-client editor
//! bundle that `q2 preview --ui editor` embeds.
//!
//! Runs hub-client's `build:preview-embed` npm script (live-share plan
//! Phase 4, bd-jt1etjbn): a hub-client production build with auth off
//! (no `VITE_GOOGLE_CLIENT_ID`), the sync server pinned to the relative
//! `/ws` the preview server itself serves, and the PWA service worker
//! disabled, emitted to `hub-client/dist-preview-embed/`. The
//! `quarto-preview` crate's `include_dir!` picks the directory up on
//! the next Rust compile (files byte-identical to the q2-preview-spa
//! viewer dist — notably the ~38 MB WASM — are stripped at embed time
//! and served through the viewer embed instead).
//!
//! Mirrors `build_q2_preview_spa.rs` for the viewer SPA.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::util::nested_command;

pub fn run() -> Result<()> {
    let project_root = find_project_root()?;
    let hub_client_dir = project_root.join("hub-client");
    if !hub_client_dir.join("package.json").is_file() {
        bail!(
            "hub-client/package.json not found under {}",
            project_root.display()
        );
    }

    println!("━━━ Building hub-client preview embed ━━━");
    let status = nested_command("npm")
        .args(["run", "build:preview-embed"])
        .current_dir(&hub_client_dir)
        .status()
        .with_context(|| format!("Failed to spawn npm in {}", hub_client_dir.display()))?;
    if !status.success() {
        bail!("hub-client preview-embed build failed");
    }
    // `build:preview-embed` ends with the gzip precompression post-pass
    // (scripts/precompress-dist.mjs), so dist-preview-embed/ already
    // carries its `.gz` siblings here.
    write_stamp(&project_root, &hub_client_dir.join("dist-preview-embed"))?;
    println!("✓ hub-client/dist-preview-embed/ is up to date");
    Ok(())
}

/// Record the fingerprint of the bundle's sources in
/// `<dist>/build-info.json`. `quarto-preview`'s build script compares it
/// with the sources at embed time, so `q2 preview --ui editor` can warn
/// when the embedded bundle predates them.
fn write_stamp(root: &Path, dist: &Path) -> Result<()> {
    use quarto_build_support::embed_stamp::{
        STAMP_FILENAME, editor_bundle_inputs, fingerprint, render_stamp,
    };
    let fp = fingerprint(root, &editor_bundle_inputs(root))
        .context("Failed to fingerprint the editor bundle's sources")?;
    std::fs::write(dist.join(STAMP_FILENAME), render_stamp(&fp))
        .with_context(|| format!("Failed to write {STAMP_FILENAME} into {}", dist.display()))
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
    use super::*;
    use quarto_build_support::embed_stamp::{
        Freshness, STAMP_FILENAME, check, editor_bundle_inputs, fingerprint,
    };

    fn read_status(root: &Path, dist: &Path) -> Freshness {
        let stamp = std::fs::read_to_string(dist.join(STAMP_FILENAME)).ok();
        let now = fingerprint(root, &editor_bundle_inputs(root)).unwrap();
        check(stamp.as_deref(), &now)
    }

    #[test]
    fn written_stamp_is_fresh_until_a_source_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let src = root.join("hub-client/src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("App.tsx"), "v1").unwrap();
        let dist = root.join("hub-client/dist-preview-embed");
        std::fs::create_dir_all(&dist).unwrap();

        assert_eq!(read_status(root, &dist), Freshness::NoStamp);
        write_stamp(root, &dist).unwrap();
        assert_eq!(read_status(root, &dist), Freshness::Fresh);
        // The stamp lives inside dist-preview-embed/, which is not an input.
        std::fs::write(src.join("App.tsx"), "v2").unwrap();
        assert_eq!(read_status(root, &dist), Freshness::Stale);
    }
}
