/*
 * tests/integration/metadata_flag_cli_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Format-resolution epic F1 (FR12): `q2 render -M KEY:VALUE` through the real
 * binary.
 */

//! `-M` is the runtime metadata layer: it sits above the document's and the
//! project's own values, for every document of the render, and it is part of
//! the Pass 1 cache key so one run's profile is never served to another.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn q2_render(dir: &Path, args: &[&str]) -> Output {
    Command::new(Q2_BIN)
        .current_dir(dir)
        .arg("render")
        .args(args)
        .env_remove("QUARTO_PROFILE")
        .output()
        .expect("q2 runs")
}

fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "q2 render failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn has_toc(html: &str) -> bool {
    html.contains("id=\"TOC\"") || html.contains("quarto-toc") || html.contains("toc-")
}

fn single_doc() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("doc.qmd"),
        "---\ntitle: Document title\n---\n\n## Section\n\nBody.\n",
    )
    .unwrap();
    dir
}

fn title_of(html: &str) -> String {
    let start = html.find("<title>").expect("<title>") + "<title>".len();
    let end = html[start..].find("</title>").expect("</title>");
    html[start..start + end].to_string()
}

#[test]
fn m_overrides_the_document_title() {
    let dir = single_doc();
    ok(&q2_render(
        dir.path(),
        &["doc.qmd", "-M", "title:From the flag"],
    ));
    let html = std::fs::read_to_string(dir.path().join("doc.html")).unwrap();
    assert!(
        title_of(&html).starts_with("From the flag"),
        "{}",
        title_of(&html)
    );
}

#[test]
fn equals_separator_works() {
    let dir = single_doc();
    ok(&q2_render(dir.path(), &["doc.qmd", "-M", "title=Hello"]));
    let html = std::fs::read_to_string(dir.path().join("doc.html")).unwrap();
    assert!(title_of(&html).starts_with("Hello"), "{}", title_of(&html));
}

#[test]
fn m_overrides_the_project_value_in_a_project_render() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(root.join("_quarto.yml"), "project:\n  type: default\n").unwrap();
    std::fs::write(root.join("a.qmd"), "---\ntitle: A\n---\n\nHi\n").unwrap();
    std::fs::write(root.join("b.qmd"), "Plain document.\n").unwrap();
    // A key neither document sets: the flag supplies it to every document.
    ok(&q2_render(root, &["-M", "lang:de"]));
    for name in ["a.html", "b.html"] {
        let html = std::fs::read_to_string(root.join(name)).unwrap();
        assert!(html.contains("lang=\"de\""), "{name} lacks lang=de");
    }
}

#[test]
fn a_scalar_is_typed_by_yaml() {
    let dir = single_doc();
    ok(&q2_render(dir.path(), &["doc.qmd"]));
    let plain = std::fs::read_to_string(dir.path().join("doc.html")).unwrap();
    assert!(!has_toc(&plain), "the fixture has a TOC without the flag");

    ok(&q2_render(dir.path(), &["doc.qmd", "-M", "toc:true"]));
    let html = std::fs::read_to_string(dir.path().join("doc.html")).unwrap();
    assert!(has_toc(&html), "-M toc:true must be a boolean");
}

#[test]
fn a_flow_map_reaches_the_html_writer() {
    let dir = single_doc();
    ok(&q2_render(
        dir.path(),
        &["doc.qmd", "-M", "format:{html:{toc:true}}"],
    ));
    let html = std::fs::read_to_string(dir.path().join("doc.html")).unwrap();
    assert!(has_toc(&html), "format.html.toc did not reach the writer");
}

#[test]
fn a_flag_without_a_separator_is_an_error() {
    let dir = single_doc();
    let out = q2_render(dir.path(), &["doc.qmd", "-M", "title"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("KEY:VALUE"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Two renders of one project with different `-M title:` values each see their
/// own title in what Pass 1 recorded (the navbar's text for a page that sets
/// none), though both use the persistent profile cache.
#[test]
fn different_flags_never_share_a_cached_profile() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("_quarto.yml"),
        "project:\n  type: website\nwebsite:\n  title: Site\n  navbar:\n    left:\n      - href: other.qmd\n",
    )
    .unwrap();
    std::fs::write(root.join("index.qmd"), "Index page.\n").unwrap();
    std::fs::write(root.join("other.qmd"), "Other page.\n").unwrap();

    let navbar_text = |title: &str| -> String {
        ok(&q2_render(root, &["-M", &format!("title:{title}")]));
        std::fs::read_to_string(root.join("_site/index.html")).unwrap()
    };
    let first = navbar_text("FirstTitle");
    assert!(
        first.contains("FirstTitle"),
        "no -M title in the first render"
    );
    let second = navbar_text("SecondTitle");
    assert!(second.contains("SecondTitle"));
    assert!(
        !second.contains("FirstTitle"),
        "the second render was served the first run's cached profile"
    );
    // And a render with no flag is not served either run's profile.
    ok(&q2_render(root, &[]));
    let plain = std::fs::read_to_string(root.join("_site/index.html")).unwrap();
    assert!(!plain.contains("FirstTitle") && !plain.contains("SecondTitle"));
}
