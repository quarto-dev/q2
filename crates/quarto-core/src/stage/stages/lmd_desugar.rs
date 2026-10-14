/*
 * stage/stages/lmd_desugar.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * lmd exploration spike: show the desugaring of `{lmd}` cells.
 */

//! `LmdDesugarStage`: rewrite every `` ```{lmd} `` cell in place into a
//! plain `lmd` code block whose text is the desugared Lua, marked
//! `lmd="pending"` for [`super::LmdExecuteStage`] to pick up after the
//! engine has run.
//!
//! The stage runs before engine execution so no engine is asked to run an
//! `lmd` cell: an unbraced `lmd` class is plain code to every engine. A
//! cell that fails to desugar (Lua syntax error, unsupported markdown node)
//! keeps its text, loses the braced class for the same reason, and is
//! reported as a warning. Spike 1 of
//! `claude-notes/plans/2026-10-12-lmd-exploration.md` stopped here, which
//! is why `q2 render --to markdown` of a document shows the desugared Lua
//! whenever the execute stage is absent or a cell fails to run.

use async_trait::async_trait;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::block::Block;
use quarto_pandoc_types::inline::Inline;
use quarto_source_map::SourceInfo;

use crate::ast_walk::{for_each_block_list_mut, for_each_inline_list_mut};
use crate::engine::capture_splice::engine_cell_lang;
use crate::lmd::{
    Anchor, DesugarOptions, EmitMode, LMD_LANG, LMD_PENDING_ATTR, LMD_PENDING_VALUE, LmdError,
    desugar_lua_expr_with, desugar_lua_with, inline_lmd_source,
};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use quarto_source_map::SourceContext;

#[derive(Default)]
pub struct LmdDesugarStage;

impl LmdDesugarStage {
    pub fn new() -> Self {
        Self
    }
}

/// A cell that could not be desugared and was left as written.
#[derive(Debug)]
pub struct LmdDesugarFailure {
    pub error: LmdError,
    /// The best location known: the offending line of the cell for a Lua
    /// syntax error when the cell's text is anchored exactly, the
    /// unsupported node's span when the fragment parse was anchored, the
    /// cell itself otherwise. Markdown parse errors carry the reader's
    /// own, rerooted diagnostics in the error instead.
    pub location: SourceInfo,
}

/// The best location for a desugar failure (see [`LmdDesugarFailure`]).
fn failure_location(
    error: &LmdError,
    anchor: &Anchor,
    text: &str,
    cell: &SourceInfo,
) -> SourceInfo {
    match error {
        LmdError::LuaParse { line, .. } => anchor.line_span(text, *line),
        LmdError::Unsupported { location, .. } => location.clone(),
        _ => None,
    }
    .unwrap_or_else(|| cell.clone())
}

/// Desugar every `{lmd}` cell in `blocks` in place, block cells and inline
/// cells alike, into the line-preserving form the execute stage runs.
/// `sources` anchors each cell's text in the document so fragment parses
/// and errors reroot into it. Returns one failure per cell that was left
/// as it was.
pub fn desugar_lmd_cells(
    blocks: &mut Vec<Block>,
    sources: &SourceContext,
) -> Vec<LmdDesugarFailure> {
    let mut errors = Vec::new();
    // Inline cells: `` `{lmd} expr` `` is a `Code` whose text carries the
    // tag. The tag is dropped, the expression desugared, and the node
    // marked; the execute stage evaluates it as an expression.
    for_each_inline_list_mut(blocks, &mut |list| {
        for inline in list.iter_mut() {
            let Inline::Code(code) = inline else {
                continue;
            };
            let Some(expr) = inline_lmd_source(&code.attr, &code.text) else {
                continue;
            };
            let anchor = Anchor::for_inline_cell(code, expr, sources);
            let opts = DesugarOptions {
                anchor: Some(&anchor),
                mode: EmitMode::LinePreserving,
            };
            match desugar_lua_expr_with(expr, 0, &opts) {
                Ok(lua) => {
                    code.text = lua;
                    code.attr
                        .2
                        .insert(LMD_PENDING_ATTR.to_string(), LMD_PENDING_VALUE.to_string());
                }
                Err(err) => {
                    let location = failure_location(&err, &anchor, expr, &code.source_info);
                    errors.push(LmdDesugarFailure {
                        error: err,
                        location,
                    });
                    // Left as written, but as plain `lmd` code: an engine
                    // must never be asked to run a cell lmd could not read.
                    code.text = expr.to_string();
                    code.attr.1.push(LMD_LANG.to_string());
                }
            }
        }
    });
    for_each_block_list_mut(blocks, &mut |list| {
        for block in list.iter_mut() {
            if engine_cell_lang(block) != Some(LMD_LANG) {
                continue;
            }
            let Block::CodeBlock(cell) = block else {
                continue;
            };
            let anchor = Anchor::for_block_cell(cell, sources);
            let opts = DesugarOptions {
                anchor: Some(&anchor),
                mode: EmitMode::LinePreserving,
            };
            match desugar_lua_with(&cell.text, &opts) {
                Ok(lua) => {
                    cell.text = lua;
                    let tag = format!("{{{LMD_LANG}}}");
                    for class in cell.attr.1.iter_mut() {
                        if *class == tag {
                            *class = LMD_LANG.to_string();
                        }
                    }
                    cell.attr
                        .2
                        .insert(LMD_PENDING_ATTR.to_string(), LMD_PENDING_VALUE.to_string());
                }
                Err(err) => {
                    let location = failure_location(&err, &anchor, &cell.text, &cell.source_info);
                    errors.push(LmdDesugarFailure {
                        error: err,
                        location,
                    });
                    // Left as written, but with the class un-braced: a
                    // braced `{lmd}` class is an unclaimed engine cell, and
                    // on a machine with no engine runtime that fails the
                    // whole render (CI caught this). The text stays so the
                    // author can read what did not parse.
                    let tag = format!("{{{LMD_LANG}}}");
                    for class in cell.attr.1.iter_mut() {
                        if *class == tag {
                            *class = LMD_LANG.to_string();
                        }
                    }
                }
            }
        }
    });
    errors
}

#[async_trait(?Send)]
impl PipelineStage for LmdDesugarStage {
    fn name(&self) -> &str {
        "lmd-desugar"
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

        let errors = desugar_lmd_cells(&mut doc.ast.blocks, &doc.source_context);
        for failure in errors {
            trace_event!(ctx, EventLevel::Warn, "{}", failure.error);
            // A fragment's markdown errors are the reader's own, already
            // rerooted into the document; report them as they are, under
            // the cell when one of them has no location.
            if let LmdError::MarkdownParse { diagnostics, .. } = failure.error {
                for mut diag in diagnostics {
                    if diag.location.is_none() {
                        diag.location = Some(failure.location.clone());
                    }
                    diag.hints.push(quarto_error_reporting::MessageContent::Markdown(
                        "Inside an lmd markdown fragment; the cell is left as written and will not run."
                            .to_string(),
                    ));
                    ctx.diagnostics.push(diag);
                }
                continue;
            }
            ctx.diagnostics.push(
                DiagnosticMessageBuilder::warning("lmd cell could not be desugared")
                    .with_location(failure.location)
                    .problem(failure.error.to_string())
                    .add_hint("The cell is left as written and will not run.")
                    .build(),
            );
        }

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(qmd: &str) -> (Vec<Block>, quarto_source_map::SourceContext) {
        let mut sink: Vec<u8> = Vec::new();
        let (ast, ctx, _warnings) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .expect("test document parses");
        (ast.blocks, ctx.source_context)
    }

    /// Resolve a span to its text in the test file.
    fn span_text<'a>(si: &SourceInfo, sources: &'a quarto_source_map::SourceContext) -> &'a str {
        let (fid, s, e) = si.resolve_byte_range().expect("span resolves");
        &sources
            .get_file(quarto_source_map::FileId(fid))
            .unwrap()
            .content
            .as_deref()
            .unwrap()[s..e]
    }

    fn code_block(block: &Block) -> &quarto_pandoc_types::block::CodeBlock {
        match block {
            Block::CodeBlock(cb) => cb,
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn lmd_cell_becomes_displayed_desugared_lua() {
        let (mut blocks, sources) =
            parse("::: foo\n\n```{lmd}\nreturn {= **hello** =}\n```\n\n:::\n");
        let errors = desugar_lmd_cells(&mut blocks, &sources);
        assert!(errors.is_empty(), "{errors:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div");
        };
        let cell = code_block(&div.content[0]);
        assert_eq!(cell.attr.1, vec!["lmd".to_string()]);
        assert_eq!(cell.attr.2.get("lmd").map(String::as_str), Some("pending"));
        assert!(engine_cell_lang(&div.content[0]).is_none());
        // Line-preserving form: the one-line fragment stays on one line.
        assert_eq!(
            cell.text,
            r#"return pandoc.Nodes({pandoc.Para({pandoc.Strong({pandoc.Str("hello")})})})"#
        );
        assert!(cell.text.contains("pandoc.Strong({"), "{}", cell.text);
        assert!(cell.text.contains("pandoc.Str(\"hello\")"), "{}", cell.text);
    }

    #[test]
    fn inline_cell_is_desugared_and_marked() {
        let (mut blocks, sources) = parse("Value `{lmd} f({=[ *x* ]=})` here and `plain` code.\n");
        let errors = desugar_lmd_cells(&mut blocks, &sources);
        assert!(errors.is_empty(), "{errors:?}");
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected Para");
        };
        let codes: Vec<&quarto_pandoc_types::inline::Code> = p
            .content
            .iter()
            .filter_map(|i| match i {
                Inline::Code(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(codes.len(), 2);
        assert_eq!(
            codes[0].text,
            r#"f(pandoc.Inlines({pandoc.Emph({pandoc.Str("x")})}))"#
        );
        assert_eq!(
            codes[0].attr.2.get("lmd").map(String::as_str),
            Some("pending")
        );
        assert_eq!(codes[1].text, "plain");
        assert!(codes[1].attr.2.get("lmd").is_none());
    }

    #[test]
    fn a_lua_syntax_error_is_located_on_its_line() {
        let (mut blocks, sources) =
            parse("Intro\n\n```{lmd}\nlocal x = 1\nreturn {= a =} +\n```\n");
        let errors = desugar_lmd_cells(&mut blocks, &sources);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(span_text(&errors[0].location, &sources), "return {= a =} +");
    }

    #[test]
    fn a_fragment_markdown_error_is_located_inside_the_fragment() {
        // Q-2-10 from the reader, rerooted: the bare apostrophe.
        let (mut blocks, sources) =
            parse("```{lmd}\nlocal x = 1\nreturn {=: the groups' Unique IDs :=}\n```\n");
        let errors = desugar_lmd_cells(&mut blocks, &sources);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let LmdError::MarkdownParse { diagnostics, .. } = &errors[0].error else {
            panic!("expected MarkdownParse, got {}", errors[0].error);
        };
        let loc = diagnostics[0].location.as_ref().expect("located");
        let mapped = loc.map_offset(0, &sources).expect("maps into the document");
        // Row 2 (0-based) is the `return` line; the apostrophe sits after `groups`.
        assert_eq!(mapped.location.row, 2, "{mapped:?}");
        // The reader's primary location is the space right after the
        // quote (the detail points at the quote itself): within a byte of
        // the apostrophe in `groups'`, in the document, not in the fragment.
        let file = sources.get_file(mapped.file_id).unwrap();
        let content = file.content.as_deref().unwrap();
        let off = mapped.location.offset;
        let near = &content[off.saturating_sub(1)..(off + 2).min(content.len())];
        assert!(near.contains('\''), "{near:?} at {off}");
    }

    #[test]
    fn an_unsupported_node_is_located_at_the_node() {
        let (mut blocks, sources) =
            parse("```{lmd}\nreturn {=:\n| a | b |\n|---|---|\n| 1 | 2 |\n:=}\n```\n");
        let errors = desugar_lmd_cells(&mut blocks, &sources);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(matches!(
            errors[0].error,
            LmdError::Unsupported { kind: "Table", .. }
        ));
        assert!(span_text(&errors[0].location, &sources).starts_with("| a | b |"));
    }

    #[test]
    fn a_bare_tag_mention_is_not_an_inline_cell() {
        let (mut blocks, sources) = parse("The `{lmd}` cell and the `{lmd} ` tag.\n");
        let before = blocks.clone();
        let errors = desugar_lmd_cells(&mut blocks, &sources);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(blocks, before);
    }

    #[test]
    fn other_cells_and_plain_code_are_untouched() {
        let src = "```{python}\n1 + 1\n```\n\n```lua\nreturn {= x =}\n```\n";
        let (mut blocks, sources) = parse(src);
        let before = blocks.clone();
        let errors = desugar_lmd_cells(&mut blocks, &sources);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(blocks, before);
    }

    #[test]
    fn a_broken_cell_keeps_its_text_but_is_no_longer_an_engine_cell() {
        let (mut blocks, sources) =
            parse("```{lmd}\nreturn {= a =} +\n```\n\nAnd `{lmd} {= b =} +` inline.\n");
        let errors = desugar_lmd_cells(&mut blocks, &sources);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(matches!(errors[0].error, LmdError::LuaParse { .. }));
        // The block cell: text as written, class un-braced, no marker.
        let cell = code_block(&blocks[0]);
        assert_eq!(cell.text, "return {= a =} +");
        assert_eq!(cell.attr.1, vec!["lmd".to_string()]);
        assert!(cell.attr.2.get("lmd").is_none());
        assert!(engine_cell_lang(&blocks[0]).is_none());
        // The inline cell: expression as written, tag moved to the class.
        let Block::Paragraph(p) = &blocks[1] else {
            panic!("expected Para");
        };
        let code = p
            .content
            .iter()
            .find_map(|i| match i {
                Inline::Code(c) => Some(c),
                _ => None,
            })
            .unwrap();
        assert_eq!(code.text, "{= b =} +");
        assert_eq!(code.attr.1, vec!["lmd".to_string()]);
        assert!(code.attr.2.get("lmd").is_none());
    }
}
