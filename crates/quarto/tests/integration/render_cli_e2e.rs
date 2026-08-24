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
    let cold_count = std::fs::read_dir(&profiles_dir).map_or(0, |d| d.flatten().count());
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
    let warm_count = std::fs::read_dir(&profiles_dir).map_or(0, |d| d.flatten().count());
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

/// Regression for bd-creo (Decision D1): `quarto render` exits
/// non-zero when any project file fails Pass-1 (parse / metadata
/// error), even though sibling pages render successfully. The
/// strict-vs-lenient policy lives at the consumer; the CLI is the
/// strict path. The orchestrator's structured `pass1_failures`
/// field is the authority — we don't string-match the warning
/// printout.
#[test]
fn pass1_failure_triggers_non_zero_exit() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());

    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\nwebsite:\n  sidebar:\n    contents:\n      - index.qmd\n      - about.qmd\n",
    );
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHello.\n",
    );
    // Q-2-10 quote-mark error: unescaped apostrophe after a space.
    write_file(
        &project.join("about.qmd"),
        "---\ntitle: About\n---\n\n- Reflect changes to *other* pages' titles within the next render\n",
    );

    let out = run_q2(&project, &[]);
    assert!(
        !out.status.success(),
        "render with a Pass-1 parse error should exit non-zero (bd-creo); \
         stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("profile-pass skipped") && stderr.contains("about.qmd"),
        "stderr should still contain the rich profile-pass-skipped \
         warning; got: {stderr}"
    );
}

/// Regression for the post-websites-merge default-project bug.
///
/// Mirrors the user repro at
/// `/Users/cscheid/Desktop/daily-log/2026/05/01/default-project-test/`:
/// a `_quarto.yml` containing only `project: { type: default }`
/// next to an `index.qmd`. Pre-fix: `q2 render index.qmd` failed
/// with "excluded from the render list", and `q2 render` (no args)
/// silently produced no output. Post-fix: both invocations succeed
/// and `index.html` lands beside the source.
#[test]
fn default_project_renders_named_file_and_produces_html() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(&project.join("_quarto.yml"), "project:\n  type: default\n");
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome page.\n",
    );

    // Mode B: name the file explicitly.
    let out = run_q2(&project, &["index.qmd"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "default-project Mode B render should succeed.\nstdout: {stdout}\nstderr: {stderr}",
    );
    let html_path = project.join("index.html");
    assert!(
        html_path.exists(),
        "expected {} to exist after Mode B render; stderr: {stderr}",
        html_path.display(),
    );
    let html = std::fs::read_to_string(&html_path).unwrap();
    assert!(
        html.contains("Home page."),
        "rendered HTML should include the source body; got:\n{html}",
    );
}

#[test]
fn default_project_renders_with_no_args_and_produces_html() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(&project.join("_quarto.yml"), "project:\n  type: default\n");
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome page.\n",
    );

    // Mode A: no args.
    let out = run_q2(&project, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "default-project Mode A render should succeed.\nstderr: {stderr}",
    );
    let html_path = project.join("index.html");
    assert!(
        html_path.exists(),
        "expected {} to exist after Mode A render; stderr: {stderr}",
        html_path.display(),
    );
}

/// Phase 2 of bd-h736: when a project's `project.render` globs
/// match zero files, `q2 render` should not silently no-op. It
/// should emit an Error-severity project-level diagnostic, mention
/// the render-list misconfiguration, and exit non-zero.
#[test]
fn render_list_matching_no_files_emits_diagnostic_and_nonzero_exit() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    // Render pattern that matches no real `.qmd` file in the
    // project (Phase-1 dispatcher's `Subset` branch is not
    // reachable from here — Mode A walks the project file list
    // straight from the orchestrator).
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: default\n  render:\n    - does-not-exist.qmd\n",
    );
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome.\n",
    );

    let out = run_q2(&project, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected non-zero exit on empty render set; stderr: {stderr}",
    );
    assert!(
        stderr.contains("no renderable")
            || stderr.contains("empty render set")
            || stderr.contains("Q-PROJECT-EMPTY"),
        "expected an empty-render-set diagnostic; got stderr: {stderr}",
    );
    assert!(
        stderr.contains("project.render"),
        "diagnostic should mention `project.render`; got stderr: {stderr}",
    );
}

#[test]
fn empty_default_project_with_no_qmd_emits_diagnostic() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(&project.join("_quarto.yml"), "project:\n  type: default\n");
    // No .qmd files at all.

    let out = run_q2(&project, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected non-zero exit on empty render set; stderr: {stderr}",
    );
    assert!(
        stderr.contains("no renderable") || stderr.contains("render set is empty"),
        "expected an empty-render-set diagnostic; got stderr: {stderr}",
    );
}

/// bd-6d2wj4zp: `.md` files render only when opted in via
/// `project.render`. When that opt-in is the *reason* the render set
/// came up empty, `Q-PROJECT-EMPTY` must say so — otherwise the
/// default reads as "Quarto silently ignored my files".
#[test]
fn empty_project_with_md_files_hints_at_render_list_optin() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(&project.join("_quarto.yml"), "project:\n  type: default\n");
    write_file(&project.join("notes.md"), "# notes\n");
    write_file(&project.join("docs/guide.md"), "# guide\n");
    // Excluded `.md` must not inflate the count.
    write_file(&project.join("README.md"), "# readme\n");

    let out = run_q2(&project, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected non-zero exit on empty render set; stderr: {stderr}",
    );
    assert!(
        stderr.contains("2 `.md` file"),
        "hint should count the opt-in candidates; got stderr: {stderr}",
    );
    assert!(
        stderr.contains("**/*.md"),
        "hint should show the opt-in pattern; got stderr: {stderr}",
    );
}

/// bd-6d2wj4zp: `q2 render notes.md` inside a project whose render
/// list doesn't include it fails with Q-7-6 — and because the real
/// cause is the `.md` opt-in policy (not underscore/hidden rules),
/// the hint must say how to opt the file in.
#[test]
fn rendering_md_not_in_render_list_hints_at_optin() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(&project.join("_quarto.yml"), "project:\n  type: default\n");
    write_file(&project.join("index.qmd"), "---\ntitle: T\n---\n\nhi\n");
    write_file(&project.join("notes.md"), "# notes\n");

    let out = run_q2(&project, &["notes.md"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected non-zero exit for an un-opted-in .md; stderr: {stderr}",
    );
    assert!(
        stderr.contains("Q-7-6") || stderr.contains("excluded from the render list"),
        "expected the render-list exclusion diagnostic; got stderr: {stderr}",
    );
    assert!(
        stderr.contains("**/*.md"),
        "hint should explain the `.md` opt-in; got stderr: {stderr}",
    );
}

/// bd-6d2wj4zp S5: an opted-in `.md` with an `engine:` spec renders
/// successfully (passthrough, no execution) and warns with Q-2-40
/// through the real diagnostic path.
#[test]
fn md_with_engine_spec_renders_with_q_2_40_warning() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: default\n  render:\n    - \"*.md\"\n",
    );
    write_file(
        &project.join("notes.md"),
        "---\ntitle: Notes\nengine: jupyter\n---\n\n# Hello\n\nplain text\n",
    );

    let out = run_q2(&project, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "the render itself must succeed; stderr: {stderr}",
    );
    assert!(
        stderr.contains("Q-2-40"),
        "expected the engine-ignored warning; got stderr: {stderr}",
    );
    let html = std::fs::read_to_string(project.join("notes.html")).expect("notes.html exists");
    assert!(
        html.contains("plain text"),
        "content renders as plain markdown"
    );
}

/// bd-6d2wj4zp D7: an output path equal to the input must refuse
/// rather than silently replace the source with rendered HTML.
/// (Before the guard, `--output <abs input path>` destroyed the
/// source file.)
#[test]
fn output_equal_to_input_refuses_and_preserves_source() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    let source = "---\ntitle: T\n---\n\nhello\n";
    write_file(&dir.join("doc.qmd"), source);
    let abs = dir.join("doc.qmd");

    let out = run_q2(&dir, &["doc.qmd", "--output", abs.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected refusal when output == input; stderr: {stderr}",
    );
    assert!(
        stderr.contains("overwrite"),
        "error should explain the overwrite refusal; got stderr: {stderr}",
    );
    let after = std::fs::read_to_string(&abs).expect("source still exists");
    assert_eq!(after, source, "source file must be untouched");
}

/// bd-6d2wj4zp S3: single-file format detection reads `.md` front
/// matter exactly like `.qmd`. Pinned via the non-native bail-out:
/// a `.md` declaring `format: pdf` must get the same early "not yet
/// supported" refusal a `.qmd` gets — before the fix it rendered
/// HTML bytes into a `p.pdf` file.
#[test]
fn md_with_non_native_format_gets_early_refusal_like_qmd() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("doc.md"),
        "---\ntitle: T\nformat: pdf\n---\n\nhi\n",
    );

    let out = run_q2(&dir, &["doc.md"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "expected refusal for non-native format; stderr: {stderr}",
    );
    assert!(
        stderr.contains("not yet supported"),
        "expected the same early bail-out a .qmd gets; got stderr: {stderr}",
    );
    assert!(
        !dir.join("doc.pdf").exists(),
        "must not write a fake .pdf output"
    );
}

/// bd-6d2wj4zp S7: render-list `.md` pages participate in navigation
/// and body-link rewriting exactly like `.qmd` — the Connect-docs
/// shape (`file: admin/index.md` in the sidebar, `[x](other.md)` in
/// body text).
#[test]
fn md_pages_get_nav_and_body_links_rewritten() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n  render:\n    - \"*.qmd\"\n    - \"*.md\"\n\
         website:\n  title: T\n  sidebar:\n    contents:\n      - index.qmd\n      - text: Notes\n        file: notes.md\n",
    );
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nSee [the notes](notes.md).\n",
    );
    write_file(
        &project.join("notes.md"),
        "---\ntitle: Notes\n---\n\nBack [home](index.qmd).\n",
    );

    let out = run_q2(&project, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "render failed; stderr: {stderr}");

    let index_html = std::fs::read_to_string(project.join("_site/index.html")).expect("index.html");
    assert!(
        index_html.contains("href=\"notes.html\""),
        "body link [the notes](notes.md) must rewrite to notes.html; got:\n{}",
        &index_html[..index_html.len().min(4000)]
    );

    let notes_html = std::fs::read_to_string(project.join("_site/notes.html")).expect("notes.html");
    assert!(
        notes_html.contains("href=\"index.html\""),
        "body link [home](index.qmd) from a .md page must rewrite to index.html"
    );
    // The sidebar entry `file: notes.md` must resolve on both pages.
    assert!(
        notes_html.contains("notes.html") && index_html.contains("notes.html"),
        "sidebar entry for notes.md must resolve to notes.html on every page"
    );

    // Subset render (Mode B): Pass 1 profiles every project file
    // regardless of mode, so links into a `.md` page must still
    // rewrite when only the linking page is re-rendered. (The `.md`
    // dependency-graph *edges* are pinned at the unit level in
    // navigation_href.rs — their pass-2 augmentation effect only
    // shows with always-render pages, which are listing territory.)
    std::fs::remove_file(project.join("_site/index.html")).unwrap();
    let out = run_q2(&project, &["index.qmd"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "subset render failed; stderr: {stderr}"
    );
    let index_html = std::fs::read_to_string(project.join("_site/index.html")).expect("index.html");
    assert!(
        index_html.contains("href=\"notes.html\""),
        "subset render must still rewrite the body link into the .md page"
    );
}

/// Regression guard for bd-87fu: native default-project renders
/// must continue to write theme CSS to `{stem}_files/quarto/...`
/// and embed a matching `<link>` in the HTML. The WASM-side fix
/// (mirroring the `lib_dir` branch in `RenderToHtmlRenderer`) is
/// orthogonal to native, but symmetric changes are easy to get
/// wrong — this test pins native byte-locations.
#[test]
fn default_project_native_theme_writes_under_files_dir() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(&project.join("_quarto.yml"), "project:\n  type: default\n");
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: T\nformat:\n  html:\n    theme: flatly\n---\n\nhi\n",
    );

    let out = run_q2(&project, &["index.qmd"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "render should succeed; stderr: {stderr}",
    );

    let html = std::fs::read_to_string(project.join("index.html")).expect("index.html exists");

    // The HTML should embed a `<link>` to a theme CSS file under
    // `index_files/quarto/...`. Find the href.
    let link_line = html
        .lines()
        .find(|line| line.contains("index_files/quarto/quarto-theme-") && line.contains(".css"))
        .unwrap_or_else(|| {
            panic!(
                "expected a theme <link> referencing index_files/quarto/quarto-theme-…; html head: {}",
                html.lines().take(40).collect::<Vec<_>>().join("\n"),
            )
        });

    let needle = "index_files/quarto/quarto-theme-";
    let start = link_line
        .find(needle)
        .expect("needle present (filter just confirmed it)");
    let after = &link_line[start..];
    let end_offset = after.find(".css").expect("href ends with .css") + ".css".len();
    let rel_css_path = &link_line[start..start + end_offset];

    let css_full_path = project.join(rel_css_path);
    assert!(
        css_full_path.exists(),
        "theme CSS should exist at {}; got listing: {:?}",
        css_full_path.display(),
        std::fs::read_dir(project.join("index_files/quarto"))
            .map(|d| d
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    let bytes = std::fs::read(&css_full_path).unwrap();
    assert!(
        !bytes.is_empty(),
        "theme CSS at {} should be non-empty",
        css_full_path.display(),
    );
}

/// bd-xdnk: when a custom Pandoc-style template references a variable
/// the document does not define, the doctemplate engine emits a
/// `Q-10-2` warning. That warning must surface on stderr through the
/// real `quarto render` path (it has been silently dropped before this
/// fix). The render itself should succeed with a zero exit (warning,
/// not error).
#[test]
fn custom_template_undefined_variable_emits_warning_on_stderr() {
    let temp = TempDir::new().unwrap();
    let project = canonical(temp.path());
    write_file(&project.join("_quarto.yml"), "project:\n  type: default\n");

    // Custom template references `$author-greeting$`, which the qmd
    // file does not provide. Use a distinct file name (not `post.html`)
    // so it doesn't collide with `post.qmd`'s output path.
    write_file(
        &project.join("custom.html"),
        "<!doctype html>\n\
         <html lang=\"en\">\n\
         <head><meta charset=\"utf-8\"><title>$title$</title></head>\n\
         <body>\n\
         <header>by $author-greeting$</header>\n\
         <main>$body$</main>\n\
         </body>\n\
         </html>\n",
    );

    write_file(
        &project.join("post.qmd"),
        "---\n\
         title: Source-tracked template diagnostics\n\
         template: custom.html\n\
         ---\n\
         \n\
         Body content.\n",
    );

    let out = run_q2(&project, &["post.qmd"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "render should succeed (warning, not error).\nstdout: {stdout}\nstderr: {stderr}",
    );

    // The diagnostic should appear on stderr with its error code, the
    // identifying message, and an attribution to the template file
    // (the ariadne renderer prints `path:line:col` on the location line).
    assert!(
        stderr.contains("Q-10-2"),
        "expected Q-10-2 in stderr; got: {stderr}"
    );
    assert!(
        stderr.contains("Undefined variable") && stderr.contains("author-greeting"),
        "expected 'Undefined variable: author-greeting' message in stderr; got: {stderr}"
    );
    assert!(
        stderr.contains("custom.html"),
        "diagnostic should attribute to custom.html (ariadne path:line:col); got: {stderr}"
    );

    // The rendered HTML should still exist (non-fatal warning).
    let out_html = project.join("post.html");
    assert!(
        out_html.exists(),
        "expected output {} to exist after warning render; stderr: {stderr}",
        out_html.display()
    );
    let html = std::fs::read_to_string(&out_html).unwrap();
    assert!(
        html.contains("Body content"),
        "rendered HTML should still contain the body; got:\n{html}",
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

/// A small but real PNG byte sequence — using a real header so we
/// exercise the same `file_copy` path a user-uploaded image would.
const PNG_BYTES: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89,
];

fn write_bytes(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// F5 (Phase 2, bd-cfl67): the CLI binary `q2 render` against a
/// website project that references a binary image must (a) leave
/// the source image bytes unchanged and (b) copy the image into
/// `_site/` at the position the rendered HTML expects.
///
/// This is the user-facing reproduction of the original bug
/// translated into an automated test: it spawns the real `q2`
/// binary the user runs, against a website fixture, with a single
/// PNG.
#[test]
fn render_preserves_source_image_and_copies_to_site() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());

    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\n![Caption](elephant.png)\n",
    );
    write_bytes(&dir.join("elephant.png"), PNG_BYTES);

    let out = run_q2(&dir, &["index.qmd"]);
    assert!(
        out.status.success(),
        "q2 render should succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Source image bytes unchanged.
    let source_after = std::fs::read(dir.join("elephant.png")).expect("read source after");
    assert_eq!(
        source_after, PNG_BYTES,
        "source image must be byte-identical after `q2 render`"
    );

    // Image copied into _site/elephant.png.
    let copied =
        std::fs::read(dir.join("_site/elephant.png")).expect("expected image copied to _site/");
    assert_eq!(copied, PNG_BYTES, "copied bytes must match source bytes");

    // Rendered HTML references the image.
    let html = std::fs::read_to_string(dir.join("_site/index.html")).expect("read rendered html");
    assert!(
        html.contains("elephant.png"),
        "rendered HTML must reference elephant.png; html:\n{html}",
    );
}

// ====================================================================
// bd-render-failure-unattributed-yxe0v7th — a failed page must name
// itself.
//
// Both tests are gated on a real R + knitr toolchain, following the
// `rscript_available()` / `knitr_r_package_available()` pattern in
// `quarto-core/tests/integration/marimo_engine_e2e.rs`. A skip here is
// an environment signal, not a pass.
// ====================================================================

/// `true` when `Rscript` is on PATH.
fn rscript_available() -> bool {
    Command::new("Rscript")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `true` when the R `knitr` package is installed. Checked separately
/// from [`rscript_available`] — `KnitrEngine::is_available()` only looks
/// for the binary, not the package.
fn knitr_r_package_available() -> bool {
    Command::new("Rscript")
        .args([
            "-e",
            "if (!requireNamespace(\"knitr\", quietly = TRUE)) quit(status = 1)",
        ])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Shape 1. A chunk that *executes successfully* but emits markdown q2
/// cannot parse. The parse runs against knitr's output buffer, so the
/// diagnostic's span indexes into the intermediate — not the `.qmd`.
///
/// Before the fix, `run_pipeline`'s generic `StageError` conversion
/// rebound that span to the source document's bytes. The engine output is
/// far larger than this 5-line source, so the offset landed past EOF, the
/// ariadne renderer bailed, *and* its `at <file>:<row>:<col>` fallback was
/// skipped with it: the page died with a bare title and problem statement
/// and its name appeared nowhere in the transcript.
///
/// Two independent guarantees are asserted: the frame renders against the
/// engine intermediate (Fix A, `engine_execution.rs`), and the `.qmd` is
/// named anyway (Fix B, `failure_attribution_line`).
#[test]
fn engine_output_parse_failure_names_the_qmd() {
    if !rscript_available() || !knitr_r_package_available() {
        eprintln!("skipping: R toolchain with knitr not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");
    write_file(
        &dir.join("boom.qmd"),
        "---\ntitle: Boom\n---\n\n```{r}\n#| output: asis\ncat(\"{{< fa envelope size=1x >}}\\n\")\n```\n",
    );

    let out = run_q2(dir, &[]);
    // ariadne interleaves color codes *inside* highlighted spans, so the
    // offending source line is not a contiguous substring of raw stderr.
    let stderr = crate::coalesced_diagnostics::strip_ansi(&String::from_utf8_lossy(&out.stderr));

    assert!(
        stderr.contains("boom.qmd"),
        "a failed page must name itself; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("while rendering"),
        "the span resolves to the engine intermediate, so the attribution \
         line is what supplies the .qmd name; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("size=1x"),
        "the frame must show the offending source line from the engine \
         output; stderr was:\n{stderr}"
    );
}

/// Shape 2. A chunk that calls `stop()`. The engine failure carries no
/// span at all (`location: None`), so nothing in the diagnostic can name
/// the page — before the fix, `print_render_diagnostics_text` discarded
/// the known-good `FileFailure.input` and printed a bare
/// `Error: Execution failed in knitr: R process failed`.
///
/// Note this asserts attribution only. Remapping knitr's traceback line
/// numbers (`failing.rmarkdown:NNN`) back to `.qmd` lines is deliberately
/// out of scope; see the plan doc.
#[test]
fn engine_execution_failure_names_the_qmd() {
    if !rscript_available() || !knitr_r_package_available() {
        eprintln!("skipping: R toolchain with knitr not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    write_file(&dir.join("_quarto.yml"), "project:\n  type: default\n");
    write_file(
        &dir.join("failing.qmd"),
        "---\ntitle: Failing\n---\n\n```{r}\nstop(\"boom: this chunk always fails\")\n```\n",
    );

    let out = run_q2(dir, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert!(
        stderr.contains("while rendering"),
        "a span-less engine failure must still be attributed; stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("failing.qmd"),
        "the .qmd name must appear in the transcript; stderr was:\n{stderr}"
    );
}
