/*
 * span_link_helpers.rs
 *
 * Functions for processing span, link, and image nodes in the new tree-sitter grammar.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use super::pandocnativeintermediate::PandocNativeIntermediate;
use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::{AttrSourceInfo, TargetSourceInfo, is_empty_attr};
use crate::pandoc::inline::{Image, Inline, Link, Span, make_cite_inline};
use crate::pandoc::location::{node_location, node_source_info_with_context};

/// Grammar field marking the `url` node of the pointy-bracket destination
/// form `[x](<a b.png>)` (both forms share the `url` node kind).
pub const ANGLE_BRACKET_URL_FIELD: &str = "angle_bracket_url";

/// Decode a pointy-bracket link destination (`<...>`, brackets included)
/// the way Pandoc does: drop the brackets, resolve CommonMark backslash
/// escapes (a backslash before ASCII punctuation), then percent-encode with
/// [`escape_uri`].
///
/// The grammar token already guarantees the shape (no line ending, no
/// unescaped `<`/`>`), so this only has to undo the syntax.
pub fn decode_angle_bracket_destination(raw: &str) -> String {
    let inner = raw
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(raw);
    let mut unescaped = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && next.is_ascii_punctuation()
        {
            unescaped.push(next);
            chars.next();
            continue;
        }
        unescaped.push(c);
    }
    escape_uri(&unescaped)
}

/// Pandoc's `escapeURI`: percent-encode (as UTF-8 bytes) whitespace and the
/// characters `<>|"{}[]^` and backtick. Everything else — including `%`,
/// `#`, `?`, parentheses and non-ASCII letters — passes through, so an
/// already-encoded `%20` is not double-encoded.
pub fn escape_uri(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_whitespace()
            || matches!(c, '<' | '>' | '|' | '"' | '{' | '}' | '[' | ']' | '^' | '`')
        {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Extract target (URL and title) from children
pub fn process_target(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut url = String::new();
    let mut title = String::new();
    let empty_range = quarto_source_map::Range {
        start: quarto_source_map::Location {
            offset: 0,
            row: 0,
            column: 0,
        },
        end: quarto_source_map::Location {
            offset: 0,
            row: 0,
            column: 0,
        },
    };
    let mut url_range = empty_range.clone();
    // The title's *content* provenance (quotes stripped, escapes
    // collapsed). Only read when `title` is non-empty, so the placeholder
    // never reaches a consumer.
    let mut title_source =
        quarto_source_map::SourceInfo::from_range(quarto_source_map::FileId(0), empty_range);

    for (node_name, child) in children {
        match node_name.as_str() {
            "url" => {
                if let PandocNativeIntermediate::IntermediateBaseText(text, r) = child {
                    url = text;
                    url_range = r;
                }
            }
            "title" => {
                if let PandocNativeIntermediate::IntermediateDecodedText(text, source) = child {
                    title = text;
                    title_source = source;
                }
            }
            "](" | ")" => {} // Ignore delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateTarget(url, title, url_range, title_source)
}

/// Process content node (context-aware for code_span vs links/spans/images)
pub fn process_content_node(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    if children.is_empty() {
        // No processed children - this is code_span content, return range
        PandocNativeIntermediate::IntermediateUnknown(node_location(node))
    } else {
        // Has children - this is link/span/image content, return processed inlines
        let inlines: Vec<Inline> = children
            .into_iter()
            .flat_map(|(_, child)| match child {
                PandocNativeIntermediate::IntermediateInline(inline) => vec![inline],
                PandocNativeIntermediate::IntermediateInlines(inlines) => inlines,
                _ => vec![],
            })
            .collect();
        PandocNativeIntermediate::IntermediateInlines(inlines)
    }
}

/// Process pandoc_span node (creates Link or Span based on target presence)
pub fn process_pandoc_span(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content_inlines: Vec<Inline> = Vec::new();
    let mut target: Option<(String, String)> = None;
    let mut target_source = TargetSourceInfo::empty();
    let mut attr = (String::new(), vec![], hashlink::LinkedHashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            "target" => {
                if let PandocNativeIntermediate::IntermediateTarget(
                    url,
                    title,
                    url_range,
                    title_source,
                ) = child
                {
                    target = Some((url.clone(), title.clone()));
                    // The url slot is still a raw range (no decoding);
                    // the title slot already carries content provenance.
                    target_source = TargetSourceInfo {
                        url: if !url.is_empty() {
                            Some(crate::pandoc::location::range_to_source_info_with_context(
                                &url_range, context,
                            ))
                        } else {
                            None
                        },
                        title: if !title.is_empty() {
                            Some(title_source)
                        } else {
                            None
                        },
                    };
                }
            }
            "attribute_specifier" => {
                if let PandocNativeIntermediate::IntermediateAttr(attrs, attrs_src, _) = child {
                    attr = attrs;
                    attr_source = attrs_src;
                }
            }
            "[" | "]" => {} // Skip delimiters
            _ => {}
        }
    }

    // Check if this looks like a citation pattern (no target, no attributes, contains citations)
    // This handles both single citations [@cite] and multi-citations [prefix @c1 suffix; @c2; @c3]
    if target.is_none() && is_empty_attr(&attr) {
        // Check if content contains any citations
        let has_citations = content_inlines
            .iter()
            .any(|inline| matches!(inline, Inline::Cite(_)));

        if has_citations {
            // Preprocess content_inlines to split Str nodes that end with ";"
            // The tree-sitter grammar produces "suffix;" as a single token, but make_cite_inline
            // expects semicolons to be separate Str ";" nodes for proper citation splitting
            let mut preprocessed_inlines = Vec::new();
            for inline in content_inlines {
                match inline {
                    Inline::Str(ref s) if s.text.ends_with(';') && s.text.len() > 1 => {
                        // Split "suffix;" into "suffix" and ";"
                        let text_without_semicolon = s.text[..s.text.len() - 1].to_string();
                        preprocessed_inlines.push(Inline::Str(crate::pandoc::inline::Str {
                            text: text_without_semicolon,
                            source_info: s.source_info.clone(),
                        }));
                        preprocessed_inlines.push(Inline::Str(crate::pandoc::inline::Str {
                            text: ";".to_string(),
                            source_info: s.source_info.clone(),
                        }));
                    }
                    _ => preprocessed_inlines.push(inline),
                }
            }

            // Use make_cite_inline to handle both single and multi-citation cases
            // This function will:
            // - Split content by semicolons
            // - Distribute prefix/suffix to citations
            // - Change AuthorInText mode to NormalCitation
            // - Backtrack to Span if content isn't citation-worthy
            return PandocNativeIntermediate::IntermediateInline(make_cite_inline(
                attr,
                (String::new(), String::new()), // empty target
                preprocessed_inlines,
                node_source_info_with_context(node, context),
                attr_source,
                TargetSourceInfo::empty(),
            ));
        }
    }

    // Decide what to create based on presence of target
    if let Some((url, title)) = target {
        // This is a LINK
        PandocNativeIntermediate::IntermediateInline(Inline::Link(Box::new(Link {
            attr,
            content: content_inlines,
            target: (url, title),
            source_info: node_source_info_with_context(node, context),
            attr_source,
            target_source,
        })))
    } else {
        // No target → Check for special Span classes that map to specific inline types

        // Special case: [text]{.underline} or [text]{.ul} → Underline
        if attr.0.is_empty()
            && (attr.1 == vec!["underline"] || attr.1 == vec!["ul"])
            && attr.2.is_empty()
        {
            return PandocNativeIntermediate::IntermediateInline(Inline::Underline(
                crate::pandoc::inline::Underline {
                    content: content_inlines,
                    source_info: node_source_info_with_context(node, context),
                },
            ));
        }

        // Special case: [text]{.smallcaps} → SmallCaps
        if attr.0.is_empty() && attr.1 == vec!["smallcaps"] && attr.2.is_empty() {
            return PandocNativeIntermediate::IntermediateInline(Inline::SmallCaps(
                crate::pandoc::inline::SmallCaps {
                    content: content_inlines,
                    source_info: node_source_info_with_context(node, context),
                },
            ));
        }

        // Default: SPAN (even if attributes are empty)
        // QMD design choice: [text] becomes Span, not literal brackets
        PandocNativeIntermediate::IntermediateInline(Inline::Span(Box::new(Span {
            attr,
            content: content_inlines,
            source_info: node_source_info_with_context(node, context),
            attr_source,
        })))
    }
}

/// Process pandoc_image node
pub fn process_pandoc_image(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut alt_inlines: Vec<Inline> = Vec::new();
    let mut target: Option<(String, String)> = None;
    let mut target_source = TargetSourceInfo::empty();
    let mut attr = (String::new(), vec![], hashlink::LinkedHashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    alt_inlines = inlines;
                }
            }
            "target" => {
                if let PandocNativeIntermediate::IntermediateTarget(
                    url,
                    title,
                    url_range,
                    title_source,
                ) = child
                {
                    target = Some((url.clone(), title.clone()));
                    // The url slot is still a raw range (no decoding);
                    // the title slot already carries content provenance.
                    target_source = TargetSourceInfo {
                        url: if !url.is_empty() {
                            Some(crate::pandoc::location::range_to_source_info_with_context(
                                &url_range, context,
                            ))
                        } else {
                            None
                        },
                        title: if !title.is_empty() {
                            Some(title_source)
                        } else {
                            None
                        },
                    };
                }
            }
            "attribute_specifier" => {
                if let PandocNativeIntermediate::IntermediateAttr(attrs, attrs_src, _) = child {
                    attr = attrs;
                    attr_source = attrs_src;
                }
            }
            _ => {} // Ignore other nodes (delimiters, etc.)
        }
    }

    // Create Image inline
    let (url, title) = target.unwrap_or_else(|| (String::new(), String::new()));

    PandocNativeIntermediate::IntermediateInline(Inline::Image(Box::new(Image {
        attr,
        content: alt_inlines,
        target: (url, title),
        source_info: node_source_info_with_context(node, context),
        attr_source,
        target_source,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values are pandoc 3.12.1's output for `[x](<...>)`.
    #[test]
    fn angle_destination_matches_pandoc() {
        let cases = [
            ("<a path with spaces.png>", "a%20path%20with%20spaces.png"),
            ("<nospace.png>", "nospace.png"),
            ("<>", ""),
            ("<a\\>b.html>", "a%3Eb.html"),
            ("<a(b) c.png>", "a(b)%20c.png"),
            ("<\u{e9} \u{fc}.png>", "\u{e9}%20\u{fc}.png"),
            ("<a\u{a0}b>", "a%C2%A0b"),
            ("<a \"q\">", "a%20%22q%22"),
            ("<a{b}>", "a%7Bb%7D"),
            ("<a\\\\n\\o>", "a\\n\\o"),
            ("<a%20b#c?d>", "a%20b#c?d"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                decode_angle_bracket_destination(input),
                expected,
                "input: {input}"
            );
        }
    }

    #[test]
    fn escape_uri_encodes_pandoc_set_only() {
        assert_eq!(escape_uri("<>|\"{}[]^`"), "%3C%3E%7C%22%7B%7D%5B%5D%5E%60");
        assert_eq!(escape_uri("a\tb"), "a%09b");
        assert_eq!(escape_uri("%;=&'~()"), "%;=&'~()");
    }
}
