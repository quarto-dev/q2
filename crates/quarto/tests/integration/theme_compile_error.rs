/*
 * tests/integration/theme_compile_error.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end CLI tests for theme compile failures as hard errors
 * (bd-jsvetdea Q-14-6, bd-qmpygp02 Q-14-7).
 */

//! A user theme whose SCSS fails to compile must fail the render with
//! a structured diagnostic, not silently ship the static default CSS.
//!
//! Before the fix, `compile_theme_css` caught every grass error,
//! emitted a `Warn` trace event that no CLI observer receives, and
//! returned `DEFAULT_CSS` — exit 0, "Rendered N of N files", and a
//! 7 KB stylesheet in place of the ~330 KB Bootstrap bundle.
//!
//! TDD note: written before the fix; the failure mode observed on the
//! unfixed tree is exit 0 with `doc.html` written.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// A well-formed layered theme whose one variable breaks the
/// Bootstrap grid arithmetic (grass: "Incompatible units px and rem").
const BAD_UNITS_SCSS: &str = "/*-- scss:defaults --*/\n$grid-body-width: 52rem;\n";

/// A file that exists but has no `/*-- scss:... --*/` layer marker.
const NO_MARKERS_SCSS: &str = ".plain-marker { color: #0a1b2c; }\n";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn run_q2_render(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    cmd.args(args);
    cmd.output().expect("spawn q2 binary")
}

/// The bd-jsvetdea repro: a one-line unit mistake in the user's theme
/// fails the render with Q-14-6 carrying the grass message.
#[test]
fn theme_compile_error_fails_with_q_14_6() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(
        &dir.join("doc.qmd"),
        "---\ntitle: Bad units\nformat:\n  html:\n    theme: [cosmo, bad.scss]\n---\n\nBody.\n",
    );
    write_file(&dir.join("bad.scss"), BAD_UNITS_SCSS);

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "a theme compile failure must exit non-zero; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Q-14-6"),
        "expected the Q-14-6 diagnostic on stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("Incompatible units px and rem"),
        "diagnostic must carry the grass message; got:\n{stderr}"
    );
    assert!(
        !dir.join("doc.html").exists(),
        "no HTML output must be written when the theme fails to compile"
    );
}

/// bd-qmpygp02: an existing theme file with no layer markers fails
/// with Q-14-7 naming the file.
#[test]
fn theme_without_layer_markers_fails_with_q_14_7() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(
        &dir.join("doc.qmd"),
        "---\ntitle: No markers\nformat:\n  html:\n    theme: [cosmo, nomarkers.scss]\n---\n\nBody.\n",
    );
    write_file(&dir.join("nomarkers.scss"), NO_MARKERS_SCSS);

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "a theme file without layer markers must exit non-zero; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Q-14-7"),
        "expected the Q-14-7 diagnostic on stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("nomarkers.scss"),
        "diagnostic must name the offending file; got:\n{stderr}"
    );
    assert!(
        !dir.join("doc.html").exists(),
        "no HTML output must be written when the theme fails to compile"
    );
}

/// In a website, the theme compiles once per page and only successes
/// are cached, so every page fails with the same diagnostic. Because
/// the diagnostic is anchored at the `theme:` value in `_quarto.yml`,
/// the render summary's source-location coalescing (bd-9hlja) must
/// print the grass block once with both pages listed as affected.
#[test]
fn project_theme_compile_error_is_reported_once() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: website\nformat:\n  html:\n    theme: [cosmo, bad.scss]\n",
    );
    write_file(&dir.join("bad.scss"), BAD_UNITS_SCSS);
    write_file(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nHome.\n");
    write_file(&dir.join("about.qmd"), "---\ntitle: About\n---\n\nAbout.\n");

    let output = run_q2_render(dir, &[]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "a theme compile failure must fail the project render; stderr:\n{stderr}"
    );
    assert_eq!(
        stderr.matches("Incompatible units px and rem").count(),
        1,
        "the grass block must be printed once for the whole project; got:\n{stderr}"
    );
    assert!(
        stderr.contains("index.qmd") && stderr.contains("about.qmd"),
        "the coalesced report must list both affected pages; got:\n{stderr}"
    );
}

/// Control: the same theme with a `px` value compiles and renders.
#[test]
fn well_formed_custom_theme_still_renders() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(
        &dir.join("doc.qmd"),
        "---\ntitle: Good units\nformat:\n  html:\n    theme: [cosmo, good.scss]\n---\n\nBody.\n",
    );
    write_file(
        &dir.join("good.scss"),
        "/*-- scss:defaults --*/\n$grid-body-width: 830px;\n/*-- scss:rules --*/\n.good-marker { color: #0a1b2c; }\n",
    );

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "a well-formed custom theme must render; stderr:\n{stderr}"
    );
    let css = std::fs::read_to_string(dir.join("doc_files/styles.css"))
        .expect("themed render must write doc_files/styles.css");
    assert!(
        css.contains("good-marker"),
        "compiled CSS must contain the custom theme rule"
    );
}
