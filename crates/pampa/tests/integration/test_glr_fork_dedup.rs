/*
 * test_glr_fork_dedup.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Regression test for GitHub issue #230: after an earlier error forks the
 * GLR parse, two versions can fail at the same source position with
 * different lookahead symbols. Only one of them has a corpus entry, and
 * the diagnostic dedup must keep that one rather than whichever version
 * happens to be visited first (which produced a generic "Parse error").
 */

use pampa::readers;

fn diagnostic_codes(input: &str, prune_errors: bool) -> Vec<Option<String>> {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "t.qmd",
        &mut std::io::sink(),
        prune_errors,
        None,
    );
    result
        .expect_err("input must produce parse errors")
        .iter()
        .map(|d| d.code.clone())
        .collect()
}

#[test]
fn unclosed_emphasis_after_forking_error_keeps_its_q_code() {
    // The attribute-list error on line 1 (Q-2-3) forks the parse; both
    // versions then reach the unclosed '*' on line 3.
    let input = "# Heading {key=val .class}\n\nSecond *bad paragraph.\n";
    for prune_errors in [false, true] {
        let codes = diagnostic_codes(input, prune_errors);
        assert!(
            codes.contains(&Some("Q-2-12".to_string())),
            "expected Q-2-12 for the unclosed '*' (prune_errors={prune_errors}), got {codes:?}"
        );
        assert!(
            !codes.contains(&None),
            "no generic \"Parse error\" expected (prune_errors={prune_errors}), got {codes:?}"
        );
    }
}
