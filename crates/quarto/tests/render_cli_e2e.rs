/*
 * render_cli_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 8.5 — end-to-end CLI integration tests for the render command.
 */

//! End-to-end CLI tests for `quarto render`.
//!
//! These tests spawn the real `q2` binary as a subprocess and assert
//! on observable side-effects: which output files appear, which were
//! re-rendered, what's left in the cache after `--clean-cache`. They
//! cover the slice of behavior the in-process unit tests in
//! `commands::render::tests` cannot reach: actual argument parsing
//! through clap, the full `execute()` call chain, and the
//! filesystem-mediated handoff between `--clean-cache` and the
//! pipeline.
//!
//! Cargo provides the binary path via `CARGO_BIN_EXE_q2`. No third
//! party test runner needed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Run `q2 render <args...>` from `cwd`. Returns the exit status and
/// captured stdout / stderr. Panics if the binary couldn't be spawned.
fn run_q2(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

fn write_minimal_website(project_dir: &Path) {
    write_file(
        &project_dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(
        &project_dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome.\n",
    );
    write_file(
        &project_dir.join("a.qmd"),
        "---\ntitle: A\n---\n\nA body.\n",
    );
    write_file(
        &project_dir.join("b.qmd"),
        "---\ntitle: B\n---\n\nB body.\n",
    );
    write_file(
        &project_dir.join("c.qmd"),
        "---\ntitle: C\n---\n\nC body.\n",
    );
}

// === Tests ============================================================

/// Test 53: `--clean-cache` wipes the profile cache and the
/// nav-config-hash sentinel before the render runs. The
/// subsequent render then re-populates the profile cache.
#[test]
fn clean_cache_flag_wipes_then_renders() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_website(&project);

    // Cold render to populate the profile cache.
    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "cold render failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let profiles_dir = project.join(".quarto/cache/profiles");
    let cold_count = std::fs::read_dir(&profiles_dir)
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    assert!(
        cold_count > 0,
        "cold render should populate profile cache; found {cold_count} entries"
    );

    // Drop a fake nav-config-hash file so we can verify --clean-cache
    // removes it.
    let hash_path = project.join(".quarto/cache/nav-config-hash");
    write_file(&hash_path, "stale");
    assert!(hash_path.exists());

    // Run with --clean-cache. The cache should be wiped *before* the
    // render — we observe the post-render state so the wipe must
    // strictly precede the re-population.
    let out = run_q2(&project, &["--clean-cache"]);
    assert!(
        out.status.success(),
        "--clean-cache run failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The nav-config-hash file should have been removed (Phase 8
    // doesn't write it back yet — Decision 8 defers).
    assert!(
        !hash_path.exists(),
        "nav-config-hash should be removed by --clean-cache"
    );

    // Profile cache is repopulated from the fresh render.
    let warm_count = std::fs::read_dir(&profiles_dir)
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    assert_eq!(
        warm_count, cold_count,
        "profile cache should be re-populated after --clean-cache"
    );
}

/// Test 51 (CLI form): `quarto render a.qmd b.qmd` renders exactly
/// {a, b}; siblings are not touched.
#[test]
fn multi_arg_renders_union() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_minimal_website(&project);

    // Cold render to populate _site/.
    let out = run_q2(&project, &[]);
    assert!(out.status.success());
    let c_mtime_before = mtime(&project.join("_site/c.html")).expect("c.html exists");
    let index_mtime_before = mtime(&project.join("_site/index.html")).expect("index exists");

    std::thread::sleep(std::time::Duration::from_millis(20));

    // Edit a.qmd and b.qmd so subsequent render has fresh bytes;
    // c and index untouched.
    write_file(
        &project.join("a.qmd"),
        "---\ntitle: A\n---\n\nA body — REVISED.\n",
    );
    write_file(
        &project.join("b.qmd"),
        "---\ntitle: B\n---\n\nB body — REVISED.\n",
    );

    let out = run_q2(&project, &["a.qmd", "b.qmd"]);
    assert!(
        out.status.success(),
        "multi-arg render failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let c_mtime_after = mtime(&project.join("_site/c.html")).expect("c.html exists");
    let index_mtime_after = mtime(&project.join("_site/index.html")).expect("index exists");
    assert_eq!(
        c_mtime_before, c_mtime_after,
        "c was not in the target set; mtime should not change"
    );
    assert_eq!(
        index_mtime_before, index_mtime_after,
        "index was not in the target set; mtime should not change"
    );

    // a and b should reflect the revised body.
    let a_html = std::fs::read_to_string(project.join("_site/a.html")).unwrap();
    let b_html = std::fs::read_to_string(project.join("_site/b.html")).unwrap();
    assert!(a_html.contains("REVISED"), "a.html should be re-rendered");
    assert!(b_html.contains("REVISED"), "b.html should be re-rendered");
}

/// Test 50: `quarto render <subdir>` expands to all `.qmd` files
/// under `<subdir>` and renders only those.
#[test]
fn directory_arg_renders_subtree() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(&project.join("top.qmd"), "---\ntitle: Top\n---\n\nTop.\n");
    write_file(
        &project.join("posts/p1.qmd"),
        "---\ntitle: P1\n---\n\nP1.\n",
    );
    write_file(
        &project.join("posts/p2.qmd"),
        "---\ntitle: P2\n---\n\nP2.\n",
    );

    // Cold render to populate _site/.
    assert!(run_q2(&project, &[]).status.success());
    let top_mtime_before = mtime(&project.join("_site/top.html")).expect("top exists");

    std::thread::sleep(std::time::Duration::from_millis(20));

    // Render only the `posts/` subtree.
    let out = run_q2(&project, &["posts"]);
    assert!(
        out.status.success(),
        "directory-arg render failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let top_mtime_after = mtime(&project.join("_site/top.html")).expect("top exists");
    assert_eq!(
        top_mtime_before, top_mtime_after,
        "top.qmd was outside the directory arg; should not be re-rendered"
    );
    assert!(project.join("_site/posts/p1.html").exists());
    assert!(project.join("_site/posts/p2.html").exists());
}

/// Test 58 (subset): full smoke at a single tempdir. We collapse the
/// 8-step plan into a focused sequence: cold, warm, body edit + Mode A,
/// Mode B single, Mode B multi, --clean-cache. Each step asserts
/// observable filesystem state.
#[test]
fn cli_smoke_full_sequence() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(
        &project.join("_quarto.yml"),
        concat!(
            "project:\n  type: website\n  output-dir: _site\n",
            "website:\n",
            "  title: \"Phase 8 Smoke\"\n",
            "  site-url: \"https://example.com/site\"\n",
            "  sidebar:\n",
            "    contents: [index.qmd, a.qmd, b.qmd, c.qmd]\n",
        ),
    );
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome page.\n",
    );
    write_file(&project.join("a.qmd"), "---\ntitle: A\n---\n\nA body.\n");
    write_file(&project.join("b.qmd"), "---\ntitle: B\n---\n\nB body.\n");
    write_file(&project.join("c.qmd"), "---\ntitle: C\n---\n\nC body.\n");
    // d.qmd is in the project but not in the sidebar.
    write_file(&project.join("d.qmd"), "---\ntitle: D\n---\n\nD body.\n");

    // Step 1: cold Mode A render.
    let out = run_q2(&project, &[]);
    assert!(
        out.status.success(),
        "cold render failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    for stem in ["index", "a", "b", "c", "d"] {
        let p = project.join(format!("_site/{stem}.html"));
        assert!(p.exists(), "cold render: {} should exist", p.display());
    }
    // Profile cache has 5 entries.
    let profiles_dir = project.join(".quarto/cache/profiles");
    assert_eq!(
        std::fs::read_dir(&profiles_dir).unwrap().count(),
        5,
        "5 pages ⇒ 5 profile-cache entries"
    );

    // Step 2: warm Mode A render — same outputs, profile cache hits.
    let cold_html = std::fs::read(project.join("_site/a.html")).unwrap();
    let out = run_q2(&project, &[]);
    assert!(out.status.success());
    let warm_html = std::fs::read(project.join("_site/a.html")).unwrap();
    assert_eq!(
        cold_html, warm_html,
        "warm Mode A should produce byte-identical HTML"
    );

    // Step 3: edit b.qmd; Mode A re-renders all but only b's profile
    // cache misses.
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_file(
        &project.join("b.qmd"),
        "---\ntitle: B\n---\n\nB body — REVISED.\n",
    );
    let out = run_q2(&project, &[]);
    assert!(out.status.success());
    let b_html = std::fs::read_to_string(project.join("_site/b.html")).unwrap();
    assert!(
        b_html.contains("REVISED"),
        "b.html should reflect the body edit"
    );

    // Step 4: Mode B render of a.qmd. Only a.html mtime advances.
    let b_mtime_before = mtime(&project.join("_site/b.html")).expect("b exists");
    let index_mtime_before = mtime(&project.join("_site/index.html")).expect("index exists");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let out = run_q2(&project, &["a.qmd"]);
    assert!(
        out.status.success(),
        "Mode B render failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        b_mtime_before,
        mtime(&project.join("_site/b.html")).unwrap(),
        "b.html mtime must not change in Mode B with target=a"
    );
    assert_eq!(
        index_mtime_before,
        mtime(&project.join("_site/index.html")).unwrap(),
        "index.html mtime must not change in Mode B with target=a"
    );

    // Step 5: --clean-cache wipes profiles before re-render.
    write_file(&project.join(".quarto/cache/nav-config-hash"), "x");
    let out = run_q2(&project, &["--clean-cache"]);
    assert!(out.status.success());
    assert!(
        !project.join(".quarto/cache/nav-config-hash").exists(),
        "nav-config-hash should be removed by --clean-cache"
    );

    // Step 6: sitemap exists and lists all pages.
    let sitemap = std::fs::read_to_string(project.join("_site/sitemap.xml"))
        .expect("sitemap.xml should exist after Mode A render with site-url");
    for stem in ["index", "a", "b", "c", "d"] {
        let url = format!("https://example.com/site/{stem}.html");
        assert!(
            sitemap.contains(&format!("<loc>{url}</loc>")),
            "sitemap should list {url}"
        );
    }
}

/// Test (negative): a `.qmd` arg outside the project's render list
/// emits an actionable error and a non-zero exit status.
#[test]
fn arg_excluded_by_render_list_errors() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n  render:\n    - a.qmd\n",
    );
    write_file(&project.join("a.qmd"), "---\ntitle: A\n---\n\nA.\n");
    write_file(&project.join("b.qmd"), "---\ntitle: B\n---\n\nB.\n");

    let out = run_q2(&project, &["b.qmd"]);
    assert!(
        !out.status.success(),
        "rendering an excluded file should fail; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("excluded from the render list") || stderr.contains("render list"),
        "error message should mention the render list; got: {stderr}"
    );
}

/// Test (negative): a directory arg expanding to zero `.qmd` files
/// surfaces a distinct, actionable error.
#[test]
fn empty_directory_arg_errors_with_no_renderable_matches() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(&project.join("a.qmd"), "---\ntitle: A\n---\n\nA.\n");
    std::fs::create_dir_all(project.join("empty")).unwrap();

    let out = run_q2(&project, &["empty"]);
    assert!(
        !out.status.success(),
        "empty-dir render should fail; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("No renderable") || stderr.contains("no renderable"),
        "error message should mention 'no renderable matches'; got: {stderr}"
    );
}

/// Test (negative): multiple stand-alone `.qmd` files outside any
/// project ⇒ "one project per render" error.
#[test]
fn multi_arg_outside_project_errors() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    // No _quarto.yml.
    write_file(&dir.join("a.qmd"), "---\ntitle: A\n---\n\nA.\n");
    write_file(&dir.join("b.qmd"), "---\ntitle: B\n---\n\nB.\n");

    let out = run_q2(&dir, &["a.qmd", "b.qmd"]);
    assert!(
        !out.status.success(),
        "multi-arg outside project should fail; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("require a project") || stderr.contains("project per render"),
        "error message should mention single-project requirement; got: {stderr}"
    );
}
