//! Tests for bare `<` parsing as a `Str` literal (bd-j9cf).
//!
//! Today a literal `<` outside math/code/HTML produces a parse error.
//! After bd-j9cf, a `<` that does not start a recognized HTML construct
//! (element, autolink, comment, raw-specifier) should parse as `Str "<"`.

use pampa::pandoc::{Block, Inline};
use pampa::readers;

fn parse_qmd(input: &str) -> pampa::pandoc::Pandoc {
    let (pandoc, _context, _warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("parse failed");
    pandoc
}

fn first_paragraph_inlines(pandoc: &pampa::pandoc::Pandoc) -> &Vec<Inline> {
    match &pandoc.blocks[0] {
        Block::Paragraph(p) => &p.content,
        other => panic!("expected paragraph, got {:?}", other),
    }
}

fn assert_str_texts(inlines: &[Inline], expected: &[&str]) {
    let actual: Vec<String> = inlines
        .iter()
        .map(|i| match i {
            Inline::Str(s) => format!("Str({:?})", s.text),
            Inline::Space(_) => "Space".to_string(),
            Inline::SoftBreak(_) => "SoftBreak".to_string(),
            Inline::LineBreak(_) => "LineBreak".to_string(),
            other => format!("Other({:?})", other),
        })
        .collect();
    let expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    assert_eq!(actual, expected, "inline sequence mismatch");
}

#[test]
fn bare_lt_between_digits_parses_as_str() {
    let pandoc = parse_qmd("1 < 2\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("1")"#,
            "Space",
            r#"Str("<")"#,
            "Space",
            r#"Str("2")"#,
        ],
    );
}

#[test]
fn bare_lt_at_end_of_line_parses_as_str() {
    let pandoc = parse_qmd("foo <\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(inlines, &[r#"Str("foo")"#, "Space", r#"Str("<")"#]);
}

#[test]
fn bare_lt_followed_by_digit_parses_as_str() {
    // Scanner emits Str("<"), then internal pandoc_str matches "5". The
    // post-process pass merges adjacent Strs, so the final inline sequence
    // contains Str("<5") — pandoc-compatible behavior.
    let pandoc = parse_qmd("a <5 b\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("a")"#,
            "Space",
            r#"Str("<5")"#,
            "Space",
            r#"Str("b")"#,
        ],
    );
}

#[test]
fn unclosed_tag_parses_as_str_lt_plus_text() {
    // `<foo` with no closing `>` — scanner's tag-scan walks to EOF without
    // finding `>`, retracts, and emits `<` as a Str. The internal regex
    // then matches "foo" as pandoc_str. The post-process pass merges
    // adjacent Strs into Str("<foo").
    let pandoc = parse_qmd("a <foo\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(inlines, &[r#"Str("a")"#, "Space", r#"Str("<foo")"#]);
}

#[test]
fn html_element_still_parses_as_raw_html() {
    // Regression: `<b>` is still recognized as an HTML element (raw HTML),
    // not split into `<`, `b`, `>` strings. The existing Q-2-9 warning path
    // is preserved.
    let pandoc = parse_qmd("<b>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    let kinds: Vec<&str> = inlines
        .iter()
        .map(|i| match i {
            Inline::RawInline(_) => "RawInline",
            Inline::Str(_) => "Str",
            Inline::Space(_) => "Space",
            _ => "Other",
        })
        .collect();
    assert_eq!(kinds, vec!["RawInline"], "got: {:?}", inlines);
}

#[test]
fn autolink_still_parses_as_link() {
    // Regression: autolinks unchanged.
    let pandoc = parse_qmd("<https://example.com>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    let has_link = inlines.iter().any(|i| matches!(i, Inline::Link(_)));
    assert!(has_link, "expected an autolink, got: {:?}", inlines);
}

#[test]
fn html_comment_still_parses_as_raw_html_comment() {
    // Regression: HTML comment unchanged — emitted as a RawInline with
    // format "html".
    let pandoc = parse_qmd("<!-- c -->\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::RawInline(r) => {
            assert_eq!(r.format, "html");
            assert!(r.text.contains("<!--"), "got: {:?}", r);
        }
        other => panic!("expected RawInline html comment, got: {:?}", other),
    }
}

#[test]
fn backslash_escaped_lt_is_unchanged() {
    // Regression: `\<` (already supported) still produces `Str "<"`.
    let pandoc = parse_qmd(r"a \< b" /* trailing newline added below */);
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("a")"#,
            "Space",
            r#"Str("<")"#,
            "Space",
            r#"Str("b")"#,
        ],
    );
}

#[test]
fn lt_in_math_is_unchanged() {
    // Regression: `<` inside math is part of the math text, never the
    // inline scanner's territory. We verify by inspecting the Math node.
    let pandoc = parse_qmd("$1 < 2$\n");
    let inlines = first_paragraph_inlines(&pandoc);
    let has_math = inlines.iter().any(|i| matches!(i, Inline::Math(_)));
    assert!(has_math, "expected Math inline, got: {:?}", inlines);
}

#[test]
fn lt_in_code_span_is_unchanged() {
    // Regression: `<` inside a code span is part of the code text.
    let pandoc = parse_qmd("`1 < 2`\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_eq!(inlines.len(), 1);
    match &inlines[0] {
        Inline::Code(c) => assert_eq!(c.text, "1 < 2"),
        other => panic!("expected Code inline, got: {:?}", other),
    }
}
