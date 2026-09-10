//! Diagnostics for shortcodes whose delimiters are missing the mandatory
//! separator: `{{<fa plus>}}` rather than `{{< fa plus >}}`.
//!
//! The separator is required by construction — `_shortcode_sep` in the qmd
//! grammar has no empty alternative — and that is deliberate: `{{` is also
//! Jinja/Handlebars territory, and requiring `{{< ` keeps a shortcode
//! lexically unambiguous. These tests are about *diagnosing* the mistake
//! well, not about accepting it.

use quarto_error_reporting::DiagnosticMessage;

fn diagnostics_for(input: &str) -> Vec<DiagnosticMessage> {
    match pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    ) {
        Ok(_) => panic!("expected a parse failure for:\n{input}"),
        Err(diagnostics) => diagnostics,
    }
}

fn codes(diagnostics: &[DiagnosticMessage]) -> Vec<Option<&str>> {
    diagnostics.iter().map(|d| d.code.as_deref()).collect()
}

const TIGHT_OPEN: &str = "Click the {{<fa plus >}} icon.\n";
const TIGHT_CLOSE: &str = "Click the {{< fa plus>}} icon.\n";
const BOTH_TIGHT: &str = "Click the {{<fa plus>}} icon.\n";

/// One missing space after `{{<` is one error, and it carries the
/// separator code rather than the generic uncoded "Parse error".
#[test]
fn tight_opening_delimiter_is_one_coded_error() {
    let diagnostics = diagnostics_for(TIGHT_OPEN);
    assert_eq!(
        codes(&diagnostics),
        vec![Some("Q-2-52")],
        "{diagnostics:#?}"
    );
}

/// The closing delimiter is the same mistake and gets the same code —
/// it used to be reported as Q-2-34 ("parameter starting with digit")
/// even though the parameter is `plus`.
#[test]
fn tight_closing_delimiter_is_one_coded_error() {
    let diagnostics = diagnostics_for(TIGHT_CLOSE);
    assert_eq!(
        codes(&diagnostics),
        vec![Some("Q-2-52")],
        "{diagnostics:#?}"
    );
}

#[test]
fn both_delimiters_tight_is_one_coded_error() {
    let diagnostics = diagnostics_for(BOTH_TIGHT);
    assert_eq!(
        codes(&diagnostics),
        vec![Some("Q-2-52")],
        "{diagnostics:#?}"
    );
}

/// The diagnostic points at the character where the space belongs, and
/// says what to write instead.
#[test]
fn separator_diagnostic_names_the_construct_and_offers_a_remedy() {
    let diagnostics = diagnostics_for(BOTH_TIGHT);
    let rendered = diagnostics[0].to_text(None).to_lowercase();
    assert!(
        rendered.contains("shortcode"),
        "diagnostic should name the construct being parsed:\n{rendered}"
    );
    assert!(
        rendered.contains("{{< "),
        "diagnostic should show the corrected spelling:\n{rendered}"
    );
}

/// Q-2-34's real rule: an unquoted parameter value that actually starts
/// with a digit. This is the true positive the closing-delimiter form
/// was being confused with, and it must keep working.
#[test]
fn digit_leading_parameter_still_reports_q_2_34() {
    let diagnostics = diagnostics_for("Click the {{< fa 2plus >}} icon.\n");
    assert_eq!(
        codes(&diagnostics),
        vec![Some("Q-2-34")],
        "{diagnostics:#?}"
    );
}

#[test]
fn digit_leading_keyword_value_still_reports_q_2_34() {
    let diagnostics = diagnostics_for("Click the {{< fa envelope size=1x >}} icon.\n");
    assert_eq!(
        codes(&diagnostics),
        vec![Some("Q-2-34")],
        "{diagnostics:#?}"
    );
}

/// One missing space must not be blamed on unrelated constructs further
/// down the file. Recovery from the malformed shortcode used to swallow
/// the callout's opening `:::`, so its closing `:::` — perfectly valid
/// Quarto — was reported as a third parse error.
#[test]
fn tight_delimiter_does_not_cascade_into_later_blocks() {
    let input = concat!(
        "Click the {{<fa plus>}} icon.\n",
        "\n",
        "::: {.callout-note}\n",
        "A callout well after the bad shortcode.\n",
        ":::\n",
        "\n",
        "Ordinary trailing paragraph.\n",
    );
    let diagnostics = diagnostics_for(input);
    assert_eq!(
        diagnostics.len(),
        1,
        "one missing space should be one error, got:\n{diagnostics:#?}"
    );
    assert_eq!(diagnostics[0].code.as_deref(), Some("Q-2-52"));

    // The sole error belongs on the shortcode's own line, not on the
    // callout's closing `:::` five lines below it.
    let first_line_end = input.find('\n').expect("input is multi-line");
    let offset = diagnostics[0]
        .location
        .as_ref()
        .expect("diagnostic should carry a location")
        .start_offset();
    assert!(
        offset < first_line_end,
        "error should be on the shortcode's line (offset < {first_line_end}), got {offset}"
    );
}

/// The control: the spaced spelling parses.
#[test]
fn spaced_delimiters_parse() {
    let result = pampa::readers::qmd::read(
        "Click the {{< fa plus >}} icon.\n".as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    assert!(result.is_ok(), "spaced shortcode should parse");
}

/// An unquoted numeric value is legal — `{{< video a.mp4 width=640 >}}`
/// parses — so removing the space before `>}}` is a separator mistake and
/// nothing else. It used to be reported as Q-2-34, which would have had
/// the author quote a value that was never the problem.
#[test]
fn tight_close_after_numeric_value_is_not_blamed_on_the_value() {
    let diagnostics = diagnostics_for("Click {{< video a.mp4 width=640>}} x.\n");
    assert_eq!(
        codes(&diagnostics),
        vec![Some("Q-2-52")],
        "{diagnostics:#?}"
    );
}

/// Quoting the value is what Q-2-34's remedy asks for. It reaches a
/// different parser state, which was in the table under no code at all —
/// so following the old advice moved the author from a wrong code to none.
#[test]
fn tight_close_after_quoted_value_is_coded() {
    let diagnostics = diagnostics_for("Click {{< video a.mp4 width=\"640\">}} x.\n");
    assert_eq!(
        codes(&diagnostics),
        vec![Some("Q-2-52")],
        "{diagnostics:#?}"
    );
}

// ============================================================================
// Inline-container coverage
//
// The error table is keyed on (LR state, lookahead). For the *opening*
// delimiter the state follows the innermost enclosing inline container, so
// each container below is a separate state and needs its own corpus case —
// a state with no entry falls through to the generic uncoded "Parse error"
// rather than to something approximate. Pipe-table cells are the easy
// omission: `pipe_table_cell` is built on `_line_with_maybe_spaces`, a
// sibling of the `_inlines` every other container wraps.
//
// The *closing* delimiter is different: by then the parser is inside the
// shortcode, and every container reduces to the same state.
// ============================================================================

const INLINE_CONTAINERS: &[(&str, &str, &str)] = &[
    ("paragraph", "Click ", " here.\n"),
    ("emphasis-star", "a *", "* b\n"),
    ("strong-star", "a **", "** b\n"),
    ("emphasis-underscore", "a _", "_ b\n"),
    ("strong-underscore", "a __", "__ b\n"),
    ("strikeout", "a ~~", "~~ b\n"),
    ("superscript", "a ^", "^ b\n"),
    ("subscript", "a ~", "~ b\n"),
    ("double-quote", "he said \"", "\" today\n"),
    ("single-quote", "he said '", "' today\n"),
    ("inline-note", "a ^[", "] b\n"),
    ("link-text", "a [", "](https://example.com) b\n"),
    ("image-alt", "a ![", "](image.png) b\n"),
    ("bracketed-span", "a [", "]{.class} b\n"),
    ("editorial-span", "a [!! ", "] b\n"),
    ("table-header-cell", "| ", " | b |\n|---|---|\n| a | y |\n"),
    ("table-body-cell", "| a | b |\n|---|---|\n| ", " | y |\n"),
];

/// Every shortcode spelling the matrix below exercises: a name, the
/// spelling with a delimiter written tight, and the same shortcode
/// written correctly. The third column is what makes the second
/// meaningful — it pins that the missing space is the *only* thing wrong
/// with each row.
///
/// The scanner has a separate token for single-character quoted content,
/// so `'q'` and `'quoted'` reach different parser states and both belong
/// here.
const SPELLINGS: &[(&str, &str, &str)] = &[
    ("open-tight", "{{<fa plus >}}", "{{< fa plus >}}"),
    (
        "open-tight-escaped",
        "{{{<fa plus >}}}",
        "{{{< fa plus >}}}",
    ),
    ("both-tight", "{{<fa>}}", "{{< fa >}}"),
    ("both-tight-escaped", "{{{<fa>}}}", "{{{< fa >}}}"),
    ("close-name-only", "{{< fa>}}", "{{< fa >}}"),
    ("close-naked", "{{< fa plus>}}", "{{< fa plus >}}"),
    ("close-number", "{{< fa 42>}}", "{{< fa 42 >}}"),
    ("close-squote-one-char", "{{< fa 'q'>}}", "{{< fa 'q' >}}"),
    ("close-squote", "{{< fa 'quoted'>}}", "{{< fa 'quoted' >}}"),
    (
        "close-dquote-one-char",
        "{{< fa \"q\">}}",
        "{{< fa \"q\" >}}",
    ),
    (
        "close-dquote",
        "{{< fa \"quoted\">}}",
        "{{< fa \"quoted\" >}}",
    ),
    ("close-keyword-naked", "{{< fa k=v>}}", "{{< fa k=v >}}"),
    ("close-keyword-number", "{{< fa k=42>}}", "{{< fa k=42 >}}"),
    (
        "close-keyword-squote-one-char",
        "{{< fa k='q'>}}",
        "{{< fa k='q' >}}",
    ),
    (
        "close-keyword-squote",
        "{{< fa k='quoted'>}}",
        "{{< fa k='quoted' >}}",
    ),
    (
        "close-keyword-dquote-one-char",
        "{{< fa k=\"q\">}}",
        "{{< fa k=\"q\" >}}",
    ),
    (
        "close-keyword-dquote",
        "{{< fa k=\"quoted\">}}",
        "{{< fa k=\"quoted\" >}}",
    ),
    ("close-two-positionals", "{{< fa a b>}}", "{{< fa a b >}}"),
    (
        "close-positional-then-keyword",
        "{{< fa a k=v>}}",
        "{{< fa a k=v >}}",
    ),
    (
        "close-nested-shortcode",
        "{{< fa {{< b >}}>}}",
        "{{< fa {{< b >}} >}}",
    ),
    // The *inner* shortcode's own delimiters, which the outer-delimiter
    // rows above do not reach.
    (
        "nested-inner-open-tight",
        "{{< fa {{<b >}} >}}",
        "{{< fa {{< b >}} >}}",
    ),
    (
        "nested-inner-close-tight",
        "{{< fa {{< b>}} >}}",
        "{{< fa {{< b >}} >}}",
    ),
    // Arguments the language-specifier scanner does not claim reach the
    // regex `shortcode_naked_string` production, and a different state.
    (
        "close-naked-non-ascii",
        "{{< fa größe>}}",
        "{{< fa größe >}}",
    ),
    (
        "close-naked-path",
        "{{< fa /path/x>}}",
        "{{< fa /path/x >}}",
    ),
    (
        "close-naked-url",
        "{{< fa http://e.com/q?x=1>}}",
        "{{< fa http://e.com/q?x=1 >}}",
    ),
    (
        "close-keyword-non-ascii",
        "{{< fa k=größe>}}",
        "{{< fa k=größe >}}",
    ),
    ("close-name-only-escaped", "{{{< fa>}}}", "{{{< fa >}}}"),
    (
        "close-naked-escaped",
        "{{{< fa plus>}}}",
        "{{{< fa plus >}}}",
    ),
    ("close-number-escaped", "{{{< fa 42>}}}", "{{{< fa 42 >}}}"),
    (
        "close-squote-one-char-escaped",
        "{{{< fa 'q'>}}}",
        "{{{< fa 'q' >}}}",
    ),
    (
        "close-squote-escaped",
        "{{{< fa 'quoted'>}}}",
        "{{{< fa 'quoted' >}}}",
    ),
    (
        "close-dquote-one-char-escaped",
        "{{{< fa \"q\">}}}",
        "{{{< fa \"q\" >}}}",
    ),
    (
        "close-dquote-escaped",
        "{{{< fa \"quoted\">}}}",
        "{{{< fa \"quoted\" >}}}",
    ),
    (
        "close-keyword-naked-escaped",
        "{{{< fa k=v>}}}",
        "{{{< fa k=v >}}}",
    ),
    (
        "close-keyword-number-escaped",
        "{{{< fa k=42>}}}",
        "{{{< fa k=42 >}}}",
    ),
    (
        "close-keyword-squote-one-char-escaped",
        "{{{< fa k='q'>}}}",
        "{{{< fa k='q' >}}}",
    ),
    (
        "close-keyword-squote-escaped",
        "{{{< fa k='quoted'>}}}",
        "{{{< fa k='quoted' >}}}",
    ),
    (
        "close-keyword-dquote-one-char-escaped",
        "{{{< fa k=\"q\">}}}",
        "{{{< fa k=\"q\" >}}}",
    ),
    (
        "close-keyword-dquote-escaped",
        "{{{< fa k=\"quoted\">}}}",
        "{{{< fa k=\"quoted\" >}}}",
    ),
    (
        "close-two-positionals-escaped",
        "{{{< fa a b>}}}",
        "{{{< fa a b >}}}",
    ),
    (
        "close-positional-then-keyword-escaped",
        "{{{< fa a k=v>}}}",
        "{{{< fa a k=v >}}}",
    ),
    (
        "close-nested-shortcode-escaped",
        "{{{< fa {{< b >}}>}}}",
        "{{{< fa {{< b >}} >}}}",
    ),
    (
        "close-naked-non-ascii-escaped",
        "{{{< fa größe>}}}",
        "{{{< fa größe >}}}",
    ),
    (
        "close-naked-path-escaped",
        "{{{< fa /path/x>}}}",
        "{{{< fa /path/x >}}}",
    ),
];

/// Every combination of enclosing container and shortcode spelling must
/// reach a corpus case. This is the reconciliation the error table needs:
/// a state with no entry does not degrade to an approximate message, it
/// falls through to the generic uncoded "Parse error".
///
/// It is a wide matrix because the two delimiters fail in opposite ways.
/// The opening delimiter is consumed in the enclosing container's state,
/// so it needs a case per container. The closing delimiter is reached
/// from inside the shortcode, so one case per *argument shape* usually
/// covers every container — except after a bare name, where the parser
/// has not committed to an argument list and the container shows through
/// again.
#[test]
fn every_container_and_spelling_reports_the_separator_code() {
    let mut checked = 0;
    for (container, prefix, suffix) in INLINE_CONTAINERS {
        for (spelling, tight, _spaced) in SPELLINGS {
            let input = format!("{prefix}{tight}{suffix}");
            let diagnostics = diagnostics_for(&input);
            assert_eq!(
                codes(&diagnostics),
                vec![Some("Q-2-52")],
                "{container} / {spelling}, input {input:?}: {diagnostics:#?}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, INLINE_CONTAINERS.len() * SPELLINGS.len());
}

/// The corrected spelling of every row above parses, in every container.
#[test]
fn every_spaced_spelling_parses_in_every_container() {
    for (container, prefix, suffix) in INLINE_CONTAINERS {
        for (spelling, _tight, spaced) in SPELLINGS {
            let input = format!("{prefix}{spaced}{suffix}");
            let result = pampa::readers::qmd::read(
                input.as_bytes(),
                false,
                "test.qmd",
                &mut std::io::sink(),
                true,
                None,
            );
            assert!(
                result.is_ok(),
                "{container} / {spelling}: {input:?} should parse: {:#?}",
                result.err()
            );
        }
    }
}

#[test]
fn spaced_escaped_shortcode_parses() {
    let result = pampa::readers::qmd::read(
        "Show the {{{< fa plus >}}} syntax.\n".as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    assert!(result.is_ok(), "spaced escaped shortcode should parse");
}

/// A shortcode whose positional argument follows a keyword argument is a
/// different mistake, and stays one. It is here so that the matrix above
/// is not silently widened to swallow it.
#[test]
fn positional_after_keyword_is_not_a_separator_error() {
    let diagnostics = diagnostics_for("Click {{< fa k=v a >}} here.\n");
    assert_ne!(
        diagnostics[0].code.as_deref(),
        Some("Q-2-52"),
        "{diagnostics:#?}"
    );
}

/// An escaped shortcode cannot be another shortcode's argument, spaced or
/// not — `_shortcode_value` admits `shortcode`, never `shortcode_escaped`.
/// That failure is not a missing separator and must not be claimed as one.
#[test]
fn escaped_shortcode_as_an_argument_is_not_a_separator_error() {
    let diagnostics = diagnostics_for("Click {{{< fa {{{< b >}}} >}}} here.\n");
    assert_ne!(
        diagnostics[0].code.as_deref(),
        Some("Q-2-52"),
        "{diagnostics:#?}"
    );
}

/// A shortcode can also appear where inline content cannot: inside an
/// attribute value, and inside a link or image destination. Neither is
/// reachable by wrapping a line in a prefix and suffix, and each has its
/// own parser states.
#[test]
fn tight_delimiters_are_coded_in_attribute_values_and_link_destinations() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "double-quoted attribute value",
            "A [x]{data-a=\"{{<fa>}}\"} b\n",
            "A [x]{data-a=\"{{< fa >}}\"} b\n",
        ),
        (
            "single-quoted attribute value",
            "A [x]{data-a='{{<fa>}}'} b\n",
            "A [x]{data-a='{{< fa >}}'} b\n",
        ),
        (
            "attribute value, closing delimiter",
            "A [x]{data-a=\"{{< fa>}}\"} b\n",
            "A [x]{data-a=\"{{< fa >}}\"} b\n",
        ),
        (
            "link destination",
            "A [x]({{<fa>}}) b\n",
            "A [x]({{< fa >}}) b\n",
        ),
        (
            "link destination, closing delimiter",
            "A [x]({{< fa>}}) b\n",
            "A [x]({{< fa >}}) b\n",
        ),
        (
            "image destination",
            "A ![x]({{<fa>}}) b\n",
            "A ![x]({{< fa >}}) b\n",
        ),
        (
            "shortcode as part of a destination path",
            "A [x](path/{{<fa>}}.png) b\n",
            "A [x](path/{{< fa >}}.png) b\n",
        ),
    ];

    for (name, tight, spaced) in cases {
        let diagnostics = diagnostics_for(tight);
        assert_eq!(
            codes(&diagnostics),
            vec![Some("Q-2-52")],
            "{name}, input {tight:?}: {diagnostics:#?}"
        );

        let result = pampa::readers::qmd::read(
            spaced.as_bytes(),
            false,
            "test.qmd",
            &mut std::io::sink(),
            true,
            None,
        );
        assert!(
            result.is_ok(),
            "{name}: corrected {spaced:?} should parse: {:#?}",
            result.err()
        );
    }
}
