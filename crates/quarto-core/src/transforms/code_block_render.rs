/*
 * transforms/code_block_render.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Code-block decoration *Render* transform (Phase 0 skeleton).
//!
//! Format-specific half of the code-block decoration pipeline,
//! consuming the typed
//! [`CodeBlockDecoration`](super::code_block_generate::CodeBlockDecoration)
//! produced by
//! [`CodeBlockGenerateTransform`](super::code_block_generate::CodeBlockGenerateTransform).
//!
//! Today this transform is empty — Phase 0 lands the architectural
//! shape only. Phases 1 – 3 will fill it in:
//!
//! - **Phase 1 — filename header.** Wrap the `CodeBlock` in a
//!   `<div class="code-with-filename">` and emit the filename header.
//! - **Phase 2 — copy button.** Wrap in
//!   `<div class="code-copy-outer-scaffold">` plus the
//!   `<button class="code-copy-button">` element; add `code-with-copy`
//!   to the `<pre>` class list.
//! - **Phase 3 — code folding.** Wrap in
//!   `<details class="code-fold">` *outside* the filename wrapper
//!   (Q1's DecoratedCodeBlock composition rule).
//!
//! Pipeline placement: **Finalization Phase**, alongside
//! [`CrossrefRenderTransform`](super::CrossrefRenderTransform) and
//! [`AttributionRenderTransform`](super::AttributionRenderTransform).
//! Must run after every transform that creates or modifies code
//! blocks (e.g. shortcode expansion).

use quarto_pandoc_types::pandoc::Pandoc;

use crate::Result;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// See module docs.
pub struct CodeBlockRenderTransform;

impl CodeBlockRenderTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CodeBlockRenderTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for CodeBlockRenderTransform {
    fn name(&self) -> &str {
        "code-block-render"
    }

    async fn transform(&self, _ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        // Phase 0: no-op. The skeleton exists so the pipeline-ordering
        // tests can pin the Generate/Render shape; the actual wrapping
        // logic lands in Phases 1 – 3.
        Ok(())
    }
}
