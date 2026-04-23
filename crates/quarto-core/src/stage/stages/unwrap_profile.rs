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

/// Pipeline stage that drops the [`DocumentProfile`] from an
/// [`DocumentAtProfile`] bundle and re-emits its `DocumentAst`.
///
/// The discarded profile is still useful in Phase 0 in two ways:
///
/// 1. Its existence on the pipeline-data type proves the checkpoint
///    semantics are honored.
/// 2. Pipeline integration tests (see `crates/quarto-core/tests/
///    document_profile_pipeline.rs`) tap the `AtProfile` variant
///    before this stage runs and verify clone-and-resume produces
///    byte-identical output.
///
/// Phase 1 replaces this stage with a real project-orchestration
/// consumer that reads the profile *and* resumes the pipeline.
///
/// [`DocumentProfile`]: crate::document_profile::DocumentProfile
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
        _ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::AtProfile(bundle) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };
        Ok(PipelineData::DocumentAst(bundle.ast))
    }
}
