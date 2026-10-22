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
    /// Add this text as a hint, and report the error over the source bytes
    /// `start..end` instead of the single token the parser failed at.
    HintAt {
        hint: String,
        start: usize,
        end: usize,
    },
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
        "language-with-attributes" => language_with_attributes(input, error_offset),
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

    let Some(tokens) = classify(body) else {
        return Suggestion::Nothing;
    };

    if has_two_identifiers(&tokens) {
        return Suggestion::Reject;
    }

    let ordered = in_accepted_order(&tokens, None);
    if ordered.iter().eq(tokens.iter().map(|(_, t)| t)) {
        return Suggestion::Nothing;
    }

    Suggestion::Hint(format!(
        "Reorder the attributes as `{{{}}}`: the identifier comes first, then classes, then key-value pairs.",
        ordered.join(" ")
    ))
}

/// Split an attribute list body into identifiers, classes and key-value
/// pairs. `None` if it holds anything else (a raw-format `=html`, a bare
/// word).
fn classify(body: &str) -> Option<Vec<(Kind, &str)>> {
    split_unquoted_whitespace(body)
        .into_iter()
        .map(|token| {
            let kind = if token.len() > 1 && token.starts_with('#') {
                Kind::Id
            } else if token.len() > 1 && token.starts_with('.') {
                Kind::Class
            } else if token.find('=').is_some_and(|eq| eq > 0) {
                Kind::KeyValue
            } else {
                return None;
            };
            Some((kind, token))
        })
        .collect()
}

fn has_two_identifiers(tokens: &[(Kind, &str)]) -> bool {
    tokens.iter().filter(|(k, _)| *k == Kind::Id).count() > 1
}

/// The tokens in the accepted order: identifier, classes, key-value pairs,
/// each kind keeping the author's relative order. `language`, when given,
/// is placed first among the classes.
fn in_accepted_order<'a>(tokens: &[(Kind, &'a str)], language: Option<&'a str>) -> Vec<&'a str> {
    let in_order = |kind: Kind| {
        tokens
            .iter()
            .filter(move |(k, _)| *k == kind)
            .map(|(_, t)| *t)
    };
    in_order(Kind::Id)
        .chain(language)
        .chain(in_order(Kind::Class))
        .chain(in_order(Kind::KeyValue))
        .collect()
}

/// Suggest where the language word of a fenced code block goes when it is
/// followed by an attribute list, as in ```` ```python {.cls #id} ````.
///
/// The opening line takes one language word *or* one attribute list. The
/// hint carries the author's own list in the accepted order with the word
/// folded in: as the first class for a code block, or as the leading word of
/// an executable cell `{python ...}`.
///
/// The error is reported over the braced list, the text the parser did not
/// expect, rather than the one character where it gave up.
///
/// Only a braced list after the word gets a hint. Anything else after the
/// word (`python .cls`, `python extra`) is not this mistake and is
/// [`Suggestion::Reject`]ed, as is a list with two identifiers (which
/// reordering cannot repair) and a word that is not a plain language name.
fn language_with_attributes(input: &[u8], error_offset: usize) -> Suggestion {
    let error_offset = error_offset.min(input.len());
    let line_start = input[..error_offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let line_end = input[error_offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(input.len(), |i| error_offset + i);
    let (Ok(before), Ok(after)) = (
        std::str::from_utf8(&input[line_start..error_offset]),
        std::str::from_utf8(&input[error_offset..line_end]),
    ) else {
        return Suggestion::Reject;
    };

    // `before` is the fence, optional spaces, and the word.
    let fence_len = before
        .trim_start()
        .find(|c| c != '`' && c != '~')
        .unwrap_or(0);
    let Some(word) = before.trim_start()[fence_len..].split_whitespace().next() else {
        return Suggestion::Reject;
    };
    if fence_len < 3
        || !word
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
    {
        return Suggestion::Reject;
    }

    let trimmed = after.trim_start();
    let list_start = error_offset + (after.len() - trimmed.len());
    let after = trimmed;
    let Some(close) = closing_brace(after.as_bytes()).filter(|_| after.starts_with('{')) else {
        return Suggestion::Reject;
    };
    let Some(tokens) = classify(&after[1..close]) else {
        return Suggestion::Nothing;
    };
    if has_two_identifiers(&tokens) {
        return Suggestion::Reject;
    }

    let class_word = format!(".{word}");
    let code_block = in_accepted_order(&tokens, Some(&class_word)).join(" ");
    let cell = std::iter::once(word)
        .chain(in_accepted_order(&tokens, None))
        .collect::<Vec<_>>()
        .join(" ");
    Suggestion::HintAt {
        hint: format!(
            "Put the language inside the braces: `{{{code_block}}}` for a code block, or `{{{cell}}}` for an executable cell."
        ),
        start: list_start,
        end: list_start + close + 1,
    }
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

    fn language_hint(line: &str) -> Suggestion {
        // The parser fails at the first character after the word.
        let word_start = line.rfind(['`', '~']).expect("fence") + 1;
        let offset = word_start + line[word_start..].find([' ', '{']).expect("end of word");
        language_with_attributes(line.as_bytes(), offset)
    }

    fn hint_and_span(s: Suggestion) -> (String, (usize, usize)) {
        match s {
            Suggestion::HintAt { hint, start, end } => (hint, (start, end)),
            other => panic!("expected a ranged hint, got {other:?}"),
        }
    }

    #[test]
    fn language_hint_spans_the_braced_list_not_the_space() {
        let line = "```python {.marimo} trailing";
        let (_, (start, end)) = hint_and_span(language_hint(line));
        assert_eq!(&line[start..end], "{.marimo}");
        let line = "```python{.marimo}";
        let (_, (start, end)) = hint_and_span(language_hint(line));
        assert_eq!(&line[start..end], "{.marimo}");
    }

    #[test]
    fn language_goes_first_among_classes_for_a_code_block() {
        let (h, _) = hint_and_span(language_hint("```python {.cls #id key=val}"));
        assert!(
            h.contains("`{#id .python .cls key=val}` for a code block"),
            "{h}"
        );
        assert!(
            h.contains("`{python #id .cls key=val}` for an executable cell"),
            "{h}"
        );
    }

    #[test]
    fn language_hint_without_a_space_before_the_brace() {
        let (h, _) = hint_and_span(language_hint("```python{.marimo}"));
        assert!(
            h.contains("`{.python .marimo}`") && h.contains("`{python .marimo}`"),
            "{h}"
        );
    }

    #[test]
    fn language_hint_with_indented_fence_and_tildes() {
        assert!(matches!(
            language_hint("  ~~~r {.x}"),
            Suggestion::HintAt { .. }
        ));
    }

    #[test]
    fn language_hint_keeps_quoted_values() {
        let (h, _) = hint_and_span(language_hint(r#"```python {title="a b" .c}"#));
        assert!(h.contains(r#"`{.python .c title="a b"}`"#), "{h}");
    }

    #[test]
    fn language_hint_declines_without_braces() {
        assert_eq!(language_hint("```python .marimo"), Suggestion::Reject);
        assert_eq!(language_hint("```python extra"), Suggestion::Reject);
        assert_eq!(language_hint("```python {.unclosed"), Suggestion::Reject);
    }

    #[test]
    fn language_hint_declines_two_identifiers_and_odd_words() {
        assert_eq!(language_hint("```python {#a #b}"), Suggestion::Reject);
        assert_eq!(language_hint("```c++ {.x}"), Suggestion::Reject);
    }

    #[test]
    fn language_hint_for_uninterpretable_list_is_fixed_text_only() {
        assert_eq!(language_hint("```python {=html}"), Suggestion::Nothing);
    }

    #[test]
    fn unknown_suggester_adds_nothing() {
        assert_eq!(suggest("no-such", b"{.c #i}", 4), Suggestion::Nothing);
    }
}
