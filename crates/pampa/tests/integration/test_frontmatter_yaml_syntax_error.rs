/*
 * test_frontmatter_yaml_syntax_error.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Regression coverage for bd-x30aq7ae.
 *
 * Malformed YAML frontmatter used to surface as Q-0-99 ("a bug in Quarto"),
 * with `(at crates/pampa/src/utils/diagnostic_collector.rs:NN)` in the title
 * and a span over the whole YAML block. It must be Q-1-1 "YAML Syntax
 * Error", with no internal path, located at the character where the YAML
 * scanner gave up (quarto-yaml >= 0.4.0 carries that location).
 */

use pampa::readers;
use quarto_error_reporting::DiagnosticMessage;

/// Read `input` and return every diagnostic, whether the read failed or not.
fn diagnostics(input: &str) -> Vec<DiagnosticMessage> {
    let mut sink = std::io::sink();
    match readers::qmd::read(input.as_bytes(), false, "test.qmd", &mut sink, true, None) {
        Ok((_doc, _ctx, warnings)) => warnings,
        Err(errs) => errs,
    }
}

fn yaml_syntax_error(input: &str) -> DiagnosticMessage {
    let diags = diagnostics(input);
    let mut q11: Vec<_> = diags
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-1-1"))
        .cloned()
        .collect();
    assert_eq!(q11.len(), 1, "expected exactly one Q-1-1, got: {diags:#?}");
    q11.remove(0)
}

/// The byte offset in `input` at which the diagnostic's location starts.
fn start_byte(diag: &DiagnosticMessage) -> usize {
    let location = diag.location.as_ref().expect("diagnostic has a location");
    let (_file, start, _end) = location
        .resolve_byte_range()
        .expect("location resolves to a byte range");
    start
}

fn assert_no_internal_path(diag: &DiagnosticMessage) {
    let text = diag.to_text(None);
    assert!(
        !text.contains("(at ") && !text.contains(".rs:"),
        "diagnostic leaks an internal source path: {text}"
    );
}

#[test]
fn frontmatter_yaml_syntax_error_is_q_1_1_at_the_offending_character() {
    // The unclosed flow sequence makes the scanner fail at the `:` of
    // `format: html` (file line 4, column 7).
    let input = "---\ntitle: \"Hello\"\nauthor: [a, b\nformat: html\n---\n\nBody text.\n";
    let diag = yaml_syntax_error(input);

    assert_eq!(diag.title, "YAML Syntax Error");
    assert_no_internal_path(&diag);
    let problem = diag.problem.as_ref().expect("problem statement").as_str();
    assert!(
        problem.contains("illegal placement of ':' indicator"),
        "problem should carry the scanner's message, got: {problem}"
    );
    assert!(
        !problem.contains(" at byte ") && !problem.contains(" line "),
        "problem should not repeat a position the span already carries: {problem}"
    );

    let expected = input.find("format:").unwrap() + "format".len();
    assert_eq!(start_byte(&diag), expected);
}

#[test]
fn frontmatter_yaml_syntax_error_location_is_byte_accurate_after_non_ascii() {
    // Multi-byte characters before the error: the scanner counts chars, the
    // span must be in bytes.
    let input = "---\ntitle: \"héllo ✓\"\nauthor: [a, b\nformat: html\n---\n";
    let diag = yaml_syntax_error(input);

    let expected = input.find("format:").unwrap() + "format".len();
    assert_eq!(start_byte(&diag), expected);
}

#[test]
fn frontmatter_yaml_syntax_error_at_end_of_block_is_in_bounds() {
    // Unclosed flow sequence on the last YAML line: the scanner reports at
    // end of input, which must still map inside the document.
    let input = "---\ntitle: x\nauthor: [a, b\n---\n";
    let diag = yaml_syntax_error(input);

    assert_no_internal_path(&diag);
    let start = start_byte(&diag);
    let yaml_start = "---\n".len();
    let yaml_end = input.rfind("---").unwrap();
    assert!(
        (yaml_start..=yaml_end).contains(&start),
        "start {start} outside the YAML body {yaml_start}..={yaml_end}"
    );
}
