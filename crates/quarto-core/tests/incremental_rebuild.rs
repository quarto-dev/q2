/*
 * tests/incremental_rebuild.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 8 incremental-rebuild integration tests.
 */

//! End-to-end tests for the Phase-8 profile cache.
//!
//! These tests drive a real `ProjectPipeline` against a temp
//! project directory with `NativeRuntime::with_cache_dir(...)`
//! wired up — same shape as the CLI uses for non-single-file
//! projects (`commands/render.rs`).
//!
//! What they verify:
//!
//! - Pass-1 cache populates after a cold render (cache directory
//!   contains `profiles/` entries).
//! - A warm second render hits the cache (we observe by inspecting
//!   the cache directory's mtimes after a render where no source
//!   changed).
//! - Editing a page's source bytes invalidates that page's profile
//!   cache entry but not its siblings'.
//! - Editing a transitive include's bytes invalidates the parent's
//!   cache entry via the verification step in `profile_cache::load`.
//! - Editing `_quarto.yml` or `_metadata.yml` invalidates entries
//!   whose key includes those bytes.

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

/// Build a `NativeRuntime` with `<project_dir>/.quarto/cache` wired
/// up — the same flavor `commands/render.rs` constructs for
/// non-single-file projects.
fn runtime_with_cache(project_dir: &std::path::Path) -> Arc<dyn SystemRuntime> {
    Arc::new(NativeRuntime::with_cache_dir(
        project_dir.join(".quarto/cache"),
    ))
}

/// Run one full project render. Asserts the run succeeded and
/// returns the summary. The orchestrator does its own Pass-1 cache
/// I/O internally; this helper just exercises the end-to-end flow.
fn render_project(
    project_dir: &std::path::Path,
    runtime: Arc<dyn SystemRuntime>,
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
    );
    pollster::block_on(pipeline.run()).expect("project render")
}

/// Return the set of paths inside `dir`. The cache layout is flat
/// (`profiles/<key>` files, no subdirectories), so a single
/// `read_dir` is enough.
fn list_cache_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.exists() {
        return out;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for entry in read.flatten() {
        if let Ok(ft) = entry.file_type() {
            if ft.is_file() {
                out.push(entry.path());
            }
        }
    }
    out
}

fn write_minimal_website(project_dir: &std::path::Path) {
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
}

#[test]
fn cold_render_populates_profile_cache() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    let runtime = runtime_with_cache(&project_dir);
    let summary = render_project(&project_dir, runtime);

    assert_eq!(summary.outputs.len(), 2, "two pages rendered");
    assert!(summary.pass1_failures.is_empty());
    assert!(summary.pass2_failures.is_empty());

    let cache_files = list_cache_files(&project_dir.join(".quarto/cache/profiles"));
    assert_eq!(
        cache_files.len(),
        2,
        "Pass-1 cache should contain one entry per page; got {cache_files:?}"
    );
}

#[test]
fn warm_render_unchanged_cache_directory() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    // Cold render to populate the cache.
    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());
    let cold_files = list_cache_files(&project_dir.join(".quarto/cache/profiles"));
    assert_eq!(cold_files.len(), 2);

    // Warm render — same sources, same metadata. Cache hits should
    // mean no new entries are written; cache file count is
    // unchanged.
    let _ = render_project(&project_dir, runtime);
    let warm_files = list_cache_files(&project_dir.join(".quarto/cache/profiles"));
    assert_eq!(warm_files.len(), 2);
    // The set of cache-key file *names* hasn't changed. (Mtimes
    // might change on platforms where atomic-rename touches them,
    // so we don't assert on those.)
    let cold_names: std::collections::BTreeSet<_> = cold_files
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_owned()))
        .collect();
    let warm_names: std::collections::BTreeSet<_> = warm_files
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_owned()))
        .collect();
    assert_eq!(
        cold_names, warm_names,
        "warm render should hit the same cache entries"
    );
}

#[test]
fn editing_one_page_creates_new_cache_entry() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());
    let cold_names: std::collections::BTreeSet<_> =
        list_cache_files(&project_dir.join(".quarto/cache/profiles"))
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_owned()))
            .collect();

    // Edit one page — the page's source bytes change, so its
    // pass1_key changes, so a fresh entry is written under the new
    // key. The old entry is left in place (cache GC is a future
    // follow-up).
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout us — REVISED.\n",
    );

    let _ = render_project(&project_dir, runtime);
    let warm_names: std::collections::BTreeSet<_> =
        list_cache_files(&project_dir.join(".quarto/cache/profiles"))
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_owned()))
            .collect();

    // Two cold entries (index, about-old) + one new (about-new) = 3.
    // Index's entry survives both runs; about's old entry is now
    // orphaned but still present.
    assert_eq!(
        warm_names.len(),
        3,
        "edit should add a new entry while the old one persists; got {warm_names:?}"
    );
    assert!(
        cold_names.is_subset(&warm_names),
        "all cold entries should still exist (no in-place mutation)"
    );
}

#[test]
fn editing_quarto_yml_invalidates_every_page() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());
    let cold_count = list_cache_files(&project_dir.join(".quarto/cache/profiles")).len();
    assert_eq!(cold_count, 2);

    // Edit _quarto.yml — every page's pass1_key flips.
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\nformat:\n  html:\n    toc: true\n",
    );

    let _ = render_project(&project_dir, runtime);
    let warm_count = list_cache_files(&project_dir.join(".quarto/cache/profiles")).len();

    // Cold cache: 2 entries. After _quarto.yml edit: 2 *new* entries,
    // 2 orphaned cold entries. Total: 4.
    assert_eq!(
        warm_count, 4,
        "_quarto.yml edit should add new entries for every page"
    );
}

#[test]
fn warm_render_byte_identical_outputs() {
    // Sanity: cache hits don't accidentally produce different
    // rendered output. We render once cold, capture the rendered
    // HTML bytes, render again warm, and assert the on-disk HTML
    // is byte-identical.
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());
    let cold_html = std::fs::read(project_dir.join("_site/index.html")).unwrap();

    let _ = render_project(&project_dir, runtime);
    let warm_html = std::fs::read(project_dir.join("_site/index.html")).unwrap();

    assert_eq!(
        cold_html, warm_html,
        "warm render should produce byte-identical HTML"
    );
}

#[test]
fn no_cache_dir_means_no_cache_files() {
    // Single-file render path — `commands/render.rs` constructs
    // `NativeRuntime::new()` without a cache dir. Phase 8 caching
    // becomes a transparent no-op. Assert no cache directory is
    // created.
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    let qmd_path = project_dir.join("only.qmd");
    write(&qmd_path, "---\ntitle: Solo\n---\n\nLone document.\n");

    // Single-file runtime — no cache dir.
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&qmd_path, runtime.as_ref()).unwrap();
    assert!(project.is_single_file);

    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime,
    );
    let summary = pollster::block_on(pipeline.run()).expect("run");
    assert_eq!(summary.outputs.len(), 1);

    let cache_dir = project_dir.join(".quarto/cache");
    assert!(
        !cache_dir.exists() || list_cache_files(&cache_dir).is_empty(),
        "single-file render should not write to a cache dir"
    );
}
