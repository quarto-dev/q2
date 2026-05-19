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

fn assert_diag(input: &str, expected_code: &str, why: &str) {
    let diagnostics = parse_err(input);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some(expected_code)),
        "expected {expected_code} ({why}); got: {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.as_deref())
            .collect::<Vec<_>>()
    );
    let diag = diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some(expected_code))
        .unwrap();
    assert!(
        !diag.details.is_empty(),
        "{expected_code} should anchor a note at the opening `{{` ({why})"
    );
}

// Cases that exercise distinct post-content parser states (2746, 2818,
// 3069) — Q-2-38 is the only diagnostic registered there.

#[test]
fn unclosed_attr_with_id_emits_q_2_38() {
    assert_diag("[text]{#my-id\n", "Q-2-38", "open + id content, no close");
}

#[test]
fn unclosed_attr_with_class_emits_q_2_38() {
    assert_diag("[text]{.cls\n", "Q-2-38", "open + class, no close");
}

#[test]
fn unclosed_attr_with_kv_emits_q_2_38() {
    assert_diag(
        "[text]{key=\"val\"\n",
        "Q-2-38",
        "open + key-value, no close",
    );
}

#[test]
fn unclosed_multi_line_attr_emits_q_2_38() {
    assert_diag(
        "[text]{\n  .cls\n",
        "Q-2-38",
        "multi-line attribute list, never closed",
    );
}

#[test]
fn unclosed_multi_line_image_attr_emits_q_2_38() {
    assert_diag(
        "![](img.png){\n  .cls\n",
        "Q-2-38",
        "image with multi-line attr, never closed",
    );
}

#[test]
fn blockquote_unclosed_attr_emits_q_2_38() {
    assert_diag(
        "> [text]{\n>   .cls\n",
        "Q-2-38",
        "after the e3b315bd scanner fix, an unclosed multi-line attr inside a blockquote \
         joins the top-level inline parse and should surface Q-2-38, not the generic fallback",
    );
}

#[test]
fn unclosed_attr_terminated_by_next_list_item_emits_q_2_38() {
    // A subsequent bullet starts a new list item, closing the prior paragraph.
    // The parser hits `_close_block` while still inside `{...}`, which should
    // surface Q-2-38 the same as any other end-of-block trigger.
    assert_diag(
        "* [hello]{.test1 .test2\n* test\n",
        "Q-2-38",
        "next list item closes the block while the attribute list is still unclosed",
    );
}

// Cases that hit (state=2587, sym=_close_block) — shared with the
// existing Q-2-2/simple `{[` case. Both shapes can't be distinguished at
// the LR table lookup level, so they fall under Q-2-2 ("Mismatched
// Delimiter in Attribute Specifier"). Q-2-2's anchor note still pins
// the opening `{` for the user.

#[test]
fn bare_unclosed_attr_emits_q_2_2() {
    assert_diag(
        "[text]{\n",
        "Q-2-2",
        "bare unclosed `{` shares an LR state with the `{[` mismatched-delimiter case; \
         resolves to Q-2-2",
    );
}

#[test]
fn bare_unclosed_image_attr_emits_q_2_2() {
    assert_diag(
        "![](img.png){\n",
        "Q-2-2",
        "image bare unclosed `{` shares the same LR state as the span case",
    );
}
