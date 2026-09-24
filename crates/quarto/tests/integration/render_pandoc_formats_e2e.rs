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

/// T3.5: `--to pdf` (a non-pandoc-hybrid format) must still refuse.
/// Gfm/CommonMark — the other previously-refused formats — have their own
/// success tests below since Phase 1 admits them through the gate.
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

/// e2e (real pandoc): `q2 render f.qmd --to epub` exits 0, produces a
/// real zip whose first entry is the mandatory uncompressed `mimetype`
/// file (`application/epub+zip` — the EPUB spec requires this be the
/// literal first zip entry, stored not deflated), and whose chapter
/// XHTML (under `EPUB/text/`) contains the fixture's body string.
#[test]
fn e2e_render_epub() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("f.qmd"), "---\ntitle: F\n---\n\nHelloEpubBody.\n");

    let output = run_q2(&dir, &["f.qmd", "--to", "epub"]);
    assert!(
        output.status.success(),
        "q2 render --to epub should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("f.epub")).expect("f.epub should exist");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "epub must be a real zip");

    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("epub should be a valid zip archive");

    {
        let mut entry = zip.by_index(0).expect("epub zip should have a first entry");
        assert_eq!(
            entry.name(),
            "mimetype",
            "the EPUB spec requires `mimetype` be the first zip entry"
        );
        assert_eq!(
            entry.compression(),
            zip::CompressionMethod::Stored,
            "the EPUB `mimetype` entry must be stored, not deflated"
        );
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut entry, &mut contents).unwrap();
        assert_eq!(contents, "application/epub+zip");
    }

    let chapter_names: Vec<String> = zip
        .file_names()
        .filter(|n| n.ends_with(".xhtml") && n.contains("text"))
        .map(|n| n.to_string())
        .collect();
    assert!(
        !chapter_names.is_empty(),
        "epub should contain a chapter XHTML file under a text/ directory"
    );
    let found_body = chapter_names.iter().any(|name| {
        let mut xhtml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name(name).unwrap(), &mut xhtml).unwrap();
        xhtml.contains("HelloEpubBody")
    });
    assert!(
        found_body,
        "epub body text missing from all chapter files: {chapter_names:?}"
    );
}

/// e2e (real pandoc): `q2 render f.qmd --to epub` inlines Q1's two
/// format-default CSS files (`styles-callout.html`, `formats/epub/styles.html`,
/// vendored under `resources/formats/`) into every chapter's `<head>` via
/// `--include-in-header`, and inline math renders as real MathML (epub3's
/// `--math-method=mathml`) rather than a raw TeX/image fallback.
#[test]
fn e2e_render_epub_format_defaults() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("f.qmd"),
        "---\ntitle: F\n---\n\nInline math $x^2$ here.\n",
    );

    let output = run_q2(&dir, &["f.qmd", "--to", "epub"]);
    assert!(
        output.status.success(),
        "q2 render --to epub should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("f.epub")).expect("f.epub should exist");
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("epub should be a valid zip archive");

    let mut chapter_xhtml = String::new();
    {
        let name = zip
            .file_names()
            .find(|n| n.ends_with(".xhtml") && n.contains("text") && !n.contains("title_page"))
            .map(|n| n.to_string())
            .expect("epub should contain a non-title-page chapter XHTML file");
        std::io::Read::read_to_string(&mut zip.by_name(&name).unwrap(), &mut chapter_xhtml)
            .unwrap();
    }

    assert!(
        chapter_xhtml.contains(".callout.callout-style-simple"),
        "styles-callout.html's CSS is missing from the chapter head"
    );
    assert!(
        chapter_xhtml.contains(".quarto-layout-cell"),
        "formats/epub/styles.html's CSS is missing from the chapter head"
    );
    assert!(
        chapter_xhtml.contains("http://www.w3.org/1998/Math/MathML"),
        "inline math should render as real MathML, not a TeX/image fallback"
    );
}

/// e2e (real pandoc): `epub-chapter-level: 2` in front matter forwards to
/// Pandoc's `--split-level=2`, splitting chapter files at level-2 headings
/// too (not just level-1, Pandoc's default). Discriminates against a no-op
/// wiring: the level-1-only default produces 2 non-title-page chapter
/// files for this fixture (one level-1 heading each); `epub-chapter-level:
/// 2` produces 3 (the level-2 subsection gets its own file too).
#[test]
fn e2e_render_epub_chapter_level() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    let body = "# Chapter One\n\nBody one.\n\n## Section 1.1\n\nSub body.\n\n\
                # Chapter Two\n\nBody two.\n";

    let count_chapters = |front_matter: &str, name: &str| -> usize {
        write_file(&dir.join(name), &format!("{front_matter}\n{body}"));
        let output = run_q2(&dir, &[name, "--to", "epub"]);
        assert!(
            output.status.success(),
            "q2 render {name} --to epub should succeed; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bytes = std::fs::read(dir.join(name).with_extension("epub")).unwrap();
        let cursor = std::io::Cursor::new(bytes);
        let zip = zip::ZipArchive::new(cursor).expect("epub should be a valid zip archive");
        zip.file_names()
            .filter(|n| n.ends_with(".xhtml") && n.contains("text") && !n.contains("title_page"))
            .count()
    };

    let default_count = count_chapters("---\ntitle: Split\n---\n", "default.qmd");
    let level2_count = count_chapters(
        "---\ntitle: Split\nformat:\n  epub:\n    epub-chapter-level: 2\n---\n",
        "level2.qmd",
    );

    assert_eq!(
        default_count, 2,
        "default split-level should yield 2 chapters (one per H1)"
    );
    assert_eq!(
        level2_count, 3,
        "epub-chapter-level: 2 should also split at the H2, yielding 3 chapters"
    );
}

/// e2e (real pandoc): `epub-cover-image: cover.png` in front matter
/// forwards to `--epub-cover-image=<resolved path>`, and Pandoc marks the
/// resulting manifest item `properties="cover-image"` in the OPF package
/// document — the discriminating signal a cover image was actually wired,
/// not just present as an unrelated media file.
#[test]
fn e2e_render_epub_cover_image() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("f.qmd"),
        "---\ntitle: F\nepub-cover-image: cover.png\n---\n\nBody.\n",
    );
    std::fs::write(dir.join("cover.png"), tiny_png_bytes()).unwrap();

    let output = run_q2(&dir, &["f.qmd", "--to", "epub"]);
    assert!(
        output.status.success(),
        "q2 render --to epub should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("f.epub")).unwrap();
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("epub should be a valid zip archive");
    let mut opf = String::new();
    {
        let name = zip
            .file_names()
            .find(|n| n.ends_with("content.opf"))
            .map(|n| n.to_string())
            .expect("epub should contain an OPF package document");
        std::io::Read::read_to_string(&mut zip.by_name(&name).unwrap(), &mut opf).unwrap();
    }
    assert!(
        opf.contains("cover-image"),
        "OPF manifest should mark the cover image item: {opf}"
    );
}

/// e2e (real pandoc): a document-relative `css: style.css` in front
/// matter forwards to `--css=<resolved path>`, and the file's content
/// lands verbatim in the epub's embedded stylesheet (linked from every
/// chapter) — distinguishing `--css` (a linked stylesheet *resource*
/// resolved against the document's own directory) from
/// `--include-in-header` (inlined content, Task 2's own CSS files).
#[test]
fn e2e_render_epub_css() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("f.qmd"),
        "---\ntitle: F\ncss: style.css\n---\n\nBody.\n",
    );
    write_file(
        &dir.join("style.css"),
        ".epub-css-probe-marker { color: red; }\n",
    );

    let output = run_q2(&dir, &["f.qmd", "--to", "epub"]);
    assert!(
        output.status.success(),
        "q2 render --to epub should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("f.epub")).unwrap();
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("epub should be a valid zip archive");
    let found = zip
        .file_names()
        .filter(|n| n.ends_with(".css"))
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .iter()
        .any(|name| {
            let mut contents = String::new();
            std::io::Read::read_to_string(&mut zip.by_name(name).unwrap(), &mut contents).unwrap();
            contents.contains("epub-css-probe-marker")
        });
    assert!(
        found,
        "epub-css-probe-marker missing from every embedded .css file"
    );
}

/// e2e (real pandoc): exercises the already-vendored `isEpubOutput()`-gated
/// Lua renderers through a real epub render (Phase 1's "exercise, don't
/// port" bullet — `customnodes/callout.lua:139-141`,
/// `crossref/sections.lua:48`). A callout renders with the dedicated
/// epub/revealjs Callout renderer's classes (matching
/// `resources/formats/html/styles-callout.html`'s selectors, not Q2's
/// native HTML callout markup), and a figure crossref numbers and
/// resolves correctly end to end.
///
/// **Not covered here** (see the plan's Phase 1 notes for why):
/// `customnodes/panel-tabset.lua:264`'s `isEpubOutput()` tabset branch is
/// unreachable — `isHtmlOutput()` (checked first in the same `elseif`
/// chain) already classifies epub as HTML-family
/// (`pandoc/datadir/_format.lua:150-160`), so epub gets the interactive
/// `tabbyTabs()` markup instead of the static `render_tabset_with_l4_headings`
/// fallback. This is Q1's own byte-identical behavior, not a Q2 defect —
/// flagged for the user, not fixed unilaterally, since the shared Lua file
/// also drives revealjs/latex/docx. Section-numbering's `isEpubOutput()`
/// gate (`crossref/sections.lua:48`) is unverifiable in isolation: Q2 has
/// no general section-numbering yet (bd-5aklrxgi, explicitly out of scope
/// for this plan), so both HTML and epub already render unnumbered
/// headings for unrelated reasons.
#[test]
fn e2e_render_epub_callout_and_crossref() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("f.qmd"),
        "---\ntitle: F\n---\n\n\
         ::: {.callout-note}\n## Note Title\n\nThis is a note callout.\n:::\n\n\
         ![A figure caption](fig.png){#fig-one}\n\n\
         See @fig-one for details.\n",
    );
    std::fs::write(dir.join("fig.png"), tiny_png_bytes()).unwrap();

    let output = run_q2(&dir, &["f.qmd", "--to", "epub"]);
    assert!(
        output.status.success(),
        "q2 render --to epub should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("f.epub")).unwrap();
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).expect("epub should be a valid zip archive");
    let mut chapter_xhtml = String::new();
    {
        let name = zip
            .file_names()
            .find(|n| n.ends_with(".xhtml") && n.contains("text") && !n.contains("title_page"))
            .map(|n| n.to_string())
            .expect("epub should contain a non-title-page chapter XHTML file");
        std::io::Read::read_to_string(&mut zip.by_name(&name).unwrap(), &mut chapter_xhtml)
            .unwrap();
    }

    assert!(
        chapter_xhtml.contains("callout callout-note callout-titled"),
        "callout should render via the dedicated epub/revealjs Callout renderer: {chapter_xhtml}"
    );
    // The crossref numbering uses a non-breaking space (U+00A0) between
    // "Figure" and the number, not a plain ASCII space.
    assert!(
        chapter_xhtml.contains("Figure\u{a0}1: A figure caption"),
        "figure crossref should be numbered: {chapter_xhtml}"
    );
    assert!(
        chapter_xhtml.contains("class=\"quarto-xref\">Figure\u{a0}1</a>"),
        "@fig-one reference should resolve to the numbered figure: {chapter_xhtml}"
    );
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

/// A minimal valid docx built by hand (real pandoc, `--reference-doc`
/// pass-through) whose `word/styles.xml` carries a style name unique
/// enough that its presence in another docx's output can only mean the
/// reference doc's styles were actually forwarded — per the task's
/// vacuity note, "the render succeeded with --reference-doc" is
/// non-discriminating (pandoc 3.11 succeeds identically whether or not
/// the reference doc's styles are used).
fn build_reference_doc_with_unique_style(dest: &Path, style_name: &str) {
    let temp = TempDir::new().unwrap();
    let src_dir = canonical(temp.path());
    write_file(&src_dir.join("base.qmd"), "Base content.\n");
    let base_docx = src_dir.join("base.docx");
    let status = Command::new(Q2_BIN)
        .current_dir(&src_dir)
        .arg("render")
        .arg("base.qmd")
        .arg("--to")
        .arg("docx")
        .status()
        .expect("spawn q2 to build the base reference doc");
    assert!(status.success(), "failed to build the base reference doc");

    // Unzip, inject a uniquely-named style into styles.xml, re-zip. This
    // is simpler and more robust across pandoc versions than hand-authoring
    // a whole docx from scratch.
    let bytes = std::fs::read(&base_docx).unwrap();
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).unwrap();

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
        if name == "word/styles.xml" {
            let text = String::from_utf8(buf).unwrap();
            let injected = format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"{style_name}\">\
                 <w:name w:val=\"{style_name}\"/></w:style></w:styles>"
            );
            let text = text.replace("</w:styles>", &injected);
            entries.push((name, text.into_bytes()));
        } else {
            entries.push((name, buf));
        }
    }

    let out_file = std::fs::File::create(dest).unwrap();
    let mut writer = zip::ZipWriter::new(out_file);
    for (name, buf) in entries {
        writer
            .start_file(&name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, &buf).unwrap();
    }
    writer.finish().unwrap();
}

/// T4.6 (real binary + real pandoc): `q2 render ref.qmd --to docx` with a
/// `reference-doc:` whose `word/styles.xml` carries a uniquely-named style
/// — the style must appear in the rendered output's `word/styles.xml`.
#[test]
fn e2e_reference_doc_forwarded() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    build_reference_doc_with_unique_style(&dir.join("custom-ref.docx"), "T4P6QuartoCustomStyle");
    write_file(
        &dir.join("ref.qmd"),
        "---\ntitle: Ref\nreference-doc: custom-ref.docx\n---\n\nBody.\n",
    );

    let output = run_q2(&dir, &["ref.qmd", "--to", "docx"]);
    assert!(
        output.status.success(),
        "q2 render --to docx with reference-doc should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("ref.docx")).expect("ref.docx should exist");
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).unwrap();
    let mut styles_xml = String::new();
    std::io::Read::read_to_string(
        &mut zip.by_name("word/styles.xml").unwrap(),
        &mut styles_xml,
    )
    .unwrap();
    assert!(
        styles_xml.contains("T4P6QuartoCustomStyle"),
        "reference-doc's style did not reach the rendered output"
    );
}

/// T4.7 (real binary, missing-path diagnostic): `q2 render f.qmd --to
/// docx` with `reference-doc: missing.docx` exits non-zero, and stderr
/// names the missing file **and** carries a source span pointing at the
/// `reference-doc:` line — the span is the discriminator; without the
/// Q2-side diagnostic, pandoc itself exits 99 with a bare, span-free
/// line naming the file.
#[test]
fn e2e_missing_reference_doc_diagnosed() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("f.qmd"),
        "---\ntitle: F\nreference-doc: missing.docx\n---\n\nBody.\n",
    );

    let output = run_q2(&dir, &["f.qmd", "--to", "docx"]);
    assert!(
        !output.status.success(),
        "render must fail when reference-doc does not exist"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing.docx"),
        "expected the missing filename on stderr: {stderr}"
    );
    assert!(
        stderr.contains("Q-5-30"),
        "expected the Q-5-30 diagnostic code: {stderr}"
    );
    assert!(
        stderr.contains("f.qmd"),
        "expected a source span naming f.qmd: {stderr}"
    );
    assert!(
        !dir.join("f.docx").exists(),
        "no partial docx should be left on disk"
    );
}

/// T4.9: `--to latex` still refuses cleanly — the latex stub
/// (`pandoc_formats::latex`) adds no `FormatIdentifier::Latex` variant.
#[test]
fn e2e_latex_still_unknown_format() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(&dir.join("f.qmd"), "---\ntitle: F\n---\n\nBody.\n");

    let output = run_q2(&dir, &["f.qmd", "--to", "latex"]);
    assert!(
        !output.status.success(),
        "q2 render --to latex must still fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Unknown format: latex"),
        "unexpected refusal message: {stderr}"
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
fn zip_entry_text(bytes: &[u8], name: &str) -> String {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut zip = zip::ZipArchive::new(cursor).expect("output should be a valid zip archive");
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut zip
            .by_name(name)
            .unwrap_or_else(|_| panic!("output should contain {name}")),
        &mut text,
    )
    .unwrap();
    text
}

/// T6.3 (real binary + real pandoc): `q2 render meta.qmd --to docx` with
/// `title: My Title` / `author: Alice` puts the title in
/// `docProps/core.xml` **and** in `word/document.xml` as a
/// `w:pStyle w:val="Title"`-styled paragraph — both surfaces, because
/// they come from different pandoc code paths (a `Meta.title` that
/// reaches core props but not the body, or vice versa, is a real partial
/// failure a single assertion would miss). The value must be asserted,
/// not just the element's presence: **(measured)** pandoc emits
/// `<dc:title></dc:title>` (empty) when `Meta` carries no title at all.
#[test]
fn e2e_docx_meta_title() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("meta.qmd"),
        "---\ntitle: My Title\nauthor: Alice\ndate: 2026-01-02\n---\n\nBody.\n",
    );

    let output = run_q2(&dir, &["meta.qmd", "--to", "docx"]);
    assert!(
        output.status.success(),
        "q2 render --to docx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("meta.docx")).expect("meta.docx should exist");
    let core_xml = zip_entry_text(&bytes, "docProps/core.xml");
    assert!(
        core_xml.contains("<dc:title>My Title</dc:title>"),
        "docProps/core.xml missing the title value: {core_xml}"
    );
    assert!(
        core_xml.contains("<dc:creator>Alice</dc:creator>"),
        "docProps/core.xml missing the author value: {core_xml}"
    );

    let document_xml = zip_entry_text(&bytes, "word/document.xml");
    assert!(
        document_xml.contains(r#"w:pStyle w:val="Title""#) && document_xml.contains("My Title"),
        "word/document.xml missing a Title-styled paragraph containing the title text"
    );
}

/// T6.4 (real binary + real pandoc): same, pptx — `docProps/core.xml`
/// carries the title.
#[test]
fn e2e_pptx_meta_title() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("meta.qmd"),
        "---\ntitle: My Title\nauthor: Alice\n---\n\nBody.\n",
    );

    let output = run_q2(&dir, &["meta.qmd", "--to", "pptx"]);
    assert!(
        output.status.success(),
        "q2 render --to pptx should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("meta.pptx")).expect("meta.pptx should exist");
    let core_xml = zip_entry_text(&bytes, "docProps/core.xml");
    assert!(
        core_xml.contains("<dc:title>My Title</dc:title>"),
        "docProps/core.xml missing the title value: {core_xml}"
    );
}

/// Phase 1 gate reachability: `q2 render f.qmd --to gfm` exits 0 and
/// produces **markdown**, not HTML — the body text is present as plain
/// text and no `<h1` tag appears, which discriminates against an
/// accidental fall-through to the native HTML writer under the `.md`
/// output extension.
#[test]
fn e2e_render_gfm() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("f.qmd"),
        "---\ntitle: F\n---\n\n# Head\n\nHelloGfmBody.\n",
    );

    let output = run_q2(&dir, &["f.qmd", "--to", "gfm"]);
    assert!(
        output.status.success(),
        "q2 render --to gfm should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let md = std::fs::read_to_string(dir.join("f.md")).expect("f.md should exist");
    assert!(md.contains("HelloGfmBody"), "gfm body text missing: {md}");
    assert!(
        md.contains("# Head"),
        "expected a markdown ATX heading, got: {md}"
    );
    assert!(!md.contains("<h1"), "gfm output must not be HTML: {md}");
}

/// Phase 1 gate reachability: `--to commonmark`, same contract as
/// `e2e_render_gfm` but through the commonmark writer arm.
#[test]
fn e2e_render_commonmark() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("f.qmd"),
        "---\ntitle: F\n---\n\n# Head\n\nHelloCommonmarkBody.\n",
    );

    let output = run_q2(&dir, &["f.qmd", "--to", "commonmark"]);
    assert!(
        output.status.success(),
        "q2 render --to commonmark should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let md = std::fs::read_to_string(dir.join("f.md")).expect("f.md should exist");
    assert!(
        md.contains("HelloCommonmarkBody"),
        "commonmark body text missing: {md}"
    );
    assert!(
        md.contains("# Head"),
        "expected a markdown ATX heading, got: {md}"
    );
    assert!(
        !md.contains("<h1"),
        "commonmark output must not be HTML: {md}"
    );
}

/// Long-tail Phase 2 gate reachability (real binary + real pandoc):
/// `q2 render f.qmd --to odt` exits 0 and writes a real ODF zip — the
/// `mimetype` entry is the ODF text-document type and the body text
/// landed in `content.xml` (both would miss if the CLI admitted the
/// format but the writer arm were wrong).
#[test]
fn e2e_render_odt() {
    let temp = TempDir::new().unwrap();
    let dir = canonical(temp.path());
    write_file(
        &dir.join("f.qmd"),
        "---\ntitle: F\n---\n\n# Head\n\nHelloOdtBody.\n",
    );

    let output = run_q2(&dir, &["f.qmd", "--to", "odt"]);
    assert!(
        output.status.success(),
        "q2 render --to odt should succeed; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = std::fs::read(dir.join("f.odt")).expect("f.odt should exist");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "odt output must be a zip");
    let mimetype = zip_entry_text(&bytes, "mimetype");
    assert_eq!(
        mimetype, "application/vnd.oasis.opendocument.text",
        "odt mimetype entry"
    );
    let content = zip_entry_text(&bytes, "content.xml");
    assert!(
        content.contains("HelloOdtBody"),
        "content.xml missing the body text"
    );
}
