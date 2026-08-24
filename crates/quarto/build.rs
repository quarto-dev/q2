//! Build script that locates the embedded-docs tree for `q2 docs llms`
//! (bd-hwop1zii).
//!
//! Mirrors `crates/quarto-trace-server/build.rs`. The `include_dir!`
//! macro needs a concrete compile-time path; this script resolves it:
//!
//! 1. If `agents-docs-dist/llms.txt` exists at the workspace root (the
//!    tree staged by `cargo xtask build-agents-docs`), embed that
//!    directory.
//! 2. Otherwise, write a placeholder directory into `OUT_DIR` — just an
//!    `embed-info.json` marking the embed as a placeholder — and embed
//!    that, so fresh clones build without the docs staged. `q2 docs
//!    llms` then fails at runtime pointing at the xtask.
//!
//! The chosen path is exposed as `QUARTO_DOCS_LLMS_EMBED_DIR` via
//! `cargo:rustc-env`, consumed by `src/commands/docs_llms.rs` through
//! `include_dir!("$QUARTO_DOCS_LLMS_EMBED_DIR")`.

use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir.join("..").join("..");
    let real_dist = workspace_root.join("agents-docs-dist");

    let embed_dir = if real_dist.join("llms.txt").is_file() {
        real_dist.clone()
    } else {
        make_placeholder_dist()
    };

    println!(
        "cargo:rustc-env=QUARTO_DOCS_LLMS_EMBED_DIR={}",
        embed_dir.display()
    );

    // Re-run if the staged tree changes. A directory-mtime watch misses
    // in-place file rewrites (re-staging overwrites files at the same
    // paths), so emit one rerun-if-changed per file; the directory
    // entry picks up additions and removals.
    println!("cargo:rerun-if-changed={}", real_dist.display());
    if real_dist.is_dir() {
        watch_recursive(&real_dist);
    }
}

/// Emit `cargo:rerun-if-changed=<path>` for every file and directory
/// under `root`.
fn watch_recursive(root: &Path) {
    let entries = match std::fs::read_dir(root) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            watch_recursive(&path);
        }
    }
}

fn make_placeholder_dist() -> PathBuf {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dist = out_dir.join("docs-llms-placeholder");
    std::fs::create_dir_all(&dist).expect("create docs-llms placeholder dir");

    // The runtime detects a placeholder by the absence of `llms.txt`;
    // the sidecar makes the state explicit for `--embed-info`.
    let sidecar = dist.join("embed-info.json");
    write_if_changed(&sidecar, "{\"placeholder\":true}\n");

    println!(
        "cargo:warning=agents-docs-dist/llms.txt not found; embedding a \
         placeholder for `q2 docs llms`. Run `cargo xtask build-agents-docs` \
         and rebuild to embed the real docs."
    );

    dist
}

fn write_if_changed(path: &Path, contents: &str) {
    let existing = std::fs::read_to_string(path).ok();
    if existing.as_deref() != Some(contents) {
        std::fs::write(path, contents).expect("write placeholder embed-info.json");
    }
}
