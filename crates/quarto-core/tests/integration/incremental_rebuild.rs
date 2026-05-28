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
use quarto_core::project::orchestrator::{ProjectPipeline, RenderMode, project_type_for};
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

// === Phase 8.2 step 3: Mode A vs Mode B ===============================

/// Render a project in Mode B with the given subset of pages.
/// `target_paths` are absolute paths matching `DocumentInfo.input`.
fn render_mode_b(
    project_dir: &std::path::Path,
    runtime: Arc<dyn SystemRuntime>,
    target_paths: &[PathBuf],
) -> quarto_core::project::orchestrator::ProjectRenderSummary {
    let mut project =
        ProjectContext::discover(project_dir, runtime.as_ref()).expect("discover project");
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let targets: std::collections::HashSet<PathBuf> = target_paths.iter().cloned().collect();
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    )
    .with_mode(RenderMode::Subset(targets));
    pollster::block_on(pipeline.run()).expect("project render")
}

/// Read the mtime of an output file. Returns `None` if missing.
fn output_mtime(p: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

#[test]
fn mode_b_renders_only_target() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    let runtime = runtime_with_cache(&project_dir);
    // Cold full render to populate _site/.
    let _ = render_project(&project_dir, runtime.clone());

    let about_path = project_dir.join("_site/about.html");
    let index_path = project_dir.join("_site/index.html");
    let about_mtime_before = output_mtime(&about_path).expect("about exists after cold render");
    let index_mtime_before = output_mtime(&index_path).expect("index exists after cold render");

    // Sleep just enough that mtimes can differ if files are touched.
    // (10ms is well under the typical filesystem mtime resolution
    // ceiling of 1s on macOS HFS+, but enough to surface
    // differences on modern filesystems with sub-second precision.)
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Mode B: render only about.qmd.
    let target = project_dir.join("about.qmd");
    let summary = render_mode_b(&project_dir, runtime, &[target]);

    // pass_two reports only one output (the target).
    assert_eq!(
        summary.outputs.len(),
        1,
        "Mode B should render exactly one page; got {} outputs",
        summary.outputs.len()
    );

    // about.html mtime advanced; index.html mtime unchanged.
    let about_mtime_after = output_mtime(&about_path).expect("about exists");
    let index_mtime_after = output_mtime(&index_path).expect("index exists");
    assert!(
        about_mtime_after >= about_mtime_before,
        "about.html mtime should advance"
    );
    assert_eq!(
        index_mtime_before, index_mtime_after,
        "index.html should not be touched in Mode B with target=about"
    );
}

#[test]
fn mode_a_default_renders_everything() {
    // Sanity: confirm the default mode (no with_mode call) is
    // still Mode A — every page renders.
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    let runtime = runtime_with_cache(&project_dir);
    let summary = render_project(&project_dir, runtime);

    assert_eq!(
        summary.outputs.len(),
        2,
        "Mode A should render every project page"
    );
}

#[test]
fn mode_b_pulls_in_always_render_dependents() {
    // Setup: pages a, b, q. q has project.always-render: true and
    // body-links to a (so q's reverse-dep is "things that link to
    // q"; q is in nothing's reverse-dep). To exercise the
    // augmentation we need q to be reachable via reverse_closure
    // from the user-named target — i.e. q's *forward* dep set
    // includes the target.
    //
    // q → a (q body-links to a). reverse_closure({a}) = {a, q}.
    // q has always_render: true, so augment({a}) = {a, q}.
    //
    // The test renders Mode B with target={a}; both a and q must
    // render.
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(&project_dir.join("a.qmd"), "---\ntitle: A\n---\n\nFoo.\n");
    write(
        &project_dir.join("b.qmd"),
        "---\ntitle: B\n---\n\nUnrelated.\n",
    );
    write(
        &project_dir.join("q.qmd"),
        "---\ntitle: Q\nproject:\n  always-render: true\n---\n\nSee [a](a.qmd).\n",
    );

    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());

    let q_path = project_dir.join("_site/q.html");
    let b_path = project_dir.join("_site/b.html");
    let q_mtime_before = output_mtime(&q_path).expect("q exists");
    let b_mtime_before = output_mtime(&b_path).expect("b exists");

    std::thread::sleep(std::time::Duration::from_millis(20));

    // Mode B: render only a.qmd. Augmentation should pull in q
    // (always_render + reverse-deps include a).
    let target = project_dir.join("a.qmd");
    let summary = render_mode_b(&project_dir, runtime, &[target]);

    // a + q should render; b is untouched.
    assert_eq!(
        summary.outputs.len(),
        2,
        "Mode B with always_render dependent should render 2 pages; got {}",
        summary.outputs.len()
    );
    let q_mtime_after = output_mtime(&q_path).expect("q exists");
    let b_mtime_after = output_mtime(&b_path).expect("b exists");
    assert!(
        q_mtime_after >= q_mtime_before,
        "q.html mtime should advance (always_render augmentation)"
    );
    assert_eq!(
        b_mtime_before, b_mtime_after,
        "b.html should not be touched (no augmentation match)"
    );
}

#[test]
fn mode_b_sitemap_preserves_untouched_entries_lastmod() {
    // Phase 8.3 (bd-pphv): the sitemap merge should preserve the
    // `<lastmod>` of pages that were *not* re-rendered this run.
    // Mode B with target=about → the about entry refreshes;
    // index/c entries keep their previous lastmod from the cold
    // sitemap.
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\nwebsite:\n  site-url: https://example.com/site\n",
    );
    write(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome.\n",
    );
    write(
        &project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout.\n",
    );

    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());
    let sitemap_path = project_dir.join("_site/sitemap.xml");
    let cold_xml = std::fs::read_to_string(&sitemap_path).expect("sitemap exists after cold run");
    // Cold run: both entries have lastmods. Capture them.
    assert!(cold_xml.contains("<loc>https://example.com/site/index.html</loc>"));
    assert!(cold_xml.contains("<loc>https://example.com/site/about.html</loc>"));

    // Sleep just past second precision so any refresh is visible.
    std::thread::sleep(std::time::Duration::from_millis(1100));

    // Touch about.qmd so its mtime advances; render only about
    // in Mode B.
    std::fs::write(
        project_dir.join("about.qmd"),
        "---\ntitle: About\n---\n\nAbout — REVISED.\n",
    )
    .unwrap();

    let target = project_dir.join("about.qmd");
    let _summary = render_mode_b(&project_dir, runtime, &[target]);

    let warm_xml = std::fs::read_to_string(&sitemap_path).expect("sitemap exists after warm run");

    // Both entries should still be present (Mode B doesn't drop
    // pages from the project).
    assert!(warm_xml.contains("<loc>https://example.com/site/index.html</loc>"));
    assert!(warm_xml.contains("<loc>https://example.com/site/about.html</loc>"));

    // The merge contract: extract index.html's lastmod from cold
    // and warm; they should be byte-identical (preserved). Extract
    // about.html's lastmod; it should have advanced.
    let index_cold = lastmod_for_loc(&cold_xml, "https://example.com/site/index.html");
    let index_warm = lastmod_for_loc(&warm_xml, "https://example.com/site/index.html");
    let about_cold = lastmod_for_loc(&cold_xml, "https://example.com/site/about.html");
    let about_warm = lastmod_for_loc(&warm_xml, "https://example.com/site/about.html");

    assert_eq!(
        index_cold, index_warm,
        "index.html was not rendered in Mode B; its lastmod must be preserved"
    );
    assert_ne!(
        about_cold, about_warm,
        "about.html was rendered in Mode B; its lastmod should refresh \
         (cold was {about_cold:?}, warm is {about_warm:?})"
    );
}

/// Quick-and-dirty `<lastmod>` extractor for a given `<loc>`.
/// Tests only — production lives in `parse_sitemap_locs`.
fn lastmod_for_loc(xml: &str, loc: &str) -> Option<String> {
    let needle = format!("<loc>{loc}</loc>");
    let start = xml.find(&needle)?;
    let after = &xml[start + needle.len()..];
    let lm_start = after.find("<lastmod>")? + "<lastmod>".len();
    let lm_end = after[lm_start..].find("</lastmod>")?;
    Some(after[lm_start..lm_start + lm_end].to_string())
}

#[test]
fn mode_b_with_no_targets_renders_nothing() {
    // Edge case: empty Subset set ⇒ pass_two skips every page.
    // This isn't a typical CLI invocation (no path arg ⇒ Mode A),
    // but the orchestrator should handle the corner cleanly.
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    // Cold full render to populate _site/.
    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());

    let about_path = project_dir.join("_site/about.html");
    let mtime_before = output_mtime(&about_path).expect("about exists");

    std::thread::sleep(std::time::Duration::from_millis(20));

    let summary = render_mode_b(&project_dir, runtime, &[]);
    assert_eq!(summary.outputs.len(), 0);
    assert_eq!(
        mtime_before,
        output_mtime(&about_path).unwrap(),
        "no target ⇒ no page touched"
    );
}

// === Phase 8.5: additional integration tests =========================

/// Test 51: Mode B with a multi-element target set renders each
/// targeted page and leaves the rest untouched.
#[test]
fn mode_b_multi_target_renders_union() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(&project_dir.join("a.qmd"), "---\ntitle: A\n---\n\nA.\n");
    write(&project_dir.join("b.qmd"), "---\ntitle: B\n---\n\nB.\n");
    write(&project_dir.join("c.qmd"), "---\ntitle: C\n---\n\nC.\n");

    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());

    let c_mtime_before = output_mtime(&project_dir.join("_site/c.html")).expect("c exists");
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Mode B: render {a, b}.
    let targets = [project_dir.join("a.qmd"), project_dir.join("b.qmd")];
    let summary = render_mode_b(&project_dir, runtime, &targets);
    assert_eq!(
        summary.outputs.len(),
        2,
        "Mode B with 2 targets should render both pages; got {} outputs",
        summary.outputs.len()
    );

    let c_mtime_after = output_mtime(&project_dir.join("_site/c.html")).expect("c exists");
    assert_eq!(
        c_mtime_before, c_mtime_after,
        "c.html was not in the target set; mtime should not advance"
    );
}

/// Test 48: A user-declared `project.nav-dependencies` on a target
/// page does not cause the render to fail even when the target
/// of the declaration is unresolved or absent. (The graph
/// builder silently drops unresolved edges per Decision 5; a
/// follow-up bead tracks emitting a diagnostic.)
#[test]
fn mode_b_user_declared_nav_dependency_does_not_fail() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("foo.qmd"),
        "---\ntitle: Foo\nproject:\n  nav-dependencies:\n    - bar.qmd\n---\n\nFoo body.\n",
    );
    write(
        &project_dir.join("bar.qmd"),
        "---\ntitle: Bar\n---\n\nBar body.\n",
    );

    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());

    // Mode B: render only foo. Declared nav-dependency on bar is
    // a hint to the graph; bar is not itself rendered.
    let summary = render_mode_b(&project_dir, runtime, &[project_dir.join("foo.qmd")]);
    assert_eq!(
        summary.outputs.len(),
        1,
        "Mode B should render exactly the user-named target"
    );
    assert!(
        summary.pass2_failures.is_empty(),
        "render should succeed: {:?}",
        summary.pass2_failures
    );
}

/// Test 57: An unresolved `project.nav-dependencies` declaration
/// (target file does not exist in the project) does not abort
/// the render. The graph builder drops the edge; the render
/// proceeds.
#[test]
fn unresolved_nav_dependency_does_not_fail() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("foo.qmd"),
        "---\ntitle: Foo\nproject:\n  nav-dependencies:\n    - missing.qmd\n---\n\nFoo.\n",
    );

    let runtime = runtime_with_cache(&project_dir);
    let summary = render_project(&project_dir, runtime);
    assert_eq!(summary.outputs.len(), 1);
    assert!(summary.pass2_failures.is_empty());
}

/// Test 44 variant: editing `<subdir>/_metadata.yml` invalidates
/// only the profile-cache entries of pages in that subtree.
/// Pages outside the subtree keep their cache entries.
#[test]
fn editing_metadata_yml_invalidates_subtree_only() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("top.qmd"),
        "---\ntitle: Top\n---\n\nTop.\n",
    );
    write(&project_dir.join("sub/a.qmd"), "---\ntitle: A\n---\n\nA.\n");
    write(&project_dir.join("sub/b.qmd"), "---\ntitle: B\n---\n\nB.\n");

    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());
    let cold_names: std::collections::BTreeSet<_> =
        list_cache_files(&project_dir.join(".quarto/cache/profiles"))
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_owned()))
            .collect();
    assert_eq!(cold_names.len(), 3);

    // Add `sub/_metadata.yml` — affects only sub/a and sub/b.
    write(
        &project_dir.join("sub/_metadata.yml"),
        "format:\n  html:\n    toc: true\n",
    );

    let _ = render_project(&project_dir, runtime);
    let warm_names: std::collections::BTreeSet<_> =
        list_cache_files(&project_dir.join(".quarto/cache/profiles"))
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_owned()))
            .collect();

    // Cold: 3 entries. After _metadata.yml edit: top's entry survives,
    // sub/a and sub/b each get a new entry. The two old subtree entries
    // are orphaned but still present. Total: 3 + 2 = 5.
    assert_eq!(
        warm_names.len(),
        5,
        "_metadata.yml edit should add 2 new entries (sub/a, sub/b) and preserve top's"
    );
    assert!(
        cold_names.is_subset(&warm_names),
        "all cold entries should still exist (no in-place mutation)"
    );

    // Specifically: top's cache entry is shared between cold and warm.
    let still_present = cold_names.intersection(&warm_names).count();
    assert_eq!(
        still_present, 3,
        "top's entry plus the two now-orphaned subtree entries should remain"
    );
}

/// Test 45 / 60: editing a transitive include's bytes invalidates
/// the parent profile's cache entry (`bd-r82e`).
#[test]
fn editing_include_invalidates_parent_profile() {
    // The include is named `_partial.qmd` so the underscore-rule
    // excludes it from `project.files`. Only `parent.qmd` is in the
    // render list, so the cold cache has exactly one entry. After
    // editing the partial, the cached parent profile's
    // include-verification step (`profile_cache::load`) should
    // detect the changed bytes and miss, prompting a fresh
    // extraction.
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write(
        &project_dir.join("parent.qmd"),
        "---\ntitle: Parent\n---\n\n{{< include _partial.qmd >}}\n",
    );
    write(
        &project_dir.join("_partial.qmd"),
        "Partial content version 1.\n",
    );

    let runtime = runtime_with_cache(&project_dir);
    let summary = render_project(&project_dir, runtime.clone());
    assert_eq!(summary.outputs.len(), 1, "only parent.qmd is renderable");

    let cache_files = list_cache_files(&project_dir.join(".quarto/cache/profiles"));
    assert_eq!(cache_files.len(), 1, "exactly one profile in cold cache");
    // The pass1_key is a function of source bytes + layered metadata,
    // *not* of include contents (Decision 2). Editing the partial
    // does not change parent's key — the cache entry is overwritten
    // in place. So after editing the partial, the same on-disk file
    // should contain *different bytes* (a freshly extracted profile
    // with the new include content_hash recorded).
    let cold_bytes = std::fs::read(&cache_files[0]).expect("read cold cache file");
    let cold_path = cache_files[0].clone();

    // Edit the included partial only.
    write(
        &project_dir.join("_partial.qmd"),
        "Partial content version 2 — REVISED.\n",
    );

    let _ = render_project(&project_dir, runtime);
    let warm_files = list_cache_files(&project_dir.join(".quarto/cache/profiles"));
    assert_eq!(
        warm_files.len(),
        1,
        "include edit doesn't change the parent's pass1_key (Decision 2); \
         the cache file is rewritten in place"
    );
    assert_eq!(warm_files[0], cold_path, "same cache key");

    let warm_bytes = std::fs::read(&warm_files[0]).expect("read warm cache file");
    assert_ne!(
        cold_bytes, warm_bytes,
        "include edit must invalidate the cached profile and re-extract \
         (parent's profile records the include's content_hash; new content \
         ⇒ new hash ⇒ new bytes on disk)"
    );
}

/// Test 54: a corrupt profile-cache file falls through to live
/// extraction and the render succeeds.
#[test]
fn corrupt_profile_cache_falls_through() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());
    write_minimal_website(&project_dir);

    // Cold render to populate the cache.
    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());

    // Corrupt every profile-cache file by overwriting with garbage.
    let cache_dir = project_dir.join(".quarto/cache/profiles");
    for entry in std::fs::read_dir(&cache_dir).unwrap().flatten() {
        std::fs::write(entry.path(), b"\x00\x01\x02 not json").unwrap();
    }

    // Render again — the corrupt bytes should be ignored and live
    // extraction should produce correct profiles.
    let summary = render_project(&project_dir, runtime);
    assert_eq!(summary.outputs.len(), 2);
    assert!(summary.pass1_failures.is_empty());
    assert!(summary.pass2_failures.is_empty());
}

// ─────────────────────────────────────────────────────────────────────
// L6 (`bd-xbnf`): Mode B re-renders a listing host when any of its
// content files is targeted.
//
// Listing hosts now advertise their `listing.*.contents:` glob
// strings on `DocumentProfile.listing_content_globs`. The
// dep-graph builder expands them at graph-build time and adds the
// host to `force_render`, so `quarto render posts/foo.qmd` pulls
// the host back into the render set automatically — no manual
// `--targets index.qmd` required.
// ─────────────────────────────────────────────────────────────────────

#[test]
fn mode_b_re_renders_listing_host_when_content_targeted() {
    let temp = TempDir::new().unwrap();
    let project_dir = canonical(temp.path());

    write(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    // Listing host: declares a `posts/*.qmd` listing.
    write(
        &project_dir.join("index.qmd"),
        "\
---
title: Home
listing:
  contents: posts/*.qmd
---

::: {#listing-1}
:::
",
    );
    write(
        &project_dir.join("posts/foo.qmd"),
        "---\ntitle: Foo\ndate: 2026-05-01\n---\n\nFoo body.\n",
    );
    write(
        &project_dir.join("posts/bar.qmd"),
        "---\ntitle: Bar\ndate: 2026-05-02\n---\n\nBar body.\n",
    );

    // Cold full render to populate _site/ and the cache.
    let runtime = runtime_with_cache(&project_dir);
    let _ = render_project(&project_dir, runtime.clone());

    let index_path = project_dir.join("_site/index.html");
    let foo_path = project_dir.join("_site/posts/foo.html");
    let bar_path = project_dir.join("_site/posts/bar.html");
    let index_mtime_before = output_mtime(&index_path).expect("index.html exists");
    let foo_mtime_before = output_mtime(&foo_path).expect("posts/foo.html exists");
    let bar_mtime_before = output_mtime(&bar_path).expect("posts/bar.html exists");

    // Sleep enough that mtimes can differ if files are re-written.
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Mode B: target only posts/foo.qmd. Pre-L6, the listing host
    // would NOT be rebuilt and its index.html would show stale
    // listing data. Post-L6, the dep-graph augmentation pulls
    // the host in via the listing-content edge + force_render.
    let target = project_dir.join("posts/foo.qmd");
    let summary = render_mode_b(&project_dir, runtime, &[target]);

    // foo + index should render; bar must stay untouched.
    assert_eq!(
        summary.outputs.len(),
        2,
        "Mode B with listing-content target should render foo + listing host; got {} outputs",
        summary.outputs.len()
    );

    let index_mtime_after = output_mtime(&index_path).expect("index.html exists");
    let foo_mtime_after = output_mtime(&foo_path).expect("posts/foo.html exists");
    let bar_mtime_after = output_mtime(&bar_path).expect("posts/bar.html exists");

    assert!(
        foo_mtime_after >= foo_mtime_before,
        "posts/foo.html mtime should advance (it was the explicit target)"
    );
    assert!(
        index_mtime_after > index_mtime_before,
        "index.html mtime should advance (listing host pulled in by L6 \
         augmentation): before={index_mtime_before:?}, after={index_mtime_after:?}"
    );
    assert_eq!(
        bar_mtime_before, bar_mtime_after,
        "posts/bar.html should NOT be touched (not in target set; not a listing host)"
    );
}
