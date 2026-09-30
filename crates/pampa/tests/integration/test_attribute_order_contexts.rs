// Coverage tests for the attribute-order diagnostics Q-2-3, Q-2-55 and Q-2-56
// (bd-6hf7nz7i).
//
// q2's attribute grammar accepts exactly one order inside `{...}`: identifier,
// then classes, then key-value pairs. There are three pairwise inversions, and
// each one has its own code:
//
//   Q-2-55  class before identifier   `{.c #i}`
//   Q-2-56  key-value before id       `{k=v #i}`
//   Q-2-3   key-value before class    `{k=v .c}`
//
// The corpus is keyed on (LR state, lookahead), and the obvious worry is that
// each construct carrying an attribute list -- span, image, code span, the four
// editorial spans, heading, bare inline, fenced div, editorial div, fenced code
// block -- is its own state, the way the inline containers are for Q-2-41. The
// audit in bd-6hf7nz7i showed they are not: every construct reaches the same
// two states (after-class and after-kv) through the shared
// `commonmark_specifier` productions. This table is the reconciliation -- one
// row per construct, run against all three inversions -- so that a grammar
// change which splits a state shows up here rather than as an uncoded
// "Parse error" in someone's document.
//
// Each diagnostic also carries a hint built from the author's own text, e.g.
// "Write the attributes as `{#tip-alignment .callout-tip}`".

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

/// `ATTR` is replaced by the attribute text, braces included.
const CONSTRUCTS: &[(&str, &str)] = &[
    ("span", "[x]ATTR\n"),
    ("image", "![alt](img.png)ATTR\n"),
    ("code-span", "`x`ATTR\n"),
    ("insert", "[++ x]ATTR\n"),
    ("delete", "[-- x]ATTR\n"),
    ("edit-comment", "[>> x]ATTR\n"),
    ("highlight", "[!! x]ATTR\n"),
    ("heading", "# H ATTR\n"),
    ("bare-inline", "text ATTR\n"),
    ("fenced-div", "::: ATTR\nx\n:::\n"),
    ("editorial-div", "::: -- ATTR\nx\n:::\n"),
    ("fenced-code", "```ATTR\nx\n```\n"),
    ("pipe-table-cell", "| a |\n|---|\n| [x]ATTR |\n"),
];

/// (shape, misordered attributes, expected code, expected canonical order)
const SHAPES: &[(&str, &str, &str, &str)] = &[
    ("class-before-id", "{.c #i}", "Q-2-55", "{#i .c}"),
    ("kv-before-id", "{k=v #i}", "Q-2-56", "{#i k=v}"),
    ("kv-before-class", "{k=v .c}", "Q-2-3", "{.c k=v}"),
];

fn hint_texts(diag: &quarto_error_reporting::DiagnosticMessage) -> Vec<&str> {
    diag.hints.iter().map(|h| h.as_str()).collect()
}

#[test]
fn every_construct_reports_every_inversion_with_a_real_text_hint() {
    let mut failures = Vec::new();
    for (construct, template) in CONSTRUCTS {
        for (shape, attrs, code, canonical) in SHAPES {
            let input = template.replace("ATTR", attrs);
            let diagnostics = diagnostics_for(&input);
            let Some(diag) = diagnostics
                .iter()
                .find(|d| d.code.as_deref() == Some(*code))
            else {
                let got: Vec<_> = diagnostics.iter().map(|d| d.code.as_deref()).collect();
                failures.push(format!(
                    "{construct}/{shape}: expected {code}, got {got:?} for {input:?}"
                ));
                continue;
            };
            let expected = format!("`{canonical}`");
            if !hint_texts(diag).iter().any(|h| h.contains(&expected)) {
                failures.push(format!(
                    "{construct}/{shape}: no hint containing {expected}, got {:?}",
                    hint_texts(diag)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

#[test]
fn hint_reorders_the_authors_own_identifiers() {
    let diagnostics = diagnostics_for("::: {.callout-tip #tip-alignment}\nx\n:::\n");
    let diag = diagnostics
        .iter()
        .find(|d| d.code.as_deref() == Some("Q-2-55"))
        .expect("Q-2-55");
    assert!(
        hint_texts(diag)
            .iter()
            .any(|h| h.contains("`{#tip-alignment .callout-tip}`")),
        "got {:?}",
        hint_texts(diag)
    );
}

#[test]
fn hint_reorders_the_whole_list_at_once() {
    // Three things are wrong in `{k=v .c #i}`; the hint fixes all of them
    // rather than making the author re-run the parser twice.
    let diagnostics = diagnostics_for("[x]{k=v .c #i}\n");
    let diag = diagnostics
        .iter()
        .find(|d| d.code.is_some())
        .expect("a coded diagnostic");
    assert!(
        hint_texts(diag).iter().any(|h| h.contains("`{#i .c k=v}`")),
        "got {:?}",
        hint_texts(diag)
    );
}

#[test]
fn hint_keeps_quoted_values_intact() {
    let diagnostics = diagnostics_for("![a](b.png){fig-alt=\"a } b {\" .lightbox #fig}\n");
    let diag = diagnostics
        .iter()
        .find(|d| d.code.is_some())
        .expect("a coded diagnostic");
    assert!(
        hint_texts(diag)
            .iter()
            .any(|h| h.contains("`{#fig .lightbox fig-alt=\"a } b {\"}`")),
        "got {:?}",
        hint_texts(diag)
    );
}

#[test]
fn a_second_identifier_is_not_reported_as_an_ordering_problem() {
    // `{#a #b}` fails in the same LR state as `{k=v #i}`, but the remedy is
    // not to reorder anything. It must not be answered with Q-2-56.
    let diagnostics = diagnostics_for("[x]{#a #b}\n");
    assert!(
        diagnostics
            .iter()
            .all(|d| !matches!(d.code.as_deref(), Some("Q-2-55" | "Q-2-56"))),
        "got {:?}",
        diagnostics
            .iter()
            .map(|d| d.code.clone())
            .collect::<Vec<_>>()
    );
}
