/*
 * uri_autolink.rs
 *
 * Functions for processing autolink nodes in the tree-sitter AST: URI
 * autolinks (`<http://example.com>`), CommonMark email autolinks
 * (`<user@example.com>`), and the raw-HTML fallback for content the scanner
 * over-approximated as an autolink (bd-email-autolink-dropped-2jj38iiv).
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, Link, RawInline, Space, Str};
use crate::pandoc::location::node_location;
use crate::utils::diagnostic_collector::DiagnosticCollector;
use hashlink::LinkedHashMap;
use quarto_error_reporting::DiagnosticMessageBuilder;
use regex::Regex;
use std::sync::OnceLock;

use super::pandocnativeintermediate::PandocNativeIntermediate;

/// The CommonMark email autolink production (spec §Autolinks), anchored.
fn email_autolink_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$",
        )
        .expect("email autolink regex must compile")
    })
}

pub fn process_uri_autolink(
    node: &tree_sitter::Node,
    input_bytes: &[u8],
    context: &ASTContext,
    error_collector: &mut DiagnosticCollector,
) -> PandocNativeIntermediate {
    // The tree-sitter scanner may include leading/trailing whitespace in the autolink token
    // because it consumes whitespace for indentation calculation before lexing inline tokens.
    // We need to split the token into separate Space nodes and the actual autolink.

    // Get the full node range (may include leading/trailing whitespace)
    let node_range = node_location(node);

    // Extract the full text from the node range
    let text = &input_bytes[node_range.start.offset..node_range.end.offset];
    let text_str = std::str::from_utf8(text).unwrap();

    // Count leading/trailing whitespace characters.
    // ASCII-only by intent: per Pandoc-compat policy in
    // claude-notes/plans/2026-04-30-unicode-whitespace-handling.md
    // (bd-rmx3, bd-8oe4), non-ASCII whitespace is content, not
    // whitespace, so it must not be peeled off into a Space node here.
    let leading_ws_count = text_str
        .chars()
        .take_while(|c| c.is_ascii_whitespace())
        .count();

    let trailing_ws_count = text_str
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_whitespace())
        .count();

    // Extract the actual autolink text (trimmed)
    let autolink_text = text_str.trim_ascii();

    // Validate it's a proper autolink with angle brackets
    if autolink_text.len() < 2 || !autolink_text.starts_with('<') || !autolink_text.ends_with('>') {
        panic!("Invalid URI autolink: {}", autolink_text);
    }

    // Extract the URL (remove angle brackets)
    let url = &autolink_text[1..autolink_text.len() - 1];

    // Calculate range for leading space (if present)
    let leading_space_range = if leading_ws_count > 0 {
        Some(quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: node_range.start.offset,
                row: node_range.start.row,
                column: node_range.start.column,
            },
            end: quarto_source_map::Location {
                offset: node_range.start.offset + leading_ws_count,
                row: node_range.start.row,
                column: node_range.start.column + leading_ws_count,
            },
        })
    } else {
        None
    };

    // Calculate range for the autolink itself (excluding whitespace)
    let autolink_range = quarto_source_map::Range {
        start: quarto_source_map::Location {
            offset: node_range.start.offset + leading_ws_count,
            row: node_range.start.row,
            column: node_range.start.column + leading_ws_count,
        },
        end: quarto_source_map::Location {
            offset: node_range.end.offset - trailing_ws_count,
            row: node_range.end.row,
            column: node_range.end.column - trailing_ws_count,
        },
    };

    // Calculate range for trailing space (if present)
    let trailing_space_range = if trailing_ws_count > 0 {
        Some(quarto_source_map::Range {
            start: quarto_source_map::Location {
                offset: node_range.end.offset - trailing_ws_count,
                row: node_range.end.row,
                column: node_range.end.column - trailing_ws_count,
            },
            end: quarto_source_map::Location {
                offset: node_range.end.offset,
                row: node_range.end.row,
                column: node_range.end.column,
            },
        })
    } else {
        None
    };

    // Classify the token (bd-email-autolink-dropped-2jj38iiv). Order
    // matters: a valid email whose local part contains '%' (legal there)
    // must classify as email, not URI.
    let is_email = email_autolink_regex().is_match(url);
    let is_uri_like = url.contains(':') || url.contains('%');
    if !is_email && !is_uri_like {
        // The scanner over-approximated (it lexes any whitespace-free
        // '<...@...>' as an autolink candidate). Before the email-autolink
        // change this content lexed as HTML_ELEMENT; reproduce that arm's
        // treatment exactly: Q-2-9 warning + RawInline html.
        return raw_html_fallback(
            autolink_text,
            context,
            leading_space_range,
            autolink_range,
            trailing_space_range,
            error_collector,
        );
    }

    // Build the result with separate nodes for spaces and autolink
    let mut result = Vec::new();

    // Add leading space if present
    if let Some(space_range) = leading_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    // Email autolinks link to mailto: with the bare address as text and
    // class "email"; URI autolinks link to the literal content with class
    // "uri". Both match pandoc's markdown reader (and Quarto 1).
    let (target, class) = if is_email {
        (format!("mailto:{}", url), "email")
    } else {
        (url.to_string(), "uri")
    };

    let mut attr = (String::new(), vec![], LinkedHashMap::new());
    attr.1.push(class.to_string());

    result.push(Inline::Link(Link {
        content: vec![Inline::Str(Str {
            text: url.to_string(),
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                autolink_range.clone(),
            ),
        })],
        attr,
        target: (target, String::new()),
        source_info: quarto_source_map::SourceInfo::from_range(
            context.current_file_id(),
            autolink_range,
        ),
        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
        target_source: crate::pandoc::attr::TargetSourceInfo::empty(),
    }));

    // Add trailing space if present
    if let Some(space_range) = trailing_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    // Return as IntermediateInlines (multiple nodes) instead of single IntermediateInline
    PandocNativeIntermediate::IntermediateInlines(result)
}

/// Emit the treatment `<...>` content received when it lexed as an HTML
/// element (the `html_element` arm in treesitter.rs): a Q-2-9 warning and a
/// RawInline with format "html", with any scanner-captured whitespace split
/// out as adjacent Space inlines.
fn raw_html_fallback(
    autolink_text: &str,
    context: &ASTContext,
    leading_space_range: Option<quarto_source_map::Range>,
    autolink_range: quarto_source_map::Range,
    trailing_space_range: Option<quarto_source_map::Range>,
    error_collector: &mut DiagnosticCollector,
) -> PandocNativeIntermediate {
    let trimmed_source_info =
        quarto_source_map::SourceInfo::from_range(context.current_file_id(), autolink_range);

    let msg = DiagnosticMessageBuilder::warning("HTML element converted to raw HTML")
        .with_code("Q-2-9")
        .with_location(trimmed_source_info.clone())
        .add_info("HTML elements are automatically converted to RawInline nodes with format 'html'")
        .add_hint("To be explicit, use: `<element>`{=html}")
        .build();
    error_collector.add(msg);

    let mut result = Vec::new();

    if let Some(space_range) = leading_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    result.push(Inline::RawInline(RawInline {
        format: "html".to_string(),
        text: autolink_text.to_string(),
        source_info: trimmed_source_info,
    }));

    if let Some(space_range) = trailing_space_range {
        result.push(Inline::Space(Space {
            source_info: quarto_source_map::SourceInfo::from_range(
                context.current_file_id(),
                space_range,
            ),
        }));
    }

    PandocNativeIntermediate::IntermediateInlines(result)
}
