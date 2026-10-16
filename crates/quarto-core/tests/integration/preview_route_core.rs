/*
 * tests/integration/preview_route_core.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * q2-preview hub-parity Plan 9: the core options the preview routes drive
 * (`attribution_json`) and the odt baseline (9.1). Tests needing `pandoc`
 * skip when it is absent.
 */

use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::render_to_file::{RenderToFileOptions, render_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

/// A 1x1 transparent PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xff, 0xff, 0x3f,
    0x00, 0x05, 0xfe, 0x02, 0xfe, 0xa7, 0x35, 0x81, 0x84, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn have_pandoc() -> bool {
    Command::new("pandoc")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn zip_entries(path: &Path) -> zip::ZipArchive<std::fs::File> {
    zip::ZipArchive::new(std::fs::File::open(path).unwrap()).expect("a zip")
}

/// 9.1 baseline: a native odt render into a directory outside the source
/// tree (as the route does) is a valid odt that carries the image.
#[test]
fn odt_render_into_another_directory_keeps_its_image() {
    if !have_pandoc() {
        eprintln!("SKIP: pandoc is required");
        return;
    }
    let proj = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    write(&root.join("img/dot.png"), PNG);
    write(
        &root.join("a.qmd"),
        "---\ntitle: T\n---\n\nHello\n\n![dot](img/dot.png)\n",
    );
    let options = RenderToFileOptions {
        output_dir: Some(out.path().to_path_buf()),
        quiet: true,
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&root.join("a.qmd"), "odt", &options, runtime)
        .unwrap_or_else(|e| panic!("render failed: {e}"));
    assert_eq!(result.output_path.extension().unwrap(), "odt");
    let mut zip = zip_entries(&result.output_path);
    let mut mimetype = String::new();
    zip.by_name("mimetype")
        .expect("an odt has a mimetype entry")
        .read_to_string(&mut mimetype)
        .unwrap();
    assert_eq!(mimetype, "application/vnd.oasis.opendocument.text");
    let pictures: Vec<String> = zip
        .file_names()
        .map(|n| n.unwrap().to_string())
        .filter(|n| n.starts_with("Pictures/"))
        .collect();
    assert!(!pictures.is_empty(), "no Pictures/ entry in the odt");
}

/// 9.7: `attribution_json` stamps the payload's author on a docx's
/// track-changes marks, with no git involved.
#[test]
fn attribution_json_stamps_the_payload_author_in_a_docx() {
    if !have_pandoc() {
        eprintln!("SKIP: pandoc is required");
        return;
    }
    let proj = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    let root = proj.path().canonicalize().unwrap();
    let qmd = "---\ntitle: T\n---\n\nIt was [++ added] here.\n";
    write(&root.join("a.qmd"), qmd);
    let json = serde_json::json!({
        "runs": [{ "start": 0, "end": qmd.len(), "actor": "alice@example.com", "time": 1_700_000_000_000_i64 }],
        "identities": { "alice@example.com": { "name": "Alice Payload", "color": "#ff0000" } }
    })
    .to_string();
    let options = RenderToFileOptions {
        output_dir: Some(out.path().to_path_buf()),
        quiet: true,
        attribution_json: Some(json),
        ..Default::default()
    };
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_to_file(&root.join("a.qmd"), "docx", &options, runtime)
        .unwrap_or_else(|e| panic!("render failed: {e}"));
    let mut xml = String::new();
    zip_entries(&result.output_path)
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("Alice Payload"), "{xml}");
}
