/*
 * autoid.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::{Inline, Inlines};

/// Identifier Pandoc falls back to when a heading's text yields nothing.
///
/// Without it a heading such as `## ![](img.png)` derives the empty string,
/// which the caller then emits as a section with no `id` attribute at all —
/// an unlinkable heading — while the dedup counter hands `-1`, `-2` to the
/// ones that follow.
const EMPTY_ID_FALLBACK: &str = "section";

/// Flatten inlines to the plain text an identifier is derived from.
///
/// This mirrors Pandoc's `stringify` as it is used by
/// `inlineListToIdentifier`: every container inline is walked into, because
/// the markup is what the slug filter drops — not the words inside it.
///
/// The match is deliberately **exhaustive**: a `_` arm is how the next new
/// inline kind would silently start vanishing from anchor ids, which is the
/// bug this function had (bd-heading-id-drops-inline-content-fl84n3ql).
/// Adding an `Inline` variant should fail to compile here.
///
/// This is an *identifier* helper, not a display-text one. It wants math
/// source text, no quote delimiters and no line-break semantics, so it does
/// not share an implementation with `pampa::toc::inlines_to_text` or the
/// other `inlines_to_text` copies (bd-zzke tracks consolidating those).
fn collect_text(inlines: &Inlines, result: &mut String) {
    for inline in inlines {
        match inline {
            // Leaf kinds that carry text of their own.
            Inline::Str(s) => result.push_str(&s.text),
            Inline::Code(c) => result.push_str(&c.text),
            Inline::Math(m) => result.push_str(&m.text),

            // All whitespace collapses to a separator.
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => result.push(' '),

            // Container kinds: recurse.
            Inline::Emph(e) => collect_text(&e.content, result),
            Inline::Underline(u) => collect_text(&u.content, result),
            Inline::Strong(s) => collect_text(&s.content, result),
            Inline::Strikeout(s) => collect_text(&s.content, result),
            Inline::Superscript(s) => collect_text(&s.content, result),
            Inline::Subscript(s) => collect_text(&s.content, result),
            Inline::SmallCaps(s) => collect_text(&s.content, result),
            // Pandoc keeps a citation's *source* inlines as the `Cite`
            // content, so `[see @key, p. 33]` stringifies to that literal
            // text. pampa only populates `content` for author-in-text
            // citations; for the bracketed form it leaves `content` empty and
            // keeps everything in `citations`, so reconstruct from those.
            // Prefix and suffix already carry their own spacing (including
            // the separator before a second citation in `[@a; @b]`).
            Inline::Cite(c) if c.content.is_empty() => {
                for citation in &c.citations {
                    collect_text(&citation.prefix, result);
                    result.push_str(&citation.id);
                    collect_text(&citation.suffix, result);
                }
            }
            Inline::Cite(c) => collect_text(&c.content, result),
            Inline::Link(l) => collect_text(&l.content, result),
            Inline::Image(i) => collect_text(&i.content, result),
            Inline::Span(s) => collect_text(&s.content, result),
            Inline::Insert(i) => collect_text(&i.content, result),
            Inline::Highlight(h) => collect_text(&h.content, result),

            // `Quoted` recurses *without* emitting delimiters. Pandoc's
            // `deQuote` does emit U+2018/U+2019 or U+201C/U+201D, but the
            // slug filter below then strips them, so the resulting id is
            // identical either way. Any future shared inlines-to-text helper
            // must not assume the two agree: the TOC wants the glyphs
            // (bd-toc-smart-quotes-6nro57ed), an identifier does not care.
            Inline::Quoted(q) => collect_text(&q.content, result),

            // Kinds with no textual form in an identifier. Pandoc drops
            // footnote bodies (`deNote`) and raw inlines of every format.
            Inline::Note(_) | Inline::NoteReference(_) => {}
            Inline::RawInline(_) => {}
            // Deleted text is not part of the rendered heading.
            Inline::Delete(_) => {}
            // Markers with no rendered text at all.
            Inline::Attr(_) => {}
            Inline::EditComment(_) => {}
            Inline::Custom(_) => {}

            // The id is derived in the reader's postprocess pass, before
            // shortcodes are expanded, so an unexpanded shortcode has no
            // text to contribute. Quarto 1 expands shortcodes in a pre-Pandoc
            // text pass and folds the expansion into the id; matching that
            // would mean deriving ids after expansion. Tracked as
            // bd-2wv8431v.
            Inline::Shortcode(_) => {}
        }
    }
}

pub fn auto_generated_id(inlines: &Inlines) -> String {
    let mut text = String::new();
    collect_text(inlines, &mut text);

    // Match Pandoc's `inlineListToIdentifier`:
    // - Keep alphanumeric (lowercased), periods, underscores, hyphens
    // - Convert spaces to hyphens
    // - Remove other characters
    let ident = text
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '_' || c == '-' {
                Some(c)
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-");

    // Pandoc's `dropNonLetter`: an identifier starts at its first letter, so
    // `## 1 leading digit` becomes `leading-digit`, not `1-leading-digit`.
    let ident = ident.trim_start_matches(|c: char| !c.is_alphabetic());

    if ident.is_empty() {
        EMPTY_ID_FALLBACK.to_string()
    } else {
        ident.to_string()
    }
}
