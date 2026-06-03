/*
 * tests/get_config_merge.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Phase 1 integration test for `q2 get-config` (bd-xoaic, GH #256).
 *
 * Verifies that `quarto_core::get_config::merge_document_metadata` reproduces
 * the real render merge: project `_quarto.yml`, directory `_metadata.yml`
 * layers, document frontmatter, and `format.<id>.*` flattening — and that the
 * result projects cleanly to JSON via `pampa::config_json`.
 *
 * Plan: claude-notes/plans/2026-06-02-get-config-command.md
 */

use std::sync::Arc;

use tempfile::TempDir;

use pampa::config_json::{ProseMode, config_value_to_json, navigate};
use pampa::pandoc::ASTContext;
use quarto_core::format::Format;
use quarto_core::get_config::merge_document_metadata;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_pandoc_types::ConfigValue;
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use serde_json::{Value, json};

/// Build a small on-disk project:
///
/// ```text
/// <root>/_quarto.yml             toc: false, format.html.toc: true, description: "from project"
/// <root>/sub/_metadata.yml       description: "from dir", keywords: [dir]
/// <root>/sub/doc.qmd             frontmatter: title: Hello _world_!, author: Alice
/// ```
fn make_project() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonicalize temp root");

    std::fs::write(
        root.join("_quarto.yml"),
        "project:\n  type: default\ntoc: false\ndescription: \"from project\"\nformat:\n  html:\n    toc: true\n",
    )
    .unwrap();

    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        sub.join("_metadata.yml"),
        "description: \"from dir\"\nkeywords: [dir]\n",
    )
    .unwrap();

    let doc = sub.join("doc.qmd");
    std::fs::write(
        &doc,
        "---\ntitle: Hello _world_!\nauthor: Alice\n---\n\n# Body\n",
    )
    .unwrap();

    (dir, doc)
}

/// Merge `doc`'s metadata under the html format and return the projected JSON
/// (value mode) together with the raw merged `ConfigValue`.
fn merge_doc_html(doc: &std::path::Path) -> (Value, ConfigValue, ASTContext) {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project =
        ProjectContext::discover(doc, runtime.as_ref()).expect("discover project context");
    assert!(
        !project.is_single_file,
        "fixture should be discovered as a real project (has _quarto.yml)"
    );

    let format = Format::from_format_string("html").expect("html format");
    let doc_info = DocumentInfo::from_path(doc);
    let source_bytes = std::fs::read(doc).unwrap();

    let (meta, ctx) = pollster::block_on(merge_document_metadata(
        runtime.clone(),
        &project,
        &format,
        &doc_info,
        &source_bytes,
    ))
    .expect("merge_document_metadata");

    let json = config_value_to_json(&meta, ProseMode::Value, &ctx);
    (json, meta, ctx)
}

#[test]
fn format_flattening_html_overrides_top_level_toc() {
    let (_dir, doc) = make_project();
    let (json, _meta, _ctx) = merge_doc_html(&doc);
    // Project has top-level `toc: false` plus `format.html.toc: true`.
    // Flattening html within the project layer makes the effective toc `true`.
    assert_eq!(json["toc"], json!(true), "merged json: {json:#}");
}

#[test]
fn directory_metadata_overrides_project() {
    let (_dir, doc) = make_project();
    let (json, _meta, _ctx) = merge_doc_html(&doc);
    // `description` set in project (_quarto.yml) and overridden in the
    // directory `_metadata.yml`; the deeper directory layer wins.
    assert_eq!(
        json["description"],
        json!("from dir"),
        "merged json: {json:#}"
    );
}

#[test]
fn frontmatter_prose_title_round_trips_to_markdown() {
    let (_dir, doc) = make_project();
    let (json, _meta, _ctx) = merge_doc_html(&doc);
    // Title comes from the document frontmatter and is parsed as markdown
    // (document-metadata context); value mode renders it back to markdown.
    assert_eq!(
        json["title"],
        json!("Hello *world*!"),
        "merged json: {json:#}"
    );
    assert_eq!(json["author"], json!("Alice"), "merged json: {json:#}");
}

#[test]
fn navigate_into_merged_metadata() {
    let (_dir, doc) = make_project();
    let (_json, meta, ctx) = merge_doc_html(&doc);

    // Empty path → whole object.
    let whole = config_value_to_json(navigate(&meta, "").unwrap(), ProseMode::Value, &ctx);
    assert!(whole.is_object());

    // Single key.
    let title = navigate(&meta, "title").expect("title present");
    assert_eq!(
        config_value_to_json(title, ProseMode::Value, &ctx),
        json!("Hello *world*!")
    );

    // Missing key.
    assert!(navigate(&meta, "does-not-exist").is_none());
}
