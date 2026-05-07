/*
 * pipes.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Pipe-transformation evaluator.
//!
//! Doctemplate variable references and partial outputs may carry a
//! sequence of pipes that transform the value before it reaches the
//! [`Doc`](crate::doc::Doc) tree. The supported pipe set is fixed by
//! the tree-sitter grammar — see
//! `crates/tree-sitter-doctemplate/grammar/grammar.js` lines 56–73.
//! Adding a new pipe name requires a grammar change.
//!
//! Each pipe is a pure function over [`TemplateValue`]. Unknown names
//! emit `Q-10-6`; bad arguments emit `Q-10-7`. In both error cases
//! the value passes through unchanged.
//!
//! Pipes are applied left-to-right. `$xs/uppercase/length$` first
//! uppercases each string in the list, then takes the list length.

use crate::ast::{Pipe, PipeArg};
use crate::context::TemplateValue;
use crate::eval_context::EvalContext;

/// Apply a sequence of pipes to a value, in source order.
///
/// On unknown pipe names or bad argument types, emits a diagnostic on
/// `ctx` and the value passes through unchanged. Returns the
/// possibly-transformed value.
pub fn apply_pipes(value: TemplateValue, pipes: &[Pipe], ctx: &mut EvalContext) -> TemplateValue {
    let mut current = value;
    for pipe in pipes {
        current = apply_pipe(current, pipe, ctx);
    }
    current
}

/// Apply a single pipe.
///
/// Public so callers that want to dispatch a single named pipe (e.g.
/// future cmdline tooling) can do so. In normal evaluation
/// [`apply_pipes`] is the only call site.
pub fn apply_pipe(value: TemplateValue, pipe: &Pipe, ctx: &mut EvalContext) -> TemplateValue {
    match pipe.name.as_str() {
        "pairs" => pipe_pairs(value),
        "first" => pipe_first(value),
        "last" => pipe_last(value),
        "rest" => pipe_rest(value),
        "allbutlast" => pipe_allbutlast(value),
        "length" => pipe_length(value),
        "uppercase" => pipe_uppercase(value),
        "lowercase" => pipe_lowercase(value),
        "reverse" => pipe_reverse(value),
        "chomp" => pipe_chomp(value),
        "nowrap" => value, // No-op for markdown-output use case; see module docs.
        "alpha" => pipe_alpha(value, pipe, ctx),
        "roman" => pipe_roman(value, pipe, ctx),
        "left" => pipe_pad(value, &pipe.args, PadKind::Right, pipe, ctx),
        "center" => pipe_pad(value, &pipe.args, PadKind::Center, pipe, ctx),
        "right" => pipe_pad(value, &pipe.args, PadKind::Left, pipe, ctx),
        other => {
            ctx.warn_or_error_with_code(
                "Q-10-6",
                format!("Unknown pipe: {}", other),
                &pipe.source_info,
            );
            value
        }
    }
}

fn pipe_pairs(value: TemplateValue) -> TemplateValue {
    match value {
        TemplateValue::Map(m) => {
            // Pandoc's `pairs` over a map produces a list of
            // `{ key: <k>, value: <v> }` records. Custom templates
            // iterate as `$for(it)$$it.key$$endfor$`.
            let mut entries: Vec<(String, TemplateValue)> = m.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            TemplateValue::List(
                entries
                    .into_iter()
                    .map(|(k, v)| {
                        let mut pair = std::collections::HashMap::new();
                        pair.insert("key".to_string(), TemplateValue::String(k));
                        pair.insert("value".to_string(), v);
                        TemplateValue::Map(pair)
                    })
                    .collect(),
            )
        }
        other => other,
    }
}

fn pipe_first(value: TemplateValue) -> TemplateValue {
    match value {
        TemplateValue::List(mut items) if !items.is_empty() => items.remove(0),
        TemplateValue::String(s) => match s.chars().next() {
            Some(c) => TemplateValue::String(c.to_string()),
            None => TemplateValue::String(String::new()),
        },
        other => other,
    }
}

fn pipe_last(value: TemplateValue) -> TemplateValue {
    match value {
        TemplateValue::List(mut items) if !items.is_empty() => items.pop().unwrap(),
        TemplateValue::String(s) => match s.chars().last() {
            Some(c) => TemplateValue::String(c.to_string()),
            None => TemplateValue::String(String::new()),
        },
        other => other,
    }
}

fn pipe_rest(value: TemplateValue) -> TemplateValue {
    match value {
        TemplateValue::List(items) => {
            if items.is_empty() {
                TemplateValue::List(Vec::new())
            } else {
                TemplateValue::List(items.into_iter().skip(1).collect())
            }
        }
        TemplateValue::String(s) => {
            let mut chars = s.chars();
            chars.next();
            TemplateValue::String(chars.collect())
        }
        other => other,
    }
}

fn pipe_allbutlast(value: TemplateValue) -> TemplateValue {
    match value {
        TemplateValue::List(items) => {
            if items.is_empty() {
                TemplateValue::List(Vec::new())
            } else {
                let n = items.len() - 1;
                TemplateValue::List(items.into_iter().take(n).collect())
            }
        }
        TemplateValue::String(s) => {
            let total = s.chars().count();
            if total == 0 {
                TemplateValue::String(String::new())
            } else {
                TemplateValue::String(s.chars().take(total - 1).collect())
            }
        }
        other => other,
    }
}

fn pipe_length(value: TemplateValue) -> TemplateValue {
    let n = match &value {
        TemplateValue::List(items) => items.len(),
        TemplateValue::String(s) => s.chars().count(),
        TemplateValue::Map(m) => m.len(),
        // Bool / Null have no meaningful length; return 0 to match
        // Pandoc's behavior on empty values.
        _ => 0,
    };
    TemplateValue::String(n.to_string())
}

fn pipe_uppercase(value: TemplateValue) -> TemplateValue {
    map_strings(value, |s| s.to_uppercase())
}

fn pipe_lowercase(value: TemplateValue) -> TemplateValue {
    map_strings(value, |s| s.to_lowercase())
}

fn pipe_reverse(value: TemplateValue) -> TemplateValue {
    match value {
        TemplateValue::List(items) => TemplateValue::List(items.into_iter().rev().collect()),
        TemplateValue::String(s) => TemplateValue::String(s.chars().rev().collect()),
        other => other,
    }
}

fn pipe_chomp(value: TemplateValue) -> TemplateValue {
    match value {
        TemplateValue::String(s) => {
            let trimmed = s
                .strip_suffix("\r\n")
                .or_else(|| s.strip_suffix('\n'))
                .map(|t| t.to_string())
                .unwrap_or(s);
            TemplateValue::String(trimmed)
        }
        TemplateValue::List(items) => {
            TemplateValue::List(items.into_iter().map(pipe_chomp).collect())
        }
        other => other,
    }
}

fn pipe_alpha(value: TemplateValue, pipe: &Pipe, ctx: &mut EvalContext) -> TemplateValue {
    let n = match parse_positive_integer(&value) {
        Some(n) => n,
        None => {
            ctx.warn_or_error_with_code(
                "Q-10-7",
                "alpha: input must be a positive integer".to_string(),
                &pipe.source_info,
            );
            return value;
        }
    };
    TemplateValue::String(int_to_alpha(n))
}

fn pipe_roman(value: TemplateValue, pipe: &Pipe, ctx: &mut EvalContext) -> TemplateValue {
    let n = match parse_positive_integer(&value) {
        Some(n) => n,
        None => {
            ctx.warn_or_error_with_code(
                "Q-10-7",
                "roman: input must be a positive integer".to_string(),
                &pipe.source_info,
            );
            return value;
        }
    };
    TemplateValue::String(int_to_roman_lower(n))
}

#[derive(Debug, Clone, Copy)]
enum PadKind {
    Left,
    Right,
    Center,
}

fn pipe_pad(
    value: TemplateValue,
    args: &[PipeArg],
    kind: PadKind,
    pipe: &Pipe,
    ctx: &mut EvalContext,
) -> TemplateValue {
    // Pandoc's grammar for `left|center|right` is
    //   `<name> N "leftborder" "rightborder"`
    // (always three args; empty strings allowed). The borders are
    // prepended/appended verbatim to a non-empty value, and the body
    // pads to N chars on the appropriate side using ASCII spaces.
    // Empty input produces an empty result with no borders emitted —
    // matches Pandoc's "skip when value is empty" behavior. Borders
    // are tolerated as missing (defaulting to "") so synthesized
    // calls from Rust without the full triple still work.
    let width = match args.first() {
        Some(PipeArg::Integer(n)) if *n >= 0 => *n as usize,
        Some(_) => {
            ctx.warn_or_error_with_code(
                "Q-10-7",
                format!(
                    "{}: width argument must be a non-negative integer",
                    pipe.name
                ),
                &pipe.source_info,
            );
            return value;
        }
        None => {
            ctx.warn_or_error_with_code(
                "Q-10-7",
                format!("{}: missing width argument", pipe.name),
                &pipe.source_info,
            );
            return value;
        }
    };
    let leftborder = match args.get(1) {
        Some(PipeArg::String(s)) => s.clone(),
        None => String::new(),
        Some(_) => {
            ctx.warn_or_error_with_code(
                "Q-10-7",
                format!("{}: leftborder argument must be a string", pipe.name),
                &pipe.source_info,
            );
            return value;
        }
    };
    let rightborder = match args.get(2) {
        Some(PipeArg::String(s)) => s.clone(),
        None => String::new(),
        Some(_) => {
            ctx.warn_or_error_with_code(
                "Q-10-7",
                format!("{}: rightborder argument must be a string", pipe.name),
                &pipe.source_info,
            );
            return value;
        }
    };
    let s = match &value {
        TemplateValue::String(s) => s.clone(),
        TemplateValue::Bool(true) => "true".to_string(),
        TemplateValue::Bool(false) => String::new(),
        TemplateValue::Null => String::new(),
        // Lists/maps don't pad meaningfully — pass through.
        _ => return value,
    };
    if s.is_empty() {
        return TemplateValue::String(String::new());
    }
    let len = s.chars().count();
    let body = if len >= width {
        s
    } else {
        let missing = width - len;
        match kind {
            PadKind::Right => pad_with_spaces_right(&s, missing),
            PadKind::Left => pad_with_spaces_left(&s, missing),
            PadKind::Center => {
                let left = missing / 2;
                let right = missing - left;
                let with_left = pad_with_spaces_left(&s, left);
                pad_with_spaces_right(&with_left, right)
            }
        }
    };
    let mut out = String::new();
    out.push_str(&leftborder);
    out.push_str(&body);
    out.push_str(&rightborder);
    TemplateValue::String(out)
}

fn pad_with_spaces_right(s: &str, missing: usize) -> String {
    let mut out = s.to_string();
    for _ in 0..missing {
        out.push(' ');
    }
    out
}

fn pad_with_spaces_left(s: &str, missing: usize) -> String {
    let mut out = String::with_capacity(s.len() + missing);
    for _ in 0..missing {
        out.push(' ');
    }
    out.push_str(s);
    out
}

fn map_strings(value: TemplateValue, f: impl Fn(&str) -> String + Copy) -> TemplateValue {
    match value {
        TemplateValue::String(s) => TemplateValue::String(f(&s)),
        TemplateValue::List(items) => {
            TemplateValue::List(items.into_iter().map(|v| map_strings(v, f)).collect())
        }
        other => other,
    }
}

fn parse_positive_integer(value: &TemplateValue) -> Option<u32> {
    match value {
        TemplateValue::String(s) => s.trim().parse::<u32>().ok().filter(|n| *n > 0),
        _ => None,
    }
}

fn int_to_alpha(n: u32) -> String {
    // 1 → "a", 2 → "b", …, 26 → "z", 27 → "aa", …
    let mut n = n;
    let mut out = Vec::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.push((b'a' + rem) as char);
        n = (n - 1) / 26;
    }
    out.into_iter().rev().collect()
}

fn int_to_roman_lower(mut n: u32) -> String {
    const TABLE: &[(u32, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (v, sym) in TABLE {
        while n >= *v {
            out.push_str(sym);
            n -= *v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::TemplateContext;
    use quarto_source_map::SourceInfo;

    fn pipe(name: &str) -> Pipe {
        Pipe::new(name, SourceInfo::default())
    }

    fn pipe_with_args(name: &str, args: Vec<PipeArg>) -> Pipe {
        Pipe::with_args(name, args, SourceInfo::default())
    }

    fn s(value: &str) -> TemplateValue {
        TemplateValue::String(value.to_string())
    }

    fn list(items: Vec<TemplateValue>) -> TemplateValue {
        TemplateValue::List(items)
    }

    fn run(
        value: TemplateValue,
        p: &Pipe,
    ) -> (
        TemplateValue,
        Vec<quarto_error_reporting::DiagnosticMessage>,
    ) {
        let vars = TemplateContext::new();
        let mut ctx = EvalContext::new(&vars);
        let out = apply_pipe(value, p, &mut ctx);
        (out, ctx.into_diagnostics())
    }

    // ---- first ------------------------------------------------------

    #[test]
    fn first_of_list_returns_head() {
        let (out, diags) = run(list(vec![s("a"), s("b"), s("c")]), &pipe("first"));
        assert_eq!(out, s("a"));
        assert!(diags.is_empty());
    }

    #[test]
    fn first_of_string_returns_first_char() {
        let (out, _) = run(s("hello"), &pipe("first"));
        assert_eq!(out, s("h"));
    }

    #[test]
    fn first_of_empty_string_returns_empty() {
        let (out, _) = run(s(""), &pipe("first"));
        assert_eq!(out, s(""));
    }

    // ---- last -------------------------------------------------------

    #[test]
    fn last_of_list_returns_tail() {
        let (out, _) = run(list(vec![s("a"), s("b"), s("c")]), &pipe("last"));
        assert_eq!(out, s("c"));
    }

    #[test]
    fn last_of_string_returns_last_char() {
        let (out, _) = run(s("hello"), &pipe("last"));
        assert_eq!(out, s("o"));
    }

    // ---- rest -------------------------------------------------------

    #[test]
    fn rest_of_list_drops_head() {
        let (out, _) = run(list(vec![s("a"), s("b"), s("c")]), &pipe("rest"));
        assert_eq!(out, list(vec![s("b"), s("c")]));
    }

    #[test]
    fn rest_of_string_drops_first_char() {
        let (out, _) = run(s("hello"), &pipe("rest"));
        assert_eq!(out, s("ello"));
    }

    #[test]
    fn rest_of_empty_list_returns_empty() {
        let (out, _) = run(list(vec![]), &pipe("rest"));
        assert_eq!(out, list(vec![]));
    }

    // ---- allbutlast -------------------------------------------------

    #[test]
    fn allbutlast_of_list_drops_tail() {
        let (out, _) = run(list(vec![s("a"), s("b"), s("c")]), &pipe("allbutlast"));
        assert_eq!(out, list(vec![s("a"), s("b")]));
    }

    #[test]
    fn allbutlast_of_string_drops_last_char() {
        let (out, _) = run(s("hello"), &pipe("allbutlast"));
        assert_eq!(out, s("hell"));
    }

    // ---- length -----------------------------------------------------

    #[test]
    fn length_of_list_is_count() {
        let (out, _) = run(list(vec![s("a"), s("b"), s("c")]), &pipe("length"));
        assert_eq!(out, s("3"));
    }

    #[test]
    fn length_of_string_is_char_count() {
        let (out, _) = run(s("héllo"), &pipe("length"));
        assert_eq!(out, s("5"));
    }

    #[test]
    fn length_of_empty_list_is_zero() {
        let (out, _) = run(list(vec![]), &pipe("length"));
        assert_eq!(out, s("0"));
    }

    // ---- case conversions -----------------------------------------

    #[test]
    fn uppercase_string() {
        let (out, _) = run(s("hello"), &pipe("uppercase"));
        assert_eq!(out, s("HELLO"));
    }

    #[test]
    fn uppercase_list_strings() {
        let (out, _) = run(list(vec![s("a"), s("b")]), &pipe("uppercase"));
        assert_eq!(out, list(vec![s("A"), s("B")]));
    }

    #[test]
    fn lowercase_string() {
        let (out, _) = run(s("HELLO"), &pipe("lowercase"));
        assert_eq!(out, s("hello"));
    }

    // ---- reverse ----------------------------------------------------

    #[test]
    fn reverse_string() {
        let (out, _) = run(s("hello"), &pipe("reverse"));
        assert_eq!(out, s("olleh"));
    }

    #[test]
    fn reverse_list() {
        let (out, _) = run(list(vec![s("a"), s("b"), s("c")]), &pipe("reverse"));
        assert_eq!(out, list(vec![s("c"), s("b"), s("a")]));
    }

    // ---- chomp ------------------------------------------------------

    #[test]
    fn chomp_removes_trailing_newline() {
        let (out, _) = run(s("hello\n"), &pipe("chomp"));
        assert_eq!(out, s("hello"));
    }

    #[test]
    fn chomp_removes_trailing_crlf() {
        let (out, _) = run(s("hello\r\n"), &pipe("chomp"));
        assert_eq!(out, s("hello"));
    }

    #[test]
    fn chomp_no_trailing_newline_leaves_alone() {
        let (out, _) = run(s("hello"), &pipe("chomp"));
        assert_eq!(out, s("hello"));
    }

    // ---- nowrap (no-op for v1) -------------------------------------

    #[test]
    fn nowrap_passes_through() {
        let (out, _) = run(s("hello world"), &pipe("nowrap"));
        assert_eq!(out, s("hello world"));
    }

    // ---- alpha ------------------------------------------------------

    #[test]
    fn alpha_one_is_a() {
        let (out, _) = run(s("1"), &pipe("alpha"));
        assert_eq!(out, s("a"));
    }

    #[test]
    fn alpha_twentysix_is_z() {
        let (out, _) = run(s("26"), &pipe("alpha"));
        assert_eq!(out, s("z"));
    }

    #[test]
    fn alpha_twentyseven_is_aa() {
        let (out, _) = run(s("27"), &pipe("alpha"));
        assert_eq!(out, s("aa"));
    }

    #[test]
    fn alpha_non_integer_emits_diagnostic() {
        let (_out, diags) = run(s("abc"), &pipe("alpha"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-10-7"));
    }

    // ---- roman ------------------------------------------------------

    #[test]
    fn roman_one_is_i() {
        let (out, _) = run(s("1"), &pipe("roman"));
        assert_eq!(out, s("i"));
    }

    #[test]
    fn roman_four_is_iv() {
        let (out, _) = run(s("4"), &pipe("roman"));
        assert_eq!(out, s("iv"));
    }

    #[test]
    fn roman_fortytwo_is_xlii() {
        let (out, _) = run(s("42"), &pipe("roman"));
        assert_eq!(out, s("xlii"));
    }

    #[test]
    fn roman_1994_is_mcmxciv() {
        let (out, _) = run(s("1994"), &pipe("roman"));
        assert_eq!(out, s("mcmxciv"));
    }

    // ---- left / right / center ------------------------------------
    //
    // Pandoc's grammar for these pipes is `name N "left" "right"`.
    // Our impl tolerates the borders being absent (defaulting to "")
    // so synthetic pipe constructions stay readable. End-to-end
    // tests through the parser exercise the three-arg form
    // (see evaluator.rs e2e tests).

    #[test]
    fn left_pad_to_width() {
        let (out, _) = run(s("ab"), &pipe_with_args("left", vec![PipeArg::Integer(5)]));
        assert_eq!(out, s("ab   "));
    }

    #[test]
    fn left_no_op_when_value_longer_than_width() {
        let (out, _) = run(
            s("abcde"),
            &pipe_with_args("left", vec![PipeArg::Integer(3)]),
        );
        assert_eq!(out, s("abcde"));
    }

    #[test]
    fn right_pad_to_width() {
        let (out, _) = run(s("ab"), &pipe_with_args("right", vec![PipeArg::Integer(5)]));
        assert_eq!(out, s("   ab"));
    }

    #[test]
    fn center_pad_to_width() {
        let (out, _) = run(
            s("ab"),
            &pipe_with_args("center", vec![PipeArg::Integer(6)]),
        );
        assert_eq!(out, s("  ab  "));
    }

    #[test]
    fn left_with_borders_wraps_padded_body() {
        let (out, _) = run(
            s("ab"),
            &pipe_with_args(
                "left",
                vec![
                    PipeArg::Integer(5),
                    PipeArg::String("[".to_string()),
                    PipeArg::String("]".to_string()),
                ],
            ),
        );
        assert_eq!(out, s("[ab   ]"));
    }

    #[test]
    fn left_empty_input_emits_nothing_even_with_borders() {
        // Pandoc's documented behavior: when the value is empty, the
        // borders are not emitted either. This lets templates
        // conditionally wrap optional fields without producing
        // empty `[]` artifacts.
        let (out, _) = run(
            s(""),
            &pipe_with_args(
                "left",
                vec![
                    PipeArg::Integer(5),
                    PipeArg::String("[".to_string()),
                    PipeArg::String("]".to_string()),
                ],
            ),
        );
        assert_eq!(out, s(""));
    }

    #[test]
    fn left_missing_width_arg_emits_diagnostic() {
        let (_out, diags) = run(s("ab"), &pipe_with_args("left", vec![]));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-10-7"));
    }

    #[test]
    fn left_negative_width_emits_diagnostic() {
        let (_out, diags) = run(s("ab"), &pipe_with_args("left", vec![PipeArg::Integer(-3)]));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-10-7"));
    }

    // ---- pairs ------------------------------------------------------

    #[test]
    fn pairs_of_map_returns_sorted_kv_records() {
        let mut m = std::collections::HashMap::new();
        m.insert("b".to_string(), s("two"));
        m.insert("a".to_string(), s("one"));
        let (out, _) = run(TemplateValue::Map(m), &pipe("pairs"));
        match out {
            TemplateValue::List(items) => {
                assert_eq!(items.len(), 2);
                // Sorted by key
                if let TemplateValue::Map(first) = &items[0] {
                    assert_eq!(first.get("key"), Some(&s("a")));
                    assert_eq!(first.get("value"), Some(&s("one")));
                } else {
                    panic!("expected map at index 0");
                }
            }
            other => panic!("expected list, got {:?}", other),
        }
    }

    // ---- unknown pipe ----------------------------------------------

    #[test]
    fn unknown_pipe_emits_q_10_6_and_passes_through() {
        let (out, diags) = run(s("hello"), &pipe("nosuchpipe"));
        assert_eq!(out, s("hello"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-10-6"));
    }

    // ---- chaining (apply_pipes) ------------------------------------

    #[test]
    fn chain_uppercase_then_length() {
        let vars = TemplateContext::new();
        let mut ctx = EvalContext::new(&vars);
        let out = apply_pipes(
            list(vec![s("a"), s("bb"), s("ccc")]),
            &[pipe("uppercase"), pipe("length")],
            &mut ctx,
        );
        assert_eq!(out, s("3"));
    }
}
