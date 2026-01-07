/*
 * engine/reconcile.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Source location reconciliation after engine execution.
 */

//! Source location reconciliation for engine execution.
//!
//! When an execution engine (knitr, jupyter) processes a document, it operates
//! on text and produces text output. This loses all source location information.
//! This module reconciles the pre-engine AST with the post-engine AST to preserve
//! source locations for unchanged content.
//!
//! # Algorithm Overview
//!
//! 1. Compare blocks/inlines by content (ignoring source locations)
//! 2. For exact matches, transfer source locations from original to executed
//! 3. For structural matches (same type but different content), keep executed locations
//! 4. For new blocks (engine outputs), keep executed locations
//!
//! # Example
//!
//! ```ignore
//! let original_ast = parse_qmd(original_content);
//! let executed_ast = parse_qmd(engine_output);
//!
//! let report = reconcile_source_locations(&original_ast, &mut executed_ast);
//!
//! // executed_ast now has original source locations where content matches
//! ```

use quarto_pandoc_types::Inlines;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_pandoc_types::pandoc::Pandoc;

/// Report of what happened during reconciliation.
#[derive(Debug, Default)]
pub struct ReconciliationReport {
    /// Blocks that matched exactly - original source locations transferred
    pub exact_matches: usize,
    /// Blocks with same structure but different content
    pub content_changes: usize,
    /// Blocks only in original (deleted by engine - unusual)
    pub deletions: usize,
    /// Blocks only in executed (added by engine - code outputs)
    pub additions: usize,
}

impl ReconciliationReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total number of blocks processed
    pub fn total(&self) -> usize {
        self.exact_matches + self.content_changes + self.deletions + self.additions
    }
}

/// Quality of match between two AST nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchQuality {
    /// Exact content match - transfer source location
    Exact,
    /// Same structure but different content - keep executed location
    StructuralOnly,
    /// No match
    NoMatch,
}

/// Reconcile source locations between pre-engine and post-engine ASTs.
///
/// For content that hasn't changed during execution, this transfers the
/// original source locations to the executed AST. For new content (execution
/// outputs), the executed AST's locations are preserved.
///
/// # Arguments
///
/// * `original` - Pre-engine AST with original source locations
/// * `executed` - Post-engine AST (will be modified in place)
///
/// # Returns
///
/// A report describing what was reconciled.
pub fn reconcile_source_locations(
    original: &Pandoc,
    executed: &mut Pandoc,
) -> ReconciliationReport {
    let mut report = ReconciliationReport::new();

    // Reconcile blocks using linear alignment
    reconcile_blocks(&original.blocks, &mut executed.blocks, &mut report);

    report
}

/// Reconcile block lists.
fn reconcile_blocks(original: &[Block], executed: &mut [Block], report: &mut ReconciliationReport) {
    // Use linear alignment - most engine outputs are nearly identical
    // to the input, with only code block outputs differing
    let mut orig_idx = 0;
    let mut exec_idx = 0;

    while orig_idx < original.len() && exec_idx < executed.len() {
        let quality = match_blocks(&original[orig_idx], &executed[exec_idx]);

        match quality {
            MatchQuality::Exact => {
                // Transfer source location from original to executed
                transfer_block_source_info(&original[orig_idx], &mut executed[exec_idx]);
                report.exact_matches += 1;
                orig_idx += 1;
                exec_idx += 1;
            }
            MatchQuality::StructuralOnly => {
                // Same structure, different content - reconcile children but keep this location
                reconcile_block_children(&original[orig_idx], &mut executed[exec_idx], report);
                report.content_changes += 1;
                orig_idx += 1;
                exec_idx += 1;
            }
            MatchQuality::NoMatch => {
                // Try to find a match ahead in executed (original block may have been deleted)
                // or find a match ahead in original (executed may have insertions)
                if let Some(ahead) = find_match_ahead(&original[orig_idx], executed, exec_idx + 1) {
                    // Blocks between exec_idx and ahead are additions
                    for _ in exec_idx..ahead {
                        report.additions += 1;
                        exec_idx += 1;
                    }
                    // Now we're at a match
                    continue;
                } else if let Some(ahead) =
                    find_match_ahead_in_original(&executed[exec_idx], original, orig_idx + 1)
                {
                    // Blocks between orig_idx and ahead were deleted
                    for _ in orig_idx..ahead {
                        report.deletions += 1;
                        orig_idx += 1;
                    }
                    // Now we're at a match
                    continue;
                } else {
                    // No match found - treat executed as addition, original as deletion
                    report.additions += 1;
                    report.deletions += 1;
                    orig_idx += 1;
                    exec_idx += 1;
                }
            }
        }
    }

    // Remaining original blocks were deleted
    while orig_idx < original.len() {
        report.deletions += 1;
        orig_idx += 1;
    }

    // Remaining executed blocks are additions
    while exec_idx < executed.len() {
        report.additions += 1;
        exec_idx += 1;
    }
}

/// Find a matching block ahead in the executed list.
fn find_match_ahead(original: &Block, executed: &[Block], start: usize) -> Option<usize> {
    // Look ahead a limited distance to avoid O(n^2) behavior
    const LOOKAHEAD: usize = 5;

    for i in start..executed.len().min(start + LOOKAHEAD) {
        if matches!(match_blocks(original, &executed[i]), MatchQuality::Exact) {
            return Some(i);
        }
    }
    None
}

/// Find a matching block ahead in the original list.
fn find_match_ahead_in_original(
    executed: &Block,
    original: &[Block],
    start: usize,
) -> Option<usize> {
    const LOOKAHEAD: usize = 5;

    for i in start..original.len().min(start + LOOKAHEAD) {
        if matches!(match_blocks(&original[i], executed), MatchQuality::Exact) {
            return Some(i);
        }
    }
    None
}

/// Match two blocks, returning the quality of match.
fn match_blocks(original: &Block, executed: &Block) -> MatchQuality {
    // First, types must match
    if std::mem::discriminant(original) != std::mem::discriminant(executed) {
        return MatchQuality::NoMatch;
    }

    match (original, executed) {
        // Code blocks: match by attributes (language, id, classes)
        // Content may differ due to execution
        (Block::CodeBlock(a), Block::CodeBlock(b)) => {
            if attrs_equal(&a.attr, &b.attr) {
                if a.text == b.text {
                    MatchQuality::Exact
                } else {
                    MatchQuality::StructuralOnly
                }
            } else {
                MatchQuality::NoMatch
            }
        }

        // Headers: match by level and content
        (Block::Header(a), Block::Header(b)) => {
            if a.level == b.level && inlines_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // Paragraphs: content-based matching
        (Block::Paragraph(a), Block::Paragraph(b)) => {
            if inlines_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // Plain: content-based matching
        (Block::Plain(a), Block::Plain(b)) => {
            if inlines_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // LineBlock: content-based matching
        (Block::LineBlock(a), Block::LineBlock(b)) => {
            if a.content.len() == b.content.len()
                && a.content
                    .iter()
                    .zip(b.content.iter())
                    .all(|(ai, bi)| inlines_content_equal(ai, bi))
            {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // RawBlock: match by format and text
        (Block::RawBlock(a), Block::RawBlock(b)) => {
            if a.format == b.format && a.text == b.text {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // BlockQuote: recurse into content
        (Block::BlockQuote(a), Block::BlockQuote(b)) => {
            if blocks_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else if a.content.len() == b.content.len() {
                MatchQuality::StructuralOnly
            } else {
                MatchQuality::NoMatch
            }
        }

        // OrderedList: match by attributes and content
        (Block::OrderedList(a), Block::OrderedList(b)) => {
            if a.attr == b.attr && lists_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else if a.content.len() == b.content.len() {
                MatchQuality::StructuralOnly
            } else {
                MatchQuality::NoMatch
            }
        }

        // BulletList: content-based matching
        (Block::BulletList(a), Block::BulletList(b)) => {
            if lists_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else if a.content.len() == b.content.len() {
                MatchQuality::StructuralOnly
            } else {
                MatchQuality::NoMatch
            }
        }

        // DefinitionList: content-based matching
        (Block::DefinitionList(a), Block::DefinitionList(b)) => {
            if a.content.len() == b.content.len()
                && a.content.iter().zip(b.content.iter()).all(|(ai, bi)| {
                    inlines_content_equal(&ai.0, &bi.0) && lists_content_equal(&ai.1, &bi.1)
                })
            {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // HorizontalRule: always matches if both are HorizontalRule
        (Block::HorizontalRule(_), Block::HorizontalRule(_)) => MatchQuality::Exact,

        // Div: match by attributes, recurse into content
        (Block::Div(a), Block::Div(b)) => {
            if attrs_equal(&a.attr, &b.attr) {
                if blocks_content_equal(&a.content, &b.content) {
                    MatchQuality::Exact
                } else {
                    MatchQuality::StructuralOnly
                }
            } else {
                MatchQuality::NoMatch
            }
        }

        // Figure: match by attributes, recurse into content
        (Block::Figure(a), Block::Figure(b)) => {
            if attrs_equal(&a.attr, &b.attr) && blocks_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else if attrs_equal(&a.attr, &b.attr) {
                MatchQuality::StructuralOnly
            } else {
                MatchQuality::NoMatch
            }
        }

        // Table: structural match only (tables are complex)
        (Block::Table(_), Block::Table(_)) => {
            // Tables are complex - for now, treat as structural match
            // A full implementation would compare all table components
            MatchQuality::StructuralOnly
        }

        // MetaBlock: match by metadata content
        (Block::BlockMetadata(a), Block::BlockMetadata(b)) => {
            if a.meta == b.meta {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // NoteDefinitionPara: match by id and content
        (Block::NoteDefinitionPara(a), Block::NoteDefinitionPara(b)) => {
            if a.id == b.id && inlines_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // NoteDefinitionFencedBlock: match by id, recurse into content
        (Block::NoteDefinitionFencedBlock(a), Block::NoteDefinitionFencedBlock(b)) => {
            if a.id == b.id && blocks_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else if a.id == b.id {
                MatchQuality::StructuralOnly
            } else {
                MatchQuality::NoMatch
            }
        }

        // CaptionBlock: match by content
        (Block::CaptionBlock(a), Block::CaptionBlock(b)) => {
            if inlines_content_equal(&a.content, &b.content) {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // Custom nodes: structural match only
        (Block::Custom(_), Block::Custom(_)) => MatchQuality::StructuralOnly,

        // Different types - shouldn't reach here due to discriminant check
        _ => MatchQuality::NoMatch,
    }
}

/// Transfer source info from original block to executed block.
fn transfer_block_source_info(original: &Block, executed: &mut Block) {
    match (original, executed) {
        (Block::Plain(o), Block::Plain(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Block::Paragraph(o), Block::Paragraph(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Block::LineBlock(o), Block::LineBlock(e)) => {
            e.source_info = o.source_info.clone();
            for (oc, ec) in o.content.iter().zip(e.content.iter_mut()) {
                transfer_inlines_source_info(oc, ec);
            }
        }
        (Block::CodeBlock(o), Block::CodeBlock(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
        }
        (Block::RawBlock(o), Block::RawBlock(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Block::BlockQuote(o), Block::BlockQuote(e)) => {
            e.source_info = o.source_info.clone();
            transfer_blocks_source_info(&o.content, &mut e.content);
        }
        (Block::OrderedList(o), Block::OrderedList(e)) => {
            e.source_info = o.source_info.clone();
            for (oc, ec) in o.content.iter().zip(e.content.iter_mut()) {
                transfer_blocks_source_info(oc, ec);
            }
        }
        (Block::BulletList(o), Block::BulletList(e)) => {
            e.source_info = o.source_info.clone();
            for (oc, ec) in o.content.iter().zip(e.content.iter_mut()) {
                transfer_blocks_source_info(oc, ec);
            }
        }
        (Block::DefinitionList(o), Block::DefinitionList(e)) => {
            e.source_info = o.source_info.clone();
            for ((oterm, odefs), (eterm, edefs)) in o.content.iter().zip(e.content.iter_mut()) {
                transfer_inlines_source_info(oterm, eterm);
                for (od, ed) in odefs.iter().zip(edefs.iter_mut()) {
                    transfer_blocks_source_info(od, ed);
                }
            }
        }
        (Block::Header(o), Block::Header(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Block::HorizontalRule(o), Block::HorizontalRule(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Block::Div(o), Block::Div(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            transfer_blocks_source_info(&o.content, &mut e.content);
        }
        (Block::Figure(o), Block::Figure(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            transfer_blocks_source_info(&o.content, &mut e.content);
        }
        (Block::Table(_), Block::Table(_)) => {
            // Tables are complex - skip for now
        }
        (Block::BlockMetadata(o), Block::BlockMetadata(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Block::NoteDefinitionPara(o), Block::NoteDefinitionPara(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Block::NoteDefinitionFencedBlock(o), Block::NoteDefinitionFencedBlock(e)) => {
            e.source_info = o.source_info.clone();
            transfer_blocks_source_info(&o.content, &mut e.content);
        }
        (Block::CaptionBlock(o), Block::CaptionBlock(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Block::Custom(_), Block::Custom(_)) => {
            // Custom nodes - skip for now
        }
        _ => {}
    }
}

/// Transfer source info from original blocks to executed blocks.
fn transfer_blocks_source_info(original: &[Block], executed: &mut [Block]) {
    for (o, e) in original.iter().zip(executed.iter_mut()) {
        transfer_block_source_info(o, e);
    }
}

/// Transfer source info from original inlines to executed inlines.
fn transfer_inlines_source_info(original: &Inlines, executed: &mut Inlines) {
    for (o, e) in original.iter().zip(executed.iter_mut()) {
        transfer_inline_source_info(o, e);
    }
}

/// Transfer source info from original inline to executed inline.
fn transfer_inline_source_info(original: &Inline, executed: &mut Inline) {
    match (original, executed) {
        (Inline::Str(o), Inline::Str(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Inline::Emph(o), Inline::Emph(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Underline(o), Inline::Underline(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Strong(o), Inline::Strong(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Strikeout(o), Inline::Strikeout(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Superscript(o), Inline::Superscript(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Subscript(o), Inline::Subscript(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::SmallCaps(o), Inline::SmallCaps(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Quoted(o), Inline::Quoted(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Cite(o), Inline::Cite(e)) => {
            e.source_info = o.source_info.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Code(o), Inline::Code(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
        }
        (Inline::Space(o), Inline::Space(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Inline::SoftBreak(o), Inline::SoftBreak(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Inline::LineBreak(o), Inline::LineBreak(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Inline::Math(o), Inline::Math(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Inline::RawInline(o), Inline::RawInline(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Inline::Link(o), Inline::Link(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            e.target_source = o.target_source.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Image(o), Inline::Image(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            e.target_source = o.target_source.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Note(o), Inline::Note(e)) => {
            e.source_info = o.source_info.clone();
            transfer_blocks_source_info(&o.content, &mut e.content);
        }
        (Inline::Span(o), Inline::Span(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Shortcode(_), Inline::Shortcode(_)) => {
            // Shortcodes shouldn't appear after desugaring
        }
        (Inline::NoteReference(o), Inline::NoteReference(e)) => {
            e.source_info = o.source_info.clone();
        }
        (Inline::Attr(_, o_source), Inline::Attr(_, e_source)) => {
            *e_source = o_source.clone();
        }
        (Inline::Insert(o), Inline::Insert(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Delete(o), Inline::Delete(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Highlight(o), Inline::Highlight(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::EditComment(o), Inline::EditComment(e)) => {
            e.source_info = o.source_info.clone();
            e.attr_source = o.attr_source.clone();
            transfer_inlines_source_info(&o.content, &mut e.content);
        }
        (Inline::Custom(_), Inline::Custom(_)) => {
            // Custom nodes - skip for now
        }
        _ => {}
    }
}

/// Reconcile children of a block when the block itself is a structural match.
fn reconcile_block_children(
    original: &Block,
    executed: &mut Block,
    report: &mut ReconciliationReport,
) {
    match (original, executed) {
        (Block::BlockQuote(o), Block::BlockQuote(e)) => {
            reconcile_blocks(&o.content, &mut e.content, report);
        }
        (Block::OrderedList(o), Block::OrderedList(e)) => {
            for (oc, ec) in o.content.iter().zip(e.content.iter_mut()) {
                reconcile_blocks(oc, ec, report);
            }
        }
        (Block::BulletList(o), Block::BulletList(e)) => {
            for (oc, ec) in o.content.iter().zip(e.content.iter_mut()) {
                reconcile_blocks(oc, ec, report);
            }
        }
        (Block::DefinitionList(o), Block::DefinitionList(e)) => {
            for ((_, odefs), (_, edefs)) in o.content.iter().zip(e.content.iter_mut()) {
                for (od, ed) in odefs.iter().zip(edefs.iter_mut()) {
                    reconcile_blocks(od, ed, report);
                }
            }
        }
        (Block::Div(o), Block::Div(e)) => {
            reconcile_blocks(&o.content, &mut e.content, report);
        }
        (Block::Figure(o), Block::Figure(e)) => {
            reconcile_blocks(&o.content, &mut e.content, report);
        }
        (Block::NoteDefinitionFencedBlock(o), Block::NoteDefinitionFencedBlock(e)) => {
            reconcile_blocks(&o.content, &mut e.content, report);
        }
        (Block::CodeBlock(o), Block::CodeBlock(e)) => {
            // For code blocks with changed content, transfer attr_source
            // (the cell options came from the original)
            e.attr_source = o.attr_source.clone();
        }
        _ => {
            // No children to reconcile for other types
        }
    }
}

// === Content Equality Functions ===

/// Compare two Attr values for equality (ignoring order of classes/attributes).
fn attrs_equal(a: &quarto_pandoc_types::attr::Attr, b: &quarto_pandoc_types::attr::Attr) -> bool {
    // Attr is (String, Vec<String>, LinkedHashMap<String, String>)
    // id, classes, key-value attributes
    a.0 == b.0 && a.1 == b.1 && a.2 == b.2
}

/// Compare inline content for equality, ignoring source locations.
fn inlines_content_equal(a: &Inlines, b: &Inlines) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(ai, bi)| inline_content_equal(ai, bi))
}

/// Compare two inlines for content equality, ignoring source locations.
fn inline_content_equal(a: &Inline, b: &Inline) -> bool {
    match (a, b) {
        (Inline::Str(a), Inline::Str(b)) => a.text == b.text,
        (Inline::Emph(a), Inline::Emph(b)) => inlines_content_equal(&a.content, &b.content),
        (Inline::Underline(a), Inline::Underline(b)) => {
            inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Strong(a), Inline::Strong(b)) => inlines_content_equal(&a.content, &b.content),
        (Inline::Strikeout(a), Inline::Strikeout(b)) => {
            inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Superscript(a), Inline::Superscript(b)) => {
            inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Subscript(a), Inline::Subscript(b)) => {
            inlines_content_equal(&a.content, &b.content)
        }
        (Inline::SmallCaps(a), Inline::SmallCaps(b)) => {
            inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Quoted(a), Inline::Quoted(b)) => {
            a.quote_type == b.quote_type && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Cite(a), Inline::Cite(b)) => {
            // Compare citation ids and content
            a.citations.len() == b.citations.len()
                && a.citations
                    .iter()
                    .zip(b.citations.iter())
                    .all(|(ac, bc)| ac.id == bc.id)
                && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Code(a), Inline::Code(b)) => attrs_equal(&a.attr, &b.attr) && a.text == b.text,
        (Inline::Space(_), Inline::Space(_)) => true,
        (Inline::SoftBreak(_), Inline::SoftBreak(_)) => true,
        (Inline::LineBreak(_), Inline::LineBreak(_)) => true,
        (Inline::Math(a), Inline::Math(b)) => a.math_type == b.math_type && a.text == b.text,
        (Inline::RawInline(a), Inline::RawInline(b)) => a.format == b.format && a.text == b.text,
        (Inline::Link(a), Inline::Link(b)) => {
            attrs_equal(&a.attr, &b.attr)
                && a.target == b.target
                && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Image(a), Inline::Image(b)) => {
            attrs_equal(&a.attr, &b.attr)
                && a.target == b.target
                && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Note(a), Inline::Note(b)) => blocks_content_equal(&a.content, &b.content),
        (Inline::Span(a), Inline::Span(b)) => {
            attrs_equal(&a.attr, &b.attr) && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Shortcode(a), Inline::Shortcode(b)) => {
            a.is_escaped == b.is_escaped
                && a.name == b.name
                && a.positional_args == b.positional_args
                && a.keyword_args == b.keyword_args
        }
        (Inline::NoteReference(a), Inline::NoteReference(b)) => a.id == b.id,
        (Inline::Attr(a_attr, _), Inline::Attr(b_attr, _)) => attrs_equal(a_attr, b_attr),
        (Inline::Insert(a), Inline::Insert(b)) => {
            attrs_equal(&a.attr, &b.attr) && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Delete(a), Inline::Delete(b)) => {
            attrs_equal(&a.attr, &b.attr) && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Highlight(a), Inline::Highlight(b)) => {
            attrs_equal(&a.attr, &b.attr) && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::EditComment(a), Inline::EditComment(b)) => {
            attrs_equal(&a.attr, &b.attr) && inlines_content_equal(&a.content, &b.content)
        }
        (Inline::Custom(a), Inline::Custom(b)) => {
            // Custom nodes - compare by debug representation for now
            format!("{:?}", a) == format!("{:?}", b)
        }
        _ => false,
    }
}

/// Compare block lists for content equality.
fn blocks_content_equal(a: &[Block], b: &[Block]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(ai, bi)| matches!(match_blocks(ai, bi), MatchQuality::Exact))
}

/// Compare list content (Vec<Blocks>) for equality.
fn lists_content_equal(a: &[Vec<Block>], b: &[Vec<Block>]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(ai, bi)| blocks_content_equal(ai, bi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::block::{CodeBlock, HorizontalRule, Paragraph};
    use quarto_pandoc_types::inline::Str;
    use quarto_source_map::{FileId, Location, Range, SourceInfo};

    fn make_source_info(offset: usize) -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset,
                    row: 0,
                    column: offset,
                },
                end: Location {
                    offset: offset + 1,
                    row: 0,
                    column: offset + 1,
                },
            },
        )
    }

    fn make_str(text: &str, offset: usize) -> Inline {
        Inline::Str(Str {
            text: text.to_string(),
            source_info: make_source_info(offset),
        })
    }

    fn make_paragraph(text: &str, offset: usize) -> Block {
        Block::Paragraph(Paragraph {
            content: vec![make_str(text, offset)],
            source_info: make_source_info(offset),
        })
    }

    #[test]
    fn test_match_quality_exact_paragraph() {
        let a = make_paragraph("Hello", 0);
        let b = make_paragraph("Hello", 100);

        assert_eq!(match_blocks(&a, &b), MatchQuality::Exact);
    }

    #[test]
    fn test_match_quality_no_match_different_content() {
        let a = make_paragraph("Hello", 0);
        let b = make_paragraph("World", 100);

        assert_eq!(match_blocks(&a, &b), MatchQuality::NoMatch);
    }

    #[test]
    fn test_match_quality_no_match_different_type() {
        let a = make_paragraph("Hello", 0);
        let b = Block::HorizontalRule(HorizontalRule {
            source_info: make_source_info(100),
        });

        assert_eq!(match_blocks(&a, &b), MatchQuality::NoMatch);
    }

    #[test]
    fn test_transfer_source_info() {
        let original = make_paragraph("Hello", 42);
        let mut executed = make_paragraph("Hello", 100);

        transfer_block_source_info(&original, &mut executed);

        if let Block::Paragraph(p) = executed {
            // Check that the source_info was transferred
            if let SourceInfo::Original { start_offset, .. } = &p.source_info {
                assert_eq!(*start_offset, 42);
            } else {
                panic!("Expected Original source info");
            }
        } else {
            panic!("Expected Paragraph");
        }
    }

    #[test]
    fn test_reconcile_identical_documents() {
        let original = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::null(SourceInfo::default()),
            blocks: vec![make_paragraph("Hello", 0), make_paragraph("World", 10)],
        };

        let mut executed = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::null(SourceInfo::default()),
            blocks: vec![make_paragraph("Hello", 100), make_paragraph("World", 110)],
        };

        let report = reconcile_source_locations(&original, &mut executed);

        assert_eq!(report.exact_matches, 2);
        assert_eq!(report.content_changes, 0);
        assert_eq!(report.additions, 0);
        assert_eq!(report.deletions, 0);

        // Verify source info was transferred
        if let Block::Paragraph(p) = &executed.blocks[0] {
            if let SourceInfo::Original { start_offset, .. } = &p.source_info {
                assert_eq!(*start_offset, 0);
            }
        }
    }

    #[test]
    fn test_reconcile_with_additions() {
        let original = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::null(SourceInfo::default()),
            blocks: vec![make_paragraph("Hello", 0)],
        };

        let mut executed = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::null(SourceInfo::default()),
            blocks: vec![
                make_paragraph("Hello", 100),
                make_paragraph("New content", 110), // Added by engine
            ],
        };

        let report = reconcile_source_locations(&original, &mut executed);

        assert_eq!(report.exact_matches, 1);
        assert_eq!(report.additions, 1);
    }

    #[test]
    fn test_reconcile_with_deletions() {
        let original = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::null(SourceInfo::default()),
            blocks: vec![
                make_paragraph("Hello", 0),
                make_paragraph("Deleted", 10), // Will be deleted
            ],
        };

        let mut executed = Pandoc {
            meta: quarto_pandoc_types::ConfigValue::null(SourceInfo::default()),
            blocks: vec![make_paragraph("Hello", 100)],
        };

        let report = reconcile_source_locations(&original, &mut executed);

        assert_eq!(report.exact_matches, 1);
        assert_eq!(report.deletions, 1);
    }

    #[test]
    fn test_code_block_structural_match() {
        use hashlink::LinkedHashMap;

        let original = Block::CodeBlock(CodeBlock {
            attr: (
                "cell1".to_string(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "print('hello')".to_string(),
            source_info: make_source_info(0),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        });

        let executed = Block::CodeBlock(CodeBlock {
            attr: (
                "cell1".to_string(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "# Output:\n# hello".to_string(), // Different content (execution output)
            source_info: make_source_info(100),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        });

        assert_eq!(
            match_blocks(&original, &executed),
            MatchQuality::StructuralOnly
        );
    }

    #[test]
    fn test_code_block_exact_match() {
        use hashlink::LinkedHashMap;

        let original = Block::CodeBlock(CodeBlock {
            attr: (
                "cell1".to_string(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "print('hello')".to_string(),
            source_info: make_source_info(0),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        });

        let executed = Block::CodeBlock(CodeBlock {
            attr: (
                "cell1".to_string(),
                vec!["python".to_string()],
                LinkedHashMap::new(),
            ),
            text: "print('hello')".to_string(), // Same content
            source_info: make_source_info(100),
            attr_source: quarto_pandoc_types::attr::AttrSourceInfo::empty(),
        });

        assert_eq!(match_blocks(&original, &executed), MatchQuality::Exact);
    }

    #[test]
    fn test_inline_content_equality() {
        let a = vec![make_str("Hello", 0), make_str("World", 10)];
        let b = vec![make_str("Hello", 100), make_str("World", 110)];
        let c = vec![make_str("Hello", 0), make_str("Different", 10)];

        assert!(inlines_content_equal(&a, &b));
        assert!(!inlines_content_equal(&a, &c));
    }

    #[test]
    fn test_report_total() {
        let report = ReconciliationReport {
            exact_matches: 5,
            content_changes: 2,
            deletions: 1,
            additions: 3,
        };

        assert_eq!(report.total(), 11);
    }
}
