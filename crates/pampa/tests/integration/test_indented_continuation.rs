/*
 * test_indented_continuation.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Regression tests for bd-indented-continuation-parse-error-j7be7kuc.
 *
 * 92737cdd (v0.18.0) made digit/dash/plus-leading continuation lines
 * soft-break instead of terminating the paragraph, but only at indent 0:
 * with leading indentation the same lines became hard parse errors that
 * drop the whole file. These tables sweep leader x indent x context.
 *
 * Expected values follow CommonMark interruption semantics (the ones
 * 92737cdd deliberately chose over pandoc-markdown's
 * no-interruption-without-blank-line rule). For every cell that was a
 * parse error, CommonMark and pandoc 3.9 agree on the expected output;
 * cells where qmd deliberately deviates from pandoc are marked as
 * controls. Characterization sweep:
 * claude-notes/plans/2026-08-11-indented-continuation-parse-error.md
 */

use pampa::readers;
use pampa::writers;

/// Parse input; Ok(whitespace-normalized native output) on success,
/// Err(diagnostic titles) when parsing fails or reports an error
/// diagnostic.
fn parse_native(input: &str) -> Result<String, String> {
    let result = readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    );
    match result {
        Ok((pandoc, ctx, diagnostics)) => {
            let errors: Vec<String> = diagnostics
                .iter()
                .filter(|d| d.kind == quarto_error_reporting::DiagnosticKind::Error)
                .map(|d| d.title.clone())
                .collect();
            if !errors.is_empty() {
                return Err(errors.join("; "));
            }
            let mut buf = Vec::new();
            writers::native::write(&pandoc, &ctx, &mut buf).unwrap();
            Ok(normalize_ws(&String::from_utf8(buf).unwrap()))
        }
        Err(diagnostics) => Err(diagnostics
            .iter()
            .map(|d| d.title.clone())
            .collect::<Vec<_>>()
            .join("; ")),
    }
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The four block contexts a continuation line is swept through.
/// `{}` is replaced by the (indented) continuation line.
const CTX_TOP: (&str, &str) = ("top", "hello\n{}\n");
const CTX_BULLET: (&str, &str) = ("bullet", "- a\n{}\n");
const CTX_ORDERED: (&str, &str) = ("ordered", "1. one\n{}\n");
const CTX_QUOTE: (&str, &str) = ("quote", "> a\n{}\n");

fn make_input(ctx_template: &str, indent: usize, line: &str) -> String {
    ctx_template.replace("{}", &format!("{}{}", " ".repeat(indent), line))
}

/// Expected AST when the continuation line is prose absorbed into the
/// open paragraph of the given context. `cont` is the inline sequence
/// for the continuation text (no leading Space: continuation-line
/// leading whitespace is stripped, matching pandoc).
fn prose_expected(ctx_name: &str, cont: &str) -> String {
    match ctx_name {
        "top" => format!("[ Para [Str \"hello\", SoftBreak, {}] ]", cont),
        "bullet" => format!("[ BulletList [[Plain [Str \"a\", SoftBreak, {}]]] ]", cont),
        "ordered" => format!(
            "[ OrderedList (1, Decimal, Period) [[Plain [Str \"one\", SoftBreak, {}]]] ]",
            cont
        ),
        "quote" => format!("[ BlockQuote [Para [Str \"a\", SoftBreak, {}]] ]", cont),
        _ => unreachable!(),
    }
}

struct Failure {
    cell: String,
    expected: String,
    actual: String,
}

fn check_cell(failures: &mut Vec<Failure>, cell: String, input: &str, expected: Result<&str, ()>) {
    let actual = parse_native(input);
    let ok = match (&expected, &actual) {
        (Ok(e), Ok(a)) => normalize_ws(e) == *a,
        (Err(()), Err(_)) => true,
        _ => false,
    };
    if !ok {
        failures.push(Failure {
            cell,
            expected: match expected {
                Ok(e) => normalize_ws(e),
                Err(()) => "<parse error>".to_string(),
            },
            actual: match actual {
                Ok(a) => a,
                Err(e) => format!("<parse error: {}>", e),
            },
        });
    }
}

fn report(failures: Vec<Failure>) {
    if failures.is_empty() {
        return;
    }
    let mut msg = format!("{} cell(s) failed:\n", failures.len());
    for f in &failures {
        msg.push_str(&format!(
            "  {}\n    expected: {}\n    actual:   {}\n",
            f.cell, f.expected, f.actual
        ));
    }
    panic!("{}", msg);
}

/// Prose-leading continuation lines (`-5`, `--`, `+5`, digit-leading)
/// must soft-break into the open paragraph at EVERY indent in EVERY
/// context. Indent 0 cells are the pre-existing behavior 92737cdd
/// fixed; indent >= 1 cells are the regression.
#[test]
fn prose_continuation_lines_all_indents_all_contexts() {
    // (line, expected inline sequence). "--" becomes an en dash via
    // qmd's default smart typography.
    let leaders: &[(&str, &str)] = &[
        ("-5 degrees", "Str \"-5\", Space, Str \"degrees\""),
        (
            "-- em dash",
            "Str \"\u{2013}\", Space, Str \"em\", Space, Str \"dash\"",
        ),
        ("+5 things", "Str \"+5\", Space, Str \"things\""),
        ("30 minutes", "Str \"30\", Space, Str \"minutes\""),
    ];
    let mut failures = Vec::new();
    for ctx in [CTX_TOP, CTX_BULLET, CTX_ORDERED, CTX_QUOTE] {
        for (line, cont) in leaders {
            for indent in [0usize, 1, 2, 3, 4, 6] {
                let input = make_input(ctx.1, indent, line);
                let expected = prose_expected(ctx.0, cont);
                check_cell(
                    &mut failures,
                    format!("ctx={} indent={} line={:?}", ctx.0, indent, line),
                    &input,
                    Ok(&expected),
                );
            }
        }
    }
    report(failures);
}

/// List markers on a continuation line, over-indented past where a
/// marker can form (relative indent >= 4): CommonMark says the line is
/// lazy paragraph continuation (indented code cannot interrupt a
/// paragraph), so it must be prose. pandoc agrees on every cell.
#[test]
fn over_indented_markers_are_prose_continuation() {
    let leaders: &[(&str, &str)] = &[
        ("- item", "Str \"-\", Space, Str \"item\""),
        ("+ item", "Str \"+\", Space, Str \"item\""),
        ("1. nested", "Str \"1.\", Space, Str \"nested\""),
    ];
    // (ctx, over-indent threshold cells). Content columns: top 0,
    // bullet 2, ordered 3, quote (lazy: no prefix matched) 0.
    let cells: &[((&str, &str), &[usize])] = &[
        ((CTX_TOP.0, CTX_TOP.1), &[4, 6, 10]),
        ((CTX_BULLET.0, CTX_BULLET.1), &[6, 10]),
        ((CTX_ORDERED.0, CTX_ORDERED.1), &[10]),
        ((CTX_QUOTE.0, CTX_QUOTE.1), &[4, 6, 10]),
    ];
    let mut failures = Vec::new();
    for ((ctx_name, ctx_template), indents) in cells {
        for (line, cont) in leaders {
            for &indent in *indents {
                let input = make_input(ctx_template, indent, line);
                let expected = prose_expected(ctx_name, cont);
                check_cell(
                    &mut failures,
                    format!("ctx={} indent={} line={:?}", ctx_name, indent, line),
                    &input,
                    Ok(&expected),
                );
            }
        }
    }
    report(failures);
}

/// List markers at a valid nesting indent (relative indent 1..=3 past
/// the item's content column) must open a nested list, exactly as at
/// the content column itself. This is the Connect-docs case: the
/// indent-4 and indent-6 cells were hard parse errors.
#[test]
fn nested_markers_at_valid_relative_indent_nest() {
    let mut failures = Vec::new();
    let nested_ordered_in_ordered = "[ OrderedList (1, Decimal, Period) \
         [[Plain [Str \"one\"], OrderedList (1, Decimal, Period) [[Plain [Str \"nested\"]]]]] ]";
    // ordered ctx, "1. nested": content col 3; indent 3 works today
    // (control), 4 and 6 are the regression.
    for indent in [3usize, 4, 6] {
        let input = make_input(CTX_ORDERED.1, indent, "1. nested");
        check_cell(
            &mut failures,
            format!("ctx=ordered indent={} line=\"1. nested\"", indent),
            &input,
            Ok(nested_ordered_in_ordered),
        );
    }
    let nested_bullet_in_bullet =
        "[ BulletList [[Plain [Str \"a\"], BulletList [[Plain [Str \"item\"]]]]] ]";
    // bullet ctx, "- item": content col 2; indent 2 and 4 work today
    // (controls) — relative indent 0 and 2.
    for indent in [2usize, 4] {
        let input = make_input(CTX_BULLET.1, indent, "- item");
        check_cell(
            &mut failures,
            format!("ctx=bullet indent={} line=\"- item\"", indent),
            &input,
            Ok(nested_bullet_in_bullet),
        );
    }
    // bullet ctx, "1. nested" at indent 4 (relative 2): nested ordered
    // list inside the bullet item. Regression cell.
    let nested_ordered_in_bullet = "[ BulletList \
         [[Plain [Str \"a\"], OrderedList (1, Decimal, Period) [[Plain [Str \"nested\"]]]]] ]";
    check_cell(
        &mut failures,
        "ctx=bullet indent=4 line=\"1. nested\"".to_string(),
        &make_input(CTX_BULLET.1, 4, "1. nested"),
        Ok(nested_ordered_in_bullet),
    );
    report(failures);
}

/// Controls: markers at indent 0..=3 interrupting a top-level
/// paragraph, and sibling items. qmd deliberately follows CommonMark
/// here (pandoc-markdown would treat these as prose); these cells
/// worked before and after 92737cdd and must not change.
#[test]
fn controls_marker_interruption_and_siblings() {
    let mut failures = Vec::new();
    let bullet_interrupts = "[ Para [Str \"hello\"], BulletList [[Plain [Str \"item\"]]] ]";
    for indent in [0usize, 1, 3] {
        check_cell(
            &mut failures,
            format!("ctx=top indent={} line=\"- item\"", indent),
            &make_input(CTX_TOP.1, indent, "- item"),
            Ok(bullet_interrupts),
        );
    }
    let ordered_interrupts =
        "[ Para [Str \"hello\"], OrderedList (1, Decimal, Period) [[Plain [Str \"nested\"]]] ]";
    for indent in [0usize, 1, 3] {
        check_cell(
            &mut failures,
            format!("ctx=top indent={} line=\"1. nested\"", indent),
            &make_input(CTX_TOP.1, indent, "1. nested"),
            Ok(ordered_interrupts),
        );
    }
    // Sibling items at indent 0.
    check_cell(
        &mut failures,
        "ctx=bullet indent=0 line=\"- item\"".to_string(),
        &make_input(CTX_BULLET.1, 0, "- item"),
        Ok("[ BulletList [[Plain [Str \"a\"]], [Plain [Str \"item\"]]] ]"),
    );
    check_cell(
        &mut failures,
        "ctx=ordered indent=0 line=\"1. nested\"".to_string(),
        &make_input(CTX_ORDERED.1, 0, "1. nested"),
        Ok(
            "[ OrderedList (1, Decimal, Period) [[Plain [Str \"one\"]], [Plain [Str \"nested\"]]] ]",
        ),
    );
    // A marker line at indent 0-3 after a block quote is NOT lazy
    // continuation (it would open a block, so laziness does not apply):
    // the quote closes and a top-level list forms. CommonMark; deviates
    // from pandoc-markdown; matches current behavior.
    check_cell(
        &mut failures,
        "ctx=quote indent=0 line=\"- item\"".to_string(),
        &make_input(CTX_QUOTE.1, 0, "- item"),
        Ok("[ BlockQuote [Para [Str \"a\"]], BulletList [[Plain [Str \"item\"]]] ]"),
    );
    check_cell(
        &mut failures,
        "ctx=quote indent=0 line=\"1. nested\"".to_string(),
        &make_input(CTX_QUOTE.1, 0, "1. nested"),
        Ok(
            "[ BlockQuote [Para [Str \"a\"]], OrderedList (1, Decimal, Period) [[Plain [Str \"nested\"]]] ]",
        ),
    );
    report(failures);
}

/// Controls: indented continuation lines whose first character starts
/// an inline external token (backtick code span, star emphasis) parse
/// today and must keep parsing. Note: current output carries an extra
/// `Space` after `SoftBreak` (pandoc emits none — the continuation
/// indent leaks into the inline stream via the external token's range).
/// That divergence predates this fix and is tracked separately; these
/// cells pin the parse-success and overall shape, Space included, so
/// any change to it is a deliberate decision.
#[test]
fn controls_backtick_and_star_continuations() {
    let mut failures = Vec::new();
    check_cell(
        &mut failures,
        "ctx=top indent=2 line=\"`code` here\"".to_string(),
        &make_input(CTX_TOP.1, 2, "`code` here"),
        Ok(
            "[ Para [Str \"hello\", SoftBreak, Space, Code ( \"\" , [] , [] ) \"code\", Space, Str \"here\"] ]",
        ),
    );
    check_cell(
        &mut failures,
        "ctx=top indent=2 line=\"*emph* here\"".to_string(),
        &make_input(CTX_TOP.1, 2, "*emph* here"),
        Ok("[ Para [Str \"hello\", SoftBreak, Space, Emph [Str \"emph\"], Space, Str \"here\"] ]"),
    );
    check_cell(
        &mut failures,
        "ctx=quote indent=2 line=\"`code` code\"".to_string(),
        &make_input(CTX_QUOTE.1, 2, "`code` code"),
        Ok(
            "[ BlockQuote [Para [Str \"a\", SoftBreak, Space, Code ( \"\" , [] , [] ) \"code\", Space, Str \"code\"]] ]",
        ),
    );
    report(failures);
}

/// Deliberate qmd strictness: an unclosed `*` emphasis is an error at
/// any indent (pandoc would fall back to a literal `*5`). These must
/// KEEP failing — they are not part of the regression.
#[test]
fn deliberate_unclosed_emphasis_errors() {
    let mut failures = Vec::new();
    for indent in [0usize, 2] {
        check_cell(
            &mut failures,
            format!("ctx=top indent={} line=\"*5 stars\"", indent),
            &make_input(CTX_TOP.1, indent, "*5 stars"),
            Err(()),
        );
    }
    report(failures);
}
