/*
 * tests/integration/brand_font_weight.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end CLI tests for brand.yml font weight ranges and the
 * invalid-weight hard error (bd-5fseopxy, Q-14-8).
 */

//! `weight: 400..700` on a `typography.fonts[]` entry must reach the
//! compiled CSS as a variable-font axis request (`wght@400..700` for
//! Google, `font-weight: 300 800` in `@font-face` for a local file),
//! and any weight Quarto does not understand must fail the render with
//! a structured `Q-14-8` diagnostic pointing into `_brand.yml`.
//!
//! TDD note: written before the fix. On the unfixed tree the Google
//! form renders with `wght@0,400;1,400` (bold silently synthetic), the
//! file form emits `font-weight: 300..800;` which fails the theme
//! compile and ships the 7 KB default CSS with exit 0, and no invalid
//! weight produces any diagnostic.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

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

/// A single branded document: `brand: _brand.yml` in the front matter,
/// `theme: brand` so the brand layers are the whole theme.
fn write_branded_doc(dir: &Path, brand_yaml: &str) {
    write_file(
        &dir.join("doc.qmd"),
        "---\ntitle: Weights\nbrand: _brand.yml\nformat:\n  html:\n    theme: brand\n---\n\nRegular and **bold**.\n",
    );
    write_file(&dir.join("_brand.yml"), brand_yaml);
}

fn read_styles(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("doc_files/styles.css"))
        .expect("themed render must write doc_files/styles.css")
}

#[test]
fn google_weight_range_is_requested_as_wght_axis() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_branded_doc(
        dir,
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: 400..700\n\
         \x20 base: EB Garamond\n",
    );

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "render must succeed; stderr:\n{stderr}"
    );

    let css = read_styles(dir);
    assert!(
        css.contains("family=EB+Garamond:ital,wght@0,400..700;1,400..700&display=swap"),
        "compiled CSS must request the variable axis range; got imports:\n{}",
        css.lines()
            .filter(|l| l.contains("@import"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn file_weight_range_is_declared_in_font_face() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_branded_doc(
        dir,
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: Local Var\n\
         \x20     source: file\n\
         \x20     files:\n\
         \x20       - path: LocalVar-VariableFont_wght.woff2\n\
         \x20         weight: 300..800\n\
         \x20 base: Local Var\n",
    );

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "render must succeed; stderr:\n{stderr}"
    );

    let css = read_styles(dir);
    // The theme CSS is minified (`font-weight:300 800`); accept the
    // unminified spelling too so a change in minification does not
    // turn into a false failure here.
    assert!(
        css.contains("font-weight:300 800") || css.contains("font-weight: 300 800"),
        "@font-face must declare the weight axis as `300 800`; css head:\n{}",
        &css[..css.len().min(1200)]
    );
    assert!(
        css.contains("@font-face"),
        "the full theme (not the DEFAULT_CSS fallback) must have compiled"
    );
}

#[test]
fn invalid_weight_range_fails_with_q_14_8_pointing_into_brand_file() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_branded_doc(
        dir,
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: 700..400\n",
    );

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "a reversed range must exit non-zero; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Q-14-8"),
        "expected the Q-14-8 diagnostic on stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("700..400"),
        "diagnostic must quote the offending weight; got:\n{stderr}"
    );
    assert!(
        stderr.contains("_brand.yml"),
        "diagnostic must name the brand file; got:\n{stderr}"
    );
    // ariadne's line gutter for the `weight:` line (line 5 of the
    // fixture) proves the span landed in `_brand.yml`, not in doc.qmd.
    assert!(
        stderr.contains("5 │"),
        "expected a source snippet with the weight line; got:\n{stderr}"
    );
    assert!(
        !dir.join("doc.html").exists(),
        "no HTML output must be written when the brand is invalid"
    );
}

#[test]
fn unknown_weight_keyword_fails_with_q_14_8() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_branded_doc(
        dir,
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20     weight: Bold\n",
    );

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(stderr.contains("Q-14-8"), "stderr:\n{stderr}");
    assert!(stderr.contains("Bold"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("typography.fonts[0].weight"),
        "diagnostic must name the YAML path; got:\n{stderr}"
    );
}

#[test]
fn slot_weight_range_fails_with_q_14_8() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_branded_doc(
        dir,
        "typography:\n\
         \x20 fonts:\n\
         \x20   - family: EB Garamond\n\
         \x20     source: google\n\
         \x20 headings:\n\
         \x20   family: EB Garamond\n\
         \x20   weight: 500..700\n",
    );

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(stderr.contains("Q-14-8"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("typography.headings.weight"),
        "diagnostic must name the slot path; got:\n{stderr}"
    );
    assert!(stderr.contains("500..700"), "stderr:\n{stderr}");
}
