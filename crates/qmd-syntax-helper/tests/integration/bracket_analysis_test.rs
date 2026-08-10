//! Detector-level tests for the shared bracket analysis
//! (bd-reference-links-unsupported-ddc4skac).
//!
//! These deliberately test the *detector*, not the rewrites. The rules key
//! off AST shape — a bare `Span` / empty-url `Image` — so if a future parser
//! change stops producing those shapes, the rules would go quiet rather than
//! fail loudly. These tests trip in that case.

use qmd_syntax_helper::conversions::bracket_analysis::{Finding, PartKind, analyze};

fn findings(source: &str) -> Vec<Finding> {
    analyze(source, "test.qmd").unwrap().findings
}

/// Helper: the source text a finding covers.
fn slice<'a>(source: &'a str, f: &Finding) -> &'a str {
    &source[f.start()..f.end()]
}

// ---------------------------------------------------------------------
// References with a matching definition
// ---------------------------------------------------------------------

#[test]
fn detects_full_reference() {
    let src = "See [the docs][gcc].\n\n[gcc]: https://example.com/gcc\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    assert!(matches!(f[0], Finding::Reference { .. }));
    assert_eq!(slice(src, &f[0]), "[the docs][gcc]");
}

#[test]
fn detects_collapsed_reference() {
    let src = "See [gcc][].\n\n[gcc]: https://example.com/gcc\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    assert!(matches!(f[0], Finding::Reference { .. }));
    assert_eq!(slice(src, &f[0]), "[gcc][]");
}

#[test]
fn detects_shortcut_reference() {
    let src = "See [gcc].\n\n[gcc]: https://example.com/gcc\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    assert!(matches!(f[0], Finding::Reference { .. }));
    assert_eq!(slice(src, &f[0]), "[gcc]");
}

#[test]
fn reference_labels_match_case_insensitively() {
    // CommonMark labels are case-folded and whitespace-normalized.
    let src = "See [docs][GCC   Toolset].\n\n[gcc toolset]: https://example.com/g\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    assert!(
        matches!(f[0], Finding::Reference { .. }),
        "label matching should be case- and whitespace-insensitive, got {:?}",
        f[0]
    );
}

// ---------------------------------------------------------------------
// Image references — the fourth shape (empty-url Image, not Span)
// ---------------------------------------------------------------------

#[test]
fn detects_full_image_reference() {
    let src = "A ![alt][r] B\n\n[r]: https://example.com/i.png\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    match &f[0] {
        Finding::Reference { kind, .. } => assert_eq!(*kind, PartKind::Image),
        other => panic!("expected an image Reference, got {other:?}"),
    }
    assert_eq!(slice(src, &f[0]), "![alt][r]");
}

#[test]
fn detects_shortcut_image_reference() {
    let src = "A ![r] B\n\n[r]: https://example.com/i.png\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    match &f[0] {
        Finding::Reference { kind, .. } => assert_eq!(*kind, PartKind::Image),
        other => panic!("expected an image Reference, got {other:?}"),
    }
    assert_eq!(slice(src, &f[0]), "![r]");
}

#[test]
fn detects_undefined_image_as_literal() {
    // `![solo]` with no definition renders as <img src=""> — a broken image.
    let src = "A ![solo] B\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    match &f[0] {
        Finding::Literal { kind, .. } => assert_eq!(*kind, PartKind::Image),
        other => panic!("expected an image Literal, got {other:?}"),
    }
    assert_eq!(slice(src, &f[0]), "![solo]");
}

// ---------------------------------------------------------------------
// Bracketed text with no definition
// ---------------------------------------------------------------------

#[test]
fn detects_undefined_brackets_as_literal() {
    let src = "Requires Posit Connect [Version TBD] or later.\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    assert!(matches!(f[0], Finding::Literal { .. }));
    assert_eq!(slice(src, &f[0]), "[Version TBD]");
}

#[test]
fn detects_multiple_literals_in_one_paragraph() {
    let src = "Upon a session [1], the server sets a cookie [2] with a token.\n";
    let f = findings(src);
    assert_eq!(f.len(), 2, "expected two findings, got {f:?}");
    assert_eq!(slice(src, &f[0]), "[1]");
    assert_eq!(slice(src, &f[1]), "[2]");
}

#[test]
fn detects_literal_spanning_a_soft_line_break() {
    // Spans cross soft line breaks, so edits must be offset-based.
    let src = "A [multi\nline] B\n";
    let f = findings(src);
    assert_eq!(f.len(), 1, "expected exactly one finding, got {f:?}");
    assert_eq!(slice(src, &f[0]), "[multi\nline]");
}

#[test]
fn undefined_pair_is_two_literals_not_a_reference() {
    // `[a][b]` with no definition for `b` is literal text in CommonMark.
    let src = "A [a][b] C\n";
    let f = findings(src);
    assert_eq!(f.len(), 2, "expected two literal findings, got {f:?}");
    assert!(f.iter().all(|f| matches!(f, Finding::Literal { .. })));
}

// ---------------------------------------------------------------------
// Shapes that must NOT be touched
// ---------------------------------------------------------------------

#[test]
fn ignores_genuine_span_syntax() {
    let src = "A [text]{.cls} B\n";
    assert!(
        findings(src).is_empty(),
        "a span with attributes is real syntax and must be left alone"
    );
}

#[test]
fn ignores_inline_links_and_images() {
    let src = "A [link](u) B ![img](i.png) C\n";
    assert!(
        findings(src).is_empty(),
        "inline links/images parse as Link/Image with a url, not as bare brackets"
    );
}

#[test]
fn ignores_brackets_inside_code() {
    let src = "Use `response['data'][0]` here.\n\n```python\nx = y['a']['b']\n```\n";
    assert!(
        findings(src).is_empty(),
        "code spans and code blocks never produce Span/Image nodes"
    );
}

#[test]
fn ignores_already_escaped_brackets() {
    // This is what makes the escaping arm idempotent under repeated passes.
    let src = "A \\[escaped\\] B and !\\[img\\] C\n";
    assert!(
        findings(src).is_empty(),
        "escaped brackets produce no Span/Image at all"
    );
}

#[test]
fn ignores_definition_lines_themselves() {
    // The `[gcc]` on the definition line must never be escaped — that would
    // corrupt the definition.
    let src = "See [the docs][gcc].\n\n[gcc]: https://example.com/gcc\n";
    let a = analyze(src, "test.qmd").unwrap();
    assert_eq!(a.definitions.len(), 1);
    assert!(
        !a.findings
            .iter()
            .any(|f| f.start() >= 22 && matches!(f, Finding::Literal { .. })),
        "the definition line's own brackets must not be reported as literal"
    );
}

// ---------------------------------------------------------------------
// Definitions
// ---------------------------------------------------------------------

#[test]
fn parses_definition_url_and_title() {
    let src = "See [d][r].\n\n[r]: https://e.com \"The Title\"\n";
    let a = analyze(src, "test.qmd").unwrap();
    assert_eq!(a.definitions.len(), 1);
    assert_eq!(a.definitions[0].url, "https://e.com");
    assert_eq!(a.definitions[0].title.as_deref(), Some("The Title"));
}

/// Pandoc peels a trailing quoted title off first and keeps everything else
/// as the destination, spaces and all. All four combinations were verified
/// against `quarto pandoc`; getting this wrong silently drops definitions.
#[test]
fn splits_destination_and_title_the_way_pandoc_does() {
    let cases: [(&str, &str, Option<&str>); 4] = [
        ("https://e.com", "https://e.com", None),
        ("https://e.com \"T\"", "https://e.com", Some("T")),
        ("https://e.com/a b.png", "https://e.com/a b.png", None),
        (
            "https://e.com/a b.png \"T\"",
            "https://e.com/a b.png",
            Some("T"),
        ),
    ];

    for (tail, expected_url, expected_title) in cases {
        let src = format!("See [d][r].\n\n[r]: {tail}\n");
        let a = analyze(&src, "test.qmd").unwrap();
        assert_eq!(a.definitions.len(), 1, "`{tail}` should be a definition");
        assert_eq!(a.definitions[0].url, expected_url, "url of `{tail}`");
        assert_eq!(
            a.definitions[0].title.as_deref(),
            expected_title,
            "title of `{tail}`"
        );
    }
}

#[test]
fn does_not_mistake_a_trailing_paren_in_a_url_for_a_title() {
    let src = "See [d][r].\n\n[r]: https://e.com/a(b)\n";
    let a = analyze(src, "test.qmd").unwrap();
    assert_eq!(a.definitions.len(), 1);
    assert_eq!(a.definitions[0].url, "https://e.com/a(b)");
    assert_eq!(a.definitions[0].title, None);
}

#[test]
fn parses_angle_bracketed_definition_url() {
    let src = "See [d][r].\n\n[r]: <https://e.com/a>\n";
    let a = analyze(src, "test.qmd").unwrap();
    assert_eq!(a.definitions.len(), 1);
    assert_eq!(
        a.definitions[0].url, "https://e.com/a",
        "angle brackets are a definition-side wrapper and are stripped"
    );
}

#[test]
fn does_not_treat_code_block_lines_as_definitions() {
    // A definition-shaped line inside a fenced code block is not a definition.
    // This is why definitions are cross-checked against the AST rather than
    // recognized by regex alone.
    let src = "```\n[ref]: https://example.com\n```\n";
    let a = analyze(src, "test.qmd").unwrap();
    assert!(
        a.definitions.is_empty(),
        "definition-shaped lines inside code blocks are not definitions"
    );
}

#[test]
fn records_unused_definitions() {
    let src = "Nothing refers to it.\n\n[orphan]: https://example.com\n";
    let a = analyze(src, "test.qmd").unwrap();
    assert_eq!(a.definitions.len(), 1);
    assert!(
        !a.is_used("orphan"),
        "an unused definition should be reported as unused"
    );
}

// ---------------------------------------------------------------------
// The ambiguity guard
// ---------------------------------------------------------------------

#[test]
fn declines_runs_of_three_or_more() {
    let src = "A [a][b][c] D\n\n[b]: https://e.com/b\n[c]: https://e.com/c\n";
    let f = findings(src);
    assert_eq!(
        f.len(),
        1,
        "a 3-run should produce one Ambiguous, got {f:?}"
    );
    match &f[0] {
        Finding::Ambiguous { count, .. } => assert_eq!(*count, 3),
        other => panic!("expected Ambiguous, got {other:?}"),
    }
    assert_eq!(slice(src, &f[0]), "[a][b][c]");
}
