//! Smoke tests for every built-in language: each grammar must
//! successfully build its `HighlightConfiguration` (i.e. its vendored
//! `highlights.scm` must parse against the grammar) and return a
//! non-empty span list for a representative snippet.
//!
//! Not a correctness test — that lives in the per-language golden
//! snapshots (task #18). This just guards against build-time / query-
//! parse-time regressions.

use quarto_highlight::{encoding, highlight, is_language_supported};

/// (class, source snippet, expected at least one capture name present).
/// The expected capture is a string that MUST appear as some span's
/// `capture` in the output. Keep this conservative — pick one capture
/// the grammar obviously emits for the given input.
fn cases() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("bash", "echo \"hi\"\n", "string"),
        ("css", "p { color: red; }\n", "property"),
        ("html", "<p class=\"x\">hi</p>\n", "tag"),
        ("javascript", "const x = 1;\n", "keyword"),
        ("jsx", "const e = <div>hi</div>;\n", "keyword"),
        ("json", r#"{"name": "value"}"#, "string"),
        ("julia", "x = 1\n", "number"),
        ("lua", "local x = 1\n", "keyword"),
        ("python", "def foo(): pass\n", "keyword"),
        ("py", "def foo(): pass\n", "keyword"),
        ("r", "x <- 1\n", "operator"),
        ("sql", "SELECT 1;\n", "keyword"),
        ("tsx", "const e = <div>hi</div>;\n", "keyword"),
        ("typescript", "const x: number = 1;\n", "keyword"),
        ("ts", "const x: number = 1;\n", "keyword"),
        ("yaml", "\"quoted\": value\n", "string"),
        ("yml", "\"quoted\": value\n", "string"),
    ]
}

#[test]
fn every_case_class_resolves_to_a_grammar() {
    for (class, _, _) in cases() {
        assert!(
            is_language_supported(class),
            "language class `{class}` should be registered"
        );
    }
}

#[test]
fn every_grammar_parses_its_query_and_emits_expected_capture() {
    for (class, source, expected_capture) in cases() {
        let encoded = highlight(class, source)
            .unwrap_or_else(|e| panic!("`{class}` highlight errored: {e}"))
            .unwrap_or_else(|| panic!("`{class}` is registered but returned None"));

        let spans = encoding::decode(&encoded)
            .unwrap_or_else(|e| panic!("`{class}` output is not valid JSON: {e}"));

        assert!(
            !spans.is_empty(),
            "`{class}` produced no spans for input {source:?}",
        );

        let has = spans.iter().any(|s| s.capture == *expected_capture);
        assert!(
            has,
            "`{class}` should have at least one `{expected_capture}` span; spans: {spans:?}",
        );
    }
}

#[test]
fn unknown_language_still_returns_none() {
    assert!(highlight("klingon", "K'tah!").unwrap().is_none());
}
