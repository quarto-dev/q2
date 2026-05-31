/*
 * tests/integration/fail_fast.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `q2 render --fail-fast` orchestrator tests (bd-gi25b).
 */

//! Integration tests for the fail-fast project-render mode.
//!
//! These drive a real `ProjectPipeline` against a temp project
//! directory (same shape as `incremental_rebuild.rs`) and assert the
//! `--fail-fast` contract:
//!
//! - Sequential (`QUARTO_JOBS=1`): stops at the first error in
//!   document order; `stopped_early` is set; fewer files are rendered
//!   than the project contains.
//! - Clean project: `--fail-fast` is a no-op — everything renders,
//!   `stopped_early == false`.
//! - Without `--fail-fast`: all errors are reported (regression guard
//!   for the default best-effort behavior).
//!
//! `QUARTO_JOBS` is set via `std::env::set_var`. Under nextest each
//! `#[test]` runs in its own process, so the env mutation is
//! process-local and does not leak into sibling tests — acceptable
//! here even though `set_var` is otherwise a shared-state hazard.

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn canonical(p: &std::path::Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn write(p: &std::path::Path, contents: &str) {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(p, contents).unwrap();
}

fn runtime_with_cache(project_dir: &std::path::Path) -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::with_cache_dir(
        project_dir.join(".quarto/cache"),
    ))
}

/// Run one full project render with the given `fail_fast` setting.
fn render_project_fail_fast(
    project_dir: &std::path::Path,
    runtime: Arc<dyn SystemRuntime>,
    fail_fast: bool,
) -> quarto_core::project::orchestrator::ProjectRenderSummary {
    let mut project =
        ProjectContext::discover(project_dir, runtime.as_ref()).expect("discover project");
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    )
    .with_fail_fast(fail_fast);
    pollster::block_on(pipeline.run()).expect("project render")
}

/// Sequential fail-fast: an early file (in `project.files` order) has
/// a parse error; later files are clean. With `QUARTO_JOBS=1` the
/// dispatch is deterministic and breaks at the first error, so it
/// produces strictly fewer outputs than the clean-file count and flags
/// `stopped_early`.
#[test]
fn fail_fast_sequential_stops_at_first_error() {
    // Force the deterministic single-threaded dispatch.
    // SAFETY/scope: process-local under nextest (one process per test).
    unsafe {
        std::env::set_var("QUARTO_JOBS", "1");
    }

    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    // `a.qmd` sorts first and has an unclosed `_` emphasis → Pass-1
    // parse error. The remaining files are clean.
    write(
        &project_dir.join("a.qmd"),
        "---\ntitle: A\n---\n\nLorem ipsum _dolor\n",
    );
    write(
        &project_dir.join("b.qmd"),
        "---\ntitle: B\n---\n\nClean b.\n",
    );
    write(
        &project_dir.join("c.qmd"),
        "---\ntitle: C\n---\n\nClean c.\n",
    );
    write(
        &project_dir.join("d.qmd"),
        "---\ntitle: D\n---\n\nClean d.\n",
    );

    let runtime = runtime_with_cache(&project_dir);
    let summary = render_project_fail_fast(&project_dir, runtime, true);

    assert!(
        !summary.pass1_failures.is_empty(),
        "fail-fast should report at least one Pass-1 failure; got none"
    );
    assert!(
        summary.stopped_early,
        "summary.stopped_early should be true under fail-fast with an error"
    );
    // There are 4 project files. The early error means Pass-2 never
    // runs (the run() barrier returns after Pass-1), so no outputs are
    // produced — strictly fewer than the 3 clean files.
    assert!(
        summary.outputs.len() < 3,
        "fail-fast should stop early and render fewer than the clean-file count; \
         got {} outputs",
        summary.outputs.len()
    );

    unsafe {
        std::env::remove_var("QUARTO_JOBS");
    }
}

/// A clean project under `--fail-fast` renders everything and does not
/// flag `stopped_early` — the flag is a no-op on error-free projects.
#[test]
fn fail_fast_clean_project_renders_all() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout us.\n",
    );

    let runtime = runtime_with_cache(&project_dir);
    let summary = render_project_fail_fast(&project_dir, runtime, true);

    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "clean project should have no failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    assert!(
        !summary.stopped_early,
        "stopped_early must be false for a clean project under --fail-fast"
    );
    assert_eq!(
        summary.outputs.len(),
        2,
        "all pages should render with --fail-fast on a clean project"
    );
}

/// Regression: WITHOUT `--fail-fast`, the default best-effort behavior
/// reports every error. Two files each with a parse error → both
/// reported, `stopped_early` stays false.
#[test]
fn no_fail_fast_reports_all_errors() {
    // Sequential so we deterministically see both files processed.
    unsafe {
        std::env::set_var("QUARTO_JOBS", "1");
    }

    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("a.qmd"),
        "---\ntitle: A\n---\n\nLorem _ipsum\n",
    );
    write(
        &project_dir.join("b.qmd"),
        "---\ntitle: B\n---\n\nDolor _sit\n",
    );

    let runtime = runtime_with_cache(&project_dir);
    let summary = render_project_fail_fast(&project_dir, runtime, false);

    assert_eq!(
        summary.pass1_failures.len(),
        2,
        "without --fail-fast both files' errors should be reported; got {:?}",
        summary.pass1_failures
    );
    assert!(
        !summary.stopped_early,
        "stopped_early must be false without --fail-fast"
    );

    unsafe {
        std::env::remove_var("QUARTO_JOBS");
    }
}
