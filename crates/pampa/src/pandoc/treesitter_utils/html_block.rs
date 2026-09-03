/*
 * html_block.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Recognising raw HTML that occupies a block position.
 *
 * Naked HTML is not a supported authoring form in qmd — the documented
 * spellings are a `{=html}` raw block/inline or a `::: {.class}` fenced div
 * (see `crates/pampa/README.md` "Important differences" and
 * `dev-docs/syntax-notes.md` "No naked HTML support"). But qmd accepts it
 * with a Q-2-9 warning rather than rejecting it, and when the tag is
 * block-level the result has to be a `RawBlock`: `<div>` and `<details>` are
 * flow content, not phrasing content, so emitting them inside a `<p>`
 * produces invalid HTML that a browser's parser silently restructures.
 *
 * The tag set is pandoc's, not CommonMark's. Pandoc's markdown reader keys
 * block-level-ness off a tag-name whitelist (`blockTags` in
 * `Text/Pandoc/Readers/HTML/TagCategories.hs`); CommonMark instead has seven
 * start conditions, the last of which promotes *any* complete tag alone on a
 * line. Adopting CommonMark's type 7 would newly diverge from Quarto 1 for
 * common shapes — a lone `<img src=…>`, `<a href=…>` or `<span class=…>` —
 * so we deliberately do not implement it.
 *
 * Two sets are deliberately excluded from the whitelist below:
 *
 *   - pandoc's DocBook names (`note`, `tip`, `warning`, `para`, `screen`, …).
 *     qmd is not a DocBook host, and those names are not HTML elements, so a
 *     browser treats them as unknown *inline* elements — leaving them inside
 *     a paragraph is valid.
 *   - CommonMark type-6 names pandoc lacks (`dialog`, `search`, `option`,
 *     `param`, `base`, `link`, `title`, `legend`, `optgroup`, `menuitem`,
 *     `frame`, `basefont`). Including them would diverge from Quarto 1.
 *
 * bd-block-html-wrapped-in-p-w8qebxig
 */

/// Tag names that put raw HTML in a block position.
///
/// This is pandoc's `blockHtmlTags` (HTML entries only) plus its
/// `eitherBlockOrInline` set, which its markdown reader also treats as
/// block-capable. Kept sorted for review against the pandoc source.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "applet",
    "area",
    "article",
    "aside",
    "audio",
    "blockquote",
    "body",
    "button",
    "canvas",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dir",
    "div",
    "dl",
    "dt",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hgroup",
    "hr",
    "html",
    "iframe",
    "ins",
    "isindex",
    "li",
    "main",
    "map",
    "menu",
    "meta",
    "nav",
    "noframes",
    "noscript",
    "object",
    "ol",
    "output",
    "p",
    "pre",
    "progress",
    "script",
    "section",
    "source",
    "style",
    "summary",
    "svg",
    "table",
    "tbody",
    "td",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "tr",
    "track",
    "ul",
    "video",
];

/// Does `text` begin raw HTML that belongs in a block position?
///
/// True for an opening or closing tag whose name is in [`BLOCK_TAGS`], and for
/// the `<!…` and `<?…` forms — comments, declarations, CDATA and processing
/// instructions — which pandoc's `isBlockTag` also treats as block-level and
/// which CommonMark covers with HTML block types 2 through 5.
pub fn starts_block_html(text: &str) -> bool {
    let rest = match text.strip_prefix('<') {
        Some(rest) => rest,
        None => return false,
    };

    // `<!--comment-->`, `<!DOCTYPE …>`, `<![CDATA[…]]>`, `<?php …?>`
    if rest.starts_with('!') || rest.starts_with('?') {
        return true;
    }

    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let name_len = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    if name_len == 0 {
        return false;
    }
    let (name, after) = rest.split_at(name_len);

    // The tag name must actually end here: `<divider>` is not a `<div>`.
    if !after.is_empty() {
        let next = after.as_bytes()[0];
        if !matches!(next, b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/') {
            return false;
        }
    }

    let name = name.to_ascii_lowercase();
    BLOCK_TAGS.binary_search(&name.as_str()).is_ok()
}

/// Is the byte at `offset` the first non-prefix content on its line?
///
/// Only whitespace and block-quote markers may precede it. Used to decide
/// whether a raw HTML tag is worth describing as block-level in a diagnostic;
/// the lift itself does not need this, because being the first inline of a
/// paragraph already implies it.
pub fn at_line_start(input: &[u8], offset: usize) -> bool {
    if offset > input.len() {
        return false;
    }
    for &b in input[..offset].iter().rev() {
        match b {
            b'\n' => return true,
            b' ' | b'\t' | b'\r' | b'>' => continue,
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_tags_are_sorted() {
        // `starts_block_html` binary-searches the list.
        let mut sorted = BLOCK_TAGS.to_vec();
        sorted.sort_unstable();
        assert_eq!(BLOCK_TAGS, sorted.as_slice());
    }

    #[test]
    fn recognises_block_tags() {
        assert!(starts_block_html("<div>"));
        assert!(starts_block_html("<div class=\"a\">"));
        assert!(starts_block_html("</div>"));
        assert!(starts_block_html("<details class=\"x\">"));
        assert!(starts_block_html("<summary>"));
        assert!(starts_block_html("<HR/>"));
        assert!(starts_block_html("<TABLE>"));
    }

    #[test]
    fn recognises_declarations_and_comments() {
        assert!(starts_block_html("<!--comment-->"));
        assert!(starts_block_html("<!DOCTYPE html>"));
        assert!(starts_block_html("<![CDATA[x]]>"));
        assert!(starts_block_html("<?xml version=\"1.0\"?>"));
    }

    #[test]
    fn rejects_inline_tags() {
        // Quarto 1 parity: these stay inside a paragraph.
        assert!(!starts_block_html("<span class=\"x\">"));
        assert!(!starts_block_html("<img src=\"a.png\">"));
        assert!(!starts_block_html("<a href=\"x\">"));
        assert!(!starts_block_html("<b>"));
        assert!(!starts_block_html("<em>"));
        assert!(!starts_block_html("<code>"));
        assert!(!starts_block_html("<br/>"));
    }

    #[test]
    fn rejects_docbook_and_commonmark_only_names() {
        // Documented scope-outs, see the module comment.
        assert!(!starts_block_html("<warning>"));
        assert!(!starts_block_html("<note>"));
        assert!(!starts_block_html("<dialog>"));
        assert!(!starts_block_html("<option>"));
    }

    #[test]
    fn requires_the_whole_tag_name_to_match() {
        assert!(!starts_block_html("<divider>"));
        assert!(!starts_block_html("<tablet>"));
        assert!(!starts_block_html("<premium>"));
        assert!(!starts_block_html("<summarize>"));
    }

    #[test]
    fn rejects_non_tags() {
        assert!(!starts_block_html("div"));
        assert!(!starts_block_html("<"));
        assert!(!starts_block_html("<>"));
        assert!(!starts_block_html("< div>"));
    }

    #[test]
    fn line_start_detection() {
        assert!(at_line_start(b"<div>", 0));
        assert!(at_line_start(b"text\n<div>", 5));
        assert!(at_line_start(b"text\n  <div>", 7));
        assert!(at_line_start(b"> <div>", 2));
        assert!(!at_line_start(b"text <div>", 5));
    }
}
