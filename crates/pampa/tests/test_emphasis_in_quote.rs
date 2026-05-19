use pampa::readers;

fn render_diagnostics(input: &str, filename: &str) -> String {
    let mut content = input.to_string();
    if !content.ends_with('\n') {
        content.push('\n');
    }

    let result = readers::qmd::read(
        content.as_bytes(),
        false,
        filename,
        &mut std::io::sink(),
        true,
        None,
    );

    let diagnostics = match result {
        Ok(_) => panic!("Expected diagnostics for input:\n{content}"),
        Err(d) => d,
    };

    let mut source_context = quarto_source_map::SourceContext::new();
    source_context.add_file(filename.to_string(), Some(content));

    let render_options = quarto_error_reporting::TextRenderOptions {
        enable_hyperlinks: false,
    };

    let mut output = String::new();
    for diagnostic in &diagnostics {
        output.push_str(&diagnostic.to_text_with_options(Some(&source_context), &render_options));
        output.push('\n');
    }
    output
}

fn assert_parses_cleanly(input: &str, filename: &str) {
    let mut content = input.to_string();
    if !content.ends_with('\n') {
        content.push('\n');
    }

    let result = readers::qmd::read(
        content.as_bytes(),
        false,
        filename,
        &mut std::io::sink(),
        true,
        None,
    );

    match result {
        Ok(_) => {}
        Err(diagnostics) => {
            let mut source_context = quarto_source_map::SourceContext::new();
            source_context.add_file(filename.to_string(), Some(content.clone()));
            let render_options = quarto_error_reporting::TextRenderOptions {
                enable_hyperlinks: false,
            };
            let mut output = String::new();
            for diagnostic in &diagnostics {
                output.push_str(
                    &diagnostic.to_text_with_options(Some(&source_context), &render_options),
                );
                output.push('\n');
            }
            panic!("Expected clean parse for input:\n{content}\nGot diagnostics:\n{output}");
        }
    }
}

#[test]
fn quoted_underscore_word_emits_q_2_5() {
    let input = "The \"_blank\" word.\n";
    let output = render_diagnostics(input, "quoted-underscore.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for unclosed `_` inside `\"..\"`. Got:\n{output}"
    );
}

#[test]
fn quoted_strong_underscore_word_emits_q_2_15() {
    let input = "The \"__blank\" word.\n";
    let output = render_diagnostics(input, "quoted-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` inside `\"..\"`. Got:\n{output}"
    );
}

#[test]
fn quoted_star_word_emits_q_2_12() {
    let input = "The \"*blank\" word.\n";
    let output = render_diagnostics(input, "quoted-star.qmd");
    assert!(
        output.contains("Q-2-12"),
        "Expected Q-2-12 for unclosed `*` inside `\"..\"`. Got:\n{output}"
    );
}

#[test]
fn quoted_strong_star_word_emits_q_2_13() {
    let input = "The \"**blank\" word.\n";
    let output = render_diagnostics(input, "quoted-strong-star.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 for unclosed `**` inside `\"..\"`. Got:\n{output}"
    );
}

// --- Single-quoted variants (currently fail; see inline_issue.md) ---

#[test]
fn single_quoted_underscore_word_emits_q_2_5() {
    let input = "The '_blank' word.\n";
    let output = render_diagnostics(input, "single-quoted-underscore.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for unclosed `_` inside `'..'`. Got:\n{output}"
    );
}

#[test]
fn single_quoted_strong_underscore_word_emits_q_2_15() {
    let input = "The '__blank' word.\n";
    let output = render_diagnostics(input, "single-quoted-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` inside `'..'`. Got:\n{output}"
    );
}

#[test]
fn single_quoted_star_word_emits_q_2_12() {
    let input = "The '*blank' word.\n";
    let output = render_diagnostics(input, "single-quoted-star.qmd");
    assert!(
        output.contains("Q-2-12"),
        "Expected Q-2-12 for unclosed `*` inside `'..'`. Got:\n{output}"
    );
}

#[test]
fn single_quoted_strong_star_word_emits_q_2_13() {
    let input = "The '**blank' word.\n";
    let output = render_diagnostics(input, "single-quoted-strong-star.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 for unclosed `**` inside `'..'`. Got:\n{output}"
    );
}

// --- Symmetric: unclosed double quote inside emphasis must emit Q-2-11 ---
// Currently fail because state 705/691/714/760 collide between outer-quote and
// outer-emphasis contexts; the corpus fix from commit 6e3ad158 redirected
// those to the emphasis codes. See inline_issue.md.

#[test]
fn unclosed_double_quote_in_star_emits_q_2_11() {
    let input = "*a\" b.*\n";
    let output = render_diagnostics(input, "unclosed-double-quote-in-star.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `*..*`. Got:\n{output}"
    );
}

#[test]
fn unclosed_double_quote_in_strong_star_emits_q_2_11() {
    let input = "**a\" b.**\n";
    let output = render_diagnostics(input, "unclosed-double-quote-in-strong-star.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `**..**`. Got:\n{output}"
    );
}

#[test]
fn unclosed_double_quote_in_underscore_emits_q_2_11() {
    let input = "_a\" b._\n";
    let output = render_diagnostics(input, "unclosed-double-quote-in-underscore.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `_.._`. Got:\n{output}"
    );
}

#[test]
fn unclosed_double_quote_in_strong_underscore_emits_q_2_11() {
    let input = "__a\" b.__\n";
    let output = render_diagnostics(input, "unclosed-double-quote-in-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `__..__`. Got:\n{output}"
    );
}

// --- Control tests: apostrophe-in-emphasis shapes must keep emitting Q-2-10 ---

#[test]
fn apostrophe_in_star_stays_q_2_10() {
    let input = "*a' b.*\n";
    let output = render_diagnostics(input, "apostrophe-in-star.qmd");
    assert!(
        output.contains("Q-2-10"),
        "Expected Q-2-10 for apostrophe inside `*..*` to remain unchanged. Got:\n{output}"
    );
}

#[test]
fn apostrophe_in_strong_star_stays_q_2_10() {
    let input = "**a' b.**\n";
    let output = render_diagnostics(input, "apostrophe-in-strong-star.qmd");
    assert!(
        output.contains("Q-2-10"),
        "Expected Q-2-10 for apostrophe inside `**..**` to remain unchanged. Got:\n{output}"
    );
}

#[test]
fn apostrophe_in_underscore_stays_q_2_10() {
    let input = "_a' b._\n";
    let output = render_diagnostics(input, "apostrophe-in-underscore.qmd");
    assert!(
        output.contains("Q-2-10"),
        "Expected Q-2-10 for apostrophe inside `_.._` to remain unchanged. Got:\n{output}"
    );
}

#[test]
fn apostrophe_in_strong_underscore_stays_q_2_10() {
    let input = "__a' b.__\n";
    let output = render_diagnostics(input, "apostrophe-in-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-10"),
        "Expected Q-2-10 for apostrophe inside `__..__` to remain unchanged. Got:\n{output}"
    );
}

// Multi-paragraph regression: the prior Merr-walker approach broke this case
// because consumed_tokens reflected end-of-parse state. The scanner-stack
// approach should handle it correctly because the scope stack is cleared on
// block boundaries.

#[test]
fn multi_paragraph_apostrophes_both_emit_q_2_10() {
    let input = "First apostrophe: a' b.\n\nSecond in bold: **c' d.**\n";
    let output = render_diagnostics(input, "multi-paragraph-apostrophes.qmd");
    assert!(
        output.matches("Q-2-10").count() >= 2,
        "Expected two Q-2-10 diagnostics across paragraphs. Got:\n{output}"
    );
}

// --- Three-level nesting: emphasis inside emphasis inside single-quote ---

#[test]
fn single_quoted_starred_strong_underscore_emits_q_2_15() {
    // Inside the single quote there's `*__blank*`: emphasis-star contains an
    // unclosed strong-underscore. The * pair actually closes; the __ is what
    // doesn't. The diagnostic should be about the __ (Q-2-15), not the *.
    let input = "The ' *__blank*' word.\n";
    let output = render_diagnostics(input, "single-quoted-starred-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` inside `*..*` inside `'..'`. Got:\n{output}"
    );
}

#[test]
fn single_quoted_strong_underscore_before_emphasis_emits_q_2_15() {
    // `__` opens strong-underscore (unclosed), then `*blank*` is a complete
    // emphasis-star pair, then the single quote tries to close. The unclosed
    // `__` is the innermost open scope; should fire Q-2-15.
    let input = "The ' __*blank*' word.\n";
    let output = render_diagnostics(input, "single-quoted-strong-underscore-before-emphasis.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` before `*..*` inside `'..'`. Got:\n{output}"
    );
}

#[test]
fn single_quoted_strong_underscore_after_emphasis_emits_q_2_15() {
    // `*blank*` is a complete emphasis-star pair, then `__` opens
    // strong-underscore (unclosed), then the single quote tries to close.
    // The unclosed `__` is the innermost open scope; should fire Q-2-15.
    let input = "The ' *blank*__' word.\n";
    let output = render_diagnostics(input, "single-quoted-strong-underscore-after-emphasis.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` after `*..*` inside `'..'`. Got:\n{output}"
    );
}

// --- Inputs that should parse cleanly (no diagnostics) ---

#[test]
fn nested_double_quote_inside_emphasis_parses_cleanly() {
    // `*a" b."*` is `<em>a"b."</em>` where the two `"` form a paired
    // double-quote span. GLR speculation hits errors in dead branches but the
    // overall parse succeeds; no diagnostics should be reported.
    assert_parses_cleanly("*a\" b.\"*\n", "nested-double-quote-in-emphasis.qmd");
}

#[test]
fn quarto_web_blank_link_target_emits_q_2_5() {
    let input = "\
| a | b |
|---|---|
| 1 | The \"_blank\" word. |
";
    let output = render_diagnostics(input, "blank-link-target.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for `\"_blank\"` inside a pipe-table cell. Got:\n{output}"
    );
}
