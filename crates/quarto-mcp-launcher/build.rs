//! Build script that locates the hub MCP server bundle at compile time.
//!
//! Mirrors `crates/quarto-preview/build.rs`. The `include_dir!` macro
//! needs a concrete compile-time path; this script resolves it:
//!
//! 1. If `ts-packages/quarto-hub-mcp/dist-bundle/index.mjs` exists,
//!    embed that directory.
//! 2. Otherwise embed a placeholder directory containing only a
//!    `BUNDLE_NOT_BUILT` marker, so the build still succeeds (fresh
//!    clones can `cargo build` before any npm step). `q2 mcp` then
//!    fails at runtime with an actionable message pointing at
//!    `cargo xtask build-hub-mcp-bundle`.
//!
//! The chosen path is exposed as `QUARTO_HUB_MCP_EMBED_DIR` via
//! `cargo:rustc-env`, consumed by `src/bundle.rs`.
//!
//! Stale-embed warning (see CLAUDE.md and the 2026-05-20 preview-SPA
//! incident): a plain `cargo build` re-embeds whatever dist-bundle/
//! was last produced. The per-file rerun-if-changed entries below make
//! the *Rust* side rebuild when the bundle changes, but nothing makes
//! the bundle itself rebuild — run `cargo xtask build-hub-mcp-bundle`
//! (or `build-all`) after touching the TypeScript sources.

use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.join("..").join("..");
    let real_bundle = workspace_root
        .join("ts-packages")
        .join("quarto-hub-mcp")
        .join("dist-bundle");

    let embed_dir = if real_bundle.join("index.mjs").is_file() {
        real_bundle.clone()
    } else {
        make_placeholder()
    };

    println!(
        "cargo:rustc-env=QUARTO_HUB_MCP_EMBED_DIR={}",
        embed_dir.display()
    );

    // Re-run if the real bundle changes. Per-file entries catch
    // in-place content rewrites that a directory-mtime watch misses.
    println!("cargo:rerun-if-changed={}", real_bundle.display());
    if real_bundle.is_dir() {
        watch_recursive(&real_bundle);
    }
}

fn watch_recursive(root: &Path) {
    let entries = match std::fs::read_dir(root) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            watch_recursive(&path);
        }
    }
}

fn make_placeholder() -> PathBuf {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dir = out_dir.join("placeholder-bundle");
    std::fs::create_dir_all(&dir).expect("create placeholder bundle dir");
    let marker = dir.join("BUNDLE_NOT_BUILT");
    let contents = "Run `cargo xtask build-hub-mcp-bundle`, then rebuild the q2 binary.\n";
    let existing = std::fs::read_to_string(&marker).ok();
    if existing.as_deref() != Some(contents) {
        std::fs::write(&marker, contents).expect("write placeholder marker");
    }
    println!(
        "cargo:warning=ts-packages/quarto-hub-mcp/dist-bundle/ not found; `q2 mcp` will be \
         non-functional in this build. Run `cargo xtask build-hub-mcp-bundle` and rebuild."
    );
    dir
}
