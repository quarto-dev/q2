/*
 * tests/integration/sass_cache_key.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * The compiled-SCSS runtime cache must key one custom theme file
 * once, however many document directories reference it. bd-79c4do6g.
 */

//! End-to-end guard for the sass cache key's path component.
//!
//! The metadata merge rewrites a project-level `theme: [custom.scss]`
//! to a document-relative spelling per document (`../custom.scss`,
//! `../../custom.scss`, …; `project/format_paths.rs`). The stage's
//! `cache_key` hashes the resolved path, so before bd-79c4do6g every
//! document *directory* got its own cache entry for byte-identical
//! CSS — on the Connect docs, 704 compiles for 4 distinct outputs and
//! 78 % of the serial render. This drives a three-depth website through
//! `ProjectPipeline` with a real cache dir (the flavor `q2 render`
//! constructs) and counts the entries the render leaves in the `sass`
//! namespace: exactly one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Cache entries in `<cache_dir>/sass`, excluding the LRU index and
/// the generation marker — one file per distinct key.
fn sass_cache_entries(cache_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(cache_dir.join("sass"))
        .expect("sass cache namespace exists after a themed render")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != "_lru_index" && n != "_version")
        .collect();
    names.sort();
    names
}

/// Render a website with one project-level custom theme and one
/// document at each of depth 0, 1 and 2. Returns the cache dir.
fn render_three_depth_site() -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n\nformat:\n  html:\n    theme: [custom.scss]\n",
    );
    write(
        &project_dir.join("custom.scss"),
        "/*-- scss:defaults --*/\n$body-bg: #fefefe;\n\n/*-- scss:rules --*/\n.cache-key-guard { color: #123456; }\n",
    );
    for page in ["index.qmd", "a/index.qmd", "a/b/index.qmd"] {
        write(
            &project_dir.join(page),
            &format!("---\ntitle: \"{page}\"\n---\n\nHello from {page}.\n"),
        );
    }

    let cache_dir = project_dir.join(".quarto/cache");
    let runtime: Arc<dyn SystemRuntime> =
        Arc::new(NativeRuntime::with_cache_dir(cache_dir.clone()));
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
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
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
    assert_eq!(summary.outputs.len(), 3, "all three pages rendered");

    (temp, cache_dir)
}

#[test]
fn one_custom_theme_referenced_from_three_depths_is_cached_once() {
    let (_temp, cache_dir) = render_three_depth_site();
    let entries = sass_cache_entries(&cache_dir);
    assert_eq!(
        entries.len(),
        1,
        "one theme file must occupy one sass cache entry regardless of \
         how many document directories reference it; got {entries:?}"
    );
}
