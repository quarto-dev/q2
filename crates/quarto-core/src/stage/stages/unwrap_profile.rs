/*
 * stage/stages/unwrap_profile.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Unwrap an AtProfile bundle back into a DocumentAst.
 */

//! Unwrap [`DocumentAtProfile`] back to [`DocumentAst`].
//!
//! Sits immediately after [`DocumentProfileStage`] in the standard
//! pipeline. The two stages together realize Phase-0 Option A (see
//! `claude-notes/plans/2026-04-23-websites-phase-0.md` §Shape) while
//! keeping every downstream stage's input kind unchanged:
//!
//! ```text
//! … → MetadataMerge (DocumentAst)
//!         ↓
//!     DocumentProfile (DocumentAst → AtProfile)   ← checkpoint in the type
//!         ↓
//!     UnwrapProfile (AtProfile → DocumentAst)      ← keeps signatures
//!         ↓
//!     PreEngineSugaring (DocumentAst) → …
//! ```
//!
//! Phase 1's two-pass orchestrator will short-circuit before this stage
//! runs, keeping the `AtProfile` bundle to feed into project-wide
//! analysis, then resuming the tail of the pipeline for each file.
//!
//! [`DocumentProfileStage`]: crate::stage::stages::DocumentProfileStage

use async_trait::async_trait;

use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

/// Pipeline stage that takes the [`DocumentProfile`] out of an
/// [`DocumentAtProfile`] bundle and re-emits its `DocumentAst`.
///
/// Since bd-0rsk07il the profile is not discarded: it is **moved onto
/// [`StageContext::document_profile`]**, so callers that drive the
/// full pipeline (single-doc renders — where no orchestrator ever
/// sees the `AtProfile` bundle) can read the document's profile after
/// the run. `run_pipeline` bridges it back to
/// `RenderContext::document_profile`. This stage runs after
/// `LinkResolutionStage`, so the stashed profile carries
/// `body_link_targets` too.
///
/// The checkpoint semantics are unchanged:
///
/// 1. The `AtProfile` pipeline-data variant still proves the
///    checkpoint is honored in the type.
/// 2. Pipeline integration tests (see `crates/quarto-core/tests/
///    integration/document_profile_pipeline.rs`) tap the `AtProfile`
///    variant before this stage runs and verify clone-and-resume
///    produces byte-identical output. The Pass-1 orchestrator
///    short-circuits before this stage and keeps the bundle.
///
/// [`DocumentProfile`]: crate::document_profile::DocumentProfile
/// [`StageContext::document_profile`]: crate::stage::StageContext
pub struct UnwrapProfileStage;

impl UnwrapProfileStage {
    pub fn new() -> Self {
        Self
    }
}

impl Default for UnwrapProfileStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for UnwrapProfileStage {
    fn name(&self) -> &str {
        "unwrap-profile"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::AtProfile
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::AtProfile(bundle) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };
        // Stash rather than discard: single-doc callers read this via
        // the `run_pipeline` → `RenderContext` bridge (bd-0rsk07il).
        ctx.document_profile = Some(bundle.profile);
        Ok(PipelineData::DocumentAst(bundle.ast))
    }
}
