//! Golden snapshot tests covering every built-in language plus one
//! fixture user grammar. The snapshots live under
//! `tests/snapshots/` and are updated via `cargo insta review` when
//! grammar / query output intentionally changes. Unreviewed changes
//! will fail CI, catching accidental drift in upstream grammar crates
//! or in our own encoding logic.
//!
//! The fixture inputs are deliberately tiny — enough shape to exercise
//! a handful of captures per grammar, not a full correctness test.
//!
//! The per-language snippets are loaded from a shared JSON file under
//! `tests/fixtures/builtin-snippets.json`. That same file is consumed
//! by the WASM-side vitest harness in `hub-client/` so the two test
//! paths can't drift.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use quarto_highlight::{encoding, highlight, highlight_with_user};
use serde::Deserialize;

/// One fixture entry in `tests/fixtures/builtin-snippets.json`.
#[derive(Debug, Deserialize)]
struct Fixture {
    name: String,
    class: String,
    source: String,
}

fn load_fixtures() -> Vec<Fixture> {
    const RAW: &str = include_str!("../fixtures/builtin-snippets.json");
    serde_json::from_str(RAW).expect("builtin-snippets.json is valid JSON")
}

fn format_spans(spans: &[quarto_highlight::HighlightSpan]) -> String {
    // Pretty, review-friendly shape: one span per line, ordered by the
    // encoder. JSON keeps the form close to what's written to
    // `data-hl-spans` on the AST.
    serde_json::to_string_pretty(spans).expect("spans serialize")
}

fn check_builtin(fixture: &Fixture) {
    let encoded = highlight(&fixture.class, &fixture.source)
        .unwrap_or_else(|e| panic!("`{}` highlight errored: {e}", fixture.class))
        .unwrap_or_else(|| panic!("`{}` is not a registered class", fixture.class));
    let spans = encoding::decode(&encoded).expect("valid JSON");
    insta::with_settings!({ description => fixture.source.clone(), omit_expression => true }, {
        insta::assert_snapshot!(fixture.name.as_str(), format_spans(&spans));
    });
}

#[test]
fn golden_all_builtins() {
    let fixtures = load_fixtures();
    assert!(!fixtures.is_empty(), "fixtures should not be empty");
    for fixture in &fixtures {
        check_builtin(fixture);
    }
}

#[test]
fn golden_user_grammar_toml() {
    // A user-supplied grammar loaded dynamically from the
    // `user-grammar-toml` fixture. Snapshot tracks that dynamic loading
    // produces the same shape of output as static built-ins.
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/user-grammar-toml");
    let mut user = quarto_highlight::UserGrammars::new();
    user.load_from_directory(&fixture_dir)
        .expect("toml fixture should load");

    let source = "name = \"value\"\ncount = 42\n";
    let encoded = highlight_with_user("toml", source, Some(&mut user))
        .expect("toml highlight succeeds")
        .expect("toml resolves via user grammars");
    let spans = encoding::decode(&encoded).expect("valid JSON");

    insta::with_settings!({ description => source, omit_expression => true }, {
        insta::assert_snapshot!("user_grammar_toml", format_spans(&spans));
    });
}
