//! Phase 1 TDD entry point: highlight a tiny Python snippet and assert
//! we get back a JSON triple-array with a recognizable `keyword` span
//! for `def` at bytes 0..3. More thorough per-language coverage lives in
//! task #18's golden snapshot tests.

use quarto_highlight::{encoding, highlight, is_language_supported};

#[test]
fn python_is_registered() {
    assert!(is_language_supported("python"));
    assert!(is_language_supported("py"));
}

#[test]
fn python_def_keyword_is_highlighted() {
    let source = "def foo(): pass";
    let encoded = highlight("python", source)
        .expect("python highlight should succeed")
        .expect("python should be a registered language");

    let spans = encoding::decode(&encoded).expect("output must be valid JSON triple array");

    // `def` should produce a keyword span at bytes 0..3.
    let def_span = spans
        .iter()
        .find(|s| s.start == 0 && s.end == 3)
        .unwrap_or_else(|| panic!("expected a span at 0..3, got: {spans:?}"));
    assert_eq!(def_span.capture, "keyword");

    // `pass` is at bytes 11..15 and is also a keyword.
    assert_eq!(&source[11..15], "pass");
    let pass_span = spans
        .iter()
        .find(|s| s.start == 11 && s.end == 15)
        .unwrap_or_else(|| panic!("expected a span at 11..15, got: {spans:?}"));
    assert_eq!(pass_span.capture, "keyword");
}

#[test]
fn unknown_language_returns_none() {
    let result = highlight("klingon", "K'tah!").expect("must not error");
    assert!(result.is_none());
}
