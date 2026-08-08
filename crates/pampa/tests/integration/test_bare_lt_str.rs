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
fn lt_gt_with_inner_whitespace_does_not_swallow_emphasis_closer() {
    // bd-ly83qewg: `< b text.* a >` used to lex as one html_element token,
    // swallowing the closing `*` and failing with Q-2-12 Unclosed Star
    // Emphasis. Whitespace immediately after `<` disqualifies the HTML
    // construct, so both brackets are literal Strs (pandoc-compatible).
    let pandoc = parse_qmd("*a < b text.* a > b\n");
    let inlines = first_paragraph_inlines(&pandoc);
    match &inlines[0] {
        Inline::Emph(e) => assert_str_texts(
            &e.content,
            &[
                r#"Str("a")"#,
                "Space",
                r#"Str("<")"#,
                "Space",
                r#"Str("b")"#,
                "Space",
                r#"Str("text.")"#,
            ],
        ),
        other => panic!("expected Emph, got {:?}", other),
    }
    assert_str_texts(
        &inlines[1..],
        &[
            "Space",
            r#"Str("a")"#,
            "Space",
            r#"Str(">")"#,
            "Space",
            r#"Str("b")"#,
        ],
    );
}

#[test]
fn lt_gt_with_inner_whitespace_in_plain_text_parses_as_strs() {
    // bd-ly83qewg: `a < b > c` used to lex `< b >` as html_element.
    let pandoc = parse_qmd("a < b > c\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("a")"#,
            "Space",
            r#"Str("<")"#,
            "Space",
            r#"Str("b")"#,
            "Space",
            r#"Str(">")"#,
            "Space",
            r#"Str("c")"#,
        ],
    );
}

#[test]
fn lt_at_end_of_line_with_gt_on_next_line_parses_as_str() {
    // bd-ly83qewg: the html_element scan crosses newlines, so a `>` on a
    // later line used to produce an html_element spanning the soft break.
    // A newline immediately after `<` disqualifies it like any whitespace.
    let pandoc = parse_qmd("foo <\nbar > baz\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("foo")"#,
            "Space",
            r#"Str("<")"#,
            "SoftBreak",
            r#"Str("bar")"#,
            "Space",
            r#"Str(">")"#,
            "Space",
            r#"Str("baz")"#,
        ],
    );
}

#[test]
fn html_element_with_whitespace_before_gt_still_parses_as_raw_html() {
    // Regression guard for bd-ly83qewg: `<div >` is a valid open tag
    // (whitespace before `>` is allowed by the HTML spec); only whitespace
    // immediately after `<` disqualifies.
    let pandoc = parse_qmd("<div >\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert!(
        matches!(inlines[0], Inline::RawInline(_)),
        "expected RawInline, got: {:?}",
        inlines
    );
}

#[test]
fn html_element_with_interior_whitespace_still_parses_as_raw_html() {
    // Regression guard for bd-ly83qewg: interior-only whitespace
    // (`<not a tag>`) keeps the best-effort html_element lexing; this class
    // of ambiguity is out of scope.
    let pandoc = parse_qmd("<not a tag>\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert!(
        matches!(inlines[0], Inline::RawInline(_)),
        "expected RawInline, got: {:?}",
        inlines
    );
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
