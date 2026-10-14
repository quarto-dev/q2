/*
 * stage/stages/lmd_execute.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * lmd exploration spikes 2, 3 and 7: run desugared `{lmd}` cells.
 */

//! `LmdExecuteStage`: run every cell `LmdDesugarStage` marked, in document
//! order, in one Lua state per document, and replace each cell with what it
//! produced.
//!
//! Sits after engine execution and right before `user-filters-pre`, so a
//! cell sees engine output and lives where the eventual "a cell is a
//! filter" story lives. The contracts for what a cell may produce are in
//! `pampa::lua::lmd`: a block cell returns a `Block`, a list of `Block`s,
//! or `nil` (the cell vanishes); an inline cell (`` `{lmd} expr` `` in a
//! paragraph) evaluates to an `Inline`, a list of `Inline`s, a string, or
//! `nil`. Block and inline cells share the state and run in one document
//! order, so a paragraph's inline cell sees the globals of the block cell
//! above it.
//!
//! Cell options use Lua comment syntax, `--| key: value`, at the top of a
//! block cell. `echo: true` keeps the desugared Lua visible as a plain
//! `lmd` code block above the result (off by default). A cell that fails,
//! at compile time, at run time, or by producing the wrong kind of value,
//! is reported as an error diagnostic and left in the document as a
//! displayed `lmd` code block or code span, so the rest of the page still
//! renders and the failing Lua can be read.
//!
//! Lua errors name document lines. Each cell runs under the chunk name
//! `@<path of the file it came from>` with as many newlines prepended as
//! the cell's text starts lines into that file, so Lua's own `path:line:`
//! in messages and tracebacks is the document position; the desugarer's
//! line-preserving output keeps that true across fragment expansions. The
//! diagnostic takes the first such position as its location, so the
//! underline lands on the line. Cells from an included file name the
//! included file, since include expansion registers it in the source
//! context under its own id.

use async_trait::async_trait;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, CodeBlock};
use quarto_pandoc_types::inline::{Code, Inline, Inlines};
use quarto_source_map::{FileId, SourceContext, SourceInfo};
use std::collections::HashMap;

use crate::ast_walk::{
    ListVisitor, block_children_mut, for_each_block_list_mut, for_each_inline_list_mut,
    inline_children_mut,
};
use crate::cell_options::{options_to_config, partition_cell_options};
use crate::lmd::anchor::file_line_span;
use crate::lmd::{Anchor, LMD_LANG, LMD_PENDING_ATTR, LMD_PENDING_VALUE};
use crate::stage::{
    EventLevel, PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext,
};
use crate::trace_event;
use pampa::lua::lmd::{LmdCellError, LmdCellValue, LmdInlineValue, LmdRuntime};

#[derive(Default)]
pub struct LmdExecuteStage;

impl LmdExecuteStage {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Block,
    Inline,
}

/// A marked cell found by the pre-order walk. Its marker attribute has
/// been overwritten with `id`, which the splice pass looks for.
struct PendingCell {
    id: usize,
    kind: CellKind,
    text: String,
    source_info: SourceInfo,
    /// Where `text` lives in the document (see [`Anchor`]).
    anchor: Anchor,
}

/// What a cell produced, ready to splice.
enum CellOutput {
    Blocks(Vec<Block>),
    Inlines(Vec<Inline>),
}

fn pending_marker(attrs: &hashlink::LinkedHashMap<String, String>) -> bool {
    attrs.get(LMD_PENDING_ATTR).map(String::as_str) == Some(LMD_PENDING_VALUE)
}

fn is_pending_block_cell(block: &Block) -> bool {
    matches!(block, Block::CodeBlock(cb) if pending_marker(&cb.attr.2))
}

fn is_pending_inline_cell(inline: &Inline) -> bool {
    matches!(inline, Inline::Code(code) if pending_marker(&code.attr.2))
}

/// Pre-order walk that numbers marked cells in document order, block and
/// inline cells together, overwriting each one's marker with its number.
struct Numberer<'a> {
    cells: &'a mut Vec<PendingCell>,
    sources: &'a SourceContext,
}

impl Numberer<'_> {
    fn record(
        &mut self,
        kind: CellKind,
        text: &str,
        source_info: &SourceInfo,
        anchor: Anchor,
    ) -> String {
        let id = self.cells.len();
        self.cells.push(PendingCell {
            id,
            kind,
            text: text.to_string(),
            source_info: source_info.clone(),
            anchor,
        });
        id.to_string()
    }
}

impl ListVisitor for Numberer<'_> {
    fn blocks(&mut self, list: &mut Vec<Block>) {
        for block in list.iter_mut() {
            if is_pending_block_cell(block) {
                let Block::CodeBlock(cb) = block else {
                    unreachable!("is_pending_block_cell matched a CodeBlock");
                };
                let anchor = Anchor::for_block_cell(cb, self.sources);
                let id = self.record(CellKind::Block, &cb.text, &cb.source_info, anchor);
                cb.attr.2.insert(LMD_PENDING_ATTR.to_string(), id);
                continue;
            }
            block_children_mut(block, self);
        }
    }

    fn inlines(&mut self, list: &mut Inlines) {
        for inline in list.iter_mut() {
            if is_pending_inline_cell(inline) {
                let Inline::Code(code) = inline else {
                    unreachable!("is_pending_inline_cell matched a Code");
                };
                let anchor = Anchor::for_inline_cell(code, &code.text, self.sources);
                let id = self.record(CellKind::Inline, &code.text, &code.source_info, anchor);
                code.attr.2.insert(LMD_PENDING_ATTR.to_string(), id);
                continue;
            }
            inline_children_mut(inline, self);
        }
    }
}

/// The displayed form of a block cell: a plain `lmd` code block holding
/// its (option-stripped) desugared Lua. Used for `echo: true` and for
/// failures.
fn displayed_block_cell(code: &str, source_info: &SourceInfo) -> Block {
    Block::CodeBlock(CodeBlock {
        attr: (
            String::new(),
            vec![LMD_LANG.to_string()],
            Default::default(),
        ),
        text: code.to_string(),
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// The displayed form of a failed inline cell: a plain `lmd` code span
/// holding its desugared expression.
fn displayed_inline_cell(expr: &str, source_info: &SourceInfo) -> Inline {
    Inline::Code(Code {
        attr: (
            String::new(),
            vec![LMD_LANG.to_string()],
            Default::default(),
        ),
        text: expr.to_string(),
        source_info: source_info.clone(),
        attr_source: AttrSourceInfo::empty(),
    })
}

fn cell_error(
    title: &str,
    location: SourceInfo,
    problem: String,
    hint: Option<&str>,
) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error(title)
        .with_location(location)
        .problem(problem);
    if let Some(hint) = hint {
        builder = builder.add_hint(hint);
    }
    builder.build()
}

/// How a cell is named to Lua, and how its lines map to the document.
struct ChunkIdentity {
    /// `@path` when the cell's file is known, else a cell number.
    name: String,
    /// Newlines to prepend so chunk lines are document lines.
    padding: usize,
    /// The file, when known, for locating `path:LINE:` positions.
    file: Option<(FileId, String)>,
}

impl ChunkIdentity {
    /// `extra_lines` are lines of the cell text that are not run (the
    /// option lines a block cell starts with).
    fn for_cell(cell: &PendingCell, sources: &SourceContext, extra_lines: usize) -> Self {
        match cell.anchor.start(sources) {
            Some((file_id, path, start_line)) => Self {
                name: format!("@{path}"),
                padding: start_line - 1 + extra_lines,
                file: Some((file_id, path)),
            },
            None => Self {
                name: format!("lmd cell {}", cell.id + 1),
                padding: 0,
                file: None,
            },
        }
    }

    /// The document line of the first `path:LINE:` in a Lua error, as the
    /// line's span. Lua abbreviates a chunk name longer than its `LUA_IDSIZE`
    /// (60 bytes) to `...` plus a tail, so the match is on the file name,
    /// which any such tail still ends with.
    fn locate(&self, message: &str, sources: &SourceContext) -> Option<SourceInfo> {
        let (file_id, path) = self.file.as_ref()?;
        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        let needle = format!("{file_name}:");
        let mut rest = message;
        while let Some(i) = rest.find(&needle) {
            let after = &rest[i + needle.len()..];
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() && after[digits.len()..].starts_with(':') {
                let line: usize = digits.parse().ok()?;
                return file_line_span(sources, *file_id, line);
            }
            rest = &rest[i + needle.len()..];
        }
        None
    }
}

fn failure_diagnostic(
    cell: &PendingCell,
    err: &LmdCellError,
    location: Option<SourceInfo>,
) -> DiagnosticMessage {
    let (title, hint) = match err {
        LmdCellError::Lua(_) => ("lmd cell failed", None),
        LmdCellError::NotBlocks { .. } => (
            "lmd cell did not return blocks",
            Some(
                "Return a Block, a list of Blocks, or nil. Write `{=: … :=}` for a markdown fragment that is blocks.",
            ),
        ),
        LmdCellError::NotInlines { .. } => (
            "inline lmd cell did not produce inlines",
            Some(
                "An inline cell's expression must be an Inline, a list of Inlines, a string, or nil. Write `{=[ … ]=}` for a markdown fragment that is inlines.",
            ),
        ),
    };
    let location = location.unwrap_or_else(|| cell.anchor.span.clone());
    cell_error(title, location, err.to_string(), hint)
}

/// Split a block cell's text into its code, the `echo` option, and the
/// number of option lines stripped (they still count toward line numbers).
fn block_cell_code_and_echo(
    cell: &PendingCell,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> (String, bool, usize) {
    // Cell options use Lua's comment syntax; the language key is `lua`
    // because `lmd` is unknown to the comment table and would default
    // to `#`. The anchor is the cell *body*, so option-line spans land on
    // the option lines.
    match partition_cell_options("lua", &cell.text, cell.anchor.span.clone()) {
        Ok(partitioned) => {
            let echo =
                partitioned
                    .options
                    .map(options_to_config)
                    .is_some_and(|(config, option_diags)| {
                        diagnostics.extend(option_diags);
                        config
                            .get("echo")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                    });
            let option_lines = cell.text[..partitioned.options_len].matches('\n').count();
            (partitioned.code, echo, option_lines)
        }
        Err(err) => {
            let location = err
                .location()
                .cloned()
                .unwrap_or_else(|| cell.anchor.span.clone());
            diagnostics.push(cell_error(
                "lmd cell options are not valid",
                location,
                err.to_string(),
                None,
            ));
            (cell.text.clone(), false, 0)
        }
    }
}

/// Run every marked cell in `blocks` and splice the results in place.
/// `sources` anchors each cell in the document (chunk names, line
/// numbers, diagnostic locations). Diagnostics (cell failures,
/// `quarto.warn` calls) go to `diagnostics`.
pub async fn execute_lmd_cells(
    blocks: &mut Vec<Block>,
    runtime: &LmdRuntime,
    sources: &SourceContext,
    diagnostics: &mut Vec<DiagnosticMessage>,
) {
    let mut cells = Vec::new();
    Numberer {
        cells: &mut cells,
        sources,
    }
    .blocks(blocks);
    if cells.is_empty() {
        return;
    }

    let mut results: HashMap<usize, CellOutput> = HashMap::new();
    for cell in &cells {
        let output = match cell.kind {
            CellKind::Block => {
                let (code, echo, option_lines) = block_cell_code_and_echo(cell, diagnostics);
                let chunk = ChunkIdentity::for_cell(cell, sources, option_lines);
                let mut out: Vec<Block> = Vec::new();
                match runtime.run_cell(&code, &chunk.name, chunk.padding).await {
                    Ok(value) => {
                        if echo {
                            out.push(displayed_block_cell(&code, &cell.source_info));
                        }
                        if let LmdCellValue::Blocks(result) = value {
                            out.extend(result);
                        }
                    }
                    Err(err) => {
                        let at = chunk.locate(&err.to_string(), sources);
                        diagnostics.push(failure_diagnostic(cell, &err, at));
                        out.push(displayed_block_cell(&code, &cell.source_info));
                    }
                }
                CellOutput::Blocks(out)
            }
            CellKind::Inline => {
                let chunk = ChunkIdentity::for_cell(cell, sources, 0);
                match runtime
                    .run_inline_cell(&cell.text, &chunk.name, chunk.padding)
                    .await
                {
                    Ok(LmdInlineValue::Nothing) => CellOutput::Inlines(Vec::new()),
                    Ok(LmdInlineValue::Inlines(result)) => CellOutput::Inlines(result),
                    Err(err) => {
                        // The code span is on the reported line and tighter
                        // than the line, so it stays the location.
                        diagnostics.push(failure_diagnostic(cell, &err, None));
                        CellOutput::Inlines(vec![displayed_inline_cell(
                            &cell.text,
                            &cell.source_info,
                        )])
                    }
                }
            }
        };
        results.insert(cell.id, output);
    }
    diagnostics.extend(runtime.take_diagnostics());

    for_each_block_list_mut(blocks, &mut |list| {
        if !list.iter().any(|b| numbered_block_cell_id(b).is_some()) {
            return;
        }
        let old = std::mem::take(list);
        for block in old {
            match numbered_block_cell_id(&block).and_then(|id| results.remove(&id)) {
                Some(CellOutput::Blocks(out)) => list.extend(out),
                Some(CellOutput::Inlines(_)) => {
                    unreachable!("a block cell never produces inlines")
                }
                None => list.push(block),
            }
        }
    });
    for_each_inline_list_mut(blocks, &mut |list| {
        if !list.iter().any(|i| numbered_inline_cell_id(i).is_some()) {
            return;
        }
        let old = std::mem::take(list);
        for inline in old {
            match numbered_inline_cell_id(&inline).and_then(|id| results.remove(&id)) {
                Some(CellOutput::Inlines(out)) => list.extend(out),
                Some(CellOutput::Blocks(_)) => {
                    unreachable!("an inline cell never produces blocks")
                }
                None => list.push(inline),
            }
        }
    });
}

fn numbered_block_cell_id(block: &Block) -> Option<usize> {
    let Block::CodeBlock(cb) = block else {
        return None;
    };
    cb.attr.2.get(LMD_PENDING_ATTR)?.parse().ok()
}

fn numbered_inline_cell_id(inline: &Inline) -> Option<usize> {
    let Inline::Code(code) = inline else {
        return None;
    };
    code.attr.2.get(LMD_PENDING_ATTR)?.parse().ok()
}

#[async_trait(?Send)]
impl PipelineStage for LmdExecuteStage {
    fn name(&self) -> &str {
        "lmd-execute"
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

        // Cheap check before building a Lua state: most documents have no
        // lmd cells.
        let mut any = false;
        for_each_block_list_mut(&mut doc.ast.blocks, &mut |list| {
            any |= list.iter().any(is_pending_block_cell);
        });
        if !any {
            for_each_inline_list_mut(&mut doc.ast.blocks, &mut |list| {
                any |= list.iter().any(is_pending_inline_cell);
            });
        }
        if !any {
            return Ok(PipelineData::DocumentAst(doc));
        }

        // Same environment a Lua filter gets (see `UserFiltersStage`),
        // with the document standing in for the script path.
        let attribution: Option<std::sync::Arc<dyn pampa::attribution::AttributionLookup>> =
            ctx.attribution_data.as_ref().map(|data| {
                std::sync::Arc::new(crate::attribution::AttributionLookupHandle::new(
                    data.clone(),
                )) as std::sync::Arc<dyn pampa::attribution::AttributionLookup>
            });
        let runtime = LmdRuntime::new(
            ctx.runtime.clone(),
            ctx.format.lua_format(),
            &ctx.document.input,
            attribution,
            &doc.ast.meta,
        )
        .map_err(|err| PipelineError::stage_error(self.name(), err.to_string()))?;

        let mut diagnostics = Vec::new();
        execute_lmd_cells(
            &mut doc.ast.blocks,
            &runtime,
            &doc.source_context,
            &mut diagnostics,
        )
        .await;
        for diag in &diagnostics {
            trace_event!(ctx, EventLevel::Warn, "{}", diag.title);
        }
        ctx.diagnostics.extend(diagnostics);

        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
    use std::path::Path;
    use std::sync::Arc;

    fn runtime(meta: &quarto_pandoc_types::config_value::ConfigValue) -> LmdRuntime {
        LmdRuntime::new(
            Arc::new(NativeRuntime::new()),
            "html",
            Path::new("doc.qmd"),
            None,
            meta,
        )
        .expect("lmd runtime")
    }

    /// Parse `qmd`, desugar its `{lmd}` cells (what the desugar stage
    /// does), and run them.
    fn render(qmd: &str) -> (Vec<Block>, Vec<DiagnosticMessage>) {
        let (blocks, diagnostics, _sources) = render_with_sources(qmd);
        (blocks, diagnostics)
    }

    fn render_with_sources(qmd: &str) -> (Vec<Block>, Vec<DiagnosticMessage>, SourceContext) {
        let mut sink: Vec<u8> = Vec::new();
        let (ast, ctx, _warnings) =
            pampa::readers::qmd::read(qmd.as_bytes(), false, "test.qmd", &mut sink, true, None)
                .expect("test document parses");
        let sources = ctx.source_context;
        let mut blocks = ast.blocks;
        let errors = super::super::lmd_desugar::desugar_lmd_cells(&mut blocks, &sources);
        assert!(errors.is_empty(), "{errors:?}");
        let mut diagnostics = Vec::new();
        pollster::block_on(execute_lmd_cells(
            &mut blocks,
            &runtime(&ast.meta),
            &sources,
            &mut diagnostics,
        ));
        (blocks, diagnostics, sources)
    }

    /// The text a diagnostic's location resolves to in the test file.
    fn located_text<'a>(diag: &DiagnosticMessage, sources: &'a SourceContext) -> &'a str {
        let loc = diag.location.as_ref().expect("diagnostic is located");
        let (fid, s, e) = loc.resolve_byte_range().expect("location resolves");
        &sources
            .get_file(FileId(fid))
            .unwrap()
            .content
            .as_deref()
            .unwrap()[s..e]
    }

    fn problem_text(diag: &DiagnosticMessage) -> String {
        diag.problem
            .as_ref()
            .map(|p| format!("{p:?}"))
            .unwrap_or_default()
    }

    fn inlines_text(inlines: &[Inline]) -> String {
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(s) => s.text.clone(),
                Inline::Space(_) => " ".to_string(),
                Inline::Strong(s) => format!("**{}**", inlines_text(&s.content)),
                Inline::Emph(e) => format!("*{}*", inlines_text(&e.content)),
                Inline::Code(c) => format!("`{}`", c.text),
                _ => "?".to_string(),
            })
            .collect()
    }

    fn para_text(block: &Block) -> String {
        let Block::Paragraph(p) = block else {
            panic!("expected Para, got {block:?}");
        };
        inlines_text(&p.content)
    }

    #[test]
    fn a_cell_is_replaced_by_the_blocks_it_returns() {
        let (blocks, diags) =
            render("Before\n\n```{lmd}\nreturn {=: **hello** :=}\n```\n\nAfter\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert_eq!(para_text(&blocks[0]), "Before");
        assert_eq!(para_text(&blocks[1]), "**hello**");
        assert_eq!(para_text(&blocks[2]), "After");
        assert!(!blocks.iter().any(is_pending_block_cell));
    }

    #[test]
    fn the_motivating_example_with_immediate_values() {
        let qmd = "::: foo\n\n```{lmd}\nlocal out = pandoc.Blocks({})\nfor _, v in ipairs({'a', 'b'}) do\n  out:extend({=: **`{lmd} v`** :=})\nend\nreturn out\n```\n\n:::\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div");
        };
        assert_eq!(div.content.len(), 2, "{:?}", div.content);
        assert_eq!(para_text(&div.content[0]), "**a**");
        assert_eq!(para_text(&div.content[1]), "**b**");
    }

    /// The example the exploration started from, now runnable as written
    /// (spike 6): metadata reached through `meta` and `quarto.metadata.get`.
    #[test]
    fn the_motivating_example_against_document_metadata() {
        let qmd = "---\nstuff: [a, b, c, d]\n---\n\n::: foo\n\n```{lmd}\nlocal out = pandoc.Blocks({})\nfor _, v in ipairs(meta.stuff) do\n  out:extend({=: **`{lmd} v`** :=})\nend\nreturn out\n```\n\n:::\n\nAlso `{lmd} quarto.metadata.get('stuff')[4]` and `{lmd} tostring(#meta.stuff)`.\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div, got {:?}", blocks[0]);
        };
        let texts: Vec<String> = div.content.iter().map(para_text).collect();
        assert_eq!(texts, vec!["**a**", "**b**", "**c**", "**d**"]);
        assert_eq!(para_text(&blocks[1]), "Also d and 4.");
    }

    #[test]
    fn nil_removes_the_cell_and_state_is_shared_in_document_order() {
        let qmd =
            "```{lmd}\nshared = 'seen'\n```\n\n```{lmd}\nreturn {=: **`{lmd} shared`** :=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert_eq!(para_text(&blocks[0]), "**seen**");
    }

    #[test]
    fn echo_keeps_the_desugared_lua_above_the_result() {
        let qmd = "```{lmd}\n--| echo: true\nreturn {=: hi :=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        let Block::CodeBlock(shown) = &blocks[0] else {
            panic!("expected the echoed cell first");
        };
        assert_eq!(shown.attr.1, vec!["lmd".to_string()]);
        assert!(
            shown.text.starts_with("return pandoc.Blocks({"),
            "{}",
            shown.text
        );
        assert!(!shown.text.contains("echo"), "{}", shown.text);
        assert_eq!(para_text(&blocks[1]), "hi");
    }

    #[test]
    fn a_failing_cell_is_reported_and_left_displayed() {
        let qmd = "```{lmd}\nerror('boom')\n```\n\n```{lmd}\nreturn {=[ not blocks ]=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags[0].title.contains("lmd cell failed"), "{:?}", diags[0]);
        assert!(
            diags[1].title.contains("did not return blocks"),
            "{:?}",
            diags[1]
        );
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        for block in &blocks {
            let Block::CodeBlock(cb) = block else {
                panic!("expected a displayed cell, got {block:?}");
            };
            assert_eq!(cb.attr.1, vec!["lmd".to_string()]);
            assert!(cb.attr.2.get(LMD_PENDING_ATTR).is_none());
        }
    }

    #[test]
    fn cells_nested_in_containers_run_in_document_order() {
        let qmd = "```{lmd}\nn = 1\n```\n\n::: box\n\n```{lmd}\nn = n + 1\nreturn {=: **`{lmd} tostring(n)`** :=}\n```\n\n:::\n\n```{lmd}\nreturn {=: **`{lmd} tostring(n * 10)`** :=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div");
        };
        assert_eq!(para_text(&div.content[0]), "**2**");
        assert_eq!(para_text(&blocks[1]), "**20**");
    }

    #[test]
    fn documents_without_cells_are_untouched() {
        let qmd = "Plain\n\n```lua\nreturn 1\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty());
        assert_eq!(blocks.len(), 2);
    }

    // ----- inline cells (spike 3) -----

    #[test]
    fn an_inline_cell_is_replaced_by_its_inlines() {
        let qmd = "```{lmd}\nname = 'world'\n```\n\nHello `{lmd} name` and `{lmd} {=[ *there* ]=}` and `{lmd} pandoc.Strong('x')`.\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert_eq!(para_text(&blocks[0]), "Hello world and *there* and **x**.");
    }

    #[test]
    fn a_string_with_spaces_is_split_and_nil_vanishes() {
        let qmd = "A `{lmd} 'b c'` D `{lmd} nil` E\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected Para");
        };
        assert_eq!(inlines_text(&p.content), "A b c D  E");
        assert!(
            p.content
                .iter()
                .any(|i| matches!(i, Inline::Str(s) if s.text == "b"))
        );
    }

    #[test]
    fn inline_and_block_cells_share_one_document_order() {
        // The inline cell in the first paragraph runs before the block
        // cell below it, and the one in the last paragraph after.
        let qmd =
            "First `{lmd} tostring(n)`.\n\n```{lmd}\nn = 2\n```\n\nThen `{lmd} tostring(n)`.\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert_eq!(para_text(&blocks[0]), "First nil.");
        assert_eq!(para_text(&blocks[1]), "Then 2.");
    }

    #[test]
    fn inline_cells_inside_headings_and_emphasis_work() {
        let qmd = "```{lmd}\nt = 'Title'\n```\n\n## The `{lmd} t`\n\n*Em `{lmd} t`*\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Header(h) = &blocks[0] else {
            panic!("expected Header, got {:?}", blocks[0]);
        };
        assert_eq!(inlines_text(&h.content), "The Title");
        assert_eq!(para_text(&blocks[1]), "*Em Title*");
    }

    // ----- pandoc.Nodes: the unmarked `{= … =}` fragment -----
    //
    // End-to-end pins of the resolution table in pampa's lua/nodes.rs, one
    // row per assertion, so the lmd syntax and the runtime cannot drift
    // apart.

    #[test]
    fn unmarked_fragment_in_a_block_cell_keeps_the_readers_para() {
        let (blocks, diags) = render("```{lmd}\nreturn {= **hello** =}\n```\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        // blocks shape, block context: the Para stays a Para, not a Plain.
        assert!(matches!(&blocks[0], Block::Paragraph(_)), "{:?}", blocks[0]);
        assert_eq!(para_text(&blocks[0]), "**hello**");
    }

    #[test]
    fn unmarked_fragment_inside_a_block_constructor_keeps_the_para() {
        let (blocks, diags) = render("```{lmd}\nreturn pandoc.Div({= hello =})\n```\n");
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected Div, got {:?}", blocks[0]);
        };
        assert_eq!(div.content.len(), 1);
        assert!(
            matches!(&div.content[0], Block::Paragraph(_)),
            "{:?}",
            div.content[0]
        );
    }

    #[test]
    fn unmarked_fragment_inside_an_inline_constructor_unwraps_to_inlines() {
        let (blocks, diags) =
            render("```{lmd}\nreturn pandoc.Para({ pandoc.Strong({= hello =}) })\n```\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(para_text(&blocks[0]), "**hello**");
    }

    #[test]
    fn unmarked_fragment_in_an_inline_cell_unwraps_to_inlines() {
        let (blocks, diags) = render("Say `{lmd} {= *hi* there =}` now.\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(para_text(&blocks[0]), "Say *hi* there now.");
    }

    #[test]
    fn nodes_with_several_blocks_in_inline_context_is_an_error() {
        // In an inline cell (a code span cannot hold a blank line, so the
        // two-block Nodes is built by hand) …
        let (blocks, diags) =
            render("Say `{lmd} pandoc.Nodes({pandoc.Para({}), pandoc.Para({})})` now.\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].title.contains("lmd cell failed"), "{:?}", diags[0]);
        assert!(
            diags[0]
                .problem
                .as_ref()
                .is_some_and(|p| format!("{p:?}").contains("holds 2 blocks")),
            "{:?}",
            diags[0]
        );
        // … the cell stays displayed and the paragraph survives.
        assert!(
            para_text(&blocks[0]).starts_with("Say `"),
            "{}",
            para_text(&blocks[0])
        );
        // … and inside an inline constructor in a block cell, where the
        // fragment syntax can hold two paragraphs.
        let (blocks, diags) =
            render("```{lmd}\nreturn pandoc.Para({ pandoc.Strong({= one\n\ntwo =}) })\n```\n");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0]
                .problem
                .as_ref()
                .is_some_and(|p| format!("{p:?}").contains("holds 2 blocks")),
            "{:?}",
            diags[0]
        );
        assert!(matches!(&blocks[0], Block::CodeBlock(_)), "{:?}", blocks[0]);
    }

    #[test]
    fn unmarked_fragments_nest_through_inline_lmd_code() {
        // The inner fragment is a Nodes element of the outer paragraph's
        // content list; it is resolved when pandoc.Para is built.
        let (blocks, diags) = render("```{lmd}\nreturn {= a `{lmd} {= b =}` c =}\n```\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert_eq!(para_text(&blocks[0]), "a b c");
    }

    #[test]
    fn a_hand_built_nodes_of_inlines_in_a_block_cell_becomes_a_plain() {
        let (blocks, diags) = render("```{lmd}\nreturn pandoc.Nodes({ pandoc.Str('x') })\n```\n");
        assert!(diags.is_empty(), "{diags:?}");
        assert!(matches!(&blocks[0], Block::Plain(_)), "{:?}", blocks[0]);
    }

    #[test]
    fn a_failing_inline_cell_is_reported_and_left_as_a_code_span() {
        let qmd = "A `{lmd} pandoc.Para({})` B `{lmd} nope()` C\n";
        let (blocks, diags) = render(qmd);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(
            diags[0].title.contains("did not produce inlines"),
            "{:?}",
            diags[0]
        );
        assert!(diags[1].title.contains("lmd cell failed"), "{:?}", diags[1]);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected Para");
        };
        let codes: Vec<&Code> = p
            .content
            .iter()
            .filter_map(|i| match i {
                Inline::Code(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(codes.len(), 2, "{:?}", p.content);
        for code in codes {
            assert_eq!(code.attr.1, vec!["lmd".to_string()]);
            assert!(code.attr.2.get(LMD_PENDING_ATTR).is_none());
        }
        assert_eq!(para_text(&blocks[0]), "A `pandoc.Para({})` B `nope()` C");
    }

    // ----- source mapping (spike 7) -----

    #[test]
    fn a_runtime_error_names_the_document_line_and_is_located_on_it() {
        let qmd = "Intro\n\n```{lmd}\nlocal x = 1\nerror('boom')\n```\n";
        let (_blocks, diags, sources) = render_with_sources(qmd);
        assert_eq!(diags.len(), 1, "{diags:?}");
        // Document line 5 is `error('boom')`.
        assert!(
            problem_text(&diags[0]).contains("test.qmd:5:"),
            "{:?}",
            diags[0]
        );
        assert_eq!(located_text(&diags[0], &sources), "error('boom')");
    }

    #[test]
    fn line_numbers_survive_fragment_expansion_and_option_lines() {
        let qmd =
            "```{lmd}\n--| echo: false\nlocal a = {=:\nOne.\n\nTwo.\n:=}\nerror('after')\n```\n";
        let (_blocks, diags, sources) = render_with_sources(qmd);
        assert_eq!(diags.len(), 1, "{diags:?}");
        // Document line 8 is `error('after')`: the option line and the
        // five-line fragment keep their lines.
        assert!(
            problem_text(&diags[0]).contains("test.qmd:8:"),
            "{:?}",
            diags[0]
        );
        assert_eq!(located_text(&diags[0], &sources), "error('after')");
    }

    #[test]
    fn an_inline_cell_error_names_its_line() {
        let qmd = "First.\n\nSecond `{lmd} nope()` here.\n";
        let (_blocks, diags, sources) = render_with_sources(qmd);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            problem_text(&diags[0]).contains("test.qmd:3:"),
            "{:?}",
            diags[0]
        );
        // The location is the expression itself (exact anchor), tighter
        // than the line.
        assert_eq!(located_text(&diags[0], &sources), "nope()");
    }

    #[test]
    fn a_cell_in_a_blockquote_still_gets_its_line() {
        // The gutter makes the body anchor approximate, but the start line
        // is known from the block span, so the chunk is still padded right.
        let qmd = "> ```{lmd}\n> local x = 1\n> error('quoted')\n> ```\n";
        let (_blocks, diags, sources) = render_with_sources(qmd);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            problem_text(&diags[0]).contains("test.qmd:3:"),
            "{:?}",
            diags[0]
        );
        assert_eq!(located_text(&diags[0], &sources), "> error('quoted')");
    }

    #[test]
    fn cell_option_errors_are_located_on_the_option_line() {
        let qmd = "```{lmd}\n--| echo: [\nreturn nil\n```\n";
        let (_blocks, diags, sources) = render_with_sources(qmd);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].title.contains("options are not valid"),
            "{:?}",
            diags[0]
        );
        let loc = diags[0].location.as_ref().expect("located");
        let mapped = loc.map_offset(0, &sources).expect("maps");
        assert_eq!(mapped.location.row, 1, "{mapped:?}");
    }
}
