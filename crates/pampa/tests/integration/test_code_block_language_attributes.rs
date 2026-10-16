// Q-2-57: a fenced code block's opening line takes one language word or one
// attribute list, not both. ```python {.marimo} gets a coded diagnostic whose
// hint is the author's own list with the language folded in; shapes that are
// not that mistake keep the generic parse error.

use pampa::readers;

fn diagnostics_for(input: &str) -> Vec<quarto_error_reporting::DiagnosticMessage> {
    match readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    ) {
        Ok(_) => panic!("expected a parse error for {input:?}"),
        Err(diagnostics) => diagnostics,
    }
}

fn first_with_code<'a>(
    diagnostics: &'a [quarto_error_reporting::DiagnosticMessage],
    code: &str,
) -> Option<&'a quarto_error_reporting::DiagnosticMessage> {
    diagnostics.iter().find(|d| d.code.as_deref() == Some(code))
}

#[test]
fn language_then_braces_gets_a_hint_in_both_forms() {
    for opening in [
        "```python {.cls #id key=val}",
        "```python{.cls #id key=val}",
    ] {
        let diagnostics = diagnostics_for(&format!("{opening}\ncode\n```\n"));
        let diag = first_with_code(&diagnostics, "Q-2-57")
            .unwrap_or_else(|| panic!("no Q-2-57 for {opening:?}: {diagnostics:?}"));
        assert_eq!(
            diag.hints.iter().map(|h| h.as_str()).collect::<Vec<_>>(),
            [
                "Put the language inside the braces: `{#id .python .cls key=val}` for a code \
                 block, or `{python #id .cls key=val}` for an executable cell."
            ],
            "{opening:?}"
        );
    }
}

#[test]
fn braceless_attributes_keep_the_generic_error() {
    let diagnostics = diagnostics_for("```python .marimo\ncode\n```\n");
    assert!(
        first_with_code(&diagnostics, "Q-2-57").is_none(),
        "{diagnostics:?}"
    );
    assert!(!diagnostics.is_empty());
}

#[test]
fn two_identifiers_keep_the_generic_error() {
    let diagnostics = diagnostics_for("```python {#a #b}\ncode\n```\n");
    assert!(
        first_with_code(&diagnostics, "Q-2-57").is_none(),
        "{diagnostics:?}"
    );
}

#[test]
fn braces_only_fence_is_unaffected() {
    assert!(
        readers::qmd::read(
            b"```{python .marimo}\ncode\n```\n",
            false,
            "test.qmd",
            &mut std::io::sink(),
            true,
            None,
        )
        .is_ok()
    );
}
