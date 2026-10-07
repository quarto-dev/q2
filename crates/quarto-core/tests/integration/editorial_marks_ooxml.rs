/*
 * tests/integration/editorial_marks_ooxml.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Document-import P6 (I23), with real pandoc: the editorial-marks export rendered through the
 * native docx/pptx pipeline, then the OOXML read back.
 *
 * - T6: the extension's eight examples with the expectations ported from its
 *   `tests/run-tests.py` (`EXPECTATIONS`), and the comments read back by pandoc;
 * - T6b: the I4 commented range, and coexistence with the extension;
 * - T7: the round trip through import (I20).
 *
 * These tests hard-fail when pandoc is missing, like the other real-pandoc tests.
 */

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use regex::Regex;
use tempfile::TempDir;

use quarto_core::import::finish_import;
use quarto_core::render_to_file::{RenderToFileOptions, render_document_to_file};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

use super::import_support as support;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/editorial-marks-ooxml")
}

fn render(dir: &Path, name: &str, qmd: &str, format: &str) -> PathBuf {
    let input = dir.join(name);
    std::fs::write(&input, qmd).unwrap();
    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let result = render_document_to_file(
        &input,
        format,
        &RenderToFileOptions::default(),
        None,
        runtime,
        None,
        None,
        None,
    )
    .unwrap_or_else(|e| panic!("{name} to {format}: {e:?}"));
    result.output_path
}

fn zip_parts(path: &Path) -> Vec<(String, Vec<u8>)> {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).expect("a ZIP archive");
    (0..zip.len())
        .map(|i| {
            let mut entry = zip.by_index(i).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (entry.name().to_string(), bytes)
        })
        .collect()
}

fn part(parts: &[(String, Vec<u8>)], name: &str) -> Option<String> {
    parts
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, b)| String::from_utf8(b.clone()).unwrap())
}

fn count(pattern: &str, text: &str) -> usize {
    Regex::new(pattern).unwrap().find_iter(text).count()
}

fn captures(pattern: &str, text: &str) -> Vec<String> {
    let mut found: Vec<String> = Regex::new(pattern)
        .unwrap()
        .captures_iter(text)
        .map(|c| c[1].to_string())
        .collect();
    found.sort();
    found
}

fn assert_well_formed(parts: &[(String, Vec<u8>)], label: &str) {
    for (name, bytes) in parts {
        if name.ends_with(".xml") || name.ends_with(".rels") {
            let text = std::str::from_utf8(bytes).unwrap();
            roxmltree::Document::parse(text)
                .unwrap_or_else(|e| panic!("{label}: {name} is not well-formed XML: {e}"));
        }
    }
}

/// `pandoc -f docx --track-changes=all -t json`: what pandoc's reader makes of the docx.
fn pandoc_reads_docx(docx: &Path) -> String {
    let output = Command::new("pandoc")
        .args(["-f", "docx", "--track-changes=all", "-t", "json"])
        .arg(docx)
        .output()
        .expect("pandoc must be installed for these tests");
    assert!(
        output.status.success(),
        "pandoc -f docx failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

// ---------------------------------------------------------------------------
// T6: the extension's examples
// ---------------------------------------------------------------------------

/// Expectations ported from the extension's `tests/run-tests.py` (`EXPECTATIONS`,
/// lines 118-172), one row per fixture. `highlight` counts runs, not marks.
struct DocxExpect {
    file: &'static str,
    ins: usize,
    del: usize,
    /// `None` where the extension doesn't assert it.
    highlight: Option<usize>,
    comments: usize,
    authors: Option<&'static [&'static str]>,
    comment_authors: Option<&'static [&'static str]>,
}

const DOCX_EXPECTATIONS: &[DocxExpect] = &[
    DocxExpect {
        file: "01-inline-marks.qmd",
        ins: 2,
        del: 2,
        highlight: Some(2),
        comments: 2,
        authors: None,
        comment_authors: None,
    },
    // The one changed row: the extension asserts `comment_ids=["comment-id"]` (the mark's own
    // id verbatim); P6 numbers comments 0, 1, ... because OOXML's `w:id` is an integer, so
    // the assertion becomes the count and the integer check below.
    DocxExpect {
        file: "02-inline-marks-attributes.qmd",
        ins: 2,
        del: 2,
        highlight: Some(1),
        comments: 2,
        authors: None,
        comment_authors: None,
    },
    // 03 and 04 re-derived 2026-10-03 by running the extension on `pampa -t json` of each:
    // they equal `EXPECTATIONS`.
    DocxExpect {
        file: "03-block-marks.qmd",
        ins: 4,
        del: 2,
        highlight: Some(5),
        comments: 2,
        authors: None,
        comment_authors: None,
    },
    DocxExpect {
        file: "04-mixed-contexts.qmd",
        ins: 2,
        del: 3,
        highlight: Some(2),
        comments: 2,
        authors: None,
        comment_authors: None,
    },
    // Three comments, not four: the comment inside a block comment's body folds into the
    // message.
    DocxExpect {
        file: "05-comment-on-code.qmd",
        ins: 0,
        del: 0,
        highlight: Some(0),
        comments: 3,
        authors: None,
        comment_authors: None,
    },
    DocxExpect {
        file: "07-authors.qmd",
        ins: 2,
        del: 2,
        highlight: None,
        comments: 1,
        authors: Some(&["Eve", "Frank", "Gina", "Hal"]),
        comment_authors: Some(&["Ivy"]),
    },
    DocxExpect {
        file: "08-multi-author.qmd",
        ins: 3,
        del: 1,
        highlight: Some(1),
        comments: 3,
        authors: Some(&["Alice Author", "Bob Builder", "Bob Builder", "Bob Builder"]),
        comment_authors: Some(&["Bob Builder", "Carol Critic", "Dana Explicit"]),
    },
];

fn render_fixture(dir: &Path, file: &str, format: &str) -> PathBuf {
    let qmd = std::fs::read_to_string(fixture_dir().join(file)).unwrap();
    render(dir, file, &qmd, format)
}

#[test]
fn the_extensions_docx_examples_produce_its_ooxml_counts() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let comment_id = Regex::new(r#"<w:comment [^>]*w:id="([^"]*)""#).unwrap();
    for expect in DOCX_EXPECTATIONS {
        let docx = render_fixture(&dir, expect.file, "docx");
        let parts = zip_parts(&docx);
        assert_well_formed(&parts, expect.file);
        let document = part(&parts, "word/document.xml").unwrap();
        let label = expect.file;

        assert_eq!(count(r"<w:ins\b", &document), expect.ins, "{label}: w:ins");
        assert_eq!(count(r"<w:del\b", &document), expect.del, "{label}: w:del");
        if let Some(highlight) = expect.highlight {
            assert_eq!(
                count(r#"<w:highlight w:val="yellow""#, &document),
                highlight,
                "{label}: w:highlight"
            );
        }
        assert_eq!(
            count(r"<w:commentRangeStart\b", &document),
            expect.comments,
            "{label}: commentRangeStart"
        );
        assert_eq!(
            count(r"<w:commentRangeEnd\b", &document),
            expect.comments,
            "{label}: commentRangeEnd"
        );
        if let Some(authors) = expect.authors {
            assert_eq!(
                captures(r#"<w:(?:ins|del) [^>]*w:author="([^"]*)""#, &document),
                authors,
                "{label}: tracked-change authors"
            );
        }

        if expect.comments > 0 {
            let comments = part(&parts, "word/comments.xml")
                .unwrap_or_else(|| panic!("{label}: expected word/comments.xml"));
            assert_eq!(
                count(r"<w:comment\b", &comments),
                expect.comments,
                "{label}: w:comment entries"
            );
            assert!(
                part(&parts, "[Content_Types].xml")
                    .unwrap()
                    .contains("word/comments.xml"),
                "{label}: comments.xml missing from [Content_Types].xml"
            );
            assert!(
                part(&parts, "word/_rels/document.xml.rels")
                    .unwrap()
                    .contains("relationships/comments"),
                "{label}: comments relationship missing"
            );
            // New in P6: every comment has an author (pandoc's reader drops one without),
            // and every comment id is an integer, 0, 1, ... in order.
            assert_eq!(
                count(r#"<w:comment [^>]*w:author="[^"]*""#, &comments),
                expect.comments,
                "{label}: every w:comment has a w:author"
            );
            let ids: Vec<String> = comment_id
                .captures_iter(&comments)
                .map(|c| c[1].to_string())
                .collect();
            let expected_ids: Vec<String> = (0..expect.comments).map(|i| i.to_string()).collect();
            assert_eq!(ids, expected_ids, "{label}: comment ids");
            if let Some(authors) = expect.comment_authors {
                assert_eq!(
                    captures(r#"<w:comment [^>]*w:author="([^"]*)""#, &comments),
                    authors,
                    "{label}: comment authors"
                );
            }
        } else {
            assert!(
                part(&parts, "word/comments.xml").is_none_or(|c| count(r"<w:comment\b", &c) == 0),
                "{label}: unexpected comments"
            );
        }

        // pandoc reads the comments back, text and all.
        let json = pandoc_reads_docx(&docx);
        assert_eq!(
            count(r#""comment-start""#, &json),
            expect.comments,
            "{label}: comments pandoc reads back"
        );
    }
}

#[test]
fn the_extensions_pptx_example_produces_its_runs() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let pptx = render_fixture(&dir, "06-pptx-marks.qmd", "pptx");
    let parts = zip_parts(&pptx);
    assert_well_formed(&parts, "06-pptx-marks");
    let mut slides: Vec<&(String, Vec<u8>)> = parts
        .iter()
        .filter(|(n, _)| {
            Regex::new(r"^ppt/slides/slide\d+\.xml$")
                .unwrap()
                .is_match(n)
        })
        .collect();
    slides.sort();
    let combined: String = slides
        .iter()
        .map(|(_, b)| String::from_utf8(b.clone()).unwrap())
        .collect();
    assert_eq!(count(r#"u="sng""#, &combined), 1, "underline");
    assert_eq!(count(r#"strike="sngStrike""#, &combined), 1, "strike");
    assert_eq!(count(r"<a:highlight>", &combined), 1, "highlight");
    assert_eq!(count(r"C00000", &combined), 1, "comment fallback");
}

// ---------------------------------------------------------------------------
// T6b: the I4 commented range
// ---------------------------------------------------------------------------

/// `qmd` rendered to docx, read back by pandoc, and imported (P3) to qmd again.
fn through_word(dir: &Path, name: &str, qmd: &str) -> String {
    let docx = render(
        dir,
        name,
        &format!("---\nformat: docx\n---\n\n{qmd}"),
        "docx",
    );
    import_docx(&docx)
}

fn import_docx(docx: &Path) -> String {
    let json = pandoc_reads_docx(docx);
    let outcome = finish_import(&json, "", "doc.qmd", "[]", Some("docx"));
    assert!(outcome.success, "import failed: {:?}", outcome.diagnostics);
    outcome.qmd.unwrap()
}

const ANN: &str = r#"{author="Ann" date="2026-09-01T10:00:00Z"}"#;
const DEE: &str = r#"{author="Dee" date="2026-09-01T11:00:00Z"}"#;

#[test]
fn commented_ranges_survive_word_and_read_back_as_the_equal_range_shape() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    // Each is the qmd P3's import produces for its Word shape, so it must come back equal.
    let cases = [
        // a range with one comment
        format!("A [range of words[>> One.]{ANN}] here.\n"),
        // a comment with a reply (the equal-range group)
        format!("A [range with a reply[>> Parent.]{ANN}[>> Reply.]{DEE}] here.\n"),
        // a comment with two replies
        format!("A [range[>> One.]{ANN}[>> Two.]{DEE}[>> Three.]{ANN}] here.\n"),
        // a range starting with emphasis
        format!("A [*words* plain[>> One.]{ANN}] here.\n"),
        // nested ranges
        format!("A [outer [inner[>> In.]{DEE}] words[>> Out.]{ANN}] here.\n"),
        // a commented range inside an insertion
        format!(
            "A [++ [inserted range[>> One.]{ANN}] more]{{author=\"Bob\" date=\"2026-09-02T10:00:00Z\"}} here.\n"
        ),
        // a range ending in a point comment: `pt` with reply `c` (the known ambiguity)
        format!("A [text[>> Point.]{ANN}[>> Then.]{DEE}] here.\n"),
        // a point comment
        format!("A point[>> Here.]{ANN} comment.\n"),
    ];
    for (i, qmd) in cases.iter().enumerate() {
        let back = through_word(&dir, &format!("case{i}.qmd"), qmd);
        assert_eq!(&back, qmd, "case {i}");
    }
}

// ---------------------------------------------------------------------------
// T6b: coexistence with the extension
// ---------------------------------------------------------------------------

#[test]
fn a_project_that_still_lists_the_extension_renders_the_same_docx() {
    // SAFETY/scope: process-local under nextest (one process per test). pandoc stamps
    // docProps and tracked-change times from it, so the two runs can be compared.
    unsafe {
        std::env::set_var("SOURCE_DATE_EPOCH", "1700000000");
    }
    let render_project = |with_extension: bool| -> Vec<(String, Vec<u8>)> {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().canonicalize().unwrap();
        let mut config = String::from("project:\n  type: default\n");
        if with_extension {
            config.push_str("\nfilters:\n  - quarto-ooxml-editorial-marks\n");
            copy_dir(
                &fixture_dir().join("extension/_extensions"),
                &dir.join("_extensions"),
            );
        }
        std::fs::write(dir.join("_quarto.yml"), config).unwrap();
        let mut out = Vec::new();
        for expect in DOCX_EXPECTATIONS {
            let docx = render_fixture(&dir, expect.file, "docx");
            let parts = zip_parts(&docx);
            for name in ["word/document.xml", "word/comments.xml"] {
                if let Some(text) = part(&parts, name) {
                    out.push((format!("{}:{name}", expect.file), text.into_bytes()));
                }
            }
        }
        out
    };
    let without = render_project(false);
    let with = render_project(true);
    assert_eq!(
        with.iter().map(|(n, _)| n).collect::<Vec<_>>(),
        without.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    let mut differing = Vec::new();
    for ((name, a), (_, b)) in with.iter().zip(&without) {
        let (a, b) = (String::from_utf8_lossy(a), String::from_utf8_lossy(b));
        if a != b {
            let at = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
            let around = |s: &str| {
                let lo = s.floor_char_boundary(at.saturating_sub(150));
                let hi = s.floor_char_boundary((at + 250).min(s.len()));
                s[lo..hi].to_string()
            };
            differing.push(format!(
                "{name} (from byte {at}):\n--- with ---\n{}\n--- without ---\n{}",
                around(&a),
                around(&b)
            ));
        }
    }
    assert!(
        differing.is_empty(),
        "differs when the extension is listed:\n{}",
        differing.join("\n\n")
    );
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
// T7: the round trip through import (I20)
// ---------------------------------------------------------------------------

fn first_import(name: &str) -> String {
    let outcome = finish_import(
        &support::pandoc_json(name),
        &support::stderr(name),
        "doc.qmd",
        "[]",
        Some("docx"),
    );
    assert!(outcome.success, "{name}: {:?}", outcome.diagnostics);
    outcome.qmd.unwrap()
}

/// docx -> qmd -> docx (this transform, real pandoc) -> `pandoc -f docx` -> import: the marks,
/// their authors and dates, the comment ranges and the reply groups equal those of the first
/// import. No exceptions at the qmd level: the shapes that don't invert to Word already took
/// their qmd form at the first import and come back equal.
///
/// The fixtures between them hold each of the shapes that don't invert: a point comment
/// (`comments-edge-docx` has none mid-sentence, so the hand-written case below does), an I13
/// block comment (`track-changes-docx`: "A range spanning into[>> ...]", a comment on a range
/// crossing paragraphs, attached to the first paragraph's end), a comment at a paragraph's end
/// (the same one), and a range ending in a point comment.
#[test]
fn the_round_trip_through_import_is_stable_for_the_import_fixtures() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    for name in [
        "track-changes-docx",
        "highlights-docx",
        "comments-edge-docx",
    ] {
        let first = first_import(name);
        let docx = render(&dir, &format!("{name}.qmd"), &first, "docx");
        let second = import_docx(&docx);
        assert_eq!(
            second, first,
            "{name}: qmd changed across the Word round trip"
        );
    }
}

#[test]
fn the_shapes_that_dont_invert_come_back_equal_at_the_qmd_level() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let qmd = format!(
        "A point comment[>> Mid-sentence.]{ANN} stays one.\n\n\
         A comment at a paragraph's end.[>> Ends it.]{DEE}\n\n\
         A block comment's paragraph[>> Attached to the block.]{ANN}\n\n\
         The next paragraph here.\n\n\
         A range ending in a point comment [text[>> Point.]{ANN}[>> Reply.]{DEE}] after.\n"
    );
    let docx = render(&dir, "shapes.qmd", &qmd, "docx");
    assert_eq!(import_docx(&docx), qmd);
}
