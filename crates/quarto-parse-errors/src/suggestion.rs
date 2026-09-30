/*
 * suggestion.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Hints built from the author's own source text.
//!
//! Corpus entries carry fixed message text, which cannot say "write
//! `{#tip-alignment .callout-tip}`" using the identifiers the author actually
//! typed. An entry may instead name a built-in *suggester* (the corpus
//! `suggestion` field); the suggester reads the source around the error
//! position and produces one extra hint.
//!
//! Suggesters work from the source bytes rather than from the tokens
//! tree-sitter consumed before failing. Those tokens are a best-effort
//! re-synchronisation and may be recorded more than once or out of order; the
//! source text is always exactly what the author wrote.

/// What a suggester made of the source at an error position.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Suggestion {
    /// Add this text as a hint.
    Hint(String),
    /// Nothing useful to add; report the entry's fixed text alone.
    Nothing,
    /// The source is not the mistake this entry describes, even though it
    /// failed in the same parser state. Drop the entry.
    Reject,
}

/// Run the suggester called `name`. Unknown names add nothing.
pub(crate) fn suggest(name: &str, input: &[u8], error_offset: usize) -> Suggestion {
    match name {
        "attribute-order" => attribute_order(input, error_offset),
        _ => Suggestion::Nothing,
    }
}

#[derive(PartialEq, Eq)]
enum Kind {
    Id,
    Class,
    KeyValue,
}

/// Suggest the canonical order — identifier, classes, key-value pairs — for
/// the `{...}` attribute list containing `error_offset`.
///
/// The list is read from the one line holding the error; attribute lists do
/// not span lines. Relative order within each kind is preserved, and quoted
/// values are carried through untouched.
///
/// A list with more than one identifier is [`Suggestion::Reject`]ed: `{#a #b}`
/// fails in the very parser state that `{k=v #i}` does, but reordering cannot
/// repair it. A list holding anything that is not an identifier, class or
/// key-value pair (a raw-format `=html`, a bare word) is left to the entry's
/// fixed text.
fn attribute_order(input: &[u8], error_offset: usize) -> Suggestion {
    let Some(list) = enclosing_braces(input, error_offset) else {
        return Suggestion::Nothing;
    };
    let Ok(body) = std::str::from_utf8(&input[list.0 + 1..list.1 - 1]) else {
        return Suggestion::Nothing;
    };

    let mut tokens: Vec<(Kind, &str)> = Vec::new();
    for token in split_unquoted_whitespace(body) {
        let kind = if token.len() > 1 && token.starts_with('#') {
            Kind::Id
        } else if token.len() > 1 && token.starts_with('.') {
            Kind::Class
        } else if token.find('=').is_some_and(|eq| eq > 0) {
            Kind::KeyValue
        } else {
            return Suggestion::Nothing;
        };
        tokens.push((kind, token));
    }

    if tokens.iter().filter(|(k, _)| *k == Kind::Id).count() > 1 {
        return Suggestion::Reject;
    }

    let in_order = |kind: Kind| tokens.iter().filter(move |(k, _)| *k == kind);
    let ordered: Vec<&str> = in_order(Kind::Id)
        .chain(in_order(Kind::Class))
        .chain(in_order(Kind::KeyValue))
        .map(|(_, t)| *t)
        .collect();
    if ordered.iter().eq(tokens.iter().map(|(_, t)| t)) {
        return Suggestion::Nothing;
    }

    Suggestion::Hint(format!(
        "Reorder the attributes as `{{{}}}`: the identifier comes first, then classes, then key-value pairs.",
        ordered.join(" ")
    ))
}

/// Byte range `[open, close)` of the innermost `{...}` on the error's line
/// that contains `error_offset`; `close` is one past the `}`.
fn enclosing_braces(input: &[u8], error_offset: usize) -> Option<(usize, usize)> {
    let error_offset = error_offset.min(input.len());
    let line_start = input[..error_offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let line_end = input[error_offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(input.len(), |i| error_offset + i);
    let line = &input[line_start..line_end];

    let mut best = None;
    for (i, &b) in line.iter().enumerate() {
        if b != b'{' || line_start + i >= error_offset {
            continue;
        }
        if let Some(close) = closing_brace(&line[i..]) {
            let (open, close) = (line_start + i, line_start + i + close + 1);
            if error_offset < close {
                best = Some((open, close));
            }
        }
    }
    best
}

/// Index of the `}` closing the `{` at the start of `s`, skipping quoted
/// text.
fn closing_brace(s: &[u8]) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut i = 1;
    while i < s.len() {
        match (quote, s[i]) {
            (_, b'\\') => i += 1,
            (Some(q), b) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(s[i]),
            (None, b'}') => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

fn split_unquoted_whitespace(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut quote: Option<u8> = None;
    let mut start: Option<usize> = None;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => {
                quote = Some(b);
                start.get_or_insert(i);
            }
            None if b.is_ascii_whitespace() => {
                if let Some(st) = start.take() {
                    out.push(&s[st..i]);
                }
            }
            None => {
                start.get_or_insert(i);
            }
        }
        if b == b'\\' {
            i += 1;
        }
        i += 1;
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(input: &str, needle: &str) -> Suggestion {
        attribute_order(input.as_bytes(), input.find(needle).expect("needle"))
    }

    fn hint(input: &str, needle: &str) -> String {
        match at(input, needle) {
            Suggestion::Hint(h) => h,
            other => panic!("expected a hint, got {other:?}"),
        }
    }

    #[test]
    fn moves_id_before_class() {
        assert!(hint("[x]{.c #i}", "#i").contains("`{#i .c}`"));
    }

    #[test]
    fn moves_id_before_key_value() {
        assert!(hint("[x]{k=v #i}", "#i").contains("`{#i k=v}`"));
    }

    #[test]
    fn moves_class_before_key_value() {
        assert!(hint("[x]{k=v .c}", ".c").contains("`{.c k=v}`"));
    }

    #[test]
    fn reorders_whole_list_and_keeps_relative_order() {
        let h = hint("[x]{k=v .a k2=w .b #i}", ".a");
        assert!(h.contains("`{#i .a .b k=v k2=w}`"), "{h}");
    }

    #[test]
    fn quoted_values_survive_intact() {
        let h = hint(r#"![a](b){alt="a } b {" .l #f}"#, ".l");
        assert!(h.contains(r#"`{#f .l alt="a } b {"}`"#), "{h}");
    }

    #[test]
    fn quoted_value_with_spaces_is_one_token() {
        let h = hint(r#"[x]{title="two words" #i}"#, "#i");
        assert!(h.contains(r#"`{#i title="two words"}`"#), "{h}");
    }

    #[test]
    fn a_second_identifier_is_rejected() {
        assert_eq!(at("[x]{#a #b}", "#b"), Suggestion::Reject);
        assert_eq!(at("[x]{#a .c #b}", "#b"), Suggestion::Reject);
    }

    #[test]
    fn list_already_in_order_suggests_nothing() {
        assert_eq!(at("[x]{#i .c k=v}", ".c"), Suggestion::Nothing);
    }

    #[test]
    fn uninterpretable_token_suggests_nothing() {
        assert_eq!(at("```{python k=v .c}", ".c"), Suggestion::Nothing);
        assert_eq!(at("[x]{=html .c #i}", "#i"), Suggestion::Nothing);
    }

    #[test]
    fn no_enclosing_braces_suggests_nothing() {
        assert_eq!(at("plain #i", "#i"), Suggestion::Nothing);
    }

    #[test]
    fn unterminated_list_suggests_nothing() {
        assert_eq!(at("[x]{.c #i", "#i"), Suggestion::Nothing);
    }

    #[test]
    fn picks_the_list_containing_the_error_not_an_earlier_one() {
        let h = hint("[a]{#ok} and [b]{.c #i}", "#i");
        assert!(h.contains("`{#i .c}`"), "{h}");
    }

    #[test]
    fn only_the_error_line_is_read() {
        let h = hint("{\n[x]{.c #i}\n}", "#i");
        assert!(h.contains("`{#i .c}`"), "{h}");
    }

    #[test]
    fn unknown_suggester_adds_nothing() {
        assert_eq!(suggest("no-such", b"{.c #i}", 4), Suggestion::Nothing);
    }
}
