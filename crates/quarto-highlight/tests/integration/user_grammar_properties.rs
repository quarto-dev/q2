//! End-to-end test for the native user-grammar loader: load
//! `tree-sitter-properties` (Java `.properties` files) from a fixture
//! directory (a `.wasm` grammar + a `highlights.scm`), highlight a tiny
//! document, and verify that the JSON triple-array encoding contains the
//! expected captures. `properties` is NOT in the built-in registry, so this
//! unambiguously exercises the dynamic loader rather than a statically
//! linked grammar.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use quarto_highlight::{encoding, is_language_supported};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/user-grammar-properties")
}

#[test]
fn properties_is_not_a_built_in() {
    // Sanity: if `properties` were ever added to the built-in registry we'd
    // want to switch this test to a different non-built-in fixture. Loud
    // failure is better than silent confusion. (This fixture used TOML
    // until TOML became a built-in, bd-z6xw3psd.)
    assert!(
        !is_language_supported("properties"),
        "`properties` is unexpectedly a built-in language; pick a different fixture grammar",
    );
}

#[test]
fn user_grammar_loads_and_highlights_properties() {
    let mut user = quarto_highlight::UserGrammars::new();
    let class = user
        .load_from_directory(fixture_dir())
        .expect("properties fixture should load");
    assert_eq!(class, "properties");

    let source = "# server\nhost = localhost\n";
    let encoded = quarto_highlight::highlight_with_user("properties", source, Some(&mut user))
        .expect("properties highlight should succeed")
        .expect("properties should resolve through the user-grammar set");

    let spans = encoding::decode(&encoded).expect("output must be valid JSON");

    // Assert only on the *presence* of the captures this input must
    // produce: the test proves the loader + highlighter produced meaningful
    // output; fine-grained correctness is the grammar's concern.
    assert!(
        spans.iter().any(|s| s.start == 0 && s.capture == "comment"),
        "expected a comment span at byte 0; got: {spans:?}",
    );
    assert!(
        spans
            .iter()
            .any(|s| s.start == 9 && s.end == 13 && s.capture == "property"),
        "expected `host` as a property; got: {spans:?}",
    );
    assert!(
        spans.iter().any(|s| s.capture == "operator"),
        "expected an operator capture for `=`; got: {spans:?}",
    );
    assert!(
        spans.iter().any(|s| s.start == 16 && s.capture == "string"),
        "expected the value as a string starting at byte 16; got: {spans:?}",
    );
}

#[test]
fn built_in_highlight_still_works_when_no_user_grammars() {
    // Passing None (or an empty UserGrammars) should behave identically
    // to the existing built-in-only path.
    let encoded = quarto_highlight::highlight_with_user("python", "def foo(): pass\n", None)
        .expect("should not error")
        .expect("python is a built-in");
    let spans = encoding::decode(&encoded).unwrap();
    assert!(!spans.is_empty());
}
