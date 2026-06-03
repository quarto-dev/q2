//! CLI end-to-end tests for `q2 get-config` (bd-xoaic, GH #256).
//!
//! These drive the real `q2` binary against on-disk fixtures, mirroring how an
//! external tool would call the command. Plan:
//! `claude-notes/plans/2026-06-02-get-config-command.md`.
//!
//! Cargo wires the binary path through `CARGO_BIN_EXE_q2`; no `assert_cmd`
//! dependency is needed.

use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// Build a project fixture:
///
/// ```text
/// <root>/_quarto.yml         toc:false, description:"from project",
///                            format.html.toc:true, format.pdf.{toc:false,documentclass:article}
/// <root>/sub/_metadata.yml   description:"from dir"
/// <root>/sub/doc.qmd         title: Hello _world_!, authors:[{name:Alice},{name:Bob}]
/// <root>/standalone.qmd      title: Solo *doc*, toc:true   (no project)
/// ```
fn make_fixture() -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path();

    std::fs::write(
        root.join("_quarto.yml"),
        "project:\n  type: default\ntoc: false\ndescription: \"from project\"\n\
         format:\n  html:\n    toc: true\n  pdf:\n    toc: false\n    documentclass: article\n",
    )
    .unwrap();

    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("_metadata.yml"), "description: \"from dir\"\n").unwrap();
    std::fs::write(
        sub.join("doc.qmd"),
        "---\ntitle: Hello _world_!\nauthors:\n  - name: Alice\n  - name: Bob\n---\n\n# Body\n",
    )
    .unwrap();

    std::fs::write(
        root.join("standalone.qmd"),
        "---\ntitle: Solo *doc*\ntoc: true\n---\n\n# Hi\n",
    )
    .unwrap();

    dir
}

/// Run `q2 get-config <args…>` and return `(stdout, stderr, exit_code)`.
fn run(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(Q2_BIN)
        .arg("get-config")
        .args(args)
        .output()
        .expect("spawn q2 get-config");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

fn doc(root: &Path) -> String {
    root.join("sub/doc.qmd").to_string_lossy().into_owned()
}

#[test]
fn help_exits_zero() {
    let output = Command::new(Q2_BIN)
        .args(["get-config", "--help"])
        .output()
        .expect("spawn q2 get-config --help");
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn whole_metadata_reflects_merge() {
    let fx = make_fixture();
    let (stdout, stderr, code) = run(&[&doc(fx.path())]);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    let v: Value = serde_json::from_str(&stdout).expect("valid json");
    // dir _metadata.yml overrides project description.
    assert_eq!(v["description"], Value::from("from dir"));
    // format.html.toc flattening overrides top-level toc:false.
    assert_eq!(v["toc"], Value::from(true));
    // frontmatter prose title round-trips to markdown.
    assert_eq!(v["title"], Value::from("Hello *world*!"));
    assert_eq!(v["authors"][0]["name"], Value::from("Alice"));
}

#[test]
fn title_value_mode_is_markdown_string() {
    let fx = make_fixture();
    let (stdout, _e, code) = run(&[&doc(fx.path()), "title"]);
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "\"Hello *world*!\"");
}

#[test]
fn title_pandoc_mode_is_source_free_ast() {
    let fx = make_fixture();
    let (stdout, _e, code) = run(&[&doc(fx.path()), "title", "--output", "pandoc"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&stdout).expect("valid json");
    let arr = v.as_array().expect("inline array");
    assert!(arr.iter().any(|n| n["t"] == "Emph"));
    // No source-pool ids anywhere.
    assert!(
        !stdout.contains("\"s\""),
        "pandoc output must be source-free:\n{stdout}"
    );
}

#[test]
fn format_switch_changes_result() {
    let fx = make_fixture();
    let d = doc(fx.path());

    let (html, _e, _c) = run(&[&d, "toc", "--to", "html"]);
    assert_eq!(html.trim(), "true");

    let (pdf, _e, _c) = run(&[&d, "toc", "--to", "pdf"]);
    assert_eq!(pdf.trim(), "false");

    let (dc, _e, _c) = run(&[&d, "documentclass", "--to", "pdf"]);
    assert_eq!(dc.trim(), "\"article\"");
}

#[test]
fn array_index_path() {
    let fx = make_fixture();
    let (stdout, _e, code) = run(&[&doc(fx.path()), "authors.1.name"]);
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "\"Bob\"");
}

#[test]
fn compact_is_single_line() {
    let fx = make_fixture();
    let (stdout, _e, _c) = run(&[&doc(fx.path()), "--compact"]);
    assert_eq!(
        stdout.lines().count(),
        1,
        "compact output should be one line"
    );
    let _v: Value = serde_json::from_str(&stdout).expect("valid json");
}

#[test]
fn missing_path_prints_null_exit_zero() {
    let fx = make_fixture();
    let (stdout, _e, code) = run(&[&doc(fx.path()), "does.not.exist"]);
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "null");
}

#[test]
fn missing_path_strict_exits_nonzero() {
    let fx = make_fixture();
    let (_o, _e, code) = run(&[&doc(fx.path()), "does.not.exist", "--strict"]);
    assert_ne!(code, 0, "--strict should fail on a missing path");
}

#[test]
fn standalone_document_without_project() {
    let fx = make_fixture();
    let path = fx
        .path()
        .join("standalone.qmd")
        .to_string_lossy()
        .into_owned();
    let (stdout, stderr, code) = run(&[&path, "title"]);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert_eq!(stdout.trim(), "\"Solo *doc*\"");
}

#[test]
fn missing_file_is_an_error() {
    let fx = make_fixture();
    let path = fx.path().join("nope.qmd").to_string_lossy().into_owned();
    let (_o, _e, code) = run(&[&path, "title"]);
    assert_ne!(code, 0, "a missing input file must be a real error");
}
