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
    render_site(1, "theme: [custom.scss]")
}

/// `theme:` value is spliced into `format.html`; `jobs` pins the
/// Pass-2 worker count (`1` = serial).
fn render_site(jobs: usize, theme_yaml: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());

    write(
        &project_dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: website\n  output-dir: _site\n\nformat:\n  html:\n    {theme_yaml}\n"
        ),
    );
    write(
        &project_dir.join("custom.scss"),
        "/*-- scss:defaults --*/\n$body-bg: #fefefe;\n\n/*-- scss:rules --*/\n.cache-key-guard { color: #123456; }\n",
    );
    write(
        &project_dir.join("custom-dark.scss"),
        "/*-- scss:defaults --*/\n$body-bg: #101010;\n\n/*-- scss:rules --*/\n.cache-key-guard { color: #abcdef; }\n",
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
    )
    .with_jobs(jobs);
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

/// Keys the LRU index tracks in `<cache_dir>/sass`.
fn indexed_sass_keys(cache_dir: &Path) -> Vec<String> {
    let bytes = std::fs::read(cache_dir.join("sass").join("_lru_index"))
        .expect("LRU index exists after a themed render");
    let index: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut keys: Vec<String> = index["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["key"].as_str().unwrap().to_string())
        .collect();
    keys.sort();
    keys
}

/// bd-ddahjqr1: with parallel Pass 2, several workers write distinct
/// keys (here light + dark variants) at once, and the index's unlocked
/// read-modify-write can lose an entry — the value stays on disk,
/// untracked and never evicted. Every entry the render leaves in the
/// cache must be in the index. (The deterministic proof of the race is
/// `cache_lru`'s unit tests; this pins the end-to-end invariant.)
#[test]
fn parallel_render_leaves_no_untracked_sass_cache_entries() {
    let (_temp, cache_dir) = render_site(
        3,
        "theme:\n      light: [custom.scss]\n      dark: [custom-dark.scss]",
    );
    let on_disk = sass_cache_entries(&cache_dir);
    let indexed = indexed_sass_keys(&cache_dir);
    assert_eq!(
        on_disk.len(),
        2,
        "one light + one dark entry expected; got {on_disk:?}"
    );
    assert_eq!(
        on_disk, indexed,
        "every sass cache entry on disk must be tracked by the LRU index"
    );
}
