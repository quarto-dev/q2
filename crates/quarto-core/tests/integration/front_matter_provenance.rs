/*
 * tests/integration/front_matter_provenance.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * EXR-2 Phase 2, decision 11: which merged front-matter values are the
 * document's own, decided by the `SourceInfo` root file id.
 */

//! Decision 11 of the EXR-2 plan rests on these facts: after the metadata
//! merge, a document key is rooted in `FileId(0)`, layer-file keys in their
//! registered filename-hash ids, and synthesized keys in nothing.

use std::path::Path;
use std::sync::Arc;

use quarto_core::format::Format;
use quarto_core::get_config::merge_document_metadata;
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use tempfile::TempDir;

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn merged_front_matter_root_file_ids_identify_the_documents_own_keys() {
    let tmp = TempDir::new().unwrap();
    let root = quarto_system_runtime::canonicalize(tmp.path()).unwrap();
    write(
        &root.join("_extensions/acme/_extension.yml"),
        "title: Acme\nversion: 1.0.0\ncontributes:\n  metadata:\n    probe-ext-meta: e\n  formats:\n    html:\n      probe-ext-format: f\n",
    );
    write(
        &root.join("_quarto.yml"),
        "project:\n  type: default\nprobe-project: p\n",
    );
    write(&root.join("chapters/_metadata.yml"), "probe-dir: d\n");
    let doc = root.join("chapters/doc.qmd");
    write(&doc, "---\nprobe-doc: doc\n---\n\nBody\n");

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let project = ProjectContext::discover(&root, runtime.as_ref()).unwrap();
    let format = Format::from_format_string("acme-html").unwrap();
    let source = std::fs::read(&doc).unwrap();
    let (meta, ctx) = pollster::block_on(merge_document_metadata(
        runtime.clone(),
        &project,
        &format,
        &DocumentInfo::from_path(&doc),
        &source,
    ))
    .unwrap();

    let root_of = |key: &str| meta.get(key).map(|v| v.source_info.root_file_id());
    let own = quarto_source_map::FileId(0);
    // The document's own key is rooted in the document (FileId(0)); every
    // layer file carries its own non-zero filename-hash id, so `== FileId(0)`
    // is exactly "the document's own". Synthesized values have no root.
    assert_eq!(root_of("probe-doc"), Some(Some(own)));
    for key in [
        "probe-project",
        "probe-dir",
        "probe-ext-meta",
        "probe-ext-format",
    ] {
        let root = root_of(key).unwrap_or_else(|| panic!("{key} absent"));
        assert!(root.is_some() && root != Some(own), "{key}: {root:?}");
        let name = ctx
            .source_context
            .get_file(root.unwrap())
            .map(|f| f.path.clone());
        assert!(name.is_some(), "{key}: file id must be registered");
    }
    assert_eq!(
        root_of("format"),
        Some(None),
        "synthesized `format:` has no root"
    );
}
