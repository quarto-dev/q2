/*
 * test_frontmatter_delimiters.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Regression coverage for bd-mjo6ao32 / GH #671.
 *
 * The YAML frontmatter block ends at a *line* consisting of `---`; a `---`
 * anywhere inside a value is ordinary content. The reader used to rediscover
 * the block's body by `split("---")`-ing the raw node text, so the first
 * `---` inside a value truncated the metadata: a plain scalar silently lost
 * every later key, a quoted scalar failed with Q-0-99. Since PR #290 the qmd
 * writer canonicalizes every em dash to `---`, which made both shapes
 * reachable from any document with an em dash in a frontmatter string.
 *
 * The fix makes the grammar expose the body as a `yaml` child node so the
 * reader takes the range from the parse instead of re-scanning the text.
 * These tests pin the reader-visible contract (all keys survive, source
 * offsets stay exact, the writer's output re-reads to the same metadata) and
 * the tree shape itself.
 */

use pampa::pandoc::{Block, Pandoc};
use pampa::readers;
use pampa::writers;
use quarto_error_reporting::DiagnosticMessage;
use quarto_pandoc_types::{ConfigMapEntry, ConfigValueKind};
use tree_sitter_qmd::MarkdownParser;

const EM: &str = "\u{2014}";

fn read(input: &str) -> (Pandoc, Vec<DiagnosticMessage>) {
    let mut sink = std::io::sink();
    let (doc, _ctx, warnings) =
        readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut sink, true, None)
            .unwrap_or_else(|errs| panic!("read failed for {input:?}: {errs:#?}"));
    (doc, warnings)
}

fn entries(doc: &Pandoc) -> &[ConfigMapEntry] {
    match &doc.meta.value {
        ConfigValueKind::Map(entries) => entries,
        other => panic!("expected Map metadata, got {other:?}"),
    }
}

fn keys(doc: &Pandoc) -> Vec<&str> {
    entries(doc).iter().map(|e| e.key.as_str()).collect()
}

fn entry<'a>(doc: &'a Pandoc, key: &str) -> &'a ConfigMapEntry {
    entries(doc)
        .iter()
        .find(|e| e.key == key)
        .unwrap_or_else(|| panic!("no key {key:?} in {:?}", keys(doc)))
}

fn text(doc: &Pandoc, key: &str) -> String {
    let e = entry(doc, key);
    e.value
        .as_plain_text()
        .unwrap_or_else(|| panic!("key {key:?} is not text: {:?}", e.value.value))
}

/// Absolute byte offset of a key in the source file.
fn key_offset(doc: &Pandoc, key: &str) -> usize {
    let (_file, start, _end) = entry(doc, key)
        .key_source
        .resolve_byte_range()
        .unwrap_or_else(|| panic!("key {key:?} has no resolvable byte range"));
    start
}

fn write_qmd(doc: &Pandoc) -> String {
    let mut buf = Vec::new();
    writers::qmd::write(doc, &mut buf).expect("qmd writer failed");
    String::from_utf8(buf).expect("qmd writer produced invalid UTF-8")
}

// ---------------------------------------------------------------------------
// Reader contract: a `---` inside a value never ends the block
// ---------------------------------------------------------------------------

/// The hand-typed shape from GH #671: the cut used to land inside the quoted
/// scalar, so the re-read failed with Q-0-99 and the metadata came back empty.
#[test]
fn quoted_value_containing_dashes_keeps_every_key() {
    let (doc, warnings) = read("---\ndescription: \"a --- b\"\nauthor: Z\n---\n\nx\n");
    assert!(warnings.is_empty(), "unexpected diagnostics: {warnings:#?}");
    assert_eq!(keys(&doc), ["description", "author"]);
    // Metadata strings are markdown, so the reader's smart-typography pass
    // turns the run of dashes back into an em dash.
    assert_eq!(text(&doc, "description"), format!("a {EM} b"));
    assert_eq!(text(&doc, "author"), "Z");
}

/// The qmd writer's single-line form: a plain scalar. The cut used to leave
/// valid YAML (`description: Hello`), so every later key vanished silently.
#[test]
fn plain_value_containing_dashes_keeps_every_key() {
    let (doc, warnings) = read("---\ndescription: Hello --- world\nauthor: Z\n---\n\nx\n");
    assert!(warnings.is_empty(), "unexpected diagnostics: {warnings:#?}");
    assert_eq!(keys(&doc), ["description", "author"]);
    assert_eq!(text(&doc, "description"), format!("Hello {EM} world"));
    assert_eq!(text(&doc, "author"), "Z");
}

/// The qmd writer's multi-line form: a double-quoted scalar with `\n` escapes.
#[test]
fn double_quoted_multiline_value_containing_dashes_reads() {
    let (doc, warnings) = read("---\ndescription: \"Hello ---\\nworld.\"\nauthor: Z\n---\n\nx\n");
    assert!(warnings.is_empty(), "unexpected diagnostics: {warnings:#?}");
    assert_eq!(keys(&doc), ["description", "author"]);
    let description = text(&doc, "description");
    assert!(
        description.starts_with(&format!("Hello {EM}")) && description.ends_with("world."),
        "unexpected description text: {description:?}"
    );
    assert_eq!(text(&doc, "author"), "Z");
}

/// An indented `---` line inside a block scalar is YAML content, not a
/// delimiter; as markdown it is a thematic break between two paragraphs.
#[test]
fn indented_dash_line_inside_block_scalar_does_not_close_the_block() {
    let (doc, warnings) = read("---\ndescription: |\n  a\n\n  ---\n\n  b\nauthor: Z\n---\n\nx\n");
    assert!(warnings.is_empty(), "unexpected diagnostics: {warnings:#?}");
    assert_eq!(keys(&doc), ["description", "author"]);
    match &entry(&doc, "description").value.value {
        ConfigValueKind::PandocBlocks(blocks) => {
            assert_eq!(
                blocks.len(),
                3,
                "expected Para, HorizontalRule, Para: {blocks:#?}"
            );
            assert!(matches!(blocks[0], Block::Paragraph(_)), "{:?}", blocks[0]);
            assert!(
                matches!(blocks[1], Block::HorizontalRule(_)),
                "{:?}",
                blocks[1]
            );
            assert!(matches!(blocks[2], Block::Paragraph(_)), "{:?}", blocks[2]);
        }
        other => panic!("expected PandocBlocks, got {other:?}"),
    }
    assert_eq!(text(&doc, "author"), "Z");
}

/// Controls: shapes the scanner accepted before the change and must keep
/// accepting.
#[test]
fn closing_delimiter_with_trailing_whitespace_still_closes() {
    let (doc, warnings) = read("---\ntitle: T\nauthor: Z\n---   \n\nx\n");
    assert!(warnings.is_empty(), "unexpected diagnostics: {warnings:#?}");
    assert_eq!(keys(&doc), ["title", "author"]);
    assert_eq!(text(&doc, "title"), "T");
}

#[test]
fn crlf_document_reads_every_key() {
    let (doc, warnings) = read("---\r\ndescription: \"a --- b\"\r\nauthor: Z\r\n---\r\n\r\nx\r\n");
    assert!(warnings.is_empty(), "unexpected diagnostics: {warnings:#?}");
    assert_eq!(keys(&doc), ["description", "author"]);
    assert_eq!(text(&doc, "description"), format!("a {EM} b"));
    assert_eq!(text(&doc, "author"), "Z");
}

/// A metadata block nested in a fenced div goes through a different
/// intermediate-to-RawBlock site than the document-level one.
#[test]
fn metadata_inside_fenced_div_with_dashes_in_value() {
    let (doc, warnings) = read("::: hello\n\n---\nnested: \"a --- b\"\nother: Z\n---\n\n:::\n");
    assert!(warnings.is_empty(), "unexpected diagnostics: {warnings:#?}");
    assert_eq!(keys(&doc), ["nested", "other"]);
    assert_eq!(text(&doc, "nested"), format!("a {EM} b"));
    assert_eq!(text(&doc, "other"), "Z");
}

// ---------------------------------------------------------------------------
// Source tracking: offsets are exact, not rediscovered by text search
// ---------------------------------------------------------------------------

#[test]
fn key_offsets_are_exact_after_a_value_containing_dashes() {
    //  0123 4                        27        37  41
    //  ---\n description: "a --- b"\n author: Z\n ---\n
    let input = "---\ndescription: \"a --- b\"\nauthor: Z\n---\n";
    assert_eq!(&input[4..15], "description");
    assert_eq!(&input[27..33], "author");
    let (doc, warnings) = read(input);
    assert!(warnings.is_empty(), "unexpected diagnostics: {warnings:#?}");
    assert_eq!(key_offset(&doc, "description"), 4);
    assert_eq!(key_offset(&doc, "author"), 27);
}

// ---------------------------------------------------------------------------
// Round trip: what the qmd writer emits re-reads to the same metadata
// ---------------------------------------------------------------------------

#[test]
fn em_dash_in_single_line_value_round_trips_through_qmd_writer() {
    let (first, _) = read(&format!(
        "---\ndescription: \"Hello {EM} world\"\nauthor: Z\n---\n\nx\n"
    ));
    let written = write_qmd(&first);
    // The writer canonicalizes the em dash to ASCII; that spelling is the
    // input under test, so make sure it actually appeared.
    assert!(
        written.contains("---\nworld") || written.contains(" --- "),
        "writer emitted {written:?}"
    );

    let (second, warnings) = read(&written);
    assert!(
        warnings.is_empty(),
        "re-read of {written:?} produced diagnostics: {warnings:#?}"
    );
    assert_eq!(keys(&second), keys(&first));
    assert_eq!(text(&second, "description"), text(&first, "description"));
    assert_eq!(text(&second, "author"), "Z");
}

#[test]
fn em_dash_in_multi_line_value_round_trips_through_qmd_writer() {
    let (first, _) = read(&format!(
        "---\ndescription: |\n  Hello {EM}\n  world.\nauthor: Z\n---\n\nx\n"
    ));
    let written = write_qmd(&first);
    assert!(written.contains("---"), "writer emitted {written:?}");

    let (second, warnings) = read(&written);
    assert!(
        warnings.is_empty(),
        "re-read of {written:?} produced diagnostics: {warnings:#?}"
    );
    assert_eq!(keys(&second), keys(&first));
    assert_eq!(text(&second, "description"), text(&first, "description"));
    assert_eq!(text(&second, "author"), "Z");
}

// ---------------------------------------------------------------------------
// Tree shape: the body is a child node with an exact range
// ---------------------------------------------------------------------------

#[test]
fn parser_exposes_yaml_body_child_with_exact_range() {
    let input = b"---\ndescription: \"a --- b\"\nauthor: Z\n---\n\nx\n";
    let mut parser = MarkdownParser::default();
    let tree = parser.parse(input, None).expect("parse failed");
    let root = tree.block_tree().root_node();
    let metadata = root.child(0).expect("document has no children");
    assert_eq!(metadata.kind(), "metadata", "{}", root.to_sexp());
    assert_eq!((metadata.start_byte(), metadata.end_byte()), (0, 41));

    let body = metadata
        .child_by_field_name("body")
        .unwrap_or_else(|| panic!("metadata has no body field: {}", metadata.to_sexp()));
    assert_eq!(body.kind(), "yaml");
    assert_eq!((body.start_byte(), body.end_byte()), (4, 37));
    assert_eq!(
        body.utf8_text(input).unwrap(),
        "description: \"a --- b\"\nauthor: Z\n"
    );
}

/// The pampa reader appends a missing final newline before parsing, so this
/// only matters for tools that feed the grammar raw buffers; there, a closing
/// `---` at end of file is still a closing delimiter.
#[test]
fn parser_accepts_closing_delimiter_at_eof_without_newline() {
    let input = b"---\ntitle: x\n---";
    let mut parser = MarkdownParser::default();
    let tree = parser.parse(input, None).expect("parse failed");
    let root = tree.block_tree().root_node();
    let metadata = root.child(0).expect("document has no children");
    assert_eq!(metadata.kind(), "metadata", "{}", root.to_sexp());
    let body = metadata.child_by_field_name("body").expect("no body field");
    assert_eq!(body.utf8_text(input).unwrap(), "title: x\n");
    assert_eq!(metadata.end_byte(), input.len());
}
