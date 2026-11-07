/*
 * brace_diagram_cell_diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Warn about Quarto 1 `{mermaid}` / `{dot}` diagram cells.
 */

//! Brace-form diagram cell diagnostics (GH #1067).
//!
//! Quarto 1 draws diagrams from executable cells — ```` ```{mermaid} ````
//! and ```` ```{dot} ````. Quarto 2 only draws the plain fence
//! ```` ```mermaid ````; a brace-form cell used to fall through as a code
//! block whose class was the literal `{mermaid}`, with no warning. In a
//! migrated site every diagram silently became its source.
//!
//! This transform warns (`Q-2-58`); it does not rewrite. It runs after
//! engine execution, so a cell an engine *did* claim has already been
//! replaced by the engine's output and never reaches it. A project whose
//! own engine takes the brace form can therefore silence the warning
//! (`diagnostics: {Q-2-58: {level: off}}`) without it ever firing for the
//! cells that are handled.

use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_pandoc_types::{Block, Blocks, CodeBlock};

use crate::Result;
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// A Quarto 1 brace-form diagram cell q2 does not run.
const CODE_BRACE_DIAGRAM_CELL: &str = "Q-2-58";

/// Warns about `{mermaid}` / `{dot}` cells without modifying the AST.
pub struct BraceDiagramCellDiagnosticsTransform;

impl BraceDiagramCellDiagnosticsTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BraceDiagramCellDiagnosticsTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for BraceDiagramCellDiagnosticsTransform {
    fn name(&self) -> &str {
        "brace-diagram-cell-diagnostics"
    }

    fn phase(&self) -> TransformPhase {
        // Format-agnostic, reads the AST as parsed, mutates nothing.
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        let mut out = Vec::new();
        collect(&ast.blocks, &mut out);
        ctx.diagnostics.extend(out);
        Ok(())
    }
}

/// `mermaid` or `dot` when `cb` is a brace-form cell of that language.
fn brace_diagram_language(cb: &CodeBlock) -> Option<(usize, &'static str)> {
    cb.attr.1.iter().enumerate().find_map(|(i, class)| {
        match class.strip_prefix('{').and_then(|s| s.strip_suffix('}'))? {
            "mermaid" => Some((i, "mermaid")),
            "dot" => Some((i, "dot")),
            _ => None,
        }
    })
}

fn diagnostic(cb: &CodeBlock, index: usize, lang: &str) -> DiagnosticMessage {
    let location = cb
        .attr_source
        .classes
        .get(index)
        .and_then(|s| s.clone())
        .unwrap_or_else(|| cb.source_info.clone());
    let builder = DiagnosticMessageBuilder::warning("Quarto 1 diagram cell is not run")
        .with_code(CODE_BRACE_DIAGRAM_CELL)
        .with_location(location);
    if lang == "mermaid" {
        builder
            .problem("Quarto 2 does not run `{mermaid}` cells, so this block renders as a code block, not a diagram.")
            .add_hint("Drop the braces: write ```` ```mermaid ````. The diagram body and `%%|` options need no other change.")
            .build()
    } else {
        builder
            .problem("Quarto 2 does not run `{dot}` cells and has no Graphviz support yet, so this block renders as a code block, not a diagram.")
            .add_hint("To silence this warning when an engine handles `{dot}` cells, set `diagnostics: {Q-2-58: off}`.")
            .build()
    }
}

fn collect(blocks: &Blocks, out: &mut Vec<DiagnosticMessage>) {
    for block in blocks {
        match block {
            Block::CodeBlock(cb) => {
                if let Some((i, lang)) = brace_diagram_language(cb) {
                    out.push(diagnostic(cb, i, lang));
                }
            }
            Block::BlockQuote(bq) => collect(&bq.content, out),
            Block::Div(div) => collect(&div.content, out),
            Block::Figure(fig) => collect(&fig.content, out),
            Block::OrderedList(list) => list.content.iter().for_each(|b| collect(b, out)),
            Block::BulletList(list) => list.content.iter().for_each(|b| collect(b, out)),
            Block::DefinitionList(dl) => dl
                .content
                .iter()
                .flat_map(|(_term, defs)| defs.iter())
                .for_each(|b| collect(b, out)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{AttrSourceInfo, Div};
    use quarto_source_map::SourceInfo;

    fn cell(class: &str) -> Block {
        Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![class.to_string()], Default::default()),
            text: "A --> B".to_string(),
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        })
    }

    fn codes(blocks: &Blocks) -> Vec<Option<String>> {
        let mut out = Vec::new();
        collect(blocks, &mut out);
        out.into_iter().map(|d| d.code).collect()
    }

    #[test]
    fn brace_mermaid_and_dot_warn() {
        for class in ["{mermaid}", "{dot}"] {
            assert_eq!(codes(&vec![cell(class)]), vec![Some("Q-2-58".to_string())]);
        }
    }

    #[test]
    fn plain_fences_and_other_cells_do_not_warn() {
        for class in ["mermaid", "dot", "{python}", "{{mermaid}}", "{ojs}"] {
            assert!(codes(&vec![cell(class)]).is_empty(), "{class}");
        }
    }

    #[test]
    fn nested_cells_warn() {
        let div = Block::Div(Div {
            attr: Default::default(),
            content: vec![cell("{mermaid}")],
            source_info: SourceInfo::for_test(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(codes(&vec![div]).len(), 1);
    }
}
