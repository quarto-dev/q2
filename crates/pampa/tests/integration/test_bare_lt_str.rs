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
    //
    // The subject here is the *lexer*. `div` is a block-level name and this
    // tag opens the line, so the reader lifts it out of the paragraph into a
    // RawBlock (bd-block-html-wrapped-in-p-w8qebxig) — which is just as good
    // a witness that the scanner recognised it as raw HTML.
    let pandoc = parse_qmd("<div >\n");
    match &pandoc.blocks[0] {
        Block::RawBlock(r) => {
            assert_eq!(r.format, "html");
            assert!(r.text.contains("<div"), "got: {:?}", r);
        }
        other => panic!("expected RawBlock html, got: {:?}", other),
    }
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
    // Regression: HTML comment unchanged — still raw HTML with format "html".
    //
    // A comment on its own line is now a RawBlock rather than a paragraph
    // holding one RawInline (bd-block-html-wrapped-in-p-w8qebxig); the lexing
    // this guards is unaffected. See `comment_in_running_text_stays_inline`
    // below for the inline half.
    let pandoc = parse_qmd("<!-- c -->\n");
    match &pandoc.blocks[0] {
        Block::RawBlock(r) => {
            assert_eq!(r.format, "html");
            assert!(r.text.contains("<!--"), "got: {:?}", r);
        }
        other => panic!("expected RawBlock html comment, got: {:?}", other),
    }
}

#[test]
fn comment_in_running_text_stays_inline() {
    // The inline half of the case above: a comment that does not open a line
    // stays a RawInline inside its paragraph.
    let pandoc = parse_qmd("text <!-- c --> more\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert!(
        inlines
            .iter()
            .any(|i| matches!(i, Inline::RawInline(r) if r.text.contains("<!--"))),
        "expected an inline html comment, got: {:?}",
        inlines
    );
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

// ---------------------------------------------------------------------------
// bd-html-element-runaway-k1eo50h8: the html_element scan must (A) only start
// on a plausible tag name and (B) never cross a blank line.
// ---------------------------------------------------------------------------

fn parse_qmd_with_diagnostics(
    input: &str,
) -> (
    pampa::pandoc::Pandoc,
    Vec<quarto_error_reporting::DiagnosticMessage>,
) {
    let (pandoc, _context, diagnostics) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("parse failed");
    (pandoc, diagnostics)
}

fn paragraph_inlines(block: &Block) -> &Vec<Inline> {
    match block {
        Block::Paragraph(p) => &p.content,
        other => panic!("expected paragraph, got {:?}", other),
    }
}

#[test]
fn lt_digit_with_later_gt_parses_as_strs() {
    // The reported shape: `<6.1` cannot begin a tag (a tag name starts with
    // a letter), so the scan must not walk on to the `>` later in the
    // paragraph. Pandoc: Str "<6.1" … Str ">".
    let (pandoc, diagnostics) = parse_qmd_with_diagnostics("supports <6.1 and x > y\n");
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        diagnostics
    );
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("supports")"#,
            "Space",
            r#"Str("<6.1")"#,
            "Space",
            r#"Str("and")"#,
            "Space",
            r#"Str("x")"#,
            "Space",
            r#"Str(">")"#,
            "Space",
            r#"Str("y")"#,
        ],
    );
}

#[test]
fn lt_dash_assignment_arrows_parse_as_strs() {
    // R prose: `<-` … `->` used to lex as one html_element.
    let (pandoc, diagnostics) = parse_qmd_with_diagnostics("x <- 5 and y -> 6\n");
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        diagnostics
    );
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("x")"#,
            "Space",
            r#"Str("<-")"#,
            "Space",
            r#"Str("5")"#,
            "Space",
            r#"Str("and")"#,
            "Space",
            r#"Str("y")"#,
            "Space",
            r#"Str("->")"#,
            "Space",
            r#"Str("6")"#,
        ],
    );
}

#[test]
fn lt_equals_comparison_parses_as_strs() {
    let (pandoc, diagnostics) = parse_qmd_with_diagnostics("a <=b and c >= d\n");
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        diagnostics
    );
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("a")"#,
            "Space",
            r#"Str("<=b")"#,
            "Space",
            r#"Str("and")"#,
            "Space",
            r#"Str("c")"#,
            "Space",
            r#"Str(">=")"#,
            "Space",
            r#"Str("d")"#,
        ],
    );
}

#[test]
fn lt_digit_immediately_closed_parses_as_strs() {
    // `<5>` was a RawInline with a Q-2-9 warning; it is not a tag.
    let (pandoc, diagnostics) = parse_qmd_with_diagnostics("a <5> b\n");
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        diagnostics
    );
    let inlines = first_paragraph_inlines(&pandoc);
    assert_str_texts(
        inlines,
        &[
            r#"Str("a")"#,
            "Space",
            r#"Str("<5>")"#,
            "Space",
            r#"Str("b")"#,
        ],
    );
}

#[test]
fn unclosed_tag_does_not_cross_blank_line() {
    // An inline tag cannot span a paragraph boundary (CommonMark reading;
    // pandoc's markdown reader would produce RawInline "<foo\n\nbar>", which
    // is the divergence the plan accepts). The `<` falls back to a literal,
    // exactly as it does at EOF (bd-j9cf).
    let (pandoc, diagnostics) = parse_qmd_with_diagnostics("x <foo\n\nbar> y\n");
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        diagnostics
    );
    assert_eq!(pandoc.blocks.len(), 2, "got: {:?}", pandoc.blocks);
    assert_str_texts(
        paragraph_inlines(&pandoc.blocks[0]),
        &[r#"Str("x")"#, "Space", r#"Str("<foo")"#],
    );
    assert_str_texts(
        paragraph_inlines(&pandoc.blocks[1]),
        &[r#"Str("bar>")"#, "Space", r#"Str("y")"#],
    );
}

#[test]
fn reported_document_renders_paragraph_then_table() {
    // The user's report, verbatim: `<6.1)` in a list item followed by a pipe
    // table whose cell contains `<vsix>` inside a code span. The scanner used
    // to lex one html_element from `<6.1)` to `<vsix>`, five lines and a
    // blank line later, and the parse errors landed in the table row.
    let input = r#"5. **TypeScript gates many type-level upgrades.** Some workspaces are still on TS 4.9.
   These need TS ≥5.x:

   ...

   TS 7 is deferred: it has no programmatic API, so typescript-eslint (supports <6.1)
   and editor tooling don't work with it.

| Decision | Recommendation | Doc |
|---|---|---|
| Replace `HaaLeo/publish-vscode-extension` (bundles vsce 3.2.2) with the repo's own vsce | **Decided: do what [posit-dev/publisher](https://github.com/posit-dev/publisher/blob/main/.github/workflows/publish.yaml) does.** Package once in a reusable workflow, and upload the VSIX as an artifact. Then run separate jobs for `vsce publish --packagePath <vsix> --skip-duplicate` and `ovsx publish <vsix> --skip-duplicate`, and set `"vsce": {"dependencies": false}` in `apps/vscode/package.json`. One difference: use the repo's pinned `@vscode/vsce` devDependency, not an unpinned `npx`, so CI matches local builds. This lands with the vscode-tooling upgrade. | [vscode-tooling](deps-upgrade-vscode-tooling.md) |
"#;
    let (pandoc, diagnostics) = parse_qmd_with_diagnostics(input);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        diagnostics
    );
    assert_eq!(pandoc.blocks.len(), 2, "got: {:?}", pandoc.blocks);
    assert!(
        matches!(pandoc.blocks[0], Block::OrderedList(_)),
        "expected OrderedList first, got: {:?}",
        pandoc.blocks[0]
    );
    assert!(
        matches!(pandoc.blocks[1], Block::Table(_)),
        "expected Table second, got: {:?}",
        pandoc.blocks[1]
    );
}

#[test]
fn closing_tag_still_parses_as_raw_html() {
    // `/` after `<` stays in the tag-start gate.
    let pandoc = parse_qmd("a </b> c\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert!(
        inlines
            .iter()
            .any(|i| matches!(i, Inline::RawInline(r) if r.text == "</b>")),
        "expected RawInline </b>, got: {:?}",
        inlines
    );
}

#[test]
fn open_tag_spanning_a_newline_still_parses_as_raw_html() {
    // bd-ly83qewg decision: `<span\n class="x">` is a valid open tag; only a
    // *blank* line bounds the scan.
    let pandoc = parse_qmd("a <span\n class=\"x\"> b\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert!(
        inlines
            .iter()
            .any(|i| matches!(i, Inline::RawInline(r) if r.text == "<span\n class=\"x\">")),
        "expected RawInline spanning the newline, got: {:?}",
        inlines
    );
}

#[test]
fn anchor_shorthand_still_desugars_to_link() {
    // bd-p2tx: `<#id>` rides on the html_element token; `#` must stay in
    // the tag-start gate.
    let pandoc = parse_qmd("see <#sec-intro> here\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert!(
        inlines
            .iter()
            .any(|i| matches!(i, Inline::Link(l) if l.target.0 == "#sec-intro")),
        "expected anchor Link, got: {:?}",
        inlines
    );
}

#[test]
fn digit_led_email_autolink_still_parses_as_link() {
    // CommonMark email autolinks may start with a digit; the gate applies
    // to HTML_ELEMENT only.
    let pandoc = parse_qmd("mail <1user@example.com> now\n");
    let inlines = first_paragraph_inlines(&pandoc);
    assert!(
        inlines
            .iter()
            .any(|i| matches!(i, Inline::Link(l) if l.target.0 == "mailto:1user@example.com")),
        "expected email autolink, got: {:?}",
        inlines
    );
}
