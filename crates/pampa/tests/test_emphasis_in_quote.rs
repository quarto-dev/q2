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

// --- Single-quoted variants ---

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
// State 705/691/714/760 collide between outer-quote and outer-emphasis contexts;
// the corpus fix from commit 6e3ad158 redirected those to the emphasis codes,
// so the outer_scope discriminator is required to keep Q-2-11 firing here.

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

// --- Different-marker emphasis-in-emphasis ---
//
// Outer emphasis pairs, inner emphasis of a different flavour is unclosed.
// Cross-product: 4 outer markers × 3 different inner markers = 12 cases.

#[test]
fn emph_star_with_unclosed_strong_star_emits_q_2_13() {
    let input = "*a **b c*\n";
    let output = render_diagnostics(input, "emph-star-with-strong-star.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 for unclosed `**` inside `*..*`. Got:\n{output}"
    );
}

#[test]
fn emph_star_with_unclosed_underscore_emits_q_2_5() {
    let input = "*a _b c*\n";
    let output = render_diagnostics(input, "emph-star-with-underscore.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for unclosed `_` inside `*..*`. Got:\n{output}"
    );
}

#[test]
fn emph_star_with_unclosed_strong_underscore_emits_q_2_15() {
    let input = "*a __b c*\n";
    let output = render_diagnostics(input, "emph-star-with-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` inside `*..*`. Got:\n{output}"
    );
}

#[test]
fn emph_underscore_with_unclosed_star_emits_q_2_12() {
    let input = "_a *b c_\n";
    let output = render_diagnostics(input, "emph-underscore-with-star.qmd");
    assert!(
        output.contains("Q-2-12"),
        "Expected Q-2-12 for unclosed `*` inside `_.._`. Got:\n{output}"
    );
}

#[test]
fn emph_underscore_with_unclosed_strong_star_emits_q_2_13() {
    let input = "_a **b c_\n";
    let output = render_diagnostics(input, "emph-underscore-with-strong-star.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 for unclosed `**` inside `_.._`. Got:\n{output}"
    );
}

#[test]
fn emph_underscore_with_unclosed_strong_underscore_emits_q_2_15() {
    let input = "_a __b c_\n";
    let output = render_diagnostics(input, "emph-underscore-with-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` inside `_.._`. Got:\n{output}"
    );
}

#[test]
fn strong_star_with_unclosed_star_emits_q_2_12() {
    let input = "**a *b c**\n";
    let output = render_diagnostics(input, "strong-star-with-star.qmd");
    assert!(
        output.contains("Q-2-12"),
        "Expected Q-2-12 for unclosed `*` inside `**..**`. Got:\n{output}"
    );
}

#[test]
fn strong_star_with_unclosed_underscore_emits_q_2_5() {
    let input = "**a _b c**\n";
    let output = render_diagnostics(input, "strong-star-with-underscore.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for unclosed `_` inside `**..**`. Got:\n{output}"
    );
}

#[test]
fn strong_star_with_unclosed_strong_underscore_emits_q_2_15() {
    let input = "**a __b c**\n";
    let output = render_diagnostics(input, "strong-star-with-strong-underscore.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for unclosed `__` inside `**..**`. Got:\n{output}"
    );
}

#[test]
fn strong_underscore_with_unclosed_star_emits_q_2_12() {
    let input = "__a *b c__\n";
    let output = render_diagnostics(input, "strong-underscore-with-star.qmd");
    assert!(
        output.contains("Q-2-12"),
        "Expected Q-2-12 for unclosed `*` inside `__..__`. Got:\n{output}"
    );
}

#[test]
fn strong_underscore_with_unclosed_strong_star_emits_q_2_13() {
    let input = "__a **b c__\n";
    let output = render_diagnostics(input, "strong-underscore-with-strong-star.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 for unclosed `**` inside `__..__`. Got:\n{output}"
    );
}

#[test]
fn strong_underscore_with_unclosed_underscore_emits_q_2_5() {
    let input = "__a _b c__\n";
    let output = render_diagnostics(input, "strong-underscore-with-underscore.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for unclosed `_` inside `__..__`. Got:\n{output}"
    );
}

// --- Same-marker emphasis nesting ---
//
// Two openers of the same marker, one closer. Per user observation the error
// code is correct but the source location may be wrong (points to text instead
// of the unclosed delimiter). These tests assert the code; location is captured
// in the failure message so a regression on location can be inspected manually.

#[test]
fn double_star_with_unclosed_inner_star_emits_q_2_12() {
    let input = "*a *b c*\n";
    let output = render_diagnostics(input, "double-star-nesting.qmd");
    assert!(
        output.contains("Q-2-12"),
        "Expected Q-2-12 for `*a *b c*` (one star left unclosed). Got:\n{output}"
    );
}

#[test]
fn double_underscore_with_unclosed_inner_underscore_emits_q_2_5() {
    let input = "_a _b c_\n";
    let output = render_diagnostics(input, "double-underscore-nesting.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for `_a _b c_` (one underscore left unclosed). Got:\n{output}"
    );
}

#[test]
fn double_strong_star_with_unclosed_inner_strong_star_emits_q_2_13() {
    let input = "**a **b c**\n";
    let output = render_diagnostics(input, "double-strong-star-nesting.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 for `**a **b c**` (one strong-star left unclosed). Got:\n{output}"
    );
}

#[test]
fn double_strong_underscore_with_unclosed_inner_strong_underscore_emits_q_2_15() {
    let input = "__a __b c__\n";
    let output = render_diagnostics(input, "double-strong-underscore-nesting.qmd");
    assert!(
        output.contains("Q-2-15"),
        "Expected Q-2-15 for `__a __b c__` (one strong-underscore left unclosed). Got:\n{output}"
    );
}

// --- Quote-in-quote ---
//
// Outer quote pairs, inner quote of the other flavour is unclosed.

#[test]
fn double_quote_with_unclosed_single_quote_emits_q_2_9() {
    // Whitespace-prefixed `'` is tokenized as an opener, not an apostrophe-
    // close, so this is an Unclosed Single Quote (Q-2-9), not Q-2-10.
    let input = "The \"a 'b c\" word.\n";
    let output = render_diagnostics(input, "double-quote-with-single-quote.qmd");
    assert!(
        output.contains("Q-2-9"),
        "Expected Q-2-9 for unclosed `'` inside `\"..\"`. Got:\n{output}"
    );
}

#[test]
fn single_quote_with_unclosed_double_quote_emits_q_2_11() {
    let input = "The 'a \"b c' word.\n";
    let output = render_diagnostics(input, "single-quote-with-double-quote.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `'..'`. Got:\n{output}"
    );
}

// --- Long-form (realistic) inputs with trailing text after the closing outer ---
//
// Inputs with trailing text after the closing outer delimiter land at a
// different (state, sym) than the bare form. The unified corpus cases include
// trailing-text variants so these realistic shapes still emit the correct
// Q-code instead of a generic Parse error.

#[test]
fn long_form_emph_star_with_underscore_emits_q_2_5() {
    let input = "*a _b c* word.\n";
    let output = render_diagnostics(input, "long-form-emph-star-underscore.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for `*a _b c* word.`. Got:\n{output}"
    );
}

#[test]
fn long_form_strong_underscore_with_strong_star_emits_q_2_13() {
    let input = "__a **b c__ word.\n";
    let output = render_diagnostics(input, "long-form-strong-underscore-strong-star.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 for `__a **b c__ word.`. Got:\n{output}"
    );
}

#[test]
fn long_form_strong_star_with_unclosed_single_quote_emits_q_2_9() {
    let input = "**a 'b c** word.\n";
    let output = render_diagnostics(input, "long-form-strong-star-single-quote.qmd");
    assert!(
        output.contains("Q-2-9"),
        "Expected Q-2-9 for `**a 'b c** word.`. Got:\n{output}"
    );
}

// --- Arbitrary-depth nesting via the generic-fallback dispatch ---
//
// The corpus no longer contains explicit entries for each (state, sym,
// outer_scope) triple produced by N-level nesting. Instead, the
// error-generation path dispatches by `outer_scope` alone when no Merr
// entry matches and the parser is inside an inline scope. These tests
// verify the fallback handles 3-level and deeper nesting uniformly.

#[test]
fn three_level_single_quote_star_emph_underscore_emits_q_2_5() {
    // Outer ', middle *, inner _ unclosed.
    let input = "a '*b _c* jeloasd' asdasd\n";
    let output = render_diagnostics(input, "three-level-q25.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for 3-level nest `'*b _c*'`. Got:\n{output}"
    );
}

#[test]
fn three_level_double_quote_strong_star_emph_underscore_emits_q_2_5() {
    let input = "\"**b _c**\"\n";
    let output = render_diagnostics(input, "three-level-q25-double-strong.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for `\"**b _c**\"`. Got:\n{output}"
    );
}

#[test]
fn three_level_emph_star_single_quote_unclosed_double_emits_q_2_11() {
    // Outer *, middle ', inner " unclosed.
    let input = "*a 'b \"c'*\n";
    let output = render_diagnostics(input, "three-level-q211.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for `*a 'b \"c'*`. Got:\n{output}"
    );
}

#[test]
fn three_level_strong_star_double_quote_unclosed_underscore_emits_q_2_5() {
    let input = "**a \"b _c\"**\n";
    let output = render_diagnostics(input, "three-level-q25-strong-double.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for `**a \"b _c\"**`. Got:\n{output}"
    );
}

// --- 3-level inputs where the MIDDLE scope is unmatched (inner pairs) ---
//
// The input has 3 levels of nesting but the innermost `_c_` pairs, so the
// scope stack at error time is depth 2: [outer-quote, middle-emphasis]. The
// fallback dispatches by the innermost-still-open scope (the middle
// emphasis), emitting the emphasis-class Q-code rather than the inner-class
// one.

#[test]
fn three_level_middle_star_unmatched_emits_q_2_12() {
    // Outer ' can't close because * is on the stack; inner _c_ pairs.
    let input = "a '*b _c_ jeloasd' asdasd\n";
    let output = render_diagnostics(input, "three-level-middle-q212.qmd");
    assert!(
        output.contains("Q-2-12"),
        "Expected Q-2-12 (middle * unmatched, inner _c_ pairs). Got:\n{output}"
    );
}

#[test]
fn three_level_middle_strong_star_unmatched_emits_q_2_13() {
    // Same shape but the middle is ** (strong star).
    let input = "a \"**b _c_ jeloasd\" asdasd\n";
    let output = render_diagnostics(input, "three-level-middle-q213.qmd");
    assert!(
        output.contains("Q-2-13"),
        "Expected Q-2-13 (middle ** unmatched, inner _c_ pairs). Got:\n{output}"
    );
}

// --- 4-level deep nesting: scope stack depth 4 at error ---
//
// All four scopes open and none pair (the would-be closers are blocked by
// the inner unmatched delimiter). Verifies the walker handles arbitrarily
// deep stacks.

#[test]
fn four_level_inner_underscore_unmatched_emits_q_2_5() {
    // ', *, ", _ all opened; _ never closes.
    let input = "a '*\"_b c\"*' x\n";
    let output = render_diagnostics(input, "four-level-q25.qmd");
    assert!(
        output.contains("Q-2-5"),
        "Expected Q-2-5 for 4-level nest with innermost _ unmatched. Got:\n{output}"
    );
}

#[test]
fn four_level_inner_double_quote_unmatched_emits_q_2_11() {
    // ', *, _, " all opened; " never closes.
    let input = "a '*_b \"c d_*' x\n";
    let output = render_diagnostics(input, "four-level-q211.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for 4-level nest with innermost \" unmatched. Got:\n{output}"
    );
}

// --- Unclosed quote inside paired emphasis (the gap fix) ---
//
// Outer emphasis pairs, an inner whitespace-prefixed `"` or `'` opens but
// never closes. The `"` cases map to Q-2-11 (Unclosed Double Quote); the `'`
// cases map to Q-2-9 (Unclosed Single Quote).

#[test]
fn emph_star_with_unclosed_double_quote_emits_q_2_11() {
    let input = "*a \"b c*\n";
    let output = render_diagnostics(input, "emph-star-with-unclosed-double-quote.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `*..*`. Got:\n{output}"
    );
}

#[test]
fn emph_underscore_with_unclosed_double_quote_emits_q_2_11() {
    let input = "_a \"b c_\n";
    let output = render_diagnostics(input, "emph-underscore-with-unclosed-double-quote.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `_.._`. Got:\n{output}"
    );
}

#[test]
fn strong_star_with_unclosed_double_quote_emits_q_2_11() {
    let input = "**a \"b c**\n";
    let output = render_diagnostics(input, "strong-star-with-unclosed-double-quote.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `**..**`. Got:\n{output}"
    );
}

#[test]
fn strong_underscore_with_unclosed_double_quote_emits_q_2_11() {
    let input = "__a \"b c__\n";
    let output = render_diagnostics(input, "strong-underscore-with-unclosed-double-quote.qmd");
    assert!(
        output.contains("Q-2-11"),
        "Expected Q-2-11 for unclosed `\"` inside `__..__`. Got:\n{output}"
    );
}

#[test]
fn emph_star_with_unclosed_single_quote_emits_q_2_9() {
    let input = "*a 'b c*\n";
    let output = render_diagnostics(input, "emph-star-with-unclosed-single-quote.qmd");
    assert!(
        output.contains("Q-2-9"),
        "Expected Q-2-9 for unclosed `'` inside `*..*`. Got:\n{output}"
    );
}

#[test]
fn emph_underscore_with_unclosed_single_quote_emits_q_2_9() {
    let input = "_a 'b c_\n";
    let output = render_diagnostics(input, "emph-underscore-with-unclosed-single-quote.qmd");
    assert!(
        output.contains("Q-2-9"),
        "Expected Q-2-9 for unclosed `'` inside `_.._`. Got:\n{output}"
    );
}

#[test]
fn strong_star_with_unclosed_single_quote_emits_q_2_9() {
    let input = "**a 'b c**\n";
    let output = render_diagnostics(input, "strong-star-with-unclosed-single-quote.qmd");
    assert!(
        output.contains("Q-2-9"),
        "Expected Q-2-9 for unclosed `'` inside `**..**`. Got:\n{output}"
    );
}

#[test]
fn strong_underscore_with_unclosed_single_quote_emits_q_2_9() {
    let input = "__a 'b c__\n";
    let output = render_diagnostics(input, "strong-underscore-with-unclosed-single-quote.qmd");
    assert!(
        output.contains("Q-2-9"),
        "Expected Q-2-9 for unclosed `'` inside `__..__`. Got:\n{output}"
    );
}
