/*
 * qmd_writer_footnotes.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Multi-block footnotes through the qmd writer (document import P2, T2).
//!
//! pampa's qmd writer writes a `Note` of more than one block as a `[^nK]`
//! reference plus a fenced definition (`::: ^nK`). The pampa reader doesn't
//! turn that pair back into a `Note`: it keeps a `quarto-note-reference` span
//! and a `NoteDefinitionFencedBlock`, which quarto-core's `FootnotesTransform`
//! resolves for every output format. So the oracle lives here: pandoc-shaped
//! JSON → qmd → re-read → the real `FootnotesTransform` (under a Pandoc
//! profile, which strips the dedup marker) → the same AST as the input.
//!
//! The ASTs are compared as pampa's native text, which carries no source info.

use pampa::pandoc::Pandoc;
use pampa::{readers, writers};
use quarto_core::format::Format;
use quarto_core::transform::AstTransform;
use quarto_core::transforms::FootnotesTransform;
use quarto_core::{BinaryDependencies, DocumentInfo, ProjectConfig, ProjectContext, RenderContext};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Pandoc-shaped JSON builders
// ---------------------------------------------------------------------------

fn doc(blocks: Vec<Value>) -> Value {
    json!({"pandoc-api-version": [1, 23, 1], "meta": {}, "blocks": blocks})
}

fn str_(t: &str) -> Value {
    json!({"t": "Str", "c": t})
}

/// Words and spaces: `"two words"` → `Str Space Str`.
fn inl(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    for (i, word) in text.split(' ').enumerate() {
        if i > 0 {
            out.push(json!({"t": "Space"}));
        }
        if !word.is_empty() {
            out.push(str_(word));
        }
    }
    out
}

fn para(text: &str) -> Value {
    json!({"t": "Para", "c": inl(text)})
}

fn plain(text: &str) -> Value {
    json!({"t": "Plain", "c": inl(text)})
}

fn note(blocks: Vec<Value>) -> Value {
    json!({"t": "Note", "c": blocks})
}

fn attr(id: &str, classes: &[&str]) -> Value {
    json!([id, classes, []])
}

fn bullets(items: &[&str]) -> Value {
    json!({"t": "BulletList", "c": items.iter().map(|t| vec![plain(t)]).collect::<Vec<_>>()})
}

fn code_block(text: &str) -> Value {
    json!({"t": "CodeBlock", "c": [attr("", &[]), text]})
}

fn table(cell_text: &str) -> Value {
    let cell = |text: &str| json!([attr("", &[]), {"t": "AlignDefault"}, 1, 1, [plain(text)]]);
    let row = |text: &str| json!([attr("", &[]), [cell(text)]]);
    json!({"t": "Table", "c": [
        attr("", &[]),
        [null, []],
        [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
        [attr("", &[]), [row("head")]],
        [[attr("", &[]), 0, [], [row(cell_text)]]],
        [attr("", &[]), []]
    ]})
}

fn text_with_note(before: &str, n: Value, after: &str) -> Vec<Value> {
    let mut inlines = inl(before);
    inlines.push(n);
    inlines.extend(inl(after));
    inlines
}

// ---------------------------------------------------------------------------
// The oracle
// ---------------------------------------------------------------------------

fn read_pandoc_json(value: &Value) -> Pandoc {
    readers::json::read_completing_source_info(
        &mut value.to_string().as_bytes(),
        quarto_source_map::By::unknown(),
    )
    .expect("pandoc JSON should read")
    .0
}

fn native(pandoc: &Pandoc) -> String {
    let mut buf = Vec::new();
    writers::native::write(pandoc, &pampa::pandoc::ASTContext::new(), &mut buf)
        .expect("native write");
    String::from_utf8(buf).expect("native is UTF-8")
}

fn write_qmd(pandoc: &Pandoc) -> String {
    let mut buf = Vec::new();
    writers::qmd::write(pandoc, &mut buf).expect("qmd write");
    String::from_utf8(buf).expect("qmd is UTF-8")
}

/// Re-read `qmd` and resolve its footnotes the way every render does.
fn read_and_resolve(qmd: &str) -> Pandoc {
    let (mut pandoc, _context, _warnings) = readers::qmd::read(
        qmd.as_bytes(),
        false,
        "<generated>",
        &mut std::io::sink(),
        true,
        None,
    )
    .unwrap_or_else(|e| panic!("regenerated qmd failed to parse: {e:?}\n--- qmd ---\n{qmd}"));

    let project = ProjectContext {
        dir: std::path::PathBuf::from("/project"),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path("/project/doc.qmd")],
        output_dir: std::path::PathBuf::from("/project"),
        ..Default::default()
    };
    let document = DocumentInfo::from_path("/project/doc.qmd");
    // A Pandoc profile makes the transform strip its ref-id marker blocks.
    let format = Format::docx();
    let binaries = BinaryDependencies::new();
    let mut ctx = RenderContext::new(&project, &document, &format, &binaries);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("tokio runtime");
    runtime
        .block_on(FootnotesTransform::new().transform(&mut pandoc, &mut ctx))
        .expect("footnotes transform");
    pandoc
}

/// The two ASTs as native text, and the qmd between them. A `Plain` as a
/// note's block re-reads as a `Para`, which the oracle allows, and so does an
/// empty table caption's two spellings.
fn trip(input: &Value) -> (String, String, String) {
    let pandoc = read_pandoc_json(input);
    let qmd = write_qmd(&pandoc);
    let resolved = read_and_resolve(&qmd);
    // An empty table caption is `Caption Nothing [  ]` from pandoc JSON and
    // `Caption Nothing []` from the qmd reader: the same thing, spelled twice.
    let allow = |text: String| text.replace("Plain ", "Para ").replace("[  ]", "[]");
    (allow(native(&pandoc)), allow(native(&resolved)), qmd)
}

#[track_caller]
fn assert_roundtrip(input: Value) -> String {
    let (before, after, qmd) = trip(&input);
    assert_eq!(
        before, after,
        "AST changed across the qmd round trip\n--- qmd ---\n{qmd}"
    );
    qmd
}

// ---------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------

#[test]
fn two_paragraphs() {
    let n = note(vec![para("first paragraph"), para("second paragraph")]);
    let qmd = assert_roundtrip(doc(vec![
        json!({"t": "Para", "c": text_with_note("Text", n, " more.")}),
    ]));
    assert!(qmd.contains("[^n1]") && qmd.contains("::: ^n1"), "{qmd}");
    assert!(!qmd.contains("complex block"), "{qmd}");
}

#[test]
fn a_paragraph_and_a_bullet_list() {
    let n = note(vec![para("intro"), bullets(&["one", "two"])]);
    assert_roundtrip(doc(vec![
        json!({"t": "Para", "c": text_with_note("Text", n, ".")}),
    ]));
}

#[test]
fn the_docx_flattened_list_shape() {
    // The docx reader's footnote with a list: Para, Plain, Plain.
    let n = note(vec![para("intro"), plain("item one"), plain("item two")]);
    assert_roundtrip(doc(vec![
        json!({"t": "Para", "c": text_with_note("Text", n, ".")}),
    ]));
}

#[test]
fn a_code_block() {
    let n = note(vec![para("see"), code_block("x <- 1\ny <- 2")]);
    assert_roundtrip(doc(vec![
        json!({"t": "Para", "c": text_with_note("Text", n, ".")}),
    ]));
}

#[test]
fn a_table() {
    let n = note(vec![para("a table"), table("cell")]);
    assert_roundtrip(doc(vec![
        json!({"t": "Para", "c": text_with_note("Text", n, ".")}),
    ]));
}

#[test]
fn a_note_in_a_list_item() {
    let n = note(vec![para("first"), para("second")]);
    let item = vec![json!({"t": "Plain", "c": text_with_note("Item", n, " end")})];
    let list = json!({"t": "BulletList", "c": [item, vec![plain("other")]]});
    let qmd = assert_roundtrip(doc(vec![list, para("after")]));
    // The definition lands after the top-level list, not inside the item.
    assert!(
        qmd.find("::: ^n1").unwrap() > qmd.find("other").unwrap(),
        "{qmd}"
    );
}

#[test]
fn a_note_in_a_table_cell() {
    let n = note(vec![para("first"), para("second")]);
    let cell = json!([attr("", &[]), {"t": "AlignDefault"}, 1, 1, [json!({"t": "Plain", "c": text_with_note("cell", n, "")})]]);
    let row = |c: Value| json!([attr("", &[]), [c]]);
    let head_cell = json!([attr("", &[]), {"t": "AlignDefault"}, 1, 1, [plain("head")]]);
    let t = json!({"t": "Table", "c": [
        attr("", &[]),
        [null, []],
        [[{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]],
        [attr("", &[]), [row(head_cell)]],
        [[attr("", &[]), 0, [], [row(cell)]]],
        [attr("", &[]), []]
    ]});
    assert_roundtrip(doc(vec![t, para("after")]));
}

#[test]
fn two_notes_in_one_paragraph() {
    let a = note(vec![para("a1"), para("a2")]);
    let b = note(vec![para("b1"), para("b2")]);
    let mut inlines = text_with_note("One", a, " and two");
    inlines.push(b);
    inlines.extend(inl(" end."));
    let qmd = assert_roundtrip(doc(vec![json!({"t": "Para", "c": inlines})]));
    assert!(qmd.contains("::: ^n1") && qmd.contains("::: ^n2"), "{qmd}");
}

#[test]
fn a_single_paragraph_note_stays_inline() {
    let n = note(vec![para("only paragraph")]);
    let qmd = assert_roundtrip(doc(vec![
        json!({"t": "Para", "c": text_with_note("Text", n, ".")}),
    ]));
    assert!(qmd.contains("^[only paragraph]"), "{qmd}");
    assert!(!qmd.contains(":::"), "{qmd}");
}

#[test]
fn a_note_inside_a_note_is_written_as_two_definitions() {
    // `FootnotesTransform` doesn't resolve a reference inside a definition body
    // (footnotes in footnotes aren't a Word or pandoc construct), so there is no
    // oracle for this one: check that the writer's output is well formed and
    // that every reference has a definition.
    let inner = note(vec![para("inner one"), para("inner two")]);
    let outer = note(vec![
        json!({"t": "Para", "c": text_with_note("outer", inner, " text")}),
        para("outer two"),
    ]);
    let input = doc(vec![
        json!({"t": "Para", "c": text_with_note("Text", outer, ".")}),
    ]);
    let qmd = write_qmd(&read_pandoc_json(&input));
    assert!(
        qmd.starts_with("Text[^n1].\n\n::: ^n1\nouter[^n2] text\n"),
        "{qmd}"
    );
    assert!(
        qmd.contains("\n::: ^n2\ninner one\n\ninner two\n:::\n"),
        "{qmd}"
    );
    read_and_resolve(&qmd);
}

#[test]
fn generated_ids_skip_ids_already_in_the_document() {
    // A div with id `n1` and an existing reference to a note `n2`.
    let n = note(vec![para("first"), para("second")]);
    let div = json!({"t": "Div", "c": [attr("n1", &["box"]), [para("boxed")]]});
    let existing = json!({"t": "Para", "c": [
        str_("see"),
        json!({"t": "Span", "c": [["", ["quarto-note-reference"], [["reference-id", "n2"]]], []]}),
    ]});
    let p = json!({"t": "Para", "c": text_with_note("Text", n, ".")});
    let (pandoc, qmd) = {
        let pandoc = read_pandoc_json(&doc(vec![div, existing, p]));
        let qmd = write_qmd(&pandoc);
        (pandoc, qmd)
    };
    let _ = pandoc;
    assert!(
        qmd.contains("[^n3]") && qmd.contains("::: ^n3"),
        "ids n1 and n2 are taken:\n{qmd}"
    );
}

#[test]
fn a_note_in_metadata_keeps_the_inline_form() {
    let meta_note = note(vec![para("first"), para("second")]);
    let input = json!({
        "pandoc-api-version": [1, 23, 1],
        "meta": {"title": {"t": "MetaInlines", "c": text_with_note("Title", meta_note, "")}},
        "blocks": [para("body")]
    });
    let qmd = write_qmd(&read_pandoc_json(&input));
    // No top-level block can hold a definition for a note in metadata.
    assert!(!qmd.contains("::: ^"), "{qmd}");
    // Its paragraphs are joined with a space, as before this change.
    assert!(qmd.contains("title: \"Title^[first second]\""), "{qmd}");
}
