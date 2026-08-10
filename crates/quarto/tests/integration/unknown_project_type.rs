//! End-to-end CLI tests for the unknown-`project.type` hard error
//! (bd-sekn481x).
//!
//! Before the fix, `project.type: posit-docs` (a Quarto 1 extension
//! project type q2 doesn't support) was silently coerced to
//! `default`, so `q2 render` rendered in place and strewed a private
//! `<stem>_files/quarto/` copy of the shared JS/CSS next to every
//! document — `.md` and `.qmd` alike. The contract under test:
//! render fails up front with Q-5-17, and *nothing* is written.
//!
//! TDD note: written before the fix; the failure mode observed on
//! the unfixed tree is exit 0 with stray `*_files/` dirs.

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

fn run_q2_render(cwd: &Path) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    cmd.output().expect("spawn q2 binary")
}

/// Mixed .md/.qmd project — the shape of the posit-connect docs port
/// where the bug was reported.
fn write_project(dir: &Path, project_type: &str) {
    write_file(
        &dir.join("_quarto.yml"),
        &format!(
            "project:\n  type: {project_type}\n  render:\n    - \"**/*.md\"\n    - \"**/*.qmd\"\n\nwebsite:\n  title: repro\n"
        ),
    );
    write_file(
        &dir.join("index.qmd"),
        "---\ntitle: Home\n---\n\nSome `code`.\n",
    );
    write_file(
        &dir.join("about.md"),
        "---\ntitle: About\n---\n\nSome `code`.\n",
    );
    write_file(&dir.join("sub/page.md"), "---\ntitle: Sub\n---\n\nMore.\n");
}

/// Every path under `dir` (files and directories) whose name matches
/// `pred`, for asserting on render side effects.
fn find_matching(dir: &Path, pred: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if pred(&name) {
                hits.push(path.clone());
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    hits
}

/// The core contract: an unknown `project.type` fails the render up
/// front — no HTML outputs, no `<stem>_files/` lib copies strewn
/// into the source tree.
#[test]
fn unknown_project_type_fails_before_rendering() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_project(&dir, "posit-docs");

    let output = run_q2_render(&dir);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "unknown project.type must exit non-zero; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Q-5-17"),
        "expected the Q-5-17 diagnostic on stderr; got:\n{stderr}"
    );
    assert!(
        stderr.contains("posit-docs"),
        "diagnostic must name the offending type; got:\n{stderr}"
    );

    let stray_files_dirs = find_matching(&dir, &|name| name.ends_with("_files"));
    assert!(
        stray_files_dirs.is_empty(),
        "no per-document lib dirs may be written; found: {stray_files_dirs:?}"
    );
    let html_outputs = find_matching(&dir, &|name| name.ends_with(".html"));
    assert!(
        html_outputs.is_empty(),
        "no document may render before the config error; found: {html_outputs:?}"
    );
}

/// bd-y56u1gl7: the structured Q-5-17 diagnostic must arrive bare —
/// exactly one `Error:` header, no `Project discovery failed:`
/// wrapper. Before the fix, classify_inputs flattened the rendered
/// diagnostic into `DispatchError::Discover(String)` and anyhow
/// re-prefixed it: `Error: Project discovery failed: Error: [Q-5-17] …`.
#[test]
fn unknown_project_type_diagnostic_is_not_double_wrapped() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_project(&dir, "posit-docs");

    let output = run_q2_render(&dir);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        !stderr.contains("Project discovery failed"),
        "the generic discovery wrapper must not swallow a structured diagnostic; got:\n{stderr}"
    );
    let error_headers = stderr.matches("Error:").count();
    assert_eq!(
        error_headers, 1,
        "expected exactly one `Error:` header, got {error_headers}; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Q-5-17"),
        "the real diagnostic must still print; got:\n{stderr}"
    );
}

/// Control: the identical project with `type: website` renders
/// cleanly — shared libs deduplicated in `_site/site_libs/`, source
/// tree untouched.
#[test]
fn website_type_control_renders_shared_libs() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_project(&dir, "website");

    let output = run_q2_render(&dir);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "website render must succeed; stderr:\n{stderr}"
    );
    assert!(
        dir.join("_site/site_libs").is_dir(),
        "shared libs must land in _site/site_libs"
    );

    // No `<stem>_files/` lib copies outside the output dir.
    let source_tree_files_dirs: Vec<_> = find_matching(&dir, &|name| name.ends_with("_files"))
        .into_iter()
        .filter(|p| !p.starts_with(dir.join("_site")))
        .collect();
    assert!(
        source_tree_files_dirs.is_empty(),
        "source tree must stay clean; found: {source_tree_files_dirs:?}"
    );
}

/// Pin (bd-h5rfw3ao): the Q-5-17 snippet anchors at the `type:` value
/// in `_quarto.yml`. Guards the candidate-matched binding refactor —
/// `project_type_error` must keep anchoring correctly in the file the
/// span actually originates from.
#[test]
fn unknown_type_snippet_anchors_in_quarto_yml() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("_quarto.yml"),
        "project:\n  type: no-such-type-anywhere\n",
    );
    write_file(&dir.join("index.qmd"), "---\ntitle: Home\n---\n\nBody.\n");

    let output = run_q2_render(&dir);
    assert!(!output.status.success(), "unknown type must abort");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[Q-5-17]"),
        "diagnostic should carry Q-5-17; got: {stderr}"
    );
    assert!(
        stderr.contains("_quarto.yml:2:"),
        "snippet should anchor at the type: line of _quarto.yml; got: {stderr}"
    );
    assert!(
        stderr.contains("no-such-type-anywhere"),
        "snippet should show the offending value; got: {stderr}"
    );
}
