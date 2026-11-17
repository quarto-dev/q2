/*
 * quarto_version_marker_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-gw8fmaz3 — end-to-end CLI tests for the Quarto-version declaration
 * (`quarto-required`) in `_quarto.yml`.
 */

//! End-to-end CLI tests for the `quarto-required` declaration
//! (bd-gw8fmaz3).
//!
//! An explicit `_quarto.yml` without `quarto-required: "^2"` draws one
//! Q-5-53 per render (not one per document); `--strict` turns it into a
//! failure; a project-level `diagnostics:` entry silences it.
//! Plan: claude-notes/plans/2026-11-17-project-quarto-version-marker.md

use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Run `q2 render <args>` in `cwd`; returns (success, stderr).
fn run_q2_render(cwd: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(Q2_BIN)
        .current_dir(cwd)
        .arg("render")
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn q2 binary");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A two-document default project with the given `_quarto.yml`.
fn fixture(quarto_yml: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("_quarto.yml"), quarto_yml);
    write_file(&dir.join("a.qmd"), "---\ntitle: A\n---\n\nalpha\n");
    write_file(&dir.join("b.qmd"), "---\ntitle: B\n---\n\nbeta\n");
    (temp, dir)
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn unmarked_project_warns_once_per_render() {
    let (_temp, dir) = fixture("project:\n  type: default\n");
    let (ok, stderr) = run_q2_render(&dir, &[]);
    assert!(ok, "render should succeed:\n{stderr}");
    assert_eq!(
        count(&stderr, "Q-5-53"),
        1,
        "one Q-5-53 expected:\n{stderr}"
    );
    assert!(
        stderr.contains("`_quarto.yml` does not declare a Quarto version"),
        "{stderr}"
    );
    assert!(
        stderr.contains("add `quarto-required: \"^2\"` to `_quarto.yml`"),
        "{stderr}"
    );
}

#[test]
fn unmarked_project_fails_under_strict() {
    let (_temp, dir) = fixture("project:\n  type: default\n");
    let (ok, stderr) = run_q2_render(&dir, &["--strict"]);
    assert!(!ok, "--strict should fail:\n{stderr}");
    assert_eq!(count(&stderr, "Q-5-53"), 1, "{stderr}");
}

#[test]
fn rendering_one_file_of_an_unmarked_project_warns() {
    let (_temp, dir) = fixture("project:\n  type: default\n");
    let (ok, stderr) = run_q2_render(&dir, &["a.qmd"]);
    assert!(ok, "{stderr}");
    assert_eq!(count(&stderr, "Q-5-53"), 1, "{stderr}");
}

#[test]
fn marked_project_is_silent_and_passes_strict() {
    let (_temp, dir) = fixture("project:\n  type: default\nquarto-required: \"^2\"\n");
    let (ok, stderr) = run_q2_render(&dir, &["--strict"]);
    assert!(ok, "{stderr}");
    assert!(!stderr.contains("Q-5-5"), "{stderr}");
}

#[test]
fn open_range_warns_q_5_54_at_the_value_in_quarto_yml() {
    let (_temp, dir) = fixture("project:\n  type: default\nquarto-required: \">=1.4\"\n");
    let (ok, stderr) = run_q2_render(&dir, &[]);
    assert!(ok, "{stderr}");
    assert_eq!(count(&stderr, "Q-5-54"), 1, "{stderr}");
    assert!(!stderr.contains("Q-5-53"), "{stderr}");
    // The source excerpt names the config file and shows the value.
    assert!(stderr.contains("_quarto.yml:3"), "{stderr}");
    assert!(stderr.contains("quarto-required: \">=1.4\""), "{stderr}");
}

#[test]
fn project_level_diagnostics_policy_silences_it_even_under_strict() {
    let (_temp, dir) = fixture(
        "project:\n  type: default\ndiagnostics:\n  Q-5-53:\n    level: off\n    reason: a Quarto 1 project\n",
    );
    let (ok, stderr) = run_q2_render(&dir, &["--strict"]);
    assert!(ok, "{stderr}");
    assert!(!stderr.contains("Q-5-53"), "{stderr}");
}

#[test]
fn document_level_diagnostics_policy_does_not_silence_it() {
    let (_temp, dir) = fixture("project:\n  type: default\n");
    write_file(
        &dir.join("a.qmd"),
        "---\ntitle: A\ndiagnostics:\n  Q-5-53: off\n---\n\nalpha\n",
    );
    let (ok, stderr) = run_q2_render(&dir, &[]);
    assert!(ok, "{stderr}");
    assert_eq!(count(&stderr, "Q-5-53"), 1, "{stderr}");
}

#[test]
fn single_file_outside_a_project_is_not_checked() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("doc.qmd"), "---\ntitle: Doc\n---\n\nhello\n");
    let (ok, stderr) = run_q2_render(&dir, &["doc.qmd", "--strict"]);
    assert!(ok, "{stderr}");
    assert!(!stderr.contains("Q-5-5"), "{stderr}");
}

#[test]
fn dry_run_reports_it_too() {
    let (_temp, dir) = fixture("project:\n  type: default\n");
    let (ok, stderr) = run_q2_render(&dir, &["--dry-run"]);
    assert!(ok, "{stderr}");
    assert_eq!(count(&stderr, "Q-5-53"), 1, "{stderr}");
}
