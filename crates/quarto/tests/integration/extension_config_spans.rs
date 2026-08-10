/*
 * extension_config_spans.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * bd-m6wmztln / bd-p86nlm92 — diagnostics for extension-contributed
 * project config must anchor their spans in the contributing
 * `_extension.yml`, not in the project's `_quarto.yml`.
 */

//! End-to-end span-correctness tests for diagnostics anchored at
//! **extension-contributed** project configuration
//! (`contributes.metadata.project.*`, bd-ad7i1pc6 Phase 5).
//!
//! Such config values carry a `SourceInfo` whose FileId is
//! quarto-yaml's hash of the extension manifest's path. Diagnostic
//! assembly used to bind `_quarto.yml`'s content to that FileId
//! unconditionally, rendering the manifest's byte offsets against the
//! wrong file (bd-m6wmztln). These tests drive the real `q2` binary
//! and assert on the file named in the ariadne snippet header.
//!
//! The render-scripts variant of the same bug is covered in
//! `render_scripts_cli.rs`
//! (`failing_extension_contributed_script_snippet_names_extension_yml`);
//! this module holds the script-free cases.

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

fn run_q2_render(project: &Path) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(project);
    cmd.arg("render");
    cmd.output().expect("spawn q2 binary")
}

/// An out-of-project `project.resources` pattern contributed by an
/// extension must anchor its Q-5-1 snippet in the extension's
/// `_extension.yml` (bd-p86nlm92).
#[test]
fn out_of_project_resource_contributed_by_extension_names_extension_yml() {
    let temp = TempDir::new().unwrap();
    let project = temp.path().canonicalize().unwrap();
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n  render:\n    - \"**/*.qmd\"\n    - \"!drafts/\"\n",
    );
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome body.\n",
    );
    write_file(
        &project.join("_extensions/acme/escaper/_extension.yml"),
        "title: escaper\nauthor: Acme\nversion: 0.0.1\ncontributes:\n  metadata:\n    project:\n      resources:\n        - \"../escape.csv\"\n",
    );

    let out = run_q2_render(&project);
    assert!(
        !out.status.success(),
        "out-of-project resource pattern must abort the render"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[Q-5-1]"),
        "diagnostic should carry the Q-5-1 code; got: {stderr}"
    );
    assert!(
        stderr.contains("_extension.yml:"),
        "snippet should anchor in the extension manifest; got: {stderr}"
    );
    assert!(
        !stderr.contains("_quarto.yml:"),
        "snippet must not anchor in _quarto.yml (the pattern is not declared there); got: {stderr}"
    );
}

/// An out-of-project `resources` pattern inherited from a directory
/// `_metadata.yml` layer must anchor its Q-5-1 snippet in that
/// `_metadata.yml` — not in the document that inherited it
/// (bd-x113wg9v, the doc-level sibling of bd-p86nlm92).
#[test]
fn out_of_project_resource_from_metadata_yml_names_metadata_yml() {
    let temp = TempDir::new().unwrap();
    let project = temp.path().canonicalize().unwrap();
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome body.\n",
    );
    write_file(
        &project.join("blog/_metadata.yml"),
        "resources:\n  - \"../../escape.csv\"\n",
    );
    write_file(
        &project.join("blog/post.qmd"),
        "---\ntitle: Post\n---\n\nPost body.\n",
    );

    let out = run_q2_render(&project);
    assert!(
        !out.status.success(),
        "out-of-project resource pattern must abort the render"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[Q-5-1]"),
        "diagnostic should carry the Q-5-1 code; got: {stderr}"
    );
    assert!(
        stderr.contains("_metadata.yml:"),
        "snippet should anchor in the declaring _metadata.yml; got: {stderr}"
    );
    assert!(
        !stderr.contains("post.qmd:"),
        "snippet must not anchor in the inheriting document; got: {stderr}"
    );
}

/// Control: a `resources` pattern declared in the document's own
/// frontmatter keeps anchoring in that document (the doc's spans root
/// at its dense FileId, not a filename hash — the candidate list must
/// carry both id schemes).
#[test]
fn out_of_project_resource_declared_in_doc_names_doc() {
    let temp = TempDir::new().unwrap();
    let project = temp.path().canonicalize().unwrap();
    write_file(
        &project.join("_quarto.yml"),
        "project:\n  type: website\n  output-dir: _site\n",
    );
    write_file(
        &project.join("index.qmd"),
        "---\ntitle: Home\n---\n\nHome body.\n",
    );
    write_file(
        &project.join("blog/post.qmd"),
        "---\ntitle: Post\nresources:\n  - \"../../escape.csv\"\n---\n\nPost body.\n",
    );

    let out = run_q2_render(&project);
    assert!(
        !out.status.success(),
        "out-of-project resource pattern must abort the render"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("[Q-5-1]"),
        "diagnostic should carry the Q-5-1 code; got: {stderr}"
    );
    assert!(
        stderr.contains("post.qmd:"),
        "snippet should anchor in the declaring document; got: {stderr}"
    );
}
