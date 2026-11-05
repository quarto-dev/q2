/*
 * tests/integration/jupyter_kernel_selection.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Kernel selection through the real render pipeline (bd-yt2o2ovt, Phase 3).
 */

//! Kernel selection through the real render pipeline.
//!
//! An explicit kernel name in the front matter must be honored, and an
//! unknown one must fail the render instead of silently running the
//! language's default kernel (bd-9pvolzwl).
//!
//! These tests start no kernel: an unknown kernelspec fails before any
//! launch. They skip when no Python kernel is registered, because then the
//! old (ignored-key) behavior would also fail, for a different reason, and
//! the test could not tell the two apart.

use std::sync::Arc;

use quarto_core::engine::jupyter::{ResolvedKernel, list_kernelspecs};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// Sync on purpose: the engine drives its own runtime with `block_on`, which
/// panics inside a `#[tokio::test]` runtime (see `capture_splice_engines.rs`).
fn python_kernel_available() -> bool {
    // Its own short-lived runtime: `list_kernelspecs` spawns `jupyter --paths`.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let specs: Vec<ResolvedKernel> = rt.block_on(list_kernelspecs());
    specs.iter().any(|s| s.language.to_lowercase() == "python")
}

#[test]
fn unknown_explicit_kernel_fails_the_render() {
    if !python_kernel_available() {
        eprintln!("no Python kernel registered, skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("doc.qmd");
    std::fs::write(
        &source,
        "---\njupyter: nosuchkernel\n---\n\n```{python}\n1 + 1\n```\n",
    )
    .unwrap();

    // The same entry `q2 render` uses (render_document_to_file underneath).
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&source, "html", &RenderToFileOptions::default(), runtime);

    let err = result.expect_err("an unknown kernel must fail the render");
    let text = format!("{err:?}");
    assert!(text.contains("nosuchkernel"), "error must name it: {text}");
    assert!(text.contains("not found"), "got: {text}");
}
