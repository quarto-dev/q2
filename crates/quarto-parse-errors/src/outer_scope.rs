/*
 * outer_scope.rs
 * Copyright (c) 2025 Posit, PBC
 */

//! Outer-inline-scope detection for tree-sitter parse errors.
//!
//! Several `(LR state, lookahead symbol)` pairs in the QMD error table are
//! reached by inputs that differ only in their enclosing inline scope. For
//! example `The '_blank' word.` and `_a' b._` both reach `(704, _whitespace)`
//! at error time, but they want different diagnostics (Q-2-5 vs Q-2-10). The
//! tree-sitter LR generator has minimised these distinctions away, so the
//! corpus key cannot disambiguate them on its own.
//!
//! This module computes a third lookup-key component — `outer_scope` — by
//! walking the log of shifted terminals (`all_tokens` plus `consumed_tokens`)
//! sorted by source position. It applies the same push/pop and block-boundary
//! clearing rules the external scanner uses, and returns the outermost open
//! inline scope at the error position.
//!
//! `outer_scope` values:
//! - `none`: error at block level, no inline scope active
//! - `single_quote`, `double_quote`: inside `'..'` / `"..."`
//! - `emph_star`, `emph_underscore`: inside `*..*` / `_.._`
//! - `strong_star`, `strong_underscore`: inside `**..**` / `__..__`

use crate::tree_sitter_log::ConsumedToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OuterScope {
    None,
    SingleQuote,
    DoubleQuote,
    EmphStar,
    EmphUnderscore,
    StrongStar,
    StrongUnderscore,
}

impl OuterScope {
    pub fn as_str(self) -> &'static str {
        match self {
            OuterScope::None => "none",
            OuterScope::SingleQuote => "single_quote",
            OuterScope::DoubleQuote => "double_quote",
            OuterScope::EmphStar => "emph_star",
            OuterScope::EmphUnderscore => "emph_underscore",
            OuterScope::StrongStar => "strong_star",
            OuterScope::StrongUnderscore => "strong_underscore",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "none" => Some(OuterScope::None),
            "single_quote" => Some(OuterScope::SingleQuote),
            "double_quote" => Some(OuterScope::DoubleQuote),
            "emph_star" => Some(OuterScope::EmphStar),
            "emph_underscore" => Some(OuterScope::EmphUnderscore),
            "strong_star" => Some(OuterScope::StrongStar),
            "strong_underscore" => Some(OuterScope::StrongUnderscore),
            _ => None,
        }
    }
}

/// Compute the outermost open inline scope at the given error position by
/// walking the combined `all_tokens` + `consumed_tokens` log in source-position
/// order. Both lists must be from the same parse.
pub fn compute_outer_scope(
    all_tokens: &[ConsumedToken],
    consumed_tokens: &[ConsumedToken],
    error_row: usize,
    error_column: usize,
    input: &[u8],
) -> OuterScope {
    let mut tokens: Vec<&ConsumedToken> = all_tokens.iter().chain(consumed_tokens.iter()).collect();
    tokens.sort_by_key(|t| (t.row, t.column));

    let mut stack: Vec<OuterScope> = Vec::new();
    for tok in tokens {
        if !appears_before(tok, error_row, error_column) {
            break;
        }
        if is_block_boundary(&tok.sym) {
            stack.clear();
            continue;
        }
        if let Some(scope) = scope_for_token(tok, input) {
            if stack.contains(&scope) {
                // Already open: this token is a close attempt. Pop only if the
                // matching scope is on top (mirroring the scanner's
                // pop_if_top semantics).
                if stack.last() == Some(&scope) {
                    stack.pop();
                }
                // else: orphaning close; leave the stack alone.
            } else {
                stack.push(scope);
            }
        }
    }

    // The INNERMOST open scope is at the top of the stack. This is the scope
    // that immediately contains the failing token, which is what the
    // diagnostic should base its message on. For example, in
    // `The ' *__blank*' word.` the stack at error time is
    // [single_quote, emph_star, strong_underscore]; the relevant scope is
    // `strong_underscore` (the `__` that never closes), not the outer
    // `single_quote`.
    stack.last().copied().unwrap_or(OuterScope::None)
}

fn appears_before(tok: &ConsumedToken, error_row: usize, error_column: usize) -> bool {
    tok.row < error_row || (tok.row == error_row && tok.column < error_column)
}

fn appears_strictly_after_position(
    tok: &ConsumedToken,
    other_row: usize,
    other_column: usize,
) -> bool {
    tok.row > other_row || (tok.row == other_row && tok.column > other_column)
}

/// Position of a token in the source, returned by [`find_outermost_close`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSpan {
    pub row: usize,
    pub column: usize,
    pub size: usize,
}

/// Find the would-be closing delimiter of the OUTERMOST open inline scope at
/// the given error position. For inputs like `*a _b c* trailing\n` the parser
/// opened `emph_star` at col 2 and `emph_underscore` at col 5; the `*` at
/// col 7 is the would-be closer of the outermost (`emph_star`) scope and is
/// the natural anchor for the "I reached the end of the block..." indicator.
///
/// Returns `None` if no candidate closer exists (e.g. truly unclosed at end of
/// input with no later same-kind token), in which case the caller should fall
/// back to the parser-failure position.
pub fn find_outermost_close(
    all_tokens: &[ConsumedToken],
    consumed_tokens: &[ConsumedToken],
    error_row: usize,
    error_column: usize,
    input: &[u8],
) -> Option<TokenSpan> {
    let mut tokens: Vec<&ConsumedToken> = all_tokens.iter().chain(consumed_tokens.iter()).collect();
    tokens.sort_by_key(|t| (t.row, t.column));

    let mut stack: Vec<(OuterScope, usize, usize)> = Vec::new();
    for tok in &tokens {
        if !appears_before(tok, error_row, error_column) {
            break;
        }
        if is_block_boundary(&tok.sym) {
            stack.clear();
            continue;
        }
        if let Some(scope) = scope_for_token(tok, input) {
            if stack.iter().any(|(s, _, _)| *s == scope) {
                if stack.last().map(|(s, _, _)| *s) == Some(scope) {
                    stack.pop();
                }
            } else {
                stack.push((scope, tok.row, tok.column));
            }
        }
    }

    let (outermost_scope, outermost_row, outermost_col) = *stack.first()?;

    let mut last_match: Option<TokenSpan> = None;
    for tok in &tokens {
        if !appears_before(tok, error_row, error_column) {
            break;
        }
        if !appears_strictly_after_position(tok, outermost_row, outermost_col) {
            continue;
        }
        if scope_for_token(tok, input) == Some(outermost_scope) {
            last_match = Some(TokenSpan {
                row: tok.row,
                column: tok.column,
                size: tok.size,
            });
        }
    }

    last_match.map(|span| trim_to_delimiter_run(span, outermost_scope, input))
}

/// Trim a token span to the run of consecutive delimiter bytes inside it,
/// discarding any leading or trailing whitespace the parser included in the
/// token. For example a closing `*` token at (col=7, size=2) covering `*` + ` `
/// becomes (col=7, size=1) covering just the `*`.
fn trim_to_delimiter_run(span: TokenSpan, scope: OuterScope, input: &[u8]) -> TokenSpan {
    let delimiter_byte = match scope {
        OuterScope::EmphStar | OuterScope::StrongStar => b'*',
        OuterScope::EmphUnderscore | OuterScope::StrongUnderscore => b'_',
        OuterScope::SingleQuote => b'\'',
        OuterScope::DoubleQuote => b'"',
        OuterScope::None => return span,
    };
    let Some(line_start) = line_start_offset(input, span.row) else {
        return span;
    };
    let span_start = line_start + span.column;
    let span_end = (span_start + span.size).min(input.len());
    let bytes = &input[span_start..span_end];

    let Some(first) = bytes.iter().position(|&b| b == delimiter_byte) else {
        return span;
    };
    let mut last = first;
    while last + 1 < bytes.len() && bytes[last + 1] == delimiter_byte {
        last += 1;
    }

    TokenSpan {
        row: span.row,
        column: span.column + first,
        size: last - first + 1,
    }
}

fn is_block_boundary(sym: &str) -> bool {
    matches!(
        sym,
        "_line_ending" | "_blank_line_start" | "_close_block" | "_block_close" | "_token_eof"
    )
}

fn scope_for_token(tok: &ConsumedToken, input: &[u8]) -> Option<OuterScope> {
    match tok.sym.as_str() {
        "single_quote" => Some(OuterScope::SingleQuote),
        "double_quote" => Some(OuterScope::DoubleQuote),
        "emphasis_delimiter" => {
            match delimiter_char_in_span(input, tok.row, tok.column, tok.size) {
                Some(b'*') => Some(OuterScope::EmphStar),
                Some(b'_') => Some(OuterScope::EmphUnderscore),
                _ => None,
            }
        }
        "strong_emphasis_delimiter" => {
            match delimiter_char_in_span(input, tok.row, tok.column, tok.size) {
                Some(b'*') => Some(OuterScope::StrongStar),
                Some(b'_') => Some(OuterScope::StrongUnderscore),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Find the first `*` or `_` byte inside the token's byte span. Emphasis
/// delimiter tokens may include leading whitespace in their span (size > 1),
/// so we cannot read the byte at `column` directly.
fn delimiter_char_in_span(input: &[u8], row: usize, column: usize, size: usize) -> Option<u8> {
    let line_start = line_start_offset(input, row)?;
    let start = line_start + column;
    let end = (start + size).min(input.len());
    input[start..end]
        .iter()
        .copied()
        .find(|b| matches!(*b, b'*' | b'_'))
}

fn line_start_offset(input: &[u8], row: usize) -> Option<usize> {
    if row == 0 {
        return Some(0);
    }
    let mut current_row = 0usize;
    for (i, &b) in input.iter().enumerate() {
        if b == b'\n' {
            current_row += 1;
            if current_row == row {
                return Some(i + 1);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(sym: &str, row: usize, column: usize, size: usize) -> ConsumedToken {
        ConsumedToken {
            row,
            column,
            size,
            lr_state: 0,
            sym: sym.to_string(),
        }
    }

    // === Single-quote with inner emphasis — innermost = emphasis ===

    #[test]
    fn the_underscore_blank_word() {
        let input = b"The '_blank' word.\n";
        let tokens = vec![
            tok("single_quote", 0, 4, 1),
            tok("emphasis_delimiter", 0, 5, 1),
            tok("single_quote", 0, 11, 1),
        ];
        // Stack at error: [SingleQuote, EmphUnderscore]. Innermost = EmphUnderscore.
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 12, input),
            OuterScope::EmphUnderscore
        );
    }

    #[test]
    fn the_double_underscore_blank_word() {
        let input = b"The '__blank' word.\n";
        let tokens = vec![
            tok("single_quote", 0, 4, 1),
            tok("strong_emphasis_delimiter", 0, 5, 2),
            tok("single_quote", 0, 12, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 13, input),
            OuterScope::StrongUnderscore
        );
    }

    #[test]
    fn the_star_blank_word() {
        let input = b"The '*blank' word.\n";
        let tokens = vec![
            tok("single_quote", 0, 4, 1),
            tok("emphasis_delimiter", 0, 5, 1),
            tok("single_quote", 0, 11, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 12, input),
            OuterScope::EmphStar
        );
    }

    #[test]
    fn the_double_star_blank_word() {
        let input = b"The '**blank' word.\n";
        let tokens = vec![
            tok("single_quote", 0, 4, 1),
            tok("strong_emphasis_delimiter", 0, 5, 2),
            tok("single_quote", 0, 12, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 13, input),
            OuterScope::StrongStar
        );
    }

    // === Emphasis with inner apostrophe — innermost = single_quote ===

    #[test]
    fn underscore_apostrophe() {
        let input = b"_a' b._\n";
        let tokens = vec![
            tok("emphasis_delimiter", 0, 0, 1),
            tok("single_quote", 0, 2, 1),
        ];
        // Stack at error: [EmphUnderscore, SingleQuote]. Innermost = SingleQuote.
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 3, input),
            OuterScope::SingleQuote
        );
    }

    #[test]
    fn star_apostrophe() {
        let input = b"*a' b.*\n";
        let tokens = vec![
            tok("emphasis_delimiter", 0, 0, 1),
            tok("single_quote", 0, 2, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 3, input),
            OuterScope::SingleQuote
        );
    }

    #[test]
    fn double_star_apostrophe() {
        let input = b"**a' b.**\n";
        let tokens = vec![
            tok("strong_emphasis_delimiter", 0, 0, 2),
            tok("single_quote", 0, 3, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 4, input),
            OuterScope::SingleQuote
        );
    }

    #[test]
    fn double_underscore_apostrophe() {
        let input = b"__a' b.__\n";
        let tokens = vec![
            tok("strong_emphasis_delimiter", 0, 0, 2),
            tok("single_quote", 0, 3, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 4, input),
            OuterScope::SingleQuote
        );
    }

    // === Emphasis with inner double-quote (Q-2-11 cases) — innermost = double_quote ===

    #[test]
    fn star_inch_mark() {
        let input = b"*a\" b.*\n";
        let tokens = vec![
            tok("emphasis_delimiter", 0, 0, 1),
            tok("double_quote", 0, 2, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 3, input),
            OuterScope::DoubleQuote
        );
    }

    #[test]
    fn underscore_inch_mark() {
        let input = b"_a\" b._\n";
        let tokens = vec![
            tok("emphasis_delimiter", 0, 0, 1),
            tok("double_quote", 0, 2, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 3, input),
            OuterScope::DoubleQuote
        );
    }

    // === Double-quoted with inner emphasis — innermost = emphasis ===

    #[test]
    fn the_double_quoted_underscore() {
        let input = b"The \"_blank\" word.\n";
        let tokens = vec![
            tok("double_quote", 0, 4, 1),
            tok("emphasis_delimiter", 0, 5, 1),
            tok("double_quote", 0, 11, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 12, input),
            OuterScope::EmphUnderscore
        );
    }

    // === Three-level nesting ===

    #[test]
    fn single_quote_with_star_with_strong_underscore() {
        // `The ' *__blank*' word.` — single quote contains emphasis-star
        // contains unclosed strong-underscore. Innermost = strong_underscore.
        let input = b"The ' *__blank*' word.\n";
        let tokens = vec![
            tok("single_quote", 0, 3, 2),
            tok("emphasis_delimiter", 0, 5, 2),
            tok("strong_emphasis_delimiter", 0, 7, 2),
            tok("emphasis_delimiter", 0, 14, 1),
            tok("single_quote", 0, 15, 1),
        ];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 16, input),
            OuterScope::StrongUnderscore
        );
    }

    // === Block-boundary clearing ===

    #[test]
    fn line_ending_clears_stack() {
        // Multi-paragraph: row 0 has unmatched ', row 2 has **strong** with inner '.
        // The walker must clear on the row-0 _line_ending so the row-2 error
        // sees only its own block's scopes.
        let input = b"First apostrophe: a' b.\n\nSecond in bold: **c' d.**\n";
        let tokens = vec![
            tok("single_quote", 0, 19, 1),
            tok("_line_ending", 0, 23, 1),
            tok("_blank_line_start", 1, 0, 0),
            tok("strong_emphasis_delimiter", 2, 15, 3),
            tok("single_quote", 2, 19, 1),
        ];
        // Row-2 stack: [StrongStar, SingleQuote] (' inside **). Innermost = SingleQuote.
        assert_eq!(
            compute_outer_scope(&[], &tokens, 2, 20, input),
            OuterScope::SingleQuote
        );
    }

    #[test]
    fn paired_quote_pops() {
        // 'foo' should leave stack empty after the close.
        let input = b"'foo'\n";
        let tokens = vec![tok("single_quote", 0, 0, 1), tok("single_quote", 0, 4, 1)];
        // Error past col 4 (e.g., col 5): the second ' closes the first.
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 5, input),
            OuterScope::None
        );
    }

    #[test]
    fn empty_stack_returns_none() {
        let input = b"hello\n";
        let tokens: Vec<ConsumedToken> = vec![];
        assert_eq!(
            compute_outer_scope(&[], &tokens, 0, 5, input),
            OuterScope::None
        );
    }

    #[test]
    fn outer_scope_str_roundtrip() {
        for s in [
            OuterScope::None,
            OuterScope::SingleQuote,
            OuterScope::DoubleQuote,
            OuterScope::EmphStar,
            OuterScope::EmphUnderscore,
            OuterScope::StrongStar,
            OuterScope::StrongUnderscore,
        ] {
            assert_eq!(OuterScope::from_str(s.as_str()), Some(s));
        }
        assert_eq!(OuterScope::from_str("unknown"), None);
    }

    // === find_outermost_close ===

    #[test]
    fn outermost_close_emph_with_unclosed_underscore() {
        // *a _b c* — outer * pairs at col 0/7, inner _ at col 2-3 (with leading
        // whitespace) is unclosed. Outermost is emph_star at col 0; its
        // would-be closer is the * at col 7.
        let input = b"*a _b c*\n";
        let tokens = vec![
            tok("emphasis_delimiter", 0, 0, 1),
            tok("emphasis_delimiter", 0, 2, 2),
            tok("emphasis_delimiter", 0, 7, 1),
        ];
        assert_eq!(
            find_outermost_close(&[], &tokens, 0, 8, input),
            Some(TokenSpan {
                row: 0,
                column: 7,
                size: 1
            })
        );
    }

    #[test]
    fn outermost_close_with_trailing_text() {
        // *a _b c* jeloasd — closer at col 7 even with trailing text in the
        // block.
        let input = b"*a _b c* jeloasd\n";
        let tokens = vec![
            tok("emphasis_delimiter", 0, 0, 1),
            tok("emphasis_delimiter", 0, 2, 2),
            tok("emphasis_delimiter", 0, 7, 1),
        ];
        assert_eq!(
            find_outermost_close(&[], &tokens, 0, 16, input),
            Some(TokenSpan {
                row: 0,
                column: 7,
                size: 1
            })
        );
    }

    #[test]
    fn outermost_close_strong_with_unclosed_strong() {
        // **a __b c** — outer **, inner __ unclosed. Closer at col 9 size 2.
        let input = b"**a __b c**\n";
        let tokens = vec![
            tok("strong_emphasis_delimiter", 0, 0, 2),
            tok("strong_emphasis_delimiter", 0, 3, 3),
            tok("strong_emphasis_delimiter", 0, 9, 2),
        ];
        assert_eq!(
            find_outermost_close(&[], &tokens, 0, 11, input),
            Some(TokenSpan {
                row: 0,
                column: 9,
                size: 2
            })
        );
    }

    #[test]
    fn outermost_close_mixed_emph_strong() {
        // **a *b c** — outer ** at col 0/8, inner * at col 3 unclosed.
        let input = b"**a *b c**\n";
        let tokens = vec![
            tok("strong_emphasis_delimiter", 0, 0, 2),
            tok("emphasis_delimiter", 0, 3, 2),
            tok("strong_emphasis_delimiter", 0, 8, 2),
        ];
        assert_eq!(
            find_outermost_close(&[], &tokens, 0, 10, input),
            Some(TokenSpan {
                row: 0,
                column: 8,
                size: 2
            })
        );
    }

    #[test]
    fn outermost_close_truly_unclosed_returns_none() {
        // *hello — no closing * exists; should return None so caller falls
        // back to the parser-failure position.
        let input = b"*hello\n";
        let tokens = vec![tok("emphasis_delimiter", 0, 0, 1)];
        assert_eq!(find_outermost_close(&[], &tokens, 0, 6, input), None);
    }

    #[test]
    fn outermost_close_same_marker_returns_none() {
        // *a *b c* — middle * pops the first scope, third * opens a new one
        // with no closer. Outermost open is emph_star at col 7, no later
        // matching token exists.
        let input = b"*a *b c*\n";
        let tokens = vec![
            tok("emphasis_delimiter", 0, 0, 1),
            tok("emphasis_delimiter", 0, 2, 2),
            tok("emphasis_delimiter", 0, 7, 1),
        ];
        assert_eq!(find_outermost_close(&[], &tokens, 0, 8, input), None);
    }

    #[test]
    fn outermost_close_no_open_scope_returns_none() {
        // Plain text — nothing on the scope stack.
        let input = b"plain text\n";
        let tokens: Vec<ConsumedToken> = vec![];
        assert_eq!(find_outermost_close(&[], &tokens, 0, 10, input), None);
    }

    #[test]
    fn outermost_close_quote_with_unclosed_inner_quote() {
        // 'a "b c' — outer ' pairs, inner " unclosed. The closing ' token is
        // emitted at (col=6, size=2) including the leading whitespace; the
        // returned span is trimmed to the delimiter alone (col=7, size=1).
        let input = b"'a \"b c'\n";
        let tokens = vec![
            tok("single_quote", 0, 0, 1),
            tok("double_quote", 0, 2, 2),
            tok("single_quote", 0, 6, 2),
        ];
        assert_eq!(
            find_outermost_close(&[], &tokens, 0, 8, input),
            Some(TokenSpan {
                row: 0,
                column: 7,
                size: 1
            })
        );
    }

    #[test]
    fn outermost_close_trims_trailing_whitespace() {
        // The parser emits the closing `*` at (col=7, size=2) when followed by
        // a space (so the token covers `* `). The returned span must trim to
        // just the `*` (col=7, size=1).
        let input = b"a *b _c* trailing\n";
        let tokens = vec![
            tok("emphasis_delimiter", 0, 1, 2),
            tok("emphasis_delimiter", 0, 4, 2),
            tok("emphasis_delimiter", 0, 7, 2),
        ];
        assert_eq!(
            find_outermost_close(&[], &tokens, 0, 17, input),
            Some(TokenSpan {
                row: 0,
                column: 7,
                size: 1
            })
        );
    }
}
