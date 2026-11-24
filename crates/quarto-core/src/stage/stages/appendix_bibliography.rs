/*
 * stage/stages/appendix_bibliography.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Move the bibliography citeproc produced into the HTML appendix.
 */

//! Move the bibliography into the appendix after the post user filters.
//!
//! ## Why this is a stage after user post filters (GH #1221)
//!
//! `AppendixStructureTransform` (Finalization phase, inside
//! `AstTransformsStage`) builds `#quarto-appendix` and wraps a `#refs` div
//! in a `#quarto-bibliography` section under a localized "References"
//! heading. But for a non-book HTML document the citeproc filter runs in
//! `UserFiltersStage::post`, *after* that transform, because Quarto 1 runs
//! citeproc after crossref. So the transform never sees the div, and
//! citeproc appends `#refs` as a bare block at the end of the document,
//! below any appendix. Quarto 1 never had this ordering problem: its appendix
//! is an HTML postprocessor that runs after Pandoc.
//!
//! This stage runs right after `UserFiltersStage::post` and calls
//! [`place_bibliography`]. That wraps a top-level `#refs` div exactly as the
//! transform would and slots it into the appendix before the footnotes. A
//! bibliography the transform already placed is nested inside the appendix,
//! so this stage leaves it alone. It runs in the HTML pipeline and the
//! q2-preview pipeline. The Pandoc-writer formats exclude it
//! (`PANDOC_STAGE_EXCLUDED`), so their output is unchanged. Typst's
//! bibliography placement is GH #1105.
//!
//! [`place_bibliography`]: crate::transforms::appendix::place_bibliography

use async_trait::async_trait;

use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use crate::transforms::appendix::place_bibliography;

/// Move a bibliography the post-bucket citeproc filter created into
/// `#quarto-appendix`. See the module docs.
pub struct AppendixBibliographyStage;

impl AppendixBibliographyStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AppendixBibliographyStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for AppendixBibliographyStage {
    fn name(&self) -> &str {
        "appendix-bibliography"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        if place_bibliography(&mut doc.ast) {
            trace_event!(
                ctx,
                EventLevel::Debug,
                "appendix-bibliography: moved the bibliography into the appendix"
            );
        }
        Ok(PipelineData::DocumentAst(doc))
    }
}
