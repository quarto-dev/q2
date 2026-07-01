/*
 * test_code_block_writer_roundtrip.rs
 *
 * Round-trip tests (qmd -> AST -> qmd) for the bracket-wrapped language
 * pseudo-class the parser produces for space-separated `{lang .cls}`
 * code fences: `{python .marimo}` parses to CodeBlock classes
 * `["{python}", "marimo"]` (see `test_code_block_attributes.rs`,
 * `crates/quarto-core/src/engine/capture_splice.rs::engine_cell_lang`).
 * The QMD writer must mirror that encoding on the way back out — the
 * bracket-wrapped class is the *language*, and belongs in the fence as a
 * bare, unprefixed first token, not a dot-prefixed class.
 *
 * Copyright (c) 2026 Posit, PBC
 */

use pampa::readers;
use pampa::writers;

fn qmd_roundtrip(input: &str) -> String {
    let (doc, _context, _warnings) = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
    .expect("Failed to parse QMD");

    let mut buf = Vec::new();
    writers::qmd::write(&doc, &mut buf).expect("Failed to write QMD");
    String::from_utf8(buf).expect("Invalid UTF-8 in QMD writer output")
}

#[test]
fn space_separated_lang_and_class_roundtrips_identical() {
    // THE bug: `{python .marimo}` must come back byte-identical, not
    // `{.{python} .marimo}`.
    let input = "```{python .marimo}\n1 + 1\n```\n";
    assert_eq!(qmd_roundtrip(input), input);
}

#[test]
fn space_separated_sql_lang_and_class_roundtrips_identical() {
    let input = "```{sql .marimo}\nSELECT 1\n```\n";
    assert_eq!(qmd_roundtrip(input), input);
}

#[test]
fn space_separated_lang_class_and_keyval_roundtrips_identical() {
    let input = "```{python .marimo foo=\"bar\"}\n1 + 1\n```\n";
    assert_eq!(qmd_roundtrip(input), input);
}

#[test]
fn bare_lang_fast_path_still_roundtrips_identical() {
    // Regression guard: a single bracket-wrapped class with no other
    // attributes already round-trips via write_codeblock's bare-word
    // fast path (write_attr is never involved). Must keep working.
    let input = "```{python}\n1 + 1\n```\n";
    assert_eq!(qmd_roundtrip(input), input);
}

#[test]
fn dotted_lang_form_still_roundtrips_identical() {
    // Regression guard: `{python.marimo}` parses to a single class
    // `"{python.marimo}"` and also hits the bare-word fast path.
    let input = "```{python.marimo}\n1 + 1\n```\n";
    assert_eq!(qmd_roundtrip(input), input);
}

#[test]
fn plain_div_multiple_classes_unaffected() {
    // Regression guard for generic `write_attr`: a div's classes are
    // plain (never bracket-wrapped by the parser), so they must keep
    // being dot-prefixed exactly as before this fix.
    let input = "::: {.python .marimo}\n\ncontent\n\n:::\n";
    let output = qmd_roundtrip(input);
    assert!(
        output.contains(".python .marimo"),
        "expected dot-prefixed classes unaffected by the codeblock/inline-code fix, got:\n{output}"
    );
}
