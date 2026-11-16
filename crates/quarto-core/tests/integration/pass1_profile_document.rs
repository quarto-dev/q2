/*
 * tests/integration/pass1_profile_document.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `quarto_core::pass1::profile_document`, the public single-document Pass-1
 * entry point (epic bd-3f6ijnw9, children C1 and C1b). The language server
 * calls it with an editor buffer's bytes, so these tests pin the properties it
 * relies on: the supplied bytes win over the file on disk, and the authored
 * metadata it records is the same value `q2 get-config` reports.
 *
 * Plan: claude-notes/plans/2026-11-16-lsp-document-profile-requests.md
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pampa::config_json::{ProseMode, config_value_to_json};
use quarto_core::format::Format;
use quarto_core::get_config::merge_document_metadata;
use quarto_core::pass1::profile_document;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tempfile::TempDir;

/// ```text
/// <root>/_quarto.yml          description "from project", format.html.toc: true
/// <root>/sub/_metadata.yml    description "from dir", keywords [dir]
/// <root>/sub/doc.qmd          title "On disk", author Alice
/// ```
fn make_project() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonicalize temp root");
    std::fs::write(
        root.join("_quarto.yml"),
        "project:\n  type: default\ndescription: \"from project\"\nformat:\n  html:\n    toc: true\n",
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
    std::fs::write(&doc, "---\ntitle: On disk\nauthor: Alice\n---\n\n# Body\n").unwrap();
    (dir, doc)
}

struct Setup {
    runtime: Arc<dyn SystemRuntime>,
    project: ProjectContext,
    format: Format,
    doc_info: DocumentInfo,
}

fn setup(doc: &Path) -> Setup {
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(doc, runtime.as_ref()).expect("discover project");
    assert!(!project.is_single_file, "fixture must be a real project");
    Setup {
        runtime,
        project,
        format: Format::from_format_string("html").expect("html format"),
        doc_info: DocumentInfo::from_path(doc),
    }
}

#[test]
fn supplied_bytes_win_over_the_file_on_disk() {
    let (_dir, doc) = make_project();
    let s = setup(&doc);
    let buffer = b"---\ntitle: From the buffer\n---\n\n# First\n\n## Second\n";

    let result = pollster::block_on(profile_document(
        s.runtime.clone(),
        &s.project,
        &s.format,
        &s.doc_info,
        buffer,
    ))
    .expect("pass 1 succeeds");

    assert_eq!(
        result.profile.title_text().as_deref(),
        Some("From the buffer")
    );
    // The buffer has no author; the disk copy's `Alice` must not leak in.
    assert!(
        result.profile.authors.is_empty(),
        "{:?}",
        result.profile.authors
    );
    assert_eq!(result.profile.outline.len(), 1, "one top-level heading");
    assert_eq!(result.profile.outline[0].children.len(), 1);
    // Project-relative, forward-slash, as the profile contract guarantees.
    assert_eq!(result.profile.source_path, PathBuf::from("sub/doc.qmd"));
}

#[test]
fn authored_meta_matches_get_config_and_excludes_derived_keys() {
    // bd-xzsnuoqq: Pass 1 records the merged metadata at the end of the
    // ingest phase, before the enrich stages add `quarto.language`,
    // `rendered.includes` and `listing-item`. It must equal what
    // `q2 get-config` reports, which runs the ingest phase alone.
    let (_dir, doc) = make_project();
    let s = setup(&doc);
    let bytes = std::fs::read(&doc).unwrap();

    let pass1 = pollster::block_on(profile_document(
        s.runtime.clone(),
        &s.project,
        &s.format,
        &s.doc_info,
        &bytes,
    ))
    .expect("pass 1 succeeds");
    let (merged, merged_ctx) = pollster::block_on(merge_document_metadata(
        s.runtime.clone(),
        &s.project,
        &s.format,
        &s.doc_info,
        &bytes,
    ))
    .expect("metadata merge succeeds");

    let from_pass1 =
        config_value_to_json(&pass1.authored_meta, ProseMode::Value, &pass1.ast_context);
    let from_get_config = config_value_to_json(&merged, ProseMode::Value, &merged_ctx);
    assert_eq!(from_pass1, from_get_config);

    // The layers are all there: dir beats project, format flattened.
    assert_eq!(from_pass1["description"], "from dir");
    assert_eq!(from_pass1["keywords"], serde_json::json!(["dir"]));
    assert_eq!(from_pass1["toc"], true);
    assert_eq!(from_pass1["author"], "Alice");
    // And nothing derived by the enrich phase.
    for derived in ["quarto", "rendered", "listing-item"] {
        assert!(
            from_pass1.get(derived).is_none(),
            "authored metadata must not contain `{derived}`"
        );
    }
}

#[test]
fn a_document_that_cannot_parse_is_an_error_not_an_empty_profile() {
    let (_dir, doc) = make_project();
    let s = setup(&doc);
    // An editorial-comment mark still being typed: the qmd reader rejects it
    // (Q-2-21). A client must not mistake this for "no comments".
    let buffer = b"---\ntitle: Typing\n---\n\nSome text [>> half a comment\n";

    let result = pollster::block_on(profile_document(
        s.runtime.clone(),
        &s.project,
        &s.format,
        &s.doc_info,
        buffer,
    ));
    assert!(result.is_err(), "expected a parse failure, got a profile");
}
