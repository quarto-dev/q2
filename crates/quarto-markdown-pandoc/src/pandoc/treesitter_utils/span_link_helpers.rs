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
    let mut title_range = empty_range.clone();

    for (node_name, child) in children {
        match node_name.as_str() {
            "url" => {
                if let PandocNativeIntermediate::IntermediateBaseText(text, r) = child {
                    url = text;
                    url_range = r;
                }
            }
            "title" => {
                if let PandocNativeIntermediate::IntermediateBaseText(text, r) = child {
                    title = text;
                    title_range = r;
                }
            }
            "](" | ")" => {} // Ignore delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateTarget(url, title, url_range, title_range)
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
    let mut attr = ("".to_string(), vec![], hashlink::LinkedHashMap::new());
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
                    title_range,
                ) = child
                {
                    target = Some((url.clone(), title.clone()));
                    // Populate target_source with the ranges
                    target_source = TargetSourceInfo {
                        url: if !url.is_empty() {
                            Some(crate::pandoc::location::range_to_source_info_with_context(
                                &url_range, context,
                            ))
                        } else {
                            None
                        },
                        title: if !title.is_empty() {
                            Some(crate::pandoc::location::range_to_source_info_with_context(
                                &title_range,
                                context,
                            ))
                        } else {
                            None
                        },
                    };
                }
            }
            "attribute_specifier" => {
                if let PandocNativeIntermediate::IntermediateAttr(attrs, attrs_src) = child {
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
                ("".to_string(), "".to_string()), // empty target
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
        PandocNativeIntermediate::IntermediateInline(Inline::Link(Link {
            attr,
            content: content_inlines,
            target: (url, title),
            source_info: node_source_info_with_context(node, context),
            attr_source,
            target_source,
        }))
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
        PandocNativeIntermediate::IntermediateInline(Inline::Span(Span {
            attr,
            content: content_inlines,
            source_info: node_source_info_with_context(node, context),
            attr_source,
        }))
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
    let mut attr = ("".to_string(), vec![], hashlink::LinkedHashMap::new());
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
                    title_range,
                ) = child
                {
                    target = Some((url.clone(), title.clone()));
                    // Populate target_source with the ranges
                    target_source = TargetSourceInfo {
                        url: if !url.is_empty() {
                            Some(crate::pandoc::location::range_to_source_info_with_context(
                                &url_range, context,
                            ))
                        } else {
                            None
                        },
                        title: if !title.is_empty() {
                            Some(crate::pandoc::location::range_to_source_info_with_context(
                                &title_range,
                                context,
                            ))
                        } else {
                            None
                        },
                    };
                }
            }
            "attribute_specifier" => {
                if let PandocNativeIntermediate::IntermediateAttr(attrs, attrs_src) = child {
                    attr = attrs;
                    attr_source = attrs_src;
                }
            }
            _ => {} // Ignore other nodes (delimiters, etc.)
        }
    }

    // Create Image inline
    let (url, title) = target.unwrap_or_else(|| ("".to_string(), "".to_string()));

    PandocNativeIntermediate::IntermediateInline(Inline::Image(Image {
        attr,
        content: alt_inlines,
        target: (url, title),
        source_info: node_source_info_with_context(node, context),
        attr_source,
        target_source,
    }))
}
