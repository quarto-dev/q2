use pampa::readers;

fn parse_err(input: &str) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    match result {
        Ok((_pandoc, _context, warnings)) => panic!(
            "expected parse to fail, but it succeeded; warnings: {:?}",
            warnings
                .iter()
                .map(|w| w.code.as_deref())
                .collect::<Vec<_>>()
        ),
        Err(diags) => diags,
    }
}

#[test]
fn test_blockquote_multiline_image_attrs_emits_q_2_37() {
    let input = "> ![](img.png){\n>   .cls1\n>   width=\"200px\"\n> }\n";
    let diagnostics = parse_err(input);

    let q237: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_deref() == Some("Q-2-37"))
        .collect();

    assert_eq!(
        q237.len(),
        1,
        "expected exactly one Q-2-37; got diagnostics: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );

    let diag = q237[0];
    assert_eq!(
        diag.title,
        "Multi-line inline attribute list inside blockquote"
    );
    assert!(!diag.hints.is_empty(), "Q-2-37 should include a hint");
}

#[test]
fn test_blockquote_multiline_span_attrs_emits_q_2_37() {
    let input = "> a [text]{\n>   .cls\n> } end\n";
    let diagnostics = parse_err(input);

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-37")),
        "expected a Q-2-37 diagnostic; got: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_blockquote_with_leading_indent_emits_q_2_37() {
    let input = "   > ![](img.png){\n   >   .cls\n   > }\n";
    let diagnostics = parse_err(input);

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-37")),
        "leading whitespace before `>` should still upgrade to Q-2-37; got: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_toplevel_unclosed_attr_stays_q_2_2() {
    // A regular unclosed `{[` outside a blockquote must remain Q-2-2, not be
    // mistakenly upgraded to Q-2-37 by the contextual check.
    let input = "A bad [attribute]{[\n";
    let diagnostics = parse_err(input);

    assert!(
        diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-2")),
        "top-level unclosed attribute must remain Q-2-2; got: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-37")),
        "top-level case must not produce Q-2-37; got: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
}
