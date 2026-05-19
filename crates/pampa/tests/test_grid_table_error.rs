use pampa::readers;
use quarto_error_reporting::DiagnosticKind;

fn parse_and_get_diagnostics(input: &str) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );

    match result {
        Ok((_, _, diagnostics)) => diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

/// Reconstruct the captured grid-table text from the diagnostic. The
/// diagnostic anchors on the first line (so Ariadne renders a clean
/// single-line main label) and uses per-line `note_at` details for the
/// remaining lines (so every body line shows in the snippet instead of
/// being elided to `┆`). To recover the full table, we union the main
/// location with all detail locations and slice the input accordingly.
fn captured_text<'a>(
    diag: &quarto_error_reporting::DiagnosticMessage,
    full_input: &'a str,
) -> &'a str {
    let main = diag
        .location
        .as_ref()
        .expect("grid_table diagnostic must carry a source location");
    let mut start = main.start_offset();
    let mut end = main.end_offset();
    for detail in &diag.details {
        if let Some(loc) = &detail.location {
            start = start.min(loc.start_offset());
            end = end.max(loc.end_offset());
        }
    }
    &full_input[start..end]
}

fn assert_grid_table_diagnostic(
    diagnostics: &[quarto_error_reporting::DiagnosticMessage],
    expected_capture: &str,
    full_input: &str,
) {
    let grid = diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-2-38"))
        .unwrap_or_else(|| {
            panic!(
                "no Q-2-38 diagnostic found. all diagnostics: {:?}",
                diagnostics
                    .iter()
                    .map(|d| (d.code.clone(), d.title.clone(), d.kind))
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        grid.kind,
        DiagnosticKind::Error,
        "grid_table diagnostic must be an error, got {:?}",
        grid.kind
    );

    let captured = captured_text(grid, full_input);
    assert_eq!(
        captured.trim_end(),
        expected_capture.trim_end(),
        "captured text {captured:?} does not match expected {expected_capture:?}"
    );
}

#[test]
fn plain_grid_table_emits_q_2_38_with_full_text() {
    let input = "+----+\n| oh |\n+----+\n";
    let diagnostics = parse_and_get_diagnostics(input);
    assert_grid_table_diagnostic(&diagnostics, "+----+\n| oh |\n+----+", input);
}

#[test]
fn grid_table_inside_block_quote_emits_q_2_38() {
    let input = "> +----+\n> | oh |\n> +----+\n";
    let diagnostics = parse_and_get_diagnostics(input);
    let grid = diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-2-38"))
        .expect("expected Q-2-38 diagnostic for grid table inside block quote");
    assert_eq!(grid.kind, DiagnosticKind::Error);

    let captured = captured_text(grid, input);
    assert!(
        captured.contains("+----+") && captured.contains("| oh |"),
        "captured grid-table text {captured:?} should contain both border and body lines"
    );
}

#[test]
fn nested_block_quotes_with_two_grid_tables_emit_two_q_2_38() {
    let input = "> > +----+\n> > | oh |\n> > +----+\n> | pipe-table-now |\n> |-|\n> | no |\n> > +----+\n> > | no |\n> > +----+\n";
    let diagnostics = parse_and_get_diagnostics(input);
    let grid_diagnostics: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-2-38"))
        .collect();
    assert_eq!(
        grid_diagnostics.len(),
        2,
        "expected exactly two Q-2-38 diagnostics (one per nested grid table), got {}",
        grid_diagnostics.len()
    );

    for grid in &grid_diagnostics {
        assert_eq!(grid.kind, DiagnosticKind::Error);
        let captured = captured_text(grid, input);
        assert!(
            captured.contains("+----+"),
            "captured text {captured:?} should contain a grid-table border"
        );
    }
}

#[test]
fn lone_border_line_does_not_emit_grid_table_diagnostic() {
    // A single "+----+" line is just a paragraph string and should not trip the
    // grid-table detector.
    let input = "+----+\n";
    let diagnostics = parse_and_get_diagnostics(input);
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-38")),
        "lone border line should not be flagged as a grid table"
    );
}

#[test]
fn grid_table_with_equals_separator_emits_q_2_38() {
    let input = "+====+\n| oh |\n+----+\n";
    let diagnostics = parse_and_get_diagnostics(input);
    assert_grid_table_diagnostic(&diagnostics, "+====+\n| oh |\n+----+", input);
}
