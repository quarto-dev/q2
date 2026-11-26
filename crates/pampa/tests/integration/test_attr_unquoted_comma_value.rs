/*
 * test_attr_unquoted_comma_value.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Q-2-61: an unquoted attribute value containing a comma (`widths=1,3`,
 * GH #1339). The error is coded, and because it `desynchronizes`, the
 * follow-on errors at later `:::` fences are dropped. The corpus entry is
 * guarded on the comma: other characters the attribute lexer rejects fail
 * in the same LR state and must keep the generic error rather than a
 * message about commas.
 */

use pampa::readers;

fn error_codes(input: &str) -> Vec<Option<String>> {
    match readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    ) {
        Ok(_) => vec![],
        Err(diags) => diags.into_iter().map(|d| d.code).collect(),
    }
}

#[test]
fn issue_repro_reports_one_coded_error() {
    let input = "::: {.list-table widths=1,3}\n- - A\n  - B\n:::\n\nText.\n\n::: {.note}\nLater div.\n:::\n";
    assert_eq!(error_codes(input), vec![Some("Q-2-61".to_string())]);
}

#[test]
fn coded_on_every_attribute_host() {
    for input in [
        "A [x]{aligns=l,c} b\n",
        "## Heading {widths=1,3}\n",
        "![x](y.png){a=1,3}\n",
        "`x`{a=1,3}\n",
        "A [x]{#id .cls a=\"q\" b=1,3} b\n",
    ] {
        assert_eq!(
            error_codes(input),
            vec![Some("Q-2-61".to_string())],
            "input: {input:?}"
        );
    }
}

#[test]
fn other_rejected_characters_are_not_claimed() {
    for input in ["A [x]{a=l;c} b\n", "A [x]{a=l<c} b\n"] {
        let codes = error_codes(input);
        assert!(!codes.is_empty(), "expected a parse error for {input:?}");
        assert!(
            !codes.contains(&Some("Q-2-61".to_string())),
            "{input:?} should not be reported as Q-2-61, got {codes:?}"
        );
    }
}

#[test]
fn quoted_value_parses() {
    assert!(error_codes("::: {.list-table widths=\"1,3\"}\nx\n:::\n").is_empty());
}
