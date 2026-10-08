/*
 * tests/integration/pipeline_temp_dir_cleanup.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * q2-preview hub-parity Plan 2 (P2-D2): the pipeline's lazily created
 * temp directory is removed when the render ends. Before the fix every
 * native render leaked a `quarto-pipeline_*` directory (4 MB per typst
 * render, from the vendored typst packages).
 */

//! These tests point `TMPDIR` at a private directory and count entries
//! only there; they never look at the real `$TMPDIR`. Setting a process
//! environment variable is safe here only because nextest runs each test
//! in its own process; under `cargo test` threads would race on it.

use std::path::Path;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_core::stage::StageContext;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn have(bin: &str) -> bool {
    std::process::Command::new(bin)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Entries named `quarto-pipeline_*` directly under `root`.
fn pipeline_dirs(root: &Path) -> Vec<String> {
    std::fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("quarto-pipeline_"))
        .collect()
}

/// Point `TMPDIR` at a fresh private directory and return it.
fn private_tmpdir() -> TempDir {
    let private = TempDir::new().unwrap();
    // SAFETY: one test per process under nextest (see the module comment);
    // nothing else reads the environment concurrently.
    unsafe { std::env::set_var("TMPDIR", private.path()) };
    private
}

fn stage_context(runtime: Arc<dyn SystemRuntime>) -> StageContext {
    let project = ProjectContext {
        dir: std::path::PathBuf::from("/project"),
        output_dir: std::path::PathBuf::from("/project"),
        is_single_file: true,
        ..Default::default()
    };
    let doc = DocumentInfo::from_path("/project/test.qmd");
    StageContext::new(runtime, Format::html(), project, doc).expect("stage context")
}

/// The lazy path owns the directory: it exists while the context lives
/// and is gone after it drops.
#[test]
fn lazy_temp_dir_is_removed_when_the_context_drops() {
    let private = private_tmpdir();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let ctx = stage_context(runtime);
    let dir = ctx.temp_dir().unwrap().to_path_buf();
    assert!(dir.is_dir());
    assert_eq!(pipeline_dirs(private.path()).len(), 1);
    drop(ctx);
    assert!(!dir.exists(), "temp dir must be removed with the context");
    assert!(pipeline_dirs(private.path()).is_empty());
}

/// A caller-owned path (`with_temp_dir`) is never deleted by the context.
#[test]
fn caller_owned_temp_dir_survives_the_context() {
    let _private = private_tmpdir();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mine = TempDir::new().unwrap();
    let ctx = stage_context(runtime).with_temp_dir(mine.path().to_path_buf());
    assert_eq!(ctx.temp_dir().unwrap(), mine.path());
    drop(ctx);
    assert!(mine.path().is_dir());
}

/// End to end: a `format: typst` render leaves no `quarto-pipeline_*`
/// directory behind.
#[test]
fn typst_render_leaves_no_pipeline_temp_dir() {
    if !have("pandoc") || !have("typst") {
        eprintln!("SKIP: `pandoc` and `typst` are both required");
        return;
    }
    let private = private_tmpdir();
    let proj = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    std::fs::write(root.join("a.qmd"), "---\ntitle: T\n---\n\nHello.\n").unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(
        &root.join("a.qmd"),
        "typst",
        &RenderToFileOptions {
            quiet: true,
            ..Default::default()
        },
        runtime,
    )
    .expect("typst render");
    assert!(result.output_path.is_file());
    assert!(
        pipeline_dirs(private.path()).is_empty(),
        "leaked: {:?}",
        pipeline_dirs(private.path())
    );
}

/// The failed-pandoc input JSON is copied out of the pipeline temp dir,
/// so the `Q-20-3` "input JSON retained at <path>" message stays true
/// after the context (and its temp dir) is gone.
#[test]
fn failed_pandoc_json_outlives_the_pipeline_temp_dir() {
    let private = private_tmpdir();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let ctx = stage_context(runtime);
    let json = ctx.temp_dir().unwrap().join("pandoc-input.json");
    std::fs::write(&json, b"{\"blocks\":[]}").unwrap();

    let kept = quarto_core::stage::stages::retain_failed_json(&json).expect("copy out");
    assert!(kept.starts_with(private.path()));
    drop(ctx);
    assert!(!json.exists());
    assert_eq!(std::fs::read(&kept).unwrap(), b"{\"blocks\":[]}");
}
