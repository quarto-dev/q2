//! `PandocPrepareStage`: the ungated, host-independent half of the pandoc
//! write step. It runs in the pipeline on every target (native tests and
//! wasm alike), builds a [`PandocRequest`] and leaves it in
//! [`StageContext::pandoc_request`], which `run_pipeline` restores into
//! [`crate::render::RenderContext::pandoc_request`]. Its output kind is an
//! empty [`RenderedOutput`], like `PandocWriteStage`'s.
//!
//! (A `PipelineData` variant carrying the request is the rejected
//! alternative: it touches `PipelineDataKind` and every match on it.)

use async_trait::async_trait;

use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

#[derive(Default)]
pub struct PandocPrepareStage;

impl PandocPrepareStage {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait(?Send)]
impl PipelineStage for PandocPrepareStage {
    fn name(&self) -> &str {
        "pandoc-prepare"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::RenderedOutput
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        use super::pandoc_write::{PandocWriteStage, TypstPrepInputs};
        use crate::pandoc_request::PrepareOptions;
        use crate::stage::RenderedOutput;

        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };
        let opts = match &ctx.prepare_options {
            Some(opts) => opts.clone(),
            None => PrepareOptions::native(ctx.temp_dir()?.to_path_buf()),
        };
        // No typst pre-step here: R4 brings the typst formats to the seam.
        let prepared =
            PandocWriteStage::new().prepare(&mut doc, ctx, &opts, &TypstPrepInputs::default())?;
        ctx.add_diagnostics(prepared.diagnostics);
        ctx.pandoc_request = Some(prepared.request);
        Ok(PipelineData::RenderedOutput(RenderedOutput {
            input_path: doc.path,
            output_path: prepared.output_path,
            format: ctx.format.clone(),
            content: String::new(),
            is_intermediate: prepared.is_intermediate,
            supporting_files: vec![],
            metadata: doc.ast.meta,
            source_context: doc.source_context,
        }))
    }
}
