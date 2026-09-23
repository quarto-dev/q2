/*
 * tests/integration/conditional_content_pandoc_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * P8 Task 4 — the docx/pptx smoke fixture for `.content-visible` /
 * `.content-hidden`, driven through the real `q2` binary. Deferred
 * from P8's own crate until P7-foundation's Task 3 (the `render.rs`
 * gate relaxation) and P7's Task 4 (the docx/pptx invocation builder)
 * both landed. See `claude-notes/plans/2026-09-18-pandoc-hybrid-P8-implementation.md`
 * Task 4.
 */

//! `T4.1`/`T4.2` from the P8 implementation companion. Lives in the
//! `quarto` package (not `quarto-core`, where the rest of P8's tests
//! live) because `env!("CARGO_BIN_EXE_q2")` is only available to
//! integration tests of the package that defines the `q2` binary
//! target — `quarto`, per `crates/quarto/Cargo.toml`'s `[[bin]] name =
//! "q2"`. Mirrors the `Q2_BIN` + zip-inspection pattern already used by
//! `render_pandoc_formats_e2e.rs` and the `.content-visible` CLI
//! coverage in `conditional_content_cli.rs`, rather than the plan's
//! originally-stated `crates/quarto-core/tests/...` path, which would
//! not compile (no such binary target in that package).

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

/// One `.content-visible when-format="docx"` block (`DOCX-ONLY-SENTINEL`),
/// one `.content-visible when-format="pptx"` block (`PPTX-ONLY-SENTINEL`),
/// one `.content-hidden when-format="docx"` block (`NOT-IN-DOCX-SENTINEL`),
/// and one unconditional paragraph (`ALWAYS-SENTINEL`) — the
/// transform-independent positive control.
const FIXTURE: &str = "::: {.content-visible when-format=\"docx\"}\nDOCX-ONLY-SENTINEL\n:::\n\n\
     ::: {.content-visible when-format=\"pptx\"}\nPPTX-ONLY-SENTINEL\n:::\n\n\
     ::: {.content-hidden when-format=\"docx\"}\nNOT-IN-DOCX-SENTINEL\n:::\n\n\
     ALWAYS-SENTINEL\n";

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// Run `q2 render <args...>` from `cwd`.
fn run_q2(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

fn zip_member_text(bytes: &[u8], member: &str) -> String {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut zip = zip::ZipArchive::new(cursor).expect("output should be a valid zip archive");
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut zip
            .by_name(member)
            .unwrap_or_else(|e| panic!("{member} should exist in the archive: {e}")),
        &mut text,
    )
    .unwrap();
    text
}

/// T4.1 (real binary + real pandoc): `q2 render <fixture>.qmd --to docx`
/// exits 0 and writes a non-empty docx whose `word/document.xml`
/// contains `DOCX-ONLY-SENTINEL` and `ALWAYS-SENTINEL`, and neither
/// `NOT-IN-DOCX-SENTINEL` nor `PPTX-ONLY-SENTINEL`.
#[test]
fn e2e_docx_content_hidden_gating() {
    let temp = TempDir::new().unwrap();
    let dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());
    write_file(&dir.join("cond.qmd"), FIXTURE);

    let output = run_q2(&dir, &["cond.qmd", "--to", "docx"]);
    assert!(
        output.status.success(),
        "q2 render --to docx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let docx_path = dir.join("cond.docx");
    let bytes = std::fs::read(&docx_path).expect("cond.docx should exist");
    assert!(!bytes.is_empty(), "cond.docx must be non-empty");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "docx output should be a zip");

    let xml = zip_member_text(&bytes, "word/document.xml");
    assert!(
        xml.contains("DOCX-ONLY-SENTINEL"),
        "when-format=docx content must survive in docx output: {xml}"
    );
    assert!(
        xml.contains("ALWAYS-SENTINEL"),
        "unconditional content must survive (positive control): {xml}"
    );
    assert!(
        !xml.contains("NOT-IN-DOCX-SENTINEL"),
        "content-hidden when-format=docx must not survive in docx output: {xml}"
    );
    assert!(
        !xml.contains("PPTX-ONLY-SENTINEL"),
        "when-format=pptx content must not survive in docx output: {xml}"
    );
}

/// T4.2: the pptx mirror of T4.1 — `PPTX-ONLY-SENTINEL` and
/// `ALWAYS-SENTINEL` present across `ppt/slides/*.xml`,
/// `DOCX-ONLY-SENTINEL` absent.
#[test]
fn e2e_pptx_content_hidden_gating() {
    let temp = TempDir::new().unwrap();
    let dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());
    write_file(&dir.join("cond.qmd"), FIXTURE);

    let output = run_q2(&dir, &["cond.qmd", "--to", "pptx"]);
    assert!(
        output.status.success(),
        "q2 render --to pptx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let pptx_path = dir.join("cond.pptx");
    let bytes = std::fs::read(&pptx_path).expect("cond.pptx should exist");
    assert!(!bytes.is_empty(), "cond.pptx must be non-empty");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "pptx output should be a zip");

    let cursor = std::io::Cursor::new(bytes.clone());
    let mut zip = zip::ZipArchive::new(cursor).expect("pptx output should be a valid zip archive");
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
        all_slides_text.contains("PPTX-ONLY-SENTINEL"),
        "when-format=pptx content must survive in pptx output: {all_slides_text}"
    );
    assert!(
        all_slides_text.contains("ALWAYS-SENTINEL"),
        "unconditional content must survive (positive control): {all_slides_text}"
    );
    assert!(
        !all_slides_text.contains("DOCX-ONLY-SENTINEL"),
        "when-format=docx content must not survive in pptx output: {all_slides_text}"
    );
}
