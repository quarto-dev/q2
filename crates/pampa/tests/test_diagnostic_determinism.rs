/*
 * test_diagnostic_determinism.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Regression test for GitHub issue #222 (bd-hwdlq): pampa diagnostic
 * output must be deterministic across runs on the same input.
 *
 * The underlying bug was that GLR parse versions were held in a
 * `HashMap<usize, TreeSitterProcessLog>` and iterated by `.values()`
 * in `produce_diagnostic_messages`. A `(row, column)` dedupe meant
 * whichever GLR version was iterated first won — and HashMap with
 * default RandomState varies that order per process.
 *
 * Each call to `readers::qmd::read` creates fresh `HashMap`s with
 * fresh seeds, so N independent invocations exercise N independent
 * iteration orders. Comparing the rendered diagnostic text across
 * N runs catches the bug with overwhelming probability.
 */

use pampa::readers;

fn render_diagnostics(input: &str) -> String {
    let input_bytes = input.as_bytes();
    let mut output = Vec::new();
    let result = readers::qmd::read(input_bytes, false, "test.qmd", &mut output, false, None);

    let diagnostics = result.expect_err("issue-222 input must produce parse errors");

    let mut source_context = quarto_source_map::SourceContext::new();
    source_context.add_file("test.qmd".to_string(), Some(input.to_string()));

    diagnostics
        .iter()
        .map(|d| d.to_text(Some(&source_context)))
        .collect::<Vec<_>>()
        .join("\n---\n")
}

#[test]
fn issue_222_diagnostics_are_deterministic_across_runs() {
    // Issue #222 repro: this input triggers a GLR parse with 3 concurrent
    // versions, all of which detect_error at (row=0, col=18). The dedupe
    // by (row, column) then picks exactly one — and before the fix, which
    // one varied per process.
    let input = "The \"_blank\" word.\n";

    // 50 runs. At the originally observed ~63/37 split between the two
    // variants, the probability that all 50 runs *happen* to land on the
    // same variant by chance is (0.63)^50 + (0.37)^50 < 1e-10. So this
    // test, if the bug is present, fails essentially every time.
    let runs = 50;
    let first = render_diagnostics(input);
    for i in 2..=runs {
        let next = render_diagnostics(input);
        assert_eq!(
            first, next,
            "Diagnostic output for issue #222 input must be identical across runs;\n\
             run 1 vs run {i} differs.\n\
             === run 1 ===\n{first}\n\
             === run {i} ===\n{next}\n"
        );
    }
}
