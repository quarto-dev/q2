/*
 * nested_projects_cli.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-nested-projects-xyb28wnl — end-to-end CLI tests for nested
 * `_quarto.yml` boundaries in the render list.
 */

//! End-to-end CLI tests for nested projects (bd-nested-projects-xyb28wnl).
//!
//! A subdirectory with its own `_quarto.yml` owns its subtree. The outer
//! render's implicit patterns skip it (one `Q-5-31` per render); an
//! explicit pattern naming it still renders there, with the outer config
//! (`Q-5-32`); `q2 render sub` keeps nearest-project-wins.
//! Plan: claude-notes/plans/2026-09-23-nested-project-boundaries.md

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

fn run_q2_render(cwd: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(Q2_BIN)
        .current_dir(cwd)
        .arg("render")
        .args(args)
        .output()
        .expect("spawn q2 binary");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The experiment layout from the strand: an outer website with a
/// nested website under `sub/` (distinct title and author), plus a
/// plain sibling directory. `outer_project_extra` is appended under the
/// outer `project:` key.
fn fixture(outer_project_extra: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: website\n{outer_project_extra}\
             website:\n  title: OUTER\n"
        ),
    );
    write_file(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nouter\n");
    write_file(&dir.join("plain/p.qmd"), "---\ntitle: P\n---\n\nplain\n");
    write_file(
        &dir.join("sub/_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  title: INNER\nauthor: INNERAUTHOR\n",
    );
    write_file(
        &dir.join("sub/index.qmd"),
        "---\ntitle: Sub\n---\n\ninner\n",
    );
    write_file(&dir.join("sub/page.qmd"), "---\ntitle: Page\n---\n\npage\n");
    write_file(&dir.join("sub/inner.md"), "inner md\n");
    write_file(
        &dir.join("sub/deep/leaf.qmd"),
        "---\ntitle: Leaf\n---\n\nleaf\n",
    );
    (temp, dir)
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn outer_render_skips_nested_project_and_warns_once() {
    let (_t, dir) = fixture("");
    let (ok, stderr) = run_q2_render(&dir, &[]);
    assert!(ok, "render failed:\n{stderr}");

    assert!(dir.join("_site/index.html").is_file());
    assert!(dir.join("_site/plain/p.html").is_file());
    assert!(
        !dir.join("_site/sub").exists(),
        "nested project pages must not land in the outer site"
    );
    assert_eq!(count(&stderr, "Q-5-31"), 1, "{stderr}");
    assert!(stderr.contains("`sub`"), "{stderr}");
    assert_eq!(count(&stderr, "Q-5-32"), 0, "{stderr}");
}

#[test]
fn explicit_reference_renders_with_outer_config_and_warns() {
    let (_t, dir) = fixture("  render:\n    - index.qmd\n    - sub/page.qmd\n");
    let (ok, stderr) = run_q2_render(&dir, &[]);
    assert!(ok, "render failed:\n{stderr}");

    let page = std::fs::read_to_string(dir.join("_site/sub/page.html")).unwrap();
    assert!(page.contains("OUTER"), "outer chrome expected");
    assert!(
        !page.contains("INNERAUTHOR"),
        "inner metadata must not apply"
    );
    assert!(!dir.join("_site/sub/index.html").exists());
    assert_eq!(count(&stderr, "Q-5-32"), 1, "{stderr}");
    assert_eq!(count(&stderr, "Q-5-31"), 0, "{stderr}");
}

#[test]
fn negation_over_nested_root_silences_the_warning() {
    let (_t, dir) = fixture("  render:\n    - \"!sub/**\"\n");
    let (ok, stderr) = run_q2_render(&dir, &[]);
    assert!(ok, "render failed:\n{stderr}");
    assert!(dir.join("_site/index.html").is_file());
    assert_eq!(count(&stderr, "Q-5-31"), 0, "{stderr}");
}

/// Spec item 4: rendering the nested directory keeps
/// nearest-project-wins, unchanged.
#[test]
fn rendering_the_nested_directory_uses_the_nested_project() {
    let (_t, dir) = fixture("");
    let (ok, stderr) = run_q2_render(&dir, &["sub"]);
    assert!(ok, "render failed:\n{stderr}");

    let page = std::fs::read_to_string(dir.join("sub/_site/page.html")).unwrap();
    assert!(page.contains("INNER"), "inner chrome expected");
    assert!(dir.join("sub/_site/deep/leaf.html").is_file());
    assert!(!dir.join("_site").exists(), "outer project must not render");
    assert_eq!(count(&stderr, "Q-5-31"), 0, "{stderr}");
}
