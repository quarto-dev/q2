/*
 * span_link_helpers.rs
 *
 * Functions for processing span, link, and image nodes in the new tree-sitter grammar.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use super::pandocnativeintermediate::PandocNativeIntermediate;
use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::{AttrSourceInfo, TargetSourceInfo};
use crate::pandoc::inline::{Image, Inline, Link, Span};
use crate::pandoc::location::{node_location, node_source_info_with_context};

/// Extract target (URL and title) from children
pub fn process_target(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut url = String::new();
    let mut title = String::new();
    let mut range = quarto_source_map::Range {
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

    for (node_name, child) in children {
        match node_name.as_str() {
            "url" => {
                if let PandocNativeIntermediate::IntermediateBaseText(text, r) = child {
                    url = text;
                    range = r;
                }
            }
            "title" => {
                if let PandocNativeIntermediate::IntermediateBaseText(text, _) = child {
                    title = text;
                }
            }
            "](" | ")" => {} // Ignore delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateTarget(url, title, range)
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
    let mut attr = ("".to_string(), vec![], std::collections::HashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            "target" => {
                if let PandocNativeIntermediate::IntermediateTarget(url, title, _) = child {
                    target = Some((url, title));
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

    // Special handling: Check if this span contains ONLY a citation
    // If so, this is a bracketed citation like [@cite] or [-@cite]
    // We need to unwrap it and potentially change the mode
    if content_inlines.len() == 1
        && target.is_none()
        && attr.0.is_empty()
        && attr.1.is_empty()
        && attr.2.is_empty()
    {
        // Check if the single inline is a Cite (without consuming it yet)
        if matches!(content_inlines.first(), Some(Inline::Cite(_))) {
            if let Some(Inline::Cite(mut cite)) = content_inlines.pop() {
                // If the citation is AuthorInText mode (from @cite), change it to NormalCitation
                // because it's wrapped in brackets [@cite]
                for citation in &mut cite.citations {
                    if citation.mode == crate::pandoc::inline::CitationMode::AuthorInText {
                        citation.mode = crate::pandoc::inline::CitationMode::NormalCitation;
                    }
                }
                return PandocNativeIntermediate::IntermediateInline(Inline::Cite(cite));
            }
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
            target_source: TargetSourceInfo::empty(),
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
    let mut attr = ("".to_string(), vec![], std::collections::HashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    alt_inlines = inlines;
                }
            }
            "target" => {
                if let PandocNativeIntermediate::IntermediateTarget(url, title, _) = child {
                    target = Some((url, title));
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
        target_source: TargetSourceInfo::empty(),
    }))
}
