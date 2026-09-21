/*
 * split.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Splitting a coarse math run (`i=1`, `4ac`, `x>0`) into the tokens a
//! target with per-token semantics needs.
//!
//! mitex lexes any run of non-delimiter characters as one word. OMML is
//! happy with that (Word classifies characters when it renders). Typst is
//! not: `4ac` in Typst math is one identifier, so letters must be emitted
//! one at a time; MathML (bd-9z83tcv0) needs `mi`/`mn`/`mo` classification.
//! This pass is a pure function of the run's text: each letter is its own
//! identifier (TeX semantics, where `ab` is two variables), a digit run
//! with optional decimal point is one number, everything else is a
//! one-character operator or punctuation token. The pieces' spans
//! partition the run's span.

use std::ops::Range;

/// Token class of one piece of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceKind {
    /// A single letter (any Unicode alphabetic character).
    Identifier,
    /// Digits with at most one interior `.`: `3`, `3.14`, `10`.
    Number,
    /// Anything else, one character at a time: `+`, `=`, `(`, `,`.
    Operator,
}

/// One piece of a split run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub kind: PieceKind,
    pub text: String,
    /// Byte range in the same coordinate space as the run's span; `None`
    /// when the run had no span.
    pub span: Option<Range<usize>>,
}

/// Split `text` (a `Run`'s text) whose bytes start at `span.start`.
pub fn split_run(text: &str, span: Option<&Range<usize>>) -> Vec<Piece> {
    let base = span.map(|s| s.start);
    let mut pieces = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap();
        let start = i;
        let kind;
        if c.is_ascii_digit() {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            // One interior decimal point followed by a digit.
            if j + 1 < bytes.len() && bytes[j] == b'.' && bytes[j + 1].is_ascii_digit() {
                j += 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
            }
            i = j;
            kind = PieceKind::Number;
        } else if c.is_alphabetic() {
            i += c.len_utf8();
            kind = PieceKind::Identifier;
        } else {
            i += c.len_utf8();
            kind = PieceKind::Operator;
        }
        pieces.push(Piece {
            kind,
            text: text[start..i].to_string(),
            span: base.map(|b| b + start..b + i),
        });
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(PieceKind, &str)> {
        let pieces = split_run(text, None);
        // Leak-free view for assertions.
        pieces
            .iter()
            .map(|p| (p.kind, &text[text.find(&p.text).unwrap()..][..p.text.len()]))
            .collect()
    }

    #[test]
    fn letters_split_one_by_one_and_numbers_stay_together() {
        use PieceKind::*;
        assert_eq!(
            kinds("4ac"),
            vec![(Number, "4"), (Identifier, "a"), (Identifier, "c")]
        );
        assert_eq!(kinds("3.14"), vec![(Number, "3.14")]);
        assert_eq!(
            kinds("i=1"),
            vec![(Identifier, "i"), (Operator, "="), (Number, "1")]
        );
        assert_eq!(kinds("10y"), vec![(Number, "10"), (Identifier, "y")]);
    }

    #[test]
    fn a_trailing_or_doubled_dot_is_an_operator() {
        use PieceKind::*;
        assert_eq!(kinds("3."), vec![(Number, "3"), (Operator, ".")]);
        let p = split_run("1.2.3", None);
        assert_eq!(
            p.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
            vec!["1.2", ".", "3"]
        );
    }

    #[test]
    fn unicode_letters_are_identifiers_with_byte_spans() {
        let p = split_run("αβ≠∅", Some(&(10..20)));
        assert_eq!(p.len(), 4);
        assert_eq!(p[0].kind, PieceKind::Identifier);
        assert_eq!(p[0].span, Some(10..12));
        assert_eq!(p[2].kind, PieceKind::Operator);
        assert_eq!(p[2].span, Some(14..17));
        assert_eq!(p[3].span, Some(17..20));
    }

    #[test]
    fn spans_partition_the_run() {
        for text in ["i=1", "4ac", "x>0", "3.14,2.71", "f(x)", "a+b=c"] {
            let start = 7;
            let p = split_run(text, Some(&(start..start + text.len())));
            let mut cursor = start;
            for piece in &p {
                let s = piece.span.clone().unwrap();
                assert_eq!(s.start, cursor, "{text}: gap before {:?}", piece.text);
                assert_eq!(&text[s.start - start..s.end - start], piece.text);
                cursor = s.end;
            }
            assert_eq!(cursor, start + text.len(), "{text}: pieces stop short");
        }
    }
}
