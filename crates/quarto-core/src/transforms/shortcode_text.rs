/*
 * shortcode_text.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Text-level shortcode parsing for non-markdown contexts.
 */

//! Parse `{{< … >}}` occurrences out of arbitrary text.
//!
//! Include files (`include-in-header` / `include-before-body` /
//! `include-after-body`) are HTML, not qmd — Quarto substitutes
//! shortcodes in them *textually*, without markdown parsing (Q1:
//! `apply_code_shortcode`, an lpeg scanner applied to raw/code text;
//! verified against a live Q1 render — see
//! `claude-notes/plans/2026-08-10-shortcodes-website-config-includes.md`).
//! This module is the q2 analog: it splits a string into literal
//! segments and parsed [`Shortcode`] values, which
//! `ShortcodeResolveTransform` then dispatches through its ordinary
//! handler registry. It is also the natural building block for the
//! other Q1 text contexts — code blocks, attributes, image src, link
//! targets — tracked as bd-fz6gwfq0.
//!
//! Syntax mirrors the qmd grammar's shortcode rules: positional args
//! (naked tokens or quoted strings), `key=value` keyword args, nested
//! `{{< … >}}` in argument position. An escaped shortcode
//! (`{{{< … >}}}`) becomes a literal segment carrying the
//! *single-brace* form — the "render literally" semantics, applied
//! once. Malformed candidates (unterminated, empty name) are left as
//! literal text.

use hashlink::LinkedHashMap;
use quarto_pandoc_types::shortcode::{Shortcode, ShortcodeArg};
use quarto_source_map::SourceInfo;

/// One piece of a text-level parse.
#[derive(Debug, PartialEq)]
pub enum TextSegment {
    /// Literal text, emitted verbatim.
    Literal(String),
    /// A parsed, non-escaped shortcode to dispatch.
    Shortcode(Shortcode),
}

/// Split `text` into literal segments and shortcodes.
///
/// Every parsed [`Shortcode`] carries `source_info` (the enclosing
/// value's source — text-level parsing has no finer-grained spans).
/// Returns `None` when the text contains no shortcode candidates at
/// all, letting callers skip re-allocation on the common path.
pub fn parse_text_shortcodes(text: &str, source_info: &SourceInfo) -> Option<Vec<TextSegment>> {
    if !text.contains("{{<") {
        return None;
    }

    let bytes = text.as_bytes();
    let mut segments: Vec<TextSegment> = Vec::new();
    let mut literal_start = 0;
    let mut i = 0;
    let mut parsed_any = false;

    while i < bytes.len() {
        let rest = &text[i..];
        if rest.starts_with("{{{<") {
            if let Some((inner, end)) = scan_escaped(text, i) {
                push_literal(&mut segments, &text[literal_start..i]);
                // Escaped → emit the single-brace form literally.
                segments.push(TextSegment::Literal(format!("{{{{<{}>}}}}", inner)));
                parsed_any = true;
                i = end;
                literal_start = i;
                continue;
            }
        } else if rest.starts_with("{{<")
            && let Some((shortcode, end)) = parse_shortcode_at(text, i, source_info)
        {
            push_literal(&mut segments, &text[literal_start..i]);
            segments.push(TextSegment::Shortcode(shortcode));
            parsed_any = true;
            i = end;
            literal_start = i;
            continue;
        }
        // Advance one full character (not byte) to stay on a char boundary.
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    push_literal(&mut segments, &text[literal_start..]);

    if parsed_any {
        Some(segments)
    } else {
        // Nothing parsed (only "{{<" lookalikes) — treat as no-op.
        None
    }
}

fn push_literal(segments: &mut Vec<TextSegment>, text: &str) {
    if !text.is_empty() {
        segments.push(TextSegment::Literal(text.to_string()));
    }
}

/// Scan an escaped shortcode starting at `start` (which points at
/// `{{{<`). Returns the raw inner text (between `{{{<` and `>}}}`)
/// and the index one past the closing marker.
fn scan_escaped(text: &str, start: usize) -> Option<(String, usize)> {
    let inner_start = start + 4;
    let close = text[inner_start..].find(">}}}")?;
    let inner = &text[inner_start..inner_start + close];
    Some((inner.to_string(), inner_start + close + 4))
}

/// Parse a non-escaped shortcode starting at `start` (which points at
/// `{{<`). Returns the shortcode and the index one past `>}}`.
fn parse_shortcode_at(
    text: &str,
    start: usize,
    source_info: &SourceInfo,
) -> Option<(Shortcode, usize)> {
    let mut p = Parser {
        text,
        pos: start + 3,
        source_info,
    };
    p.skip_ws();
    let name = p.naked_token()?;
    if name.is_empty() {
        return None;
    }

    let mut positional_args: Vec<ShortcodeArg> = Vec::new();
    let mut keyword_args: LinkedHashMap<String, ShortcodeArg> = LinkedHashMap::new();

    loop {
        p.skip_ws();
        if p.rest().starts_with(">}}") {
            let end = p.pos + 3;
            let shortcode = Shortcode {
                is_escaped: false,
                name,
                positional_args,
                keyword_args,
                source_info: source_info.clone(),
            };
            return Some((shortcode, end));
        }
        if p.rest().is_empty() {
            return None; // unterminated
        }
        match p.arg()? {
            ParsedArg::Positional(arg) => positional_args.push(arg),
            ParsedArg::Keyword(key, arg) => {
                keyword_args.insert(key, arg);
            }
        }
    }
}

enum ParsedArg {
    Positional(ShortcodeArg),
    Keyword(String, ShortcodeArg),
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    source_info: &'a SourceInfo,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.rest().chars().next() {
            if c.is_whitespace() {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
    }

    /// Read a run of naked-token characters (no whitespace, no quote,
    /// no `=`, and not the start of `>}}`).
    fn naked_token(&mut self) -> Option<String> {
        let start = self.pos;
        while let Some(c) = self.rest().chars().next() {
            if c.is_whitespace() || c == '"' || c == '\'' || c == '=' {
                break;
            }
            if self.rest().starts_with(">}}") {
                break;
            }
            self.pos += c.len_utf8();
        }
        if self.pos == start {
            return None;
        }
        Some(self.text[start..self.pos].to_string())
    }

    /// Read a quoted string (single or double quotes; backslash
    /// escapes the quote character and backslash itself).
    fn quoted_string(&mut self) -> Option<String> {
        let quote = self.rest().chars().next()?;
        debug_assert!(quote == '"' || quote == '\'');
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self.rest().chars().next()?; // None → unterminated
            self.pos += c.len_utf8();
            if c == '\\' {
                let escaped = self.rest().chars().next()?;
                self.pos += escaped.len_utf8();
                if escaped == quote || escaped == '\\' {
                    out.push(escaped);
                } else {
                    out.push(c);
                    out.push(escaped);
                }
            } else if c == quote {
                return Some(out);
            } else {
                out.push(c);
            }
        }
    }

    /// Parse one argument value: nested shortcode, quoted string, or
    /// naked token.
    fn value(&mut self) -> Option<ShortcodeArg> {
        if self.rest().starts_with("{{<") {
            let (inner, end) = parse_shortcode_at(self.text, self.pos, self.source_info)?;
            self.pos = end;
            return Some(ShortcodeArg::Shortcode(inner));
        }
        let c = self.rest().chars().next()?;
        if c == '"' || c == '\'' {
            return self.quoted_string().map(ShortcodeArg::String);
        }
        self.naked_token().map(ShortcodeArg::String)
    }

    /// Parse one argument: `key=value` or a positional value.
    fn arg(&mut self) -> Option<ParsedArg> {
        // Nested shortcodes and quoted strings are always positional.
        if self.rest().starts_with("{{<") {
            return self.value().map(ParsedArg::Positional);
        }
        let c = self.rest().chars().next()?;
        if c == '"' || c == '\'' {
            return self.value().map(ParsedArg::Positional);
        }
        let token = self.naked_token()?;
        if self.rest().starts_with('=') {
            self.pos += 1;
            let value = self.value()?;
            return Some(ParsedArg::Keyword(token, value));
        }
        Some(ParsedArg::Positional(ShortcodeArg::String(token)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Option<Vec<TextSegment>> {
        parse_text_shortcodes(text, &SourceInfo::for_test())
    }

    fn sc(seg: &TextSegment) -> &Shortcode {
        match seg {
            TextSegment::Shortcode(sc) => sc,
            other => panic!("expected shortcode segment, got {:?}", other),
        }
    }

    fn lit(seg: &TextSegment) -> &str {
        match seg {
            TextSegment::Literal(s) => s,
            other => panic!("expected literal segment, got {:?}", other),
        }
    }

    fn arg_str(arg: &ShortcodeArg) -> &str {
        match arg {
            ShortcodeArg::String(s) => s,
            other => panic!("expected string arg, got {:?}", other),
        }
    }

    #[test]
    fn no_shortcode_returns_none() {
        assert!(parse("plain text").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn simple_shortcode_between_literals() {
        let segs = parse("A {{< env HOME >}} B").unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(lit(&segs[0]), "A ");
        let s = sc(&segs[1]);
        assert_eq!(s.name, "env");
        assert_eq!(arg_str(&s.positional_args[0]), "HOME");
        assert!(!s.is_escaped);
        assert_eq!(lit(&segs[2]), " B");
    }

    #[test]
    fn dotted_meta_key_parses_as_single_arg() {
        let segs = parse("{{< meta book.title >}}").unwrap();
        let s = sc(&segs[0]);
        assert_eq!(s.name, "meta");
        assert_eq!(arg_str(&s.positional_args[0]), "book.title");
    }

    #[test]
    fn quoted_argument_keeps_spaces() {
        let segs = parse("{{< env NAME \"fall back\" >}}").unwrap();
        let s = sc(&segs[0]);
        assert_eq!(arg_str(&s.positional_args[1]), "fall back");
    }

    #[test]
    fn keyword_arguments_parse_in_order() {
        let segs = parse("{{< video src=movie.mp4 title=\"My Movie\" >}}").unwrap();
        let s = sc(&segs[0]);
        assert!(s.positional_args.is_empty());
        let kv: Vec<(&str, &str)> = s
            .keyword_args
            .iter()
            .map(|(k, v)| (k.as_str(), arg_str(v)))
            .collect();
        assert_eq!(kv, vec![("src", "movie.mp4"), ("title", "My Movie")]);
    }

    #[test]
    fn nested_shortcode_in_arg_position() {
        let segs = parse("{{< env {{< meta varname >}} >}}").unwrap();
        let s = sc(&segs[0]);
        let ShortcodeArg::Shortcode(inner) = &s.positional_args[0] else {
            panic!("expected nested shortcode arg, got {:?}", s.positional_args);
        };
        assert_eq!(inner.name, "meta");
        assert_eq!(arg_str(&inner.positional_args[0]), "varname");
    }

    #[test]
    fn escaped_shortcode_becomes_single_brace_literal() {
        let segs = parse("X {{{< meta version >}}} Y").unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(lit(&segs[1]), "{{< meta version >}}");
    }

    #[test]
    fn bare_escaped_shortcode_still_unescapes() {
        // The whole text is one escaped shortcode — the parse must
        // still report it (the caller needs the rewritten literal),
        // even though the result is a single segment.
        let segs = parse("{{{< meta version >}}}").unwrap();
        assert_eq!(segs.len(), 1);
        assert_eq!(lit(&segs[0]), "{{< meta version >}}");
    }

    #[test]
    fn unterminated_candidate_stays_literal() {
        assert!(parse("broken {{< env HOME").is_none());
    }

    #[test]
    fn unterminated_quote_stays_literal() {
        assert!(parse("{{< env \"unclosed >}}").is_none());
    }

    #[test]
    fn adjacent_shortcodes() {
        let segs = parse("{{< meta a >}}{{< meta b >}}").unwrap();
        assert_eq!(segs.len(), 2);
        assert_eq!(sc(&segs[0]).positional_args.len(), 1);
        assert_eq!(arg_str(&sc(&segs[1]).positional_args[0]), "b");
    }

    #[test]
    fn multibyte_text_around_shortcodes() {
        let segs = parse("héllo {{< meta v >}} wörld").unwrap();
        assert_eq!(lit(&segs[0]), "héllo ");
        assert_eq!(lit(&segs[2]), " wörld");
    }
}
