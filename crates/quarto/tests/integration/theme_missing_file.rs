/*
 * tests/integration/theme_missing_file.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end CLI tests for the missing-custom-theme hard error
 * (bd-of20unsb, Q-14-4).
 */

//! A `theme:` entry naming a `.scss`/`.css` file that resolves to no
//! file must fail the render with a structured Q-14-4 diagnostic.
//!
//! Before the fix, `compile_theme_css` swallowed
//! `SassError::CustomThemeNotFound` into a trace-level warning and
//! shipped the static `DEFAULT_CSS` — dropping the *entire* theme
//! list (including valid built-in entries) with exit code 0 and no
//! user-visible message.
//!
//! TDD note: written before the fix; the failure mode observed on
//! the unfixed tree is exit 0 with a 7 KB default `styles.css`.

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

/// The core contract: a dangling custom-theme entry fails the render
/// with Q-14-4 instead of silently shipping DEFAULT_CSS.
#[test]
fn missing_custom_theme_fails_with_q_14_4() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(
        &dir.join("doc.qmd"),
        "---\ntitle: Missing theme\nformat:\n  html:\n    theme: [cosmo, nope.scss]\n---\n\nBody.\n",
    );

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "missing custom theme must exit non-zero; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Q-14-4"),
        "expected the Q-14-4 diagnostic on stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("nope.scss"),
        "diagnostic must name the offending theme entry; got:\n{stderr}"
    );
    assert!(
        !dir.join("doc.html").exists(),
        "no HTML output must be written when the theme fails to resolve"
    );
}

/// Control: the same shape with the file present renders fine — the
/// validation must not reject resolvable custom themes or built-in
/// names.
#[test]
fn present_custom_theme_still_renders() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path();
    write_file(
        &dir.join("doc.qmd"),
        "---\ntitle: Present theme\nformat:\n  html:\n    theme: [cosmo, real.scss]\n---\n\nBody.\n",
    );
    write_file(
        &dir.join("real.scss"),
        "/*-- scss:rules --*/\n.real-marker { color: #0a1b2c; }\n",
    );

    let output = run_q2_render(dir, &["doc.qmd"]);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "resolvable theme list must render; stderr:\n{stderr}"
    );
    let css = std::fs::read_to_string(dir.join("doc_files/styles.css"))
        .expect("themed render must write doc_files/styles.css");
    assert!(
        css.contains("real-marker"),
        "compiled CSS must contain the custom theme rule"
    );
}
