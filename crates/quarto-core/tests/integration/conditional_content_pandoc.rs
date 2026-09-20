//! P8 Task 2 (T2.2/T2.3): `ConditionalContentTransform`'s resolved-Div
//! unwrapping produces an AST shape real Pandoc writers accept for docx
//! and pptx.
//!
//! Deliberately does **not** go through P4's Lua-filter transport or
//! P7's `render_qmd_to_pandoc` tail: the unit under test is the AST shape
//! `ConditionalContentTransform` produces, and a conditional-content-only
//! fixture contains no `CustomNode`, so a plain `pandoc -f json -t <fmt>`
//! invocation (no `-L main.lua`, no `QUARTO_SHARE_PATH`) is the lowest
//! faithful tier. See `claude-notes/plans/2026-09-18-pandoc-hybrid-P8-implementation.md`
//! Task 2.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use quarto_core::format::Format;
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::transform::AstTransform;
use quarto_core::transforms::ConditionalContentTransform;

/// One `.content-visible`/`.content-hidden` visible + hidden Div pair,
/// each `when-format`-gated on the format under test, plus an
/// unconditional paragraph — the transform-independent positive control.
fn fixture(target_format: &str) -> String {
    format!(
        "::: {{.content-visible when-format=\"{target_format}\"}}\nVISIBLE-SENTINEL\n:::\n\n\
         ::: {{.content-hidden when-format=\"{target_format}\"}}\nHIDDEN-SENTINEL\n:::\n\n\
         ALWAYS-SENTINEL\n"
    )
}

/// Unlike `pandoc_filters::harness::assert_pandoc_available` (calibrated
/// for the Lua-filter transport tests' 3.10 floor), this test uses no
/// Lua filter and no `QUARTO_SHARE_PATH` -- a bare `pandoc -f json -t
/// docx`/`pptx` invocation, which the oracle range's floor of 3.6
/// (`crates/pampa/tests/integration/test.rs`) already covers. Requiring
/// the higher floor here would fail on an otherwise-adequate local
/// pandoc for a version constraint this test does not actually have.
fn assert_pandoc_available() {
    let output = Command::new("pandoc")
        .arg("--version")
        .output()
        .expect("pandoc must be installed and on PATH to run this test");
    assert!(
        output.status.success(),
        "pandoc --version exited with {:?}",
        output.status.code()
    );
}

/// Parse `qmd`, run the real `ConditionalContentTransform` with a real
/// `Format::from_format_string(target_format)`, and serialize the result
/// to Pandoc JSON via the production streaming writer
/// (`pampa::writers::json::write`).
fn run_conditional_content(qmd: &str, target_format: &str) -> String {
    let (mut ast, ast_context, _warnings) = pampa::readers::qmd::read(
        qmd.as_bytes(),
        false,
        "<fixture>",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("qmd parse");

    let project = ProjectContext {
        dir: std::path::PathBuf::from("/p"),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![],
        output_dir: std::path::PathBuf::from("/p"),
        ..Default::default()
    };
    let doc = DocumentInfo::from_path("/p/t.qmd");
    let format = Format::from_format_string(target_format)
        .unwrap_or_else(|e| panic!("Format::from_format_string({target_format:?}): {e}"));
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &doc, &format, &binaries);

    pollster::block_on(ConditionalContentTransform::new().transform(&mut ast, &mut ctx))
        .expect("conditional-content transform");

    let mut buf = Vec::new();
    pampa::writers::json::write(&ast, &ast_context, &mut buf).expect("json write");
    String::from_utf8(buf).expect("writer output is valid UTF-8")
}

/// Feed `json` to a real, plain `pandoc -f json -t <to_format>` and return
/// the produced file's bytes. No `-L`, no share directory: this is
/// intentionally the bare transport, since the fixture carries no
/// `CustomNode`.
fn pandoc_json_to_bytes(json: &str, to_format: &str) -> Vec<u8> {
    let out_file = tempfile::Builder::new()
        .suffix(match to_format {
            "docx" => ".docx",
            "pptx" => ".pptx",
            other => panic!("unsupported to_format in this harness: {other}"),
        })
        .tempfile()
        .expect("failed to create temp output file");
    let out_path = out_file.path().to_path_buf();

    let mut child = Command::new("pandoc")
        .arg("-f")
        .arg("json")
        .arg("-t")
        .arg(to_format)
        .arg("-o")
        .arg(&out_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start pandoc process");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(json.as_bytes())
        .expect("failed to write JSON to pandoc stdin");
    let output = child.wait_with_output().expect("failed to wait for pandoc");
    assert!(
        output.status.success(),
        "pandoc -f json -t {to_format} exited with {:?}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(&out_path).expect("pandoc output file should exist");
    assert!(!bytes.is_empty(), "pandoc output file should be non-empty");
    bytes
}

/// Extract one member's text content from a docx/pptx zip archive.
fn zip_member_text(bytes: &[u8], member: &str) -> String {
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("output should be a valid ZIP archive");
    let mut file = zip
        .by_name(member)
        .unwrap_or_else(|e| panic!("{member} should exist in the archive: {e}"));
    let mut text = String::new();
    file.read_to_string(&mut text)
        .unwrap_or_else(|e| panic!("{member} should be valid UTF-8: {e}"));
    text
}

/// T2.2: a resolved conditional-content document, rendered to docx, keeps
/// the visible content and the unconditional control, and drops the
/// hidden content -- with the transform having actually run (no
/// `content-hidden` string survives in the pre-pandoc JSON) and no stray
/// empty `Div` node reaching pandoc.
#[test]
fn resolved_conditional_content_renders_correctly_to_docx() {
    assert_pandoc_available();

    let qmd = fixture("docx");
    let json = run_conditional_content(&qmd, "docx");

    assert!(
        !json.contains("content-hidden") && !json.contains("content-visible"),
        "marker classes must not survive into the pre-pandoc JSON: {json}"
    );
    assert!(
        !json.contains("\"t\":\"Div\""),
        "no stray Div node should reach pandoc -- both fixture Divs are bare \
         wrappers and should be fully resolved (spliced or dropped): {json}"
    );

    let bytes = pandoc_json_to_bytes(&json, "docx");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "docx output should be a zip");

    let xml = zip_member_text(&bytes, "word/document.xml");
    assert!(
        xml.contains("VISIBLE-SENTINEL"),
        "when-format=docx content must survive in docx output"
    );
    assert!(
        xml.contains("ALWAYS-SENTINEL"),
        "unconditional content must survive (positive control)"
    );
    assert!(
        !xml.contains("HIDDEN-SENTINEL"),
        "content-hidden when-format=docx must not survive in docx output"
    );
}

/// T2.3: the pptx mirror of T2.2.
#[test]
fn resolved_conditional_content_renders_correctly_to_pptx() {
    assert_pandoc_available();

    let qmd = fixture("pptx");
    let json = run_conditional_content(&qmd, "pptx");

    assert!(
        !json.contains("content-hidden") && !json.contains("content-visible"),
        "marker classes must not survive into the pre-pandoc JSON: {json}"
    );
    assert!(
        !json.contains("\"t\":\"Div\""),
        "no stray Div node should reach pandoc: {json}"
    );

    let bytes = pandoc_json_to_bytes(&json, "pptx");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "pptx output should be a zip");

    let cursor = std::io::Cursor::new(&bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("pptx output should be a valid ZIP archive");
    let slide_names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .collect();
    assert!(
        !slide_names.is_empty(),
        "pptx output should contain at least one ppt/slides/slideN.xml"
    );

    let mut all_slides_text = String::new();
    for name in &slide_names {
        all_slides_text.push_str(&zip_member_text(&bytes, name));
    }

    assert!(
        all_slides_text.contains("VISIBLE-SENTINEL"),
        "when-format=pptx content must survive in pptx output"
    );
    assert!(
        all_slides_text.contains("ALWAYS-SENTINEL"),
        "unconditional content must survive (positive control)"
    );
    assert!(
        !all_slides_text.contains("HIDDEN-SENTINEL"),
        "content-hidden when-format=pptx must not survive in pptx output"
    );
}
