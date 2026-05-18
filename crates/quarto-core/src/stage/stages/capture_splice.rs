/*
 * stage/stages/capture_splice.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Pipeline stage that splices server-recorded engine output into the
 * live pre-engine AST for q2 preview (bd-lucp).
 */

//! Capture-splice stage for `q2 preview` (bd-lucp).
//!
//! Sits between [`PreEngineSugaringStage`](super::PreEngineSugaringStage)
//! and [`EngineExecutionStage`](super::EngineExecutionStage) in the
//! q2-preview pipeline. When configured with an
//! [`EngineCapture`](quarto_trace::EngineCapture), it splices the
//! captured engine output blocks into the current pipeline AST,
//! leaving the rest of the pipeline to render an "as if executed"
//! document. When no capture is configured, the stage is a clean
//! pass-through.
//!
//! See `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`
//! for the architecture rationale: preview-time capture consumption
//! deliberately bypasses [`ReplayEngine`](crate::engine::ReplayEngine)'s
//! strict byte-equality miss policy in favor of AST-level splicing
//! keyed by `(structural_hash(cell), occurrence_index)`. `ReplayEngine`
//! itself is unchanged — it remains the deterministic regression-
//! testing tool bd-45yw designed.
//!
//! ## Post-splice behaviour of [`EngineExecutionStage`]
//!
//! After the splice runs, the AST's `{r}` / `{python}` / … code-blocks
//! have been replaced by `Div.cell` wrappers. `EngineExecutionStage`'s
//! engine detection still reads the document metadata (which still
//! says `engine: knitr`) and tries to look up an engine. In WASM the
//! lookup falls back to the markdown engine (no-op), which leaves the
//! spliced AST unchanged. On native, a real knitr/jupyter could in
//! principle re-execute, but the q2-preview pipeline isn't intended
//! to be run natively with a real engine after the splice; the native
//! use case is `q2 render`, which never goes through this stage.

use async_trait::async_trait;

use quarto_trace::EngineCapture;

use crate::engine::capture_splice::apply_capture_splice;
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;

/// Splices recorded engine output into the live pre-engine AST.
///
/// Created with [`CaptureSpliceStage::new`] and [`with_capture`]; the
/// q2-preview pipeline builder inserts it after `PreEngineSugaringStage`
/// and before `EngineExecutionStage`.
///
/// [`with_capture`]: CaptureSpliceStage::with_capture
pub struct CaptureSpliceStage {
    capture: Option<EngineCapture>,
}

impl CaptureSpliceStage {
    /// Build a splice stage with no capture attached. The stage runs
    /// as a clean pass-through. Used when the q2-preview pipeline is
    /// built without a recorded capture (no server-side execution
    /// happened yet, or the user has not opened a project with
    /// engine cells).
    pub fn new() -> Self {
        Self { capture: None }
    }

    /// Attach a recorded capture. On every per-doc run, the stage
    /// re-parses `capture.input_qmd` and the markdown field of
    /// `capture.result`, derives a cell-output map, and splices it
    /// onto the current AST. The capture is held by value because
    /// the stage may run on multiple pipeline executions (e.g. the
    /// q2-preview project pipeline runs once per active page).
    pub fn with_capture(mut self, capture: EngineCapture) -> Self {
        self.capture = Some(capture);
        self
    }
}

impl Default for CaptureSpliceStage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait(?Send)]
impl PipelineStage for CaptureSpliceStage {
    fn name(&self) -> &str {
        "capture-splice"
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
        let PipelineData::DocumentAst(mut doc_ast) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        let Some(capture) = self.capture.as_ref() else {
            trace_event!(ctx, EventLevel::Debug, "no capture attached; pass-through");
            return Ok(PipelineData::DocumentAst(doc_ast));
        };

        // Extract result.markdown from the opaque JSON. We don't need
        // the rest of the ExecuteResult shape here (filters, includes,
        // supporting_files) — those are engine-side concerns the
        // splice doesn't reproduce in the AST. Future work could
        // surface them through StageContext if a real splice consumer
        // needs them.
        let result_markdown = capture
            .result
            .get("markdown")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Parse both QMD strings via the same pampa reader the rest
        // of the pipeline uses. Slot in throwaway names — the parsed
        // ASTs are immediately consumed by the splice, so source-
        // attribution doesn't escape this stage.
        let (a1, _, _a1_warnings) = match pampa::readers::qmd::read(
            capture.input_qmd.as_bytes(),
            false,
            "capture-input.rmarkdown",
            &mut std::io::sink(),
            false, // don't track source locations — splice ignores them
            None,
        ) {
            Ok(t) => t,
            Err(_) => {
                // Parse failure: degrade to pass-through so a corrupt
                // capture can't take the preview down.
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to parse capture.input_qmd; falling through"
                );
                return Ok(PipelineData::DocumentAst(doc_ast));
            }
        };
        let (b1, _, _b1_warnings) = match pampa::readers::qmd::read(
            result_markdown.as_bytes(),
            false,
            "capture-result.md",
            &mut std::io::sink(),
            false,
            None,
        ) {
            Ok(t) => t,
            Err(_) => {
                trace_event!(
                    ctx,
                    EventLevel::Warn,
                    "failed to parse capture.result.markdown; falling through"
                );
                return Ok(PipelineData::DocumentAst(doc_ast));
            }
        };

        let before_blocks = doc_ast.ast.blocks.len();
        doc_ast.ast = apply_capture_splice(doc_ast.ast, &a1, &b1, &capture.engine_name);
        let after_blocks = doc_ast.ast.blocks.len();

        trace_event!(
            ctx,
            EventLevel::Debug,
            "capture-splice: engine={}, blocks {}→{}",
            capture.engine_name,
            before_blocks,
            after_blocks
        );

        Ok(PipelineData::DocumentAst(doc_ast))
    }
}
