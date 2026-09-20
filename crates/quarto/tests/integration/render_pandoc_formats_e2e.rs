/*
 * render_pandoc_formats_e2e.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * P7-foundation Task 3 — end-to-end CLI tests for the relaxed
 * `render.rs` format gate: `docx`/`pptx` now reach a real `pandoc`
 * invocation through `render_qmd_to_pandoc`.
 */

//! `T3.1`, `T3.2`, `T3.4`, `T3.5`, `T3.9`, `T3.10` from the
//! P7-foundation implementation companion. Task 4 (B3 shared
//! services) extends this file with resource-staging tests.

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

const Q2_BIN: &str = env!("CARGO_BIN_EXE_q2");

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Run `q2 render <args...>` from `cwd`. Returns the exit status and
/// captured stdout / stderr. Panics if the binary couldn't be spawned.
fn run_q2(cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(cwd);
    cmd.arg("render");
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn q2 binary")
}

/// T3.1 (real pandoc): `q2 render f.qmd --to docx` exits 0, produces a
/// real zip (`PK\x03\x04` magic), contains `word/document.xml`, and its
/// concatenated `<w:t>` text contains the fixture's body string. The
/// text assertion is what makes this more than a magic-number check —
/// see the task's vacuity note: do not strengthen this toward numbering,
/// that's P7's golden-harness job.
#[test]
fn e2e_render_docx() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("f.qmd"), "---\ntitle: F\n---\n\nHelloDocxBody.\n");

    let output = run_q2(&dir, &["f.qmd", "--to", "docx"]);
    assert!(
        output.status.success(),
        "q2 render --to docx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let docx_path = dir.join("f.docx");
    let bytes = std::fs::read(&docx_path).expect("f.docx should exist");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "docx must be a real zip");

    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("docx should be a valid zip archive");
    let mut document_xml = String::new();
    {
        let mut entry = zip
            .by_name("word/document.xml")
            .expect("docx should contain word/document.xml");
        std::io::Read::read_to_string(&mut entry, &mut document_xml).unwrap();
    }
    assert!(
        document_xml.contains("HelloDocxBody"),
        "docx body text missing from word/document.xml"
    );
}

/// T3.2 (real pandoc): `q2 render f.qmd --to pptx` exits 0, produces a
/// zip containing `ppt/slides/slide1.xml`, and its `<a:t>` text
/// contains the fixture's body string.
#[test]
fn e2e_render_pptx() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    // No front-matter `title:` — pandoc's pptx writer reserves slide1 for
    // a title slide when one is present, pushing the body paragraph to
    // slide2. Titleless input keeps the body on slide1.
    write_file(&dir.join("f.qmd"), "HelloPptxBody.\n");

    let output = run_q2(&dir, &["f.qmd", "--to", "pptx"]);
    assert!(
        output.status.success(),
        "q2 render --to pptx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("f.pptx")).expect("f.pptx should exist");
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("pptx should be a valid zip archive");
    let mut slide_xml = String::new();
    {
        let mut entry = zip
            .by_name("ppt/slides/slide1.xml")
            .expect("pptx should contain ppt/slides/slide1.xml");
        std::io::Read::read_to_string(&mut entry, &mut slide_xml).unwrap();
    }
    assert!(
        slide_xml.contains("HelloPptxBody"),
        "pptx body text missing from ppt/slides/slide1.xml"
    );
}

/// T3.4 (real pandoc + the warning wiring): a conjunction, per the
/// task's vacuity note — exit 0 **and** the `Q-18-<n>` warning **and**
/// `multi.docx` non-empty **and** `multi.html` absent. Reverting the
/// gate relaxation reddens the first clause; reverting the warning
/// wiring reddens the second; neither revert alone leaves this green.
#[test]
fn e2e_multi_format_warns() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("multi.qmd"),
        "---\ntitle: Multi\nformat:\n  docx: default\n  html: default\n---\n\nMulti body.\n",
    );

    let output = run_q2(&dir, &["multi.qmd"]);
    assert!(
        output.status.success(),
        "q2 render (no --to) should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Q-20-8"), "missing warning code: {stderr}");
    assert!(stderr.contains("docx"), "missing used key: {stderr}");
    assert!(stderr.contains("html"), "missing skipped key: {stderr}");

    let docx_len = std::fs::metadata(dir.join("multi.docx"))
        .expect("multi.docx should exist")
        .len();
    assert!(docx_len > 0, "multi.docx must be non-empty");
    assert!(
        !dir.join("multi.html").exists(),
        "multi.html must not exist — html was skipped"
    );
}

/// T3.5: `--to pdf` (and its non-Docx/Pptx siblings) must still refuse.
#[test]
fn e2e_pdf_still_refused() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("f.qmd"), "---\ntitle: F\n---\n\nBody.\n");

    let output = run_q2(&dir, &["f.qmd", "--to", "pdf"]);
    assert!(
        !output.status.success(),
        "q2 render --to pdf must still fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is not yet supported"),
        "unexpected refusal message: {stderr}"
    );
}

/// T3.9: with no `pandoc` reachable (`PATH` stripped, `QUARTO_PANDOC`
/// unset), `--to docx` exits non-zero, names the pandoc `Q-18-*` code
/// and the word "pandoc" on stderr, and leaves no partial `.docx` on
/// disk.
#[test]
fn e2e_docx_without_pandoc_on_path() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("f.qmd"), "---\ntitle: F\n---\n\nBody.\n");

    let empty_path_dir = TempDir::new().unwrap();

    let mut cmd = Command::new(Q2_BIN);
    cmd.current_dir(&dir);
    cmd.arg("render").arg("f.qmd").arg("--to").arg("docx");
    cmd.env("PATH", empty_path_dir.path());
    cmd.env_remove("QUARTO_PANDOC");
    let output = cmd.output().expect("spawn q2 binary");

    assert!(
        !output.status.success(),
        "render must fail when pandoc is unreachable"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Q-20-1"),
        "expected the pandoc-not-found code; got:\n{stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("pandoc"),
        "expected the word 'pandoc' in the diagnostic; got:\n{stderr}"
    );
    assert!(
        !dir.join("f.docx").exists(),
        "no partial docx should be left on disk"
    );
}

/// T3.10: the binary-output contract — a successful `--to docx` render
/// produces a file whose length is > 0 and which does not start with
/// `<!` (ruling out HTML bytes written to a `.docx` path — the exact
/// regression P4's `content: String::new()` decision guards against).
#[test]
fn e2e_docx_output_is_not_html() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("f.qmd"), "---\ntitle: F\n---\n\nBody.\n");

    let output = run_q2(&dir, &["f.qmd", "--to", "docx"]);
    assert!(output.status.success());

    let bytes = std::fs::read(dir.join("f.docx")).unwrap();
    assert!(!bytes.is_empty(), "docx output must be non-empty");
    assert_ne!(&bytes[..2], b"<!", "docx output must not be HTML bytes");
}

/// A real (if tiny) 1x1 PNG, reused from the `q2-preview` smoke fixture —
/// pandoc's docx writer needs valid image bytes to embed, not just a
/// present file. Bd (measured): a 0-byte file makes pandoc treat the
/// image as unfetchable, which is exactly the failure mode T4.3/T4.4
/// guard *against* seeing on a correctly-staged resource.
fn tiny_png_bytes() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/smoke-all/q2-preview/hero.png");
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

fn media_entries(bytes: &[u8]) -> Vec<String> {
    let cursor = std::io::Cursor::new(bytes);
    let zip = zip::ZipArchive::new(cursor).expect("docx should be a valid zip archive");
    zip.file_names()
        .filter(|n| n.starts_with("word/media/"))
        .map(|n| n.to_string())
        .collect()
}

fn document_rels(bytes: &[u8]) -> String {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut zip = zip::ZipArchive::new(cursor).expect("docx should be a valid zip archive");
    let mut rels = String::new();
    let mut entry = zip
        .by_name("word/_rels/document.xml.rels")
        .expect("docx should contain word/_rels/document.xml.rels");
    std::io::Read::read_to_string(&mut entry, &mut rels).unwrap();
    rels
}

/// T4.3 (real binary + real pandoc): `q2 render img.qmd --to docx` stages
/// the referenced image into `word/media/`, links it via an
/// `image`-typed relationship in `word/_rels/document.xml.rels`, and
/// emits **no** `Could not fetch resource` warning on stderr. Per the
/// task's vacuity note, "the render succeeded" alone is non-discriminating
/// here — **(measured)** pandoc 3.8.1 substitutes the image's alt text
/// and exits 0 when a resource genuinely can't be fetched. The media-
/// entry count and the absence of that stderr line are what actually
/// distinguish a staged resource from a missing one.
#[test]
fn e2e_docx_image_staged() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("img.qmd"), "![cap](sub/pic.png)\n");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/pic.png"), tiny_png_bytes()).unwrap();

    let output = run_q2(&dir, &["img.qmd", "--to", "docx"]);
    assert!(
        output.status.success(),
        "q2 render --to docx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Could not fetch resource"),
        "resource staging must not fall back to the missing-image path: {stderr}"
    );

    let bytes = std::fs::read(dir.join("img.docx")).expect("img.docx should exist");
    let media = media_entries(&bytes);
    assert_eq!(
        media.len(),
        1,
        "expected exactly one staged media entry: {media:?}"
    );

    let rels = document_rels(&bytes);
    let media_name = media[0].trim_start_matches("word/media/");
    assert!(
        rels.contains(&format!("media/{media_name}")) && rels.contains("relationships/image"),
        "expected an image-typed relationship targeting {media_name}: {rels}"
    );
}

/// T4.4: not redundant with the in-process `link_rewrite` unit tests —
/// a fix for one authored form (relative) routinely leaves the sibling
/// form (project-root-absolute, leading `/`) broken. Same fixture,
/// authored as `/sub/pic.png`.
#[test]
fn e2e_docx_image_staged_absolute_path() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("img.qmd"), "![cap](/sub/pic.png)\n");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/pic.png"), tiny_png_bytes()).unwrap();

    let output = run_q2(&dir, &["img.qmd", "--to", "docx"]);
    assert!(
        output.status.success(),
        "q2 render --to docx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Could not fetch resource"),
        "resource staging must not fall back to the missing-image path: {stderr}"
    );

    let bytes = std::fs::read(dir.join("img.docx")).expect("img.docx should exist");
    let media = media_entries(&bytes);
    assert_eq!(
        media.len(),
        1,
        "expected exactly one staged media entry: {media:?}"
    );

    let rels = document_rels(&bytes);
    let media_name = media[0].trim_start_matches("word/media/");
    assert!(
        rels.contains(&format!("media/{media_name}")) && rels.contains("relationships/image"),
        "expected an image-typed relationship targeting {media_name}: {rels}"
    );
}
