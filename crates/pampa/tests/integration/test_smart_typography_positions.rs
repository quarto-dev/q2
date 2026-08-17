/*
 * test_smart_typography_positions.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Regression coverage for bd-ellipsis-not-smart-48bv2pe6.
 *
 * Pandoc's `smart` extension rewrites every run of three dots to a HORIZONTAL
 * ELLIPSIS (U+2026) regardless of what precedes the run. q2 used to do it only
 * when the dot run followed a word character, because `startStrRegex` in
 * `grammar.js` admitted `-` but not `.` as a token-start character: a dot run at
 * token start lexed as one single-character `pandoc_str` node per dot, so
 * `apply_smart_typography` (which runs per node, by design) never saw a run of
 * three.
 *
 * These tests pin the *position* dimension, which the pre-existing
 * `smart-typography.qmd` fixture did not exercise — every dot run in it was
 * word-adjacent.
 */

use pampa::pandoc::{ASTContext, treesitter_to_pandoc};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use pampa::writers;
use tree_sitter_qmd::MarkdownParser;

/// Parse qmd and render it to the `native` writer's output.
fn to_native(input: &str) -> String {
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");
    let mut buf = Vec::new();
    let mut error_collector = DiagnosticCollector::new();
    writers::native::write(
        &treesitter_to_pandoc(
            &mut std::io::sink(),
            &tree,
            input_bytes,
            &ASTContext::anonymous(),
            &mut error_collector,
        )
        .unwrap(),
        &ASTContext::anonymous(),
        &mut buf,
    )
    .unwrap();
    String::from_utf8(buf).expect("Invalid UTF-8 in output")
}

const ELL: &str = "\u{2026}";

/// A dot run converts regardless of what precedes it.
///
/// The `-`- and smart-quote-preceded rows are the diagnostic ones: those
/// characters were already in `startStrRegex`, so they passed even before the
/// fix. They are kept as controls — if they ever start failing, the token-start
/// class regressed rather than the dot handling.
#[test]
fn test_ellipsis_converts_in_every_position() {
    let cases = [
        // (input, expected Str content)
        ("the ... menu", format!("Str \"{ELL}\"")), // space-preceded
        ("(...)", format!("Str \"({ELL})\"")),      // paren-preceded
        ("... and then", format!("Str \"{ELL}\"")), // block start
        ("a...b", format!("Str \"a{ELL}b\"")),      // word-adjacent (control)
        ("Wait for it...", format!("Str \"it{ELL}\"")), // trailing (control)
        ("1...2", format!("Str \"1{ELL}2\"")),      // digit-adjacent (control)
        ("a,...b", format!("Str \"a,{ELL}b\"")),    // comma-adjacent (control)
    ];

    for (input, expected) in cases {
        let ast = to_native(input);
        assert!(
            ast.contains(&expected),
            "input {input:?} should produce {expected}, got:\n{ast}"
        );
    }
}

/// Emphasis-wrapped dot runs convert. This is the shape the Connect docs use
/// ("the **...** menu"), and it is space-preceded once the delimiters are
/// stripped.
#[test]
fn test_ellipsis_converts_inside_emphasis() {
    for input in ["**...**", "*...*"] {
        let ast = to_native(input);
        assert!(
            ast.contains(&format!("Str \"{ELL}\"")),
            "input {input:?} should contain an ellipsis, got:\n{ast}"
        );
    }
}

/// Pandoc's rule is "three at a time, remainder literal" — so four dots become
/// an ellipsis plus one literal dot, and five become an ellipsis plus two.
/// Verified against `pandoc -f markdown -t native`, which emits `Str "\8230."`
/// for four dots.
#[test]
fn test_dot_run_remainder_stays_literal() {
    let cases = [
        ("four ....", format!("Str \"{ELL}.\"")),
        ("five .....", format!("Str \"{ELL}..\"")),
        ("six ......", format!("Str \"{ELL}{ELL}\"")),
    ];

    for (input, expected) in cases {
        let ast = to_native(input);
        assert!(
            ast.contains(&expected),
            "input {input:?} should produce {expected}, got:\n{ast}"
        );
    }
}

/// Runs shorter than three dots stay literal in every position — grouping dot
/// runs into a single token must not make `..` convert to anything.
#[test]
fn test_short_dot_runs_stay_literal() {
    let cases = [
        ("two .. dots", "Str \"..\""),
        ("one . dot", "Str \".\""),
        ("path ../foo here", "Str \"../foo\""),
    ];

    for (input, expected) in cases {
        let ast = to_native(input);
        assert!(
            ast.contains(expected),
            "input {input:?} should produce {expected}, got:\n{ast}"
        );
    }
}

/// The escape invariant. `a\.\.\.b` arrives from tree-sitter as separate
/// backslash-plus-dot nodes, so no single node ever holds a run of three; the
/// text must stay literal. This is the property that forces smart typography to
/// be applied per prose-str node rather than after `merge_strs`, and grouping
/// *unescaped* dot runs must not disturb it.
#[test]
fn test_escaped_dot_runs_stay_literal() {
    let cases = [
        ("a\\.\\.\\.b", "Str \"a...b\""),
        ("the \\.\\.\\. menu", "Str \"...\""),
    ];

    for (input, expected) in cases {
        let ast = to_native(input);
        assert!(
            ast.contains(expected),
            "input {input:?} should stay literal as {expected}, got:\n{ast}"
        );
        assert!(
            !ast.contains(ELL),
            "input {input:?} must not produce an ellipsis, got:\n{ast}"
        );
    }
}

/// Code spans are verbatim: smart typography never applies inside them.
#[test]
fn test_code_spans_are_untouched() {
    let ast = to_native("code `...` and `x...y` here");
    assert!(
        !ast.contains(ELL),
        "code spans must not be smart-converted, got:\n{ast}"
    );
}

/// Dashes were always position-independent (`--` lexes as one node because `-`
/// is in the token-start class). Kept as the control that isolates the defect
/// to the dot path.
#[test]
fn test_dashes_convert_in_every_position() {
    let cases = [
        ("x -- y", "Str \"\u{2013}\""),
        ("x --- y", "Str \"\u{2014}\""),
        ("en--dash", "Str \"en\u{2013}dash\""),
    ];

    for (input, expected) in cases {
        let ast = to_native(input);
        assert!(
            ast.contains(expected),
            "input {input:?} should produce {expected}, got:\n{ast}"
        );
    }
}

/// A dot run must survive the qmd writer round-trip.
///
/// The writer emits the ellipsis back as the three ASCII dots `...` rather than
/// the literal U+2026, and relies on the reader re-converting them. That is only
/// sound *because* of this fix: before it, a space-preceded `...` reparsed as a
/// literal three-dot run, so writing an ellipsis out and reading it back lost
/// the character. So this asserts round-trip stability of the AST, not the exact
/// bytes the writer chooses.
#[test]
fn test_ellipsis_qmd_roundtrip() {
    let input = "the ... menu\n";
    let mut parser = MarkdownParser::default();
    let input_bytes = input.as_bytes();
    let tree = parser
        .parse(input_bytes, None)
        .expect("Failed to parse input");
    let mut error_collector = DiagnosticCollector::new();
    let doc = treesitter_to_pandoc(
        &mut std::io::sink(),
        &tree,
        input_bytes,
        &ASTContext::anonymous(),
        &mut error_collector,
    )
    .unwrap();

    // The source AST holds a real ellipsis.
    let native = to_native(input);
    assert!(
        native.contains(&format!("Str \"{ELL}\"")),
        "reading should produce an ellipsis, got:\n{native}"
    );

    let mut buf = Vec::new();
    writers::qmd::write(&doc, &mut buf).unwrap();
    let written = String::from_utf8(buf).expect("Invalid UTF-8 in output");

    // Re-reading what the writer produced must yield the same AST — whichever
    // spelling it chose.
    let reparsed = to_native(&written);
    assert_eq!(
        native, reparsed,
        "qmd round-trip changed the AST; writer emitted {written:?}"
    );
    assert!(
        reparsed.contains(&format!("Str \"{ELL}\"")),
        "round-tripped qmd should still hold an ellipsis, got:\n{reparsed}"
    );
}
