//! End-to-end test for the native user-grammar loader: load
//! `tree-sitter-toml` from a fixture directory (a `.wasm` grammar + a
//! `highlights.scm`), highlight a tiny TOML document, and verify that the
//! JSON triple-array encoding contains expected captures. TOML is NOT in
//! the built-in registry, so this unambiguously exercises the dynamic
//! loader rather than a statically-linked grammar.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use quarto_highlight::{encoding, is_language_supported};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/user-grammar-toml")
}

#[test]
fn toml_is_not_a_built_in() {
    // Sanity: if TOML were ever added to the built-in registry we'd want
    // to switch this test to a different non-built-in fixture. Loud
    // failure is better than silent confusion.
    assert!(
        !is_language_supported("toml"),
        "`toml` is unexpectedly a built-in language; pick a different fixture grammar",
    );
}

#[test]
fn user_grammar_loads_and_highlights_toml() {
    let mut user = quarto_highlight::UserGrammars::new();
    let class = user
        .load_from_directory(fixture_dir())
        .expect("toml fixture should load");
    assert_eq!(class, "toml");

    let source = "name = \"value\"\n";
    let encoded = quarto_highlight::highlight_with_user("toml", source, Some(&mut user))
        .expect("toml highlight should succeed")
        .expect("toml should resolve through the user-grammar set");

    let spans = encoding::decode(&encoded).expect("output must be valid JSON");

    // Exact span boundaries are a function of the grammar's node
    // hierarchy and the query patterns — we trust tree-sitter-toml
    // upstream and assert only on the *presence* of the captures we
    // expect for this input. Test purpose: prove the loader + highlighter
    // produced a meaningful JSON output; fine-grained correctness is
    // the grammar's concern, not this crate's.

    // `=` should produce an `operator` capture.
    assert!(
        spans.iter().any(|s| s.capture == "operator"),
        "expected an operator capture for `=`; got: {spans:?}",
    );

    // `"value"` should produce a `string` capture starting at byte 7.
    assert!(
        spans.iter().any(|s| s.start == 7 && s.capture == "string"),
        "expected a string span starting at byte 7; got: {spans:?}",
    );

    // The pair (or bare_key) should produce at least one of `property`
    // or `type` somewhere.
    assert!(
        spans
            .iter()
            .any(|s| s.capture == "property" || s.capture == "type"),
        "expected a property/type capture for the key; got: {spans:?}",
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
