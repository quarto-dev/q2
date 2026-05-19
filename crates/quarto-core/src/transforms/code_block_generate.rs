/*
 * transforms/code_block_generate.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Code-block decoration *Generate* transform (Phase 0 skeleton).
//!
//! Format-agnostic half of the code-block decoration pipeline. Walks
//! every [`CodeBlock`](quarto_pandoc_types::block::CodeBlock) in the
//! document, parses its attributes plus the relevant document-level
//! defaults from `ast.meta`, and produces a typed
//! [`CodeBlockDecoration`] payload that [`CodeBlockRenderTransform`]
//! (see [`super::code_block_render`]) consumes.
//!
//! Today the payload is empty — this transform exists to lock in the
//! pipeline placement and the Generate/Render shape before Phases 1–3
//! (filename / copy / fold) add real fields. The walk runs in
//! `O(blocks)` and produces no AST mutation; for Phase 0 it is a
//! deliberate no-op end-to-end.
//!
//! Pipeline placement: **Normalization Phase**, after
//! [`MetadataNormalizeTransform`](super::MetadataNormalizeTransform)
//! so document-level defaults (e.g. `code-copy: true`) are visible
//! when computing per-block decorations.
//!
//! See `claude-notes/plans/2026-05-19-code-block-features.md` for the
//! full epic plan and the Phase 0 acceptance criteria.

use quarto_pandoc_types::block::{Block, Blocks};
use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Typed payload carrying everything format renderers need to wrap a
/// `CodeBlock` in the right outer structure (filename header, copy
/// button, fold details, etc.).
///
/// Phase 0: fields land in Phases 1 – 3 as the corresponding features
/// arrive. Kept as a dedicated struct (rather than a kv on the
/// `CodeBlock` attrs) so the data is typed at the Generate/Render
/// boundary and the storage decision (sideband vs. CustomNode) is
/// localized here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodeBlockDecoration {
    // Phase 1: pub filename: Option<String>
    // Phase 2: pub copy: CopyMode
    // Phase 3: pub fold: FoldMode, pub summary: Option<String>
}

/// See module docs.
pub struct CodeBlockGenerateTransform;

impl CodeBlockGenerateTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeBlockGenerateTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CodeBlockGenerateTransform {
    fn name(&self) -> &str {
        "code-block-generate"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Phase 0: walk the AST so the traversal cost is paid here
        // rather than added later. The walker is shaped to mutate
        // blocks (Phases 1+ will need to read attrs and may rewrite
        // them or attach sideband state), so we use `iter_mut`.
        walk_blocks_mut(&mut ast.blocks, &mut |_block| {
            // No-op for Phase 0. Future phases will:
            //   1. Read CodeBlock.attr.2 (kv map) for `filename`,
            //      `code-fold`, `code-summary`, `code-copy`, etc.
            //   2. Combine with doc-level defaults from `ast.meta`.
            //   3. Construct a CodeBlockDecoration and store it
            //      somewhere (CustomNode wrapper vs sideband map —
            //      decision deferred to Phase 1 where there's an
            //      actual consumer).
        });
        Ok(())
    }
}

/// Walk every `CodeBlock` in the document, descending into containers
/// (BlockQuote, Div, list items, table cells, figure body, …) so
/// decorations attach to nested code blocks too.
///
/// Visits every `Block::CodeBlock` reachable from `blocks` exactly
/// once. Containers we walk into are kept in sync with the structural
/// shape of `Block` — additions to that enum that can contain code
/// blocks must extend the match arms here.
fn walk_blocks_mut(blocks: &mut Blocks, f: &mut impl FnMut(&mut Block)) {
    for block in blocks.iter_mut() {
        match block {
            Block::CodeBlock(_) => f(block),
            Block::BlockQuote(bq) => walk_blocks_mut(&mut bq.content, f),
            Block::Div(div) => walk_blocks_mut(&mut div.content, f),
            Block::Figure(fig) => walk_blocks_mut(&mut fig.content, f),
            Block::OrderedList(list) => {
                for item in list.content.iter_mut() {
                    walk_blocks_mut(item, f);
                }
            }
            Block::BulletList(list) => {
                for item in list.content.iter_mut() {
                    walk_blocks_mut(item, f);
                }
            }
            Block::DefinitionList(dl) => {
                for (_term, defs) in dl.content.iter_mut() {
                    for def in defs.iter_mut() {
                        walk_blocks_mut(def, f);
                    }
                }
            }
            // Other block variants cannot contain a CodeBlock as a
            // direct child. Header / Paragraph / Plain / LineBlock /
            // RawBlock / HorizontalRule / Table / MetaBlock / Note
            // variants / CaptionBlock / Custom are all leaf-ish from
            // the perspective of block-level code-block discovery.
            _ => {}
        }
    }
}
