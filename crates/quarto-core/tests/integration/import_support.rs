//! Helpers for the document-import tests: P1's recordings under
//! `tests/fixtures/import-recordings/`, read the way the pipeline's callers read them.

use std::path::PathBuf;

use serde_json::{Value, json};

/// Every fixture directory name, `corrupt-docx` included.
pub const FIXTURES: &[&str] = &[
    "basic-docx",
    "basic-epub",
    "basic-odt",
    "basic-pptx",
    "basic-rtf",
    "comments-edge-docx",
    "corrupt-docx",
    "emf-docx",
    "highlights-docx",
    "images-docx",
    "quarto-made-docx",
    "track-changes-docx",
    "writer-bugs-docx",
];

pub fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/import-recordings")
        .join(name)
}

/// `<directory name>.qmd` at the project root: the convention every fixture's expected qmd
/// is written under (links are relative to it).
pub fn target_qmd_path(name: &str) -> String {
    format!("{name}.qmd")
}

pub fn read_text(name: &str, file: &str) -> String {
    std::fs::read_to_string(fixture_dir(name).join(file))
        .unwrap_or_else(|e| panic!("{name}/{file}: {e}"))
}

pub fn pandoc_json(name: &str) -> String {
    read_text(name, "pandoc.json")
}

pub fn stderr(name: &str) -> String {
    read_text(name, "stderr.txt")
}

/// The fixture's argv (what `prepare_import` must reproduce).
pub fn argv(name: &str) -> Vec<String> {
    serde_json::from_str(&read_text(name, "argv.json")).unwrap()
}

/// `(file name, size, sha256)` of the fixture's source, from `manifest.json`.
pub fn source_info(name: &str) -> (String, u64, String) {
    let manifest: Vec<Value> = serde_json::from_str(&read_text(name, "manifest.json")).unwrap();
    let entry = manifest
        .iter()
        .find(|e| e["path"].as_str().unwrap().starts_with("source."))
        .expect("source entry");
    (
        entry["path"].as_str().unwrap().to_string(),
        entry["size"].as_u64().unwrap(),
        entry["sha256"].as_str().unwrap().to_string(),
    )
}

/// The interface-2 media manifest P4 builds from a run's collected files: every file under
/// `media/` as a `stored` entry (README: "Turning `manifest.json` into an interface-2 media
/// manifest"), in manifest (path) order.
pub fn media_manifest(name: &str) -> Vec<Value> {
    let manifest: Vec<Value> = serde_json::from_str(&read_text(name, "manifest.json")).unwrap();
    manifest
        .iter()
        .filter_map(|e| {
            let rel = e["path"].as_str().unwrap().strip_prefix("media/")?;
            let ext = rel.rsplit_once('.').map_or("", |(_, e)| e);
            Some(json!({
                "pandoc_path": format!("/__q2_share__/import/media/{rel}"),
                "status": "stored",
                "sha256": e["sha256"],
                "ext": ext,
            }))
        })
        .collect()
}

/// `emf-docx`'s manifest as P4 produces it with the converter stubbed to fail: the EMF and
/// WMF are stored as they are, flagged `conversion_failed`.
pub fn media_manifest_failed_conversion(name: &str) -> Vec<Value> {
    media_manifest(name)
        .into_iter()
        .map(|mut e| {
            if matches!(e["ext"].as_str(), Some("emf" | "wmf")) {
                e["conversion_failed"] = json!(true);
            }
            e
        })
        .collect()
}

pub fn manifest_json(entries: &[Value]) -> String {
    serde_json::to_string(entries).unwrap()
}

/// The first 12 hex digits of a manifest entry's sha256.
pub fn sha12(entry: &Value) -> String {
    entry["sha256"].as_str().unwrap()[..12].to_string()
}

// ---------------------------------------------------------------------------
// The qmd round-trip oracle (the idea of P2's `qmd_writer_footnotes.rs`)
// ---------------------------------------------------------------------------

use pampa::pandoc::{ASTContext, Pandoc};
use quarto_core::format::Format;
use quarto_core::transform::AstTransform;
use quarto_core::transforms::FootnotesTransform;
use quarto_core::{BinaryDependencies, DocumentInfo, ProjectConfig, ProjectContext, RenderContext};

pub fn native(pandoc: &Pandoc) -> String {
    let mut buf = Vec::new();
    pampa::writers::native::write(pandoc, &ASTContext::new(), &mut buf).expect("native write");
    String::from_utf8(buf).expect("native is UTF-8")
}

pub fn write_qmd(pandoc: &Pandoc) -> String {
    let mut buf = Vec::new();
    pampa::writers::qmd::write(pandoc, &mut buf).expect("qmd write");
    String::from_utf8(buf).expect("qmd is UTF-8")
}

/// Re-read `qmd`, resolving footnotes the way every render does when `resolve_notes`.
pub fn reread(qmd: &str, resolve_notes: bool) -> Pandoc {
    let (mut pandoc, _context, _warnings) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        "<generated>",
        &mut std::io::sink(),
        true,
        None,
    )
    .unwrap_or_else(|e| panic!("regenerated qmd failed to parse: {e:?}\n--- qmd ---\n{qmd}"));
    if resolve_notes {
        let project = ProjectContext {
            dir: std::path::PathBuf::from("/project"),
            config: ProjectConfig::default(),
            is_single_file: true,
            files: vec![DocumentInfo::from_path("/project/doc.qmd")],
            output_dir: std::path::PathBuf::from("/project"),
            ..Default::default()
        };
        let document = DocumentInfo::from_path("/project/doc.qmd");
        let format = Format::docx();
        let binaries = BinaryDependencies::new();
        let mut ctx = RenderContext::new(&project, &document, &format, &binaries);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("tokio runtime");
        runtime
            .block_on(FootnotesTransform::new().transform(&mut pandoc, &mut ctx))
            .expect("footnotes transform");
    }
    pandoc
}

/// Allowances the oracle makes for spellings that differ but mean the same (as P2's).
pub fn normalize_native(text: String) -> String {
    text.replace("Plain ", "Para ").replace("[  ]", "[]")
}

/// The transformed AST, written as qmd and re-read, equals the transformed AST. Returns the
/// qmd.
#[track_caller]
pub fn assert_qmd_roundtrips(pandoc: &Pandoc, resolve_notes: bool) -> String {
    let qmd = write_qmd(pandoc);
    let again = reread(&qmd, resolve_notes);
    assert_eq!(
        normalize_native(native(pandoc)),
        normalize_native(native(&again)),
        "AST changed across the qmd round trip\n--- qmd ---\n{qmd}"
    );
    qmd
}
