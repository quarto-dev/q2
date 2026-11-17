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
        ("conf", "key = \"value\"\n", "string"),
        ("css", "p { color: red; }\n", "property"),
        ("dockerfile", "FROM ubuntu:22.04\n", "keyword"),
        ("Dockerfile", "FROM ubuntu:22.04\n", "keyword"),
        ("html", "<p class=\"x\">hi</p>\n", "tag"),
        ("ini", "[server]\nport = 8080\n", "property"),
        ("javascript", "const x = 1;\n", "keyword"),
        ("jsx", "const e = <div>hi</div>;\n", "keyword"),
        ("json", r#"{"name": "value"}"#, "string"),
        ("julia", "x = 1\n", "number"),
        ("lua", "local x = 1\n", "keyword"),
        ("python", "def foo(): pass\n", "keyword"),
        ("py", "def foo(): pass\n", "keyword"),
        ("powershell", "Write-Host \"hi\"\n", "string"),
        ("ps1", "Write-Host \"hi\"\n", "string"),
        ("r", "x <- 1\n", "operator"),
        ("rust", "fn main() {}\n", "keyword"),
        ("rs", "fn main() {}\n", "keyword"),
        ("sql", "SELECT 1;\n", "keyword"),
        ("toml", "name = \"x\"\n", "string"),
        ("tsx", "const e = <div>hi</div>;\n", "keyword"),
        ("typescript", "const x: number = 1;\n", "keyword"),
        ("ts", "const x: number = 1;\n", "keyword"),
        ("xml", "<a href=\"x\">t</a>\n", "tag"),
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

/// The ini and dockerfile upstream queries capture comments as
/// `(comment) @comment @spell`. `spell` is a Neovim spell-check directive,
/// not a highlight class; if it survived it would win the equal-extent
/// tie-break and leave every comment uncolored.
#[test]
fn spell_co_captures_do_not_hide_comments() {
    for (class, source, comment) in [
        ("ini", "; a comment\n[s]\nk = v\n", "; a comment"),
        ("dockerfile", "# a comment\nFROM x\n", "# a comment"),
    ] {
        let encoded = highlight(class, source)
            .unwrap_or_else(|e| panic!("`{class}` highlight errored: {e}"))
            .unwrap_or_else(|| panic!("`{class}` is not registered"));
        let spans = encoding::decode(&encoded).expect("valid JSON");
        let start = source.find(comment).unwrap();
        assert!(
            // The comment node may extend over the line break.
            spans.iter().any(|s| s.start == start
                && s.end >= start + comment.len()
                && s.capture == "comment"),
            "`{class}`: expected the comment as `comment`, got {spans:?}"
        );
        assert!(
            spans.iter().all(|s| s.capture != "spell"),
            "`{class}`: `spell` leaked into the spans: {spans:?}"
        );
    }
}
