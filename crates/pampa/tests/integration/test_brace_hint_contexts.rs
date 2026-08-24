// Coverage tests for Q-2-41 ("Curly braces are reserved for attribute syntax").
//
// Q-2-41 is a merr-style corpus diagnostic: the error corpus maps a
// (tree-sitter LR state, symbol) pair to a message, so a brace run that fails
// in an *unclaimed* state silently falls through to the generic uncoded
// "Parse error / unexpected character or token here".
//
// The original implementation (bd-brace-escape-hint-0tmemkyt) captured the two
// states its motivating corpus produced -- prose and link text. But the state
// is determined by the *innermost enclosing inline container*, and the grammar
// has thirteen more of those (emphasis, strong, quotes, editorial spans, pipe
// table cells, ...). Each one is its own LR state, so each one needed its own
// corpus case (bd-brace-hint-misses-emphasis-4fzv1n93).
//
// This test is the reconciliation: every context in the grammar that can hold
// inline content gets a row here. The right predicate is "every rule that
// reaches `$._inline_element`", which is NOT the same as "every rule that wraps
// `$._inlines`" -- `pipe_table_cell` (grammar.js:393) is built on
// `_line_with_maybe_spaces`, a *sibling* of `_inlines` (grammar.js:606-607), so
// a grep for `_inlines` cannot find table cells. That is exactly how the first
// pass at this fix shipped without them. When a new inline-receptive rule is
// added to the grammar, add a row -- otherwise its brace failures ship uncoded.

use pampa::readers;

fn parse(
    input: &str,
) -> Result<
    (
        pampa::pandoc::Pandoc,
        pampa::pandoc::ast_context::ASTContext,
        Vec<quarto_error_reporting::DiagnosticMessage>,
    ),
    Vec<quarto_error_reporting::DiagnosticMessage>,
> {
    readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    )
}

/// Every context in the grammar that can hold inline content. The block-level
/// contexts (paragraph, heading, block quote, list item, div body, caption) all
/// reduce to one shared state; the inline containers and the two pipe-table
/// cell positions each have their own.
const BRACE_CONTEXTS: &[(&str, &str)] = &[
    // --- already covered before bd-brace-hint-misses-emphasis-4fzv1n93 ---
    (
        "paragraph",
        "the request returns the task {guid} immediately.\n",
    ),
    ("atx-heading", "# The {guid} heading\n"),
    ("block-quote", "> the task {guid} here.\n"),
    ("list-item", "- the task {guid} here.\n"),
    (
        "link-text",
        "see [the {guid} link](https://example.com) here.\n",
    ),
    ("image-alt", "see ![the {guid} image](i.png) here.\n"),
    ("span-text", "a [{PLACEHOLDER}]{.cls} here.\n"),
    // --- the gap this strand closes: one LR state per inline container ---
    ("emphasis-star", "a *{PLACEHOLDER}* run here.\n"),
    ("emphasis-underscore", "a _{PLACEHOLDER}_ run here.\n"),
    ("strong-star", "a **{PLACEHOLDER}** run here.\n"),
    (
        "strong-star-with-text",
        "a **bold {PLACEHOLDER} run** here.\n",
    ),
    ("strong-underscore", "a __{PLACEHOLDER}__ run here.\n"),
    ("strikeout", "a ~~{PLACEHOLDER}~~ run here.\n"),
    ("superscript", "a ^{PLACEHOLDER}^ run here.\n"),
    ("subscript", "a ~{PLACEHOLDER}~ run here.\n"),
    ("double-quote", "he said \"the {PLACEHOLDER} thing\".\n"),
    ("single-quote", "he said 'the {PLACEHOLDER} thing'.\n"),
    ("inline-note", "a ^[the {PLACEHOLDER} note] here.\n"),
    ("highlight-span", "a [!! the {PLACEHOLDER} run] here.\n"),
    ("delete-span", "a [-- the {PLACEHOLDER} run] here.\n"),
    ("insert-span", "a [++ the {PLACEHOLDER} run] here.\n"),
    ("edit-comment-span", "a [>> the {PLACEHOLDER} run] here.\n"),
    // A pipe-table cell is its own container, not the shared block state -- and
    // emphasis *inside* a cell reduces to the emphasis state, so the nesting
    // row below does NOT cover the bare cell. Both are needed; the first pass
    // at this fix had only the nesting row and left bare table cells uncoded.
    ("table-header-cell", "| {X} | b |\n|---|---|\n| a | y |\n"),
    ("table-body-cell", "| a | b |\n|---|---|\n| {X} | y |\n"),
    // --- nesting: the state follows the *innermost* container ---
    (
        "emphasis-in-strong-in-link",
        "**Debug [*{SUPPORTED_APP_TYPE}*] App in Terminal**.\n",
    ),
    (
        "emphasis-in-link-text",
        "see [*{guid}*](https://example.com) here.\n",
    ),
    ("strong-in-emphasis", "a *outer **{X}** inner* here.\n"),
    ("emphasis-in-heading", "# The *{guid}* heading\n"),
    ("emphasis-in-block-quote", "> the task *{guid}* here.\n"),
    ("emphasis-in-list-item", "- the task *{guid}* here.\n"),
    (
        "emphasis-in-table-cell",
        "| a | b |\n|---|---|\n| *{X}* | y |\n",
    ),
];

#[test]
fn test_literal_braces_report_q241_in_every_inline_context() {
    let mut failures: Vec<String> = Vec::new();

    for (name, source) in BRACE_CONTEXTS {
        let diagnostics = match parse(source) {
            Ok((_pandoc, _context, warnings)) => {
                failures.push(format!(
                    "{name}: expected a Q-2-41 parse error, but the source parsed successfully \
                     (warnings: {:?})",
                    warnings
                        .iter()
                        .map(|w| w.code.as_deref())
                        .collect::<Vec<_>>()
                ));
                continue;
            }
            Err(diags) => diags,
        };

        if !diagnostics
            .iter()
            .any(|d| d.code.as_deref() == Some("Q-2-41"))
        {
            failures.push(format!(
                "{name}: literal braces reported {:?} instead of Q-2-41",
                diagnostics
                    .iter()
                    .map(|d| d.code.as_deref().unwrap_or("<uncoded generic parse error>"))
                    .collect::<Vec<_>>()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "Literal braces must explain themselves with Q-2-41 in every inline \
         context. {} of {} contexts fell through:\n  {}",
        failures.len(),
        BRACE_CONTEXTS.len(),
        failures.join("\n  ")
    );
}

/// Braces are *not* an error where the grammar treats them as literal text, and
/// the escape the Q-2-41 message recommends really does produce a literal brace
/// in the output.
///
/// Note what this can and cannot catch. It is a *grammar* regression guard, not
/// a corpus guard: the error table is consulted only after a parse has already
/// failed (`readers::qmd_error_messages::produce_diagnostic_messages`), so no
/// corpus row can ever turn one of these clean parses into an error. It fires
/// if someone later makes `{` an error inside code spans, math or link titles,
/// or if an escape stops round-tripping to a literal brace.
#[test]
fn test_braces_stay_literal_where_they_are_legal() {
    /// Where the braces are expected to end up. Only `InText` is visible to the
    /// plain-text writer; the other two are asserted as *absent* from the text,
    /// which is what distinguishes them from a brace that was simply dropped.
    #[derive(PartialEq)]
    enum Fate {
        /// Survives as literal text the reader sees.
        InText,
        /// Consumed as attribute syntax -- the point of the construct.
        AsAttribute,
        /// Survives, but in a field the plain-text writer does not render.
        /// Verified separately: `q2 render` emits `title="the {X} title"`.
        InTitleAttribute,
    }

    let legal: &[(&str, &str, Fate)] = &[
        ("code-span", "a `the {X} run` here.\n", Fate::InText),
        ("inline-math", "a $x_{i}$ run.\n", Fate::InText),
        (
            "link-title",
            "see [text](https://ex.com \"the {X} title\") here.\n",
            Fate::InTitleAttribute,
        ),
        (
            "escaped-in-prose",
            "the task \\{guid\\} here.\n",
            Fate::InText,
        ),
        (
            "escaped-in-emphasis",
            "a *\\{PLACEHOLDER\\}* run here.\n",
            Fate::InText,
        ),
        (
            "escaped-in-strong",
            "a **\\{PLACEHOLDER\\}** run here.\n",
            Fate::InText,
        ),
        (
            "real-attribute",
            "a [text]{.cls} here.\n",
            Fate::AsAttribute,
        ),
        (
            "real-attribute-on-emphasis",
            "a *text*{.cls} here.\n",
            Fate::AsAttribute,
        ),
    ];

    let mut failures: Vec<String> = Vec::new();
    for (name, source, fate) in legal {
        match parse(source) {
            Err(diags) => failures.push(format!(
                "{name}: expected a clean parse, got {:?}",
                diags
                    .iter()
                    .map(|d| d.code.as_deref().unwrap_or("<uncoded>"))
                    .collect::<Vec<_>>()
            )),
            Ok((pandoc, _context, _warnings)) => {
                // A clean parse is not enough -- the braces must still be
                // *there*. Render to plain text rather than inspecting the
                // AST's Debug output: `{:?}` on any Rust struct emits braces of
                // its own, so a Debug-based check passes vacuously.
                let mut buf: Vec<u8> = Vec::new();
                let mut ctx = pampa::writers::plaintext::PlainTextWriterContext::new();
                pampa::writers::plaintext::write_blocks(&pandoc.blocks, &mut buf, &mut ctx)
                    .expect("plaintext writer should not fail on a clean parse");
                let text = String::from_utf8_lossy(&buf);

                let has_braces = text.contains('{') && text.contains('}');
                match (fate, has_braces) {
                    (Fate::InText, false) => failures.push(format!(
                        "{name}: parsed cleanly but the braces were swallowed; \
                         plain text rendered as {text:?}"
                    )),
                    (Fate::AsAttribute | Fate::InTitleAttribute, true) => failures.push(format!(
                        "{name}: these braces should not reach the rendered \
                         text, but did: {text:?}"
                    )),
                    _ => {}
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "Legal brace uses must keep parsing with their braces intact:\n  {}",
        failures.join("\n  ")
    );
}
