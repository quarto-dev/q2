/*
 * tests/integration/root_directory_metadata.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * A `_metadata.yml` in the project root, next to `_quarto.yml`.
 */

//! A project-root `_metadata.yml` is a directory-metadata layer like any
//! other: it applies to the documents in the root and in every
//! subdirectory, below the subdirectories' own `_metadata.yml` files and
//! the documents' front matter, and above `_quarto.yml`.
//!
//! Quarto 1 applies it to root-level documents only, an accident of how it
//! splits the document's project-relative directory; q2 deliberately
//! applies it to the whole tree.
//!
//! The render tests drive `ProjectPipeline` (the path `q2 render` takes)
//! and inspect the HTML and CSS written to disk.
//!
//! Plan: `claude-notes/plans/2026-11-06-root-metadata-yml.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::get_config::{ProseMode, config_value_to_json, merge_document_metadata};
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::render_to_file::RenderToFileOptions;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const DOC: &str = "---\ntitle: T\n---\n\n## Section\n\ntext\n";

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e))
}

/// A default-type project with `doc.qmd` in the root and `sub/doc.qmd`
/// below it, plus the given root `_metadata.yml`.
fn fixture(root_metadata: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new().unwrap();
    let dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());
    write(&dir.join("_quarto.yml"), "project:\n  type: default\n");
    write(&dir.join("_metadata.yml"), root_metadata);
    write(&dir.join("doc.qmd"), DOC);
    write(&dir.join("sub/doc.qmd"), DOC);
    (temp, dir)
}

/// Render the whole project through the CLI's project pipeline.
fn render(dir: &Path) {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(dir, runtime.as_ref()).expect("discover");
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline run");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures
    );
}

fn has_toc(html: &str) -> bool {
    html.contains("<nav id=\"TOC\"")
}

/// The contents of every local stylesheet `html_path` links to.
fn linked_css(html_path: &Path) -> Vec<(PathBuf, String)> {
    let html = read(html_path);
    let base = html_path.parent().unwrap();
    html.split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|href| href.ends_with(".css") && !href.contains("://"))
        .map(|href| {
            let path = base.join(href);
            let css = read(&path);
            (path, css)
        })
        .collect()
}

/// Merge `doc`'s metadata for HTML the way a render does, as JSON.
fn merged(dir: &Path, doc: &Path) -> Value {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(dir, runtime.as_ref()).unwrap();
    let source = std::fs::read(doc).unwrap();
    let (meta, ctx) = pollster::block_on(merge_document_metadata(
        runtime.clone(),
        &project,
        &Format::html(),
        &DocumentInfo::from_path(doc),
        &source,
    ))
    .unwrap();
    config_value_to_json(&meta, ProseMode::Value, &ctx)
}

#[test]
fn top_level_toc_applies_to_root_and_subdirectory_documents() {
    let (_temp, dir) = fixture("toc: true\n");
    render(&dir);
    assert!(has_toc(&read(&dir.join("doc.html"))), "doc.html has no TOC");
    assert!(
        has_toc(&read(&dir.join("sub/doc.html"))),
        "sub/doc.html has no TOC"
    );
}

#[test]
fn format_html_toc_applies_to_root_and_subdirectory_documents() {
    let (_temp, dir) = fixture("format:\n  html:\n    toc: true\n");
    render(&dir);
    assert!(has_toc(&read(&dir.join("doc.html"))), "doc.html has no TOC");
    assert!(
        has_toc(&read(&dir.join("sub/doc.html"))),
        "sub/doc.html has no TOC"
    );
}

#[test]
fn custom_theme_applies_and_resolves_against_the_root() {
    // `t.scss` is relative to the root `_metadata.yml`, so `sub/doc.qmd`
    // must find it in the root, not in `sub/`.
    let (_temp, dir) = fixture("format:\n  html:\n    theme:\n      - t.scss\n");
    write(
        &dir.join("t.scss"),
        "/*-- scss:defaults --*/\n$body-bg: #abcdef;\n",
    );
    render(&dir);
    for page in ["doc.html", "sub/doc.html"] {
        let css = linked_css(&dir.join(page));
        assert!(
            css.iter().any(|(_, text)| text.contains("#abcdef")),
            "{page} links no stylesheet with the root theme's $body-bg; linked: {:?}",
            css.iter().map(|(p, _)| p).collect::<Vec<_>>()
        );
    }
}

#[test]
fn root_metadata_sits_between_project_and_deeper_layers() {
    let (_temp, dir) = fixture("probe-root: r\nwinner: root\n");
    write(
        &dir.join("_quarto.yml"),
        "project:\n  type: default\nprobe-project: p\nwinner: project\n",
    );
    write(&dir.join("sub/_metadata.yml"), "winner: sub\n");
    write(
        &dir.join("sub/front.qmd"),
        "---\ntitle: T\nwinner: front\n---\n\ntext\n",
    );

    let root_doc = merged(&dir, &dir.join("doc.qmd"));
    assert_eq!(root_doc["winner"], "root", "{root_doc}");
    assert_eq!(root_doc["probe-project"], "p", "{root_doc}");
    assert_eq!(root_doc["probe-root"], "r", "{root_doc}");

    let sub_doc = merged(&dir, &dir.join("sub/doc.qmd"));
    assert_eq!(sub_doc["winner"], "sub", "{sub_doc}");
    assert_eq!(sub_doc["probe-root"], "r", "{sub_doc}");

    let front = merged(&dir, &dir.join("sub/front.qmd"));
    assert_eq!(front["winner"], "front", "{front}");
    assert_eq!(front["probe-root"], "r", "{front}");
}
