/*
 * tests/integration/session_runtime_pass1.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pass 1 over a `SessionRuntime` (epic bd-3f6ijnw9, child C2).
 *
 * The language server will run project discovery and Pass 1 through a
 * `SessionRuntime` (editor buffers over a base layer over the native
 * runtime). These tests pin what that must deliver:
 *
 * - with no buffers, exactly the profile and authored metadata the native
 *   runtime gives (disk mode);
 * - unsaved edits to an *included* file and to `_metadata.yml` reach the
 *   result, and every file Pass 1 read is recorded with its version;
 * - a project that exists only in the base layer (attached mode, nothing on
 *   disk) is discovered and profiled just the same.
 *
 * Plan: claude-notes/plans/2026-11-16-lsp-document-profile-requests.md
 */

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pampa::config_json::{ProseMode, config_value_to_json};
use quarto_core::format::Format;
use quarto_core::pass1::{Pass1Document, profile_document};
use quarto_core::project::{DocumentInfo, ProjectContext};
use quarto_core::session_runtime::{
    EditorLayer, ProjectSource, SessionRuntime, VersionKey, VfsBaseLayer,
};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};
use serde_json::Value;

const QUARTO_YML: &str = "project:\n  type: default\ndescription: \"from project\"\n";
const METADATA_YML: &str = "description: \"from dir\"\n";
const DOC_QMD: &str = "---\ntitle: Doc\n---\n\n# Top\n\n{{< include _child.qmd >}}\n";
const CHILD_QMD: &str = "## From the child\n";

/// `files` as (project-relative path, contents).
fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("_quarto.yml", QUARTO_YML),
        ("sub/_metadata.yml", METADATA_YML),
        ("sub/doc.qmd", DOC_QMD),
        ("sub/_child.qmd", CHILD_QMD),
    ]
}

fn write_project(root: &Path) {
    for (rel, text) in files() {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn temp_root() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    (tmp, root)
}

fn pass1(runtime: Arc<dyn SystemRuntime>, doc: &Path) -> Pass1Document {
    let project = ProjectContext::discover(doc, runtime.as_ref()).expect("discover project");
    assert!(!project.is_single_file, "fixture must be a real project");
    let format = Format::from_format_string("html").unwrap();
    let doc_info = DocumentInfo::from_path(doc);
    let bytes = runtime.file_read(doc).expect("read the document");
    pollster::block_on(profile_document(
        runtime, &project, &format, &doc_info, &bytes,
    ))
    .expect("pass 1 succeeds")
}

/// The profile minus the fields that legitimately depend on the runtime's
/// clock or file times (listing autofill's `date-modified`), with the
/// project root spelled `<root>`: `includes[].path` is absolute (bd-36abpehm),
/// and two fixtures live under different temp roots.
fn profile_json(p: &Pass1Document, root: &Path) -> Value {
    let mut v = serde_json::to_value(&p.profile).unwrap();
    if let Some(item) = v.get_mut("listing_item").and_then(Value::as_object_mut) {
        item.remove("date_modified");
    }
    let text = serde_json::to_string(&v)
        .unwrap()
        .replace(&root.to_string_lossy().replace('\\', "\\\\"), "<root>");
    serde_json::from_str(&text).unwrap()
}

fn meta_json(p: &Pass1Document) -> Value {
    config_value_to_json(&p.authored_meta, ProseMode::Value, &p.ast_context)
}

fn outline_titles(p: &Pass1Document) -> Vec<String> {
    fn walk(entries: &[pampa::toc::TocEntry], out: &mut Vec<String>) {
        for e in entries {
            out.push(quarto_pandoc_types::inlines_to_plain_text(&e.title));
            walk(&e.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&p.profile.outline, &mut out);
    out
}

fn session(
    root: &Path,
    source: ProjectSource,
    editor: EditorLayer,
    base: VfsBaseLayer,
) -> Arc<SessionRuntime> {
    Arc::new(SessionRuntime::new(
        root,
        source,
        Arc::new(editor),
        Arc::new(base),
        Arc::new(NativeRuntime::new()),
    ))
}

#[test]
fn disk_mode_without_buffers_matches_the_native_runtime() {
    let (_tmp, root) = temp_root();
    write_project(&root);
    let doc = root.join("sub/doc.qmd");

    let native = pass1(Arc::new(NativeRuntime::new()), &doc);
    let rt = session(
        &root,
        ProjectSource::Disk,
        EditorLayer::new(),
        VfsBaseLayer::load_from_disk(&root).unwrap(),
    );
    let via_session = pass1(rt.clone(), &doc);

    assert_eq!(
        profile_json(&via_session, &root),
        profile_json(&native, &root)
    );
    assert_eq!(meta_json(&via_session), meta_json(&native));
    assert_eq!(outline_titles(&native), ["Top", "From the child"]);
}

#[test]
fn unsaved_edits_to_an_include_and_to_metadata_reach_pass_1() {
    let (_tmp, root) = temp_root();
    write_project(&root);
    let doc = root.join("sub/doc.qmd");

    let mut editor = EditorLayer::new();
    editor.set("sub/_child.qmd", b"## Edited in the editor\n".to_vec(), 3);
    editor.set(
        "sub/_metadata.yml",
        b"description: \"from an unsaved buffer\"\n".to_vec(),
        5,
    );
    let rt = session(
        &root,
        ProjectSource::Disk,
        editor,
        VfsBaseLayer::load_from_disk(&root).unwrap(),
    );
    let result = pass1(rt.clone(), &doc);

    assert_eq!(outline_titles(&result), ["Top", "Edited in the editor"]);
    assert_eq!(meta_json(&result)["description"], "from an unsaved buffer");

    // Everything Pass 1 depended on is recorded, with the version it saw.
    let reads = rt.reads();
    assert_eq!(
        reads.get(Path::new("sub/_child.qmd")),
        Some(&VersionKey::Buffer(3))
    );
    assert_eq!(
        reads.get(Path::new("sub/_metadata.yml")),
        Some(&VersionKey::Buffer(5))
    );
    assert_eq!(
        reads.get(Path::new("_quarto.yml")),
        Some(&VersionKey::content_of(QUARTO_YML.as_bytes()))
    );
    assert_eq!(
        reads.get(Path::new("sub/doc.qmd")),
        Some(&VersionKey::content_of(DOC_QMD.as_bytes()))
    );
    // Nothing on disk changed.
    assert_eq!(
        std::fs::read_to_string(root.join("sub/_child.qmd")).unwrap(),
        CHILD_QMD
    );
}

#[test]
fn a_project_that_exists_only_in_the_base_layer_is_profiled() {
    // Attached mode: the files live in the base layer (a hub, in F3), and
    // the root directory on disk is empty.
    let (_tmp, disk_root) = temp_root();
    write_project(&disk_root);
    let reference = pass1(
        Arc::new(NativeRuntime::new()),
        &disk_root.join("sub/doc.qmd"),
    );

    let (_empty, root) = temp_root();
    let mut base = VfsBaseLayer::new();
    for (rel, text) in files() {
        base.insert_file(rel, text.as_bytes().to_vec());
    }
    let rt = session(&root, ProjectSource::Attached, EditorLayer::new(), base);
    let result = pass1(rt.clone(), &root.join("sub/doc.qmd"));

    assert_eq!(outline_titles(&result), ["Top", "From the child"]);
    assert_eq!(meta_json(&result), meta_json(&reference));
    assert_eq!(
        profile_json(&result, &root),
        profile_json(&reference, &disk_root)
    );
    assert!(
        std::fs::read_dir(&root).unwrap().next().is_none(),
        "the attached session must not have written into the root"
    );
}
