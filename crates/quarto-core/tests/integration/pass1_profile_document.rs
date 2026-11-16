/*
 * tests/integration/pass1_profile_document.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * `quarto_core::pass1::profile_document`, the public single-document Pass-1
 * entry point (epic bd-3f6ijnw9, child C1). The language server calls it with
 * an editor buffer's bytes, so these tests pin the two properties it relies
 * on: the supplied bytes win over the file on disk, and the merged metadata it
 * returns is the same value `q2 get-config` reports.
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
fn checkpoint_meta_is_not_the_authored_merge() {
    // Pins the reason `Pass1Document` carries no merged metadata: stages
    // between the merge and the checkpoint add their own keys, so callers
    // that want what `q2 get-config` reports must use
    // `merge_document_metadata`. If this starts failing because the two
    // agree, `Pass1Document` can carry `meta` again.
    let (_dir, doc) = make_project();
    let s = setup(&doc);
    let bytes = std::fs::read(&doc).unwrap();

    let (merged, merged_ctx) = pollster::block_on(merge_document_metadata(
        s.runtime.clone(),
        &s.project,
        &s.format,
        &s.doc_info,
        &bytes,
    ))
    .expect("metadata merge succeeds");
    let authored = config_value_to_json(&merged, ProseMode::Value, &merged_ctx);
    assert_eq!(authored["description"], "from dir");
    assert_eq!(authored["toc"], true);
    for internal in ["quarto", "rendered", "listing-item"] {
        assert!(
            authored.get(internal).is_none(),
            "authored merge should not contain `{internal}`"
        );
    }

    let at_checkpoint = pollster::block_on(async {
        use quarto_core::pipeline::{build_head_stages, run_pipeline};
        use quarto_core::render::{BinaryDependencies, RenderContext};
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&s.project, &s.doc_info, &s.format, &binaries);
        let (out, _) = run_pipeline(
            &bytes,
            &doc.to_string_lossy(),
            &mut ctx,
            s.runtime.clone(),
            build_head_stages(),
        )
        .await
        .expect("head pipeline");
        let at = out.into_at_profile().expect("AtProfile");
        config_value_to_json(&at.ast.ast.meta, ProseMode::Value, &at.ast.ast_context)
    });
    assert!(at_checkpoint.get("quarto").is_some());
    assert_ne!(at_checkpoint, authored);
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
