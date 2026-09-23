//! The reader: mitex's parser plus the leaf → original-span side table.
//!
//! For macro-free input every leaf's span selects its own bytes, spans are
//! monotone, and the only bytes not covered are whitespace mitex drops.
//! With `\newcommand`, mitex expands macros in the lexer and the tree's
//! leaves are slices of the *definition* (macro body) or the *use* (macro
//! arguments); the side table records where each leaf's bytes really sit.

use std::ops::Range;

use mitex_parser::syntax::SyntaxKind;
use quarto_math::reader::{Leaf, Parsed, parse};
use quarto_math::spec::Spec;

use crate::fixture_corpus::load_all;

fn parsed(text: &str) -> Parsed {
    parse(text, Spec::builtin())
}

fn leaf_with_text<'a>(leaves: &'a [Leaf], text: &str) -> &'a Leaf {
    leaves
        .iter()
        .find(|l| l.text == text)
        .unwrap_or_else(|| panic!("no leaf with text {text:?} in {leaves:?}"))
}

fn offset_of(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not in {haystack:?}"))
}

/// The invariant every leaf must satisfy, macro or not: a recorded span
/// selects exactly the leaf's own bytes. The only leaves allowed to have no
/// span are error tokens, whose text the macro engine synthesizes (e.g.
/// "invalid number of arguments").
fn check_leaf_spans(input: &str, leaves: &[Leaf]) {
    for leaf in leaves {
        let Some(span) = leaf.span.clone() else {
            assert_eq!(
                leaf.kind,
                SyntaxKind::TokenError,
                "only synthesized error tokens may lack a span; {:?} has none (input {input:?})",
                leaf.text
            );
            continue;
        };
        assert!(
            span.end <= input.len() && span.start <= span.end,
            "leaf {:?} span {span:?} out of bounds for {input:?}",
            leaf.text
        );
        assert_eq!(
            &input[span.clone()],
            leaf.text,
            "leaf span must select the leaf's own bytes (input {input:?})"
        );
    }
}

/// Bytes mitex drops from its tree, besides whitespace: an environment is
/// represented by a name leaf alone (`begin(sym'("aligned"))`), so the
/// `\begin{` / `\end{` before the name and the `}` after it never reach a
/// leaf.
fn is_dropped_syntax(gap: &str) -> bool {
    let trimmed = gap.trim();
    trimmed.is_empty() || matches!(trimmed, r"\begin{" | r"\end{" | "}")
}

/// Macro-free input: the spans, in leaf order, are monotone and cover the
/// input up to the bytes mitex drops. mitex is not byte-lossless: it drops
/// the whitespace between a command and a bare argument (`\hat x`) and the
/// `\begin{…}` / `\end{…}` syntax around an environment name, so a gap is
/// legal only when it consists of exactly those (`is_dropped_syntax`).
fn check_tiling(input: &str, leaves: &[Leaf]) {
    let mut cursor = 0;
    for leaf in leaves {
        let span: Range<usize> = leaf.span.clone().expect("span");
        assert!(
            span.start >= cursor,
            "overlap before leaf {:?} in {input:?}: span {span:?}, cursor {cursor}",
            leaf.text
        );
        assert!(
            is_dropped_syntax(&input[cursor..span.start]),
            "unexpected bytes {:?} dropped before leaf {:?} in {input:?}",
            &input[cursor..span.start],
            leaf.text
        );
        cursor = span.end;
    }
    assert!(
        is_dropped_syntax(&input[cursor..]),
        "unexpected tail {:?} after the last leaf of {input:?}",
        &input[cursor..]
    );
}

#[test]
fn simple_expression_spans_tile_the_input() {
    let input = r"\frac{a}{b} + x^2";
    let p = parsed(input);
    assert_eq!(p.leaves.len(), p.token_count());
    check_leaf_spans(input, &p.leaves);
    check_tiling(input, &p.leaves);
    assert_eq!(leaf_with_text(&p.leaves, r"\frac").span, Some(0..5));
    assert_eq!(leaf_with_text(&p.leaves, "a").span, Some(6..7));
    assert_eq!(leaf_with_text(&p.leaves, "2").span, Some(16..17));
}

#[test]
fn every_macro_free_fixture_tiles() {
    let mut checked = 0;
    for fx in load_all() {
        if fx.group == "macros" || fx.text.contains(r"\newcommand") || fx.text.contains(r"\def") {
            continue;
        }
        let p = parsed(&fx.text);
        assert_eq!(p.leaves.len(), p.token_count(), "{}", fx.id());
        check_leaf_spans(&fx.text, &p.leaves);
        check_tiling(&fx.text, &p.leaves);
        checked += 1;
    }
    assert!(checked > 200, "only {checked} fixtures checked");
}

#[test]
fn every_fixture_leaf_selects_its_own_bytes() {
    // Including macros: expanded leaves are still slices of the input.
    for fx in load_all() {
        let p = parsed(&fx.text);
        check_leaf_spans(&fx.text, &p.leaves);
    }
}

#[test]
fn macro_body_maps_to_the_definition_site_and_arguments_to_the_use_site() {
    let input = r"\newcommand{\pr}[1]{P(#1)} \pr{x>0}";
    let p = parsed(input);
    check_leaf_spans(input, &p.leaves);
    let def = offset_of(input, "P(#1)");
    assert_eq!(leaf_with_text(&p.leaves, "P").span, Some(def..def + 1));
    assert_eq!(leaf_with_text(&p.leaves, "(").span, Some(def + 1..def + 2));
    let use_site = offset_of(input, "x>0");
    assert_eq!(
        leaf_with_text(&p.leaves, "x>0").span,
        Some(use_site..use_site + 3)
    );
    // The definition itself vanishes from the tree: no leaf spells `\pr`
    // followed by the body braces as text of the expansion.
    assert!(
        p.leaves.iter().all(|l| l.text != r"\newcommand"),
        "\\newcommand is consumed by the lexer: {:?}",
        p.leaves
    );
}

#[test]
fn zero_arity_macro_maps_to_its_definition() {
    let input = r"\newcommand{\R}{\mathbb{R}} x \in \R";
    let p = parsed(input);
    check_leaf_spans(input, &p.leaves);
    let def = offset_of(input, r"\mathbb{R}");
    assert_eq!(
        leaf_with_text(&p.leaves, r"\mathbb").span,
        Some(def..def + 7)
    );
    assert_eq!(leaf_with_text(&p.leaves, "R").span, Some(def + 8..def + 9));
    // The use site `x \in` keeps its own positions.
    let x = offset_of(input, "x \\in");
    assert_eq!(leaf_with_text(&p.leaves, "x").span, Some(x..x + 1));
}

#[test]
fn macro_used_twice_maps_each_argument_to_its_own_use_site() {
    let input = r"\newcommand{\pr}[1]{P(#1)} \pr{A} + \pr{B}";
    let p = parsed(input);
    check_leaf_spans(input, &p.leaves);
    let a = offset_of(input, "{A}") + 1;
    let b = offset_of(input, "{B}") + 1;
    assert_eq!(leaf_with_text(&p.leaves, "A").span, Some(a..a + 1));
    assert_eq!(leaf_with_text(&p.leaves, "B").span, Some(b..b + 1));
    // Both expansions of the body point at the single definition.
    let def = offset_of(input, "P(#1)");
    let ps: Vec<_> = p
        .leaves
        .iter()
        .filter(|l| l.text == "P")
        .map(|l| l.span.clone())
        .collect();
    assert_eq!(ps, vec![Some(def..def + 1), Some(def..def + 1)]);
}

#[test]
fn single_character_argument_splits_keep_their_spans() {
    // `\frac12`: the parser splits the word `12` into two one-character
    // arguments; each must still map to its own byte.
    let input = r"\frac12";
    let p = parsed(input);
    check_leaf_spans(input, &p.leaves);
    check_tiling(input, &p.leaves);
    assert_eq!(leaf_with_text(&p.leaves, "1").span, Some(5..6));
    assert_eq!(leaf_with_text(&p.leaves, "2").span, Some(6..7));
}

#[test]
fn multibyte_input_keeps_byte_accurate_spans() {
    let input = "α ≠ ∅ → ∞";
    let p = parsed(input);
    check_leaf_spans(input, &p.leaves);
    check_tiling(input, &p.leaves);
    let arrow = offset_of(input, "→");
    assert_eq!(leaf_with_text(&p.leaves, "→").span, Some(arrow..arrow + 3));
}

#[test]
fn empty_input_has_no_leaves() {
    let p = parsed("");
    assert!(p.leaves.is_empty());
    assert_eq!(p.token_count(), 0);
}

#[test]
fn recursive_macro_terminates_with_an_error_token() {
    // `\loop` expands to itself. TeX would spin forever; the reader must
    // stop after a bounded number of expansions and report it, because this
    // is user input from a `.qmd`.
    let input = r"\newcommand{\loop}{\loop} \loop";
    let p = parsed(input);
    check_leaf_spans(input, &p.leaves);
    let errors: Vec<&Leaf> = p
        .leaves
        .iter()
        .filter(|l| l.kind == SyntaxKind::TokenError)
        .collect();
    assert!(
        errors.iter().any(|l| l.text.contains("expansion")),
        "expected a macro-expansion-limit error token, got {errors:?}"
    );
}
