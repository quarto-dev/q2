//! `UnexecutedCellCountStage`: counts the engine cells still in the AST and
//! records the number in [`StageContext::unexecuted_cells`].
//!
//! The browser has no engines, so a pandoc-request render shows a code cell's
//! output only when the capture splice replaced the cell with it. The stage
//! sits right after the splice (and before `ast-transforms`, which rewrites
//! classes), so every cell it still sees is one without a cached result. It is
//! only in the pandoc-request stage list; native renders execute their cells.

use async_trait::async_trait;
use quarto_pandoc_types::block::{Block, Blocks};

use crate::engine::capture_splice::engine_cell_lang;
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

#[derive(Default)]
pub struct UnexecutedCellCountStage;

impl UnexecutedCellCountStage {
    pub fn new() -> Self {
        Self
    }
}

/// Count engine cells (see [`engine_cell_lang`]) anywhere in `blocks`,
/// descending into containers. Extend the arms if `Block` gains a container.
pub fn count_engine_cells(blocks: &Blocks) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            Block::CodeBlock(_) => usize::from(engine_cell_lang(block).is_some()),
            Block::BlockQuote(bq) => count_engine_cells(&bq.content),
            Block::Div(div) => count_engine_cells(&div.content),
            Block::Figure(fig) => count_engine_cells(&fig.content),
            Block::OrderedList(list) => list.content.iter().map(count_engine_cells).sum(),
            Block::BulletList(list) => list.content.iter().map(count_engine_cells).sum(),
            Block::DefinitionList(dl) => dl
                .content
                .iter()
                .flat_map(|(_term, defs)| defs.iter())
                .map(count_engine_cells)
                .sum(),
            _ => 0,
        })
        .sum()
}

#[async_trait(?Send)]
impl PipelineStage for UnexecutedCellCountStage {
    fn name(&self) -> &str {
        "unexecuted-cell-count"
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
        let PipelineData::DocumentAst(doc) = &input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };
        ctx.unexecuted_cells = count_engine_cells(&doc.ast.blocks);
        Ok(input)
    }
}
