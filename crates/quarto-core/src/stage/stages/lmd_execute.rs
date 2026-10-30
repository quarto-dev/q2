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
//!
//! What a cell returns gets the structural desugaring set
//! (`pampa::pandoc::desugar`: list-table and definition-list divs, the
//! implicit figure) before it is spliced in, per cell, so a cell that
//! builds `pandoc.Div({…}, {"list-table"})` from nodes ends up with the
//! same `Table` the reader would have made from the markdown (bd-7gqr148v).
//! A node a cell built has no document position of its own, so a Q-2-35
//! from that pass is relocated onto the cell.

use async_trait::async_trait;
use pampa::pandoc::desugar::{desugar_blocks, desugar_inlines};
use pampa::utils::diagnostic_collector::DiagnosticCollector;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::block::{Block, CodeBlock};
use quarto_pandoc_types::config_value::ConfigValue;
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
    code: &str,
    title: &str,
    location: SourceInfo,
    problem: String,
    hint: Option<&str>,
) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error(title)
        .with_code(code)
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
    // A module the cell required that did not compile is its own failure
    // (bd-76ogq4ew): the fix is in the module file, not in the cell, and
    // the dialect's own diagnostics name where.
    if let LmdCellError::Lua(lua_err) = err
        && let Some(module_err) = pampa::dialect::ModuleCompileError::find_in(lua_err)
    {
        let location = location.unwrap_or_else(|| cell.anchor.span.clone());
        let mut builder = DiagnosticMessageBuilder::error("lmd module did not compile")
            .with_code("Q-26-8")
            .with_location(location)
            .problem(module_err.to_string());
        // The dialect's own diagnostics, when they say more than the
        // message already does (the lmd desugarer's summary is the first
        // reader diagnostic's title, so a lone one adds nothing).
        let message = module_err.to_string();
        for inner in &module_err.error.diagnostics {
            if !message.contains(&inner.title) {
                builder =
                    builder.add_hint(format!("{}: {}", module_err.path.display(), inner.title));
            }
        }
        return builder
            .add_hint("Fix the module file; the cell that requires it is left as written.")
            .build();
    }
    let (code, title, hint) = match err {
        LmdCellError::Lua(_) => ("Q-26-4", "lmd cell failed", None),
        LmdCellError::NotBlocks { .. } => (
            "Q-26-5",
            "lmd cell did not return blocks",
            Some(
                "Return a Block, a list of Blocks, or nil. Write `{=: … :=}` for a markdown fragment that is blocks.",
            ),
        ),
        LmdCellError::NotInlines { .. } => (
            "Q-26-6",
            "Inline lmd cell did not produce inlines",
            Some(
                "An inline cell's expression must be an Inline, a list of Inlines, a string, a number, a boolean, or nil. Write `{=[ … ]=}` for a markdown fragment that is inlines.",
            ),
        ),
    };
    let location = location.unwrap_or_else(|| cell.anchor.span.clone());
    cell_error(code, title, location, err.to_string(), hint)
}

/// The structural desugaring set over a block cell's result, with any
/// diagnostic it raises relocated onto the cell.
fn desugar_cell_blocks(
    blocks: Vec<Block>,
    cell: &PendingCell,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<Block> {
    let mut collector = DiagnosticCollector::new();
    let blocks = desugar_blocks(blocks, &mut collector);
    diagnostics.extend(
        collector
            .diagnostics()
            .iter()
            .map(|diag| relocate_to_cell(diag, cell)),
    );
    blocks
}

/// [`desugar_cell_blocks`] for an inline cell's result (its sugar, if
/// any, sits in the blocks of a `Note`).
fn desugar_cell_inlines(
    inlines: Inlines,
    cell: &PendingCell,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Inlines {
    let mut collector = DiagnosticCollector::new();
    let inlines = desugar_inlines(inlines, &mut collector);
    diagnostics.extend(
        collector
            .diagnostics()
            .iter()
            .map(|diag| relocate_to_cell(diag, cell)),
    );
    inlines
}

/// A diagnostic the desugaring pass raised about a node the cell
/// produced, located on the cell instead. The node's own location is not
/// usable: a node the cell built carries the Lua chunk position
/// (`SourceInfo::generated`), and a node `pandoc.read` returned carries an
/// offset into that call's private source context. The whole cell is the
/// honest resolution available; the reader's precise position is
/// available only for markup the reader itself parsed, which never reaches
/// this pass with sugar left in it. Code, title, problem and hints are
/// kept; details are not, since each carries its own (unusable) location.
fn relocate_to_cell(diag: &DiagnosticMessage, cell: &PendingCell) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::new(diag.kind, diag.title.clone())
        .with_location(cell.anchor.span.clone());
    if let Some(code) = &diag.code {
        builder = builder.with_code(code.clone());
    }
    if let Some(problem) = &diag.problem {
        builder = builder.problem(problem.clone());
    }
    for hint in &diag.hints {
        builder = builder.add_hint(hint.clone());
    }
    builder
        .add_info("The node was built by this lmd cell.")
        .build()
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
                        match config.get("echo") {
                            None => false,
                            Some(value) => value.as_bool().unwrap_or_else(|| {
                                // A wrongly typed option is reported, not
                                // silently taken as `false`: a typo such as
                                // `echo: yes please` would otherwise vanish.
                                diagnostics.push(cell_error(
                                    "Q-26-7",
                                    "Invalid lmd cell options",
                                    value.source_info.clone(),
                                    "`echo` must be `true` or `false`".to_string(),
                                    None,
                                ));
                                false
                            }),
                        }
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
                "Q-26-7",
                "Invalid lmd cell options",
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
    meta: &ConfigValue,
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

    // `lmd: modules:` (bd-76ogq4ew, phase 3): the prelude, bound before
    // the first cell. A broken prelude fails the page once: every cell is
    // left as written, rather than each failing on the missing global.
    let entries = autoload_entries(meta, diagnostics);
    let pairs: Vec<(String, String)> = entries
        .iter()
        .map(|e| (e.global.clone(), e.module.clone()))
        .collect();
    if let Err((index, err)) = runtime.autoload(&pairs) {
        diagnostics.push(autoload_failure(&entries[index], &err));
        diagnostics.extend(runtime.take_diagnostics());
        let mut results: HashMap<usize, CellOutput> = HashMap::new();
        for cell in &cells {
            let output = match cell.kind {
                CellKind::Block => {
                    let (code, _echo, _) = block_cell_code_and_echo(cell, diagnostics);
                    CellOutput::Blocks(vec![displayed_block_cell(&code, &cell.source_info)])
                }
                CellKind::Inline => {
                    CellOutput::Inlines(vec![displayed_inline_cell(&cell.text, &cell.source_info)])
                }
            };
            results.insert(cell.id, output);
        }
        splice_results(blocks, results);
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
                            out.extend(desugar_cell_blocks(result, cell, diagnostics));
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
                    Ok(LmdInlineValue::Inlines(result)) => {
                        CellOutput::Inlines(desugar_cell_inlines(result, cell, diagnostics))
                    }
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
    splice_results(blocks, results);
}

/// Replace every numbered cell in `blocks` with its output.
fn splice_results(blocks: &mut Vec<Block>, mut results: HashMap<usize, CellOutput>) {
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

/// One `lmd: modules:` entry: the global it binds, the module name
/// `require` is given, and where the entry is written.
struct AutoloadEntry {
    global: String,
    module: String,
    location: SourceInfo,
}

/// Read `lmd.modules` from the merged metadata. An item is a module name
/// (the global is its last path segment: `components/cards` binds `cards`)
/// or a one-key map `global: module`. A malformed item is reported as
/// `Q-26-9` and skipped.
fn autoload_entries(
    meta: &ConfigValue,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Vec<AutoloadEntry> {
    let Some(items) = meta
        .get("lmd")
        .and_then(|lmd| lmd.get("modules"))
        .and_then(|m| m.as_array())
    else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for item in items {
        let location = item.source_info.clone();
        let parsed = if let Some(module) = item.as_plain_text() {
            let global = module
                .trim_end_matches('/')
                .rsplit(['/', '.'])
                .next()
                .unwrap_or("")
                .to_string();
            Some((global, module))
        } else if let Some(entries) = item.as_map_entries()
            && let [entry] = entries
            && let Some(module) = entry.value.as_plain_text()
        {
            Some((entry.key.clone(), module))
        } else {
            None
        };
        match parsed {
            Some((global, module)) if is_lua_name(&global) => entries.push(AutoloadEntry {
                global,
                module,
                location,
            }),
            Some((global, module)) => diagnostics.push(
                DiagnosticMessageBuilder::error("lmd autoload failed")
                    .with_code("Q-26-9")
                    .with_location(location)
                    .problem(format!(
                        "`{module}` would bind the global `{global}`, which is not a Lua name"
                    ))
                    .add_hint(
                        "Write the entry as a one-key map, `name: module`, to choose the global's name.",
                    )
                    .build(),
            ),
            None => diagnostics.push(
                DiagnosticMessageBuilder::error("lmd autoload failed")
                    .with_code("Q-26-9")
                    .with_location(location)
                    .problem(
                        "an `lmd: modules:` entry must be a module name or a one-key map `global: module`"
                            .to_string(),
                    )
                    .build(),
            ),
        }
    }
    entries
}

fn is_lua_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `Q-26-9` for an entry whose `require` failed.
fn autoload_failure(entry: &AutoloadEntry, err: &LmdCellError) -> DiagnosticMessage {
    let detail = match err {
        LmdCellError::Lua(lua_err) => match pampa::dialect::ModuleCompileError::find_in(lua_err) {
            Some(module_err) => module_err.to_string(),
            None => lua_err.to_string(),
        },
        other => other.to_string(),
    };
    DiagnosticMessageBuilder::error("lmd autoload failed")
        .with_code("Q-26-9")
        .with_location(entry.location.clone())
        .problem(format!(
            "`lmd: modules:` could not load `{}` as `{}`: {detail}",
            entry.module, entry.global
        ))
        .add_hint("Fix the module or the entry; the document's lmd cells are left as written.")
        .build()
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
            &crate::lmd::lua_options(&ctx.project, ctx.runtime.as_ref()),
        )
        .map_err(|err| PipelineError::stage_error(self.name(), err.to_string()))?;

        let mut diagnostics = Vec::new();
        execute_lmd_cells(
            &mut doc.ast.blocks,
            &runtime,
            &doc.ast.meta,
            &doc.source_context,
            &mut diagnostics,
        )
        .await;
        // Give the text each `pandoc.read` call parsed a home in the
        // document (GH #884), in both contexts and in the same order, so
        // the nodes it returned locate diagnostics in that text.
        let read_sources = runtime.take_read_sources();
        for source_context in [&mut doc.ast_context.source_context, &mut doc.source_context] {
            pampa::lua::register_read_sources(source_context, &read_sources);
        }
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

    /// A runtime whose document lives at `document`, so `require` resolves
    /// beside it; lmd is registered as it is in the stage.
    fn runtime_at(
        meta: &quarto_pandoc_types::config_value::ConfigValue,
        document: &Path,
    ) -> LmdRuntime {
        LmdRuntime::new(
            Arc::new(NativeRuntime::new()),
            "html",
            document,
            None,
            meta,
            &pampa::lua::LuaOptions::with_dialects(crate::lmd::dialects()),
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
        render_at(qmd, Path::new("doc.qmd"))
    }

    /// [`render_with_sources`] for a document at `document`.
    fn render_at(
        qmd: &str,
        document: &Path,
    ) -> (Vec<Block>, Vec<DiagnosticMessage>, SourceContext) {
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
            &runtime_at(&ast.meta, document),
            &ast.meta,
            &sources,
            &mut diagnostics,
        ));
        (blocks, diagnostics, sources)
    }

    // ---- modules: `require` of `.lmd` files (bd-76ogq4ew, phase 1) ----

    fn write_module(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    const CARDS_LMD: &str = "local M = {}\n\nfunction M.card(title, body)\n  return {=:\n::: {.card}\n### `{lmd} title`\n\n`{lmd} body`\n:::\n:=}\nend\n\nreturn M\n";

    #[test]
    fn a_cell_requires_an_lmd_module_and_uses_its_component() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_module(tmp.path(), "components/cards.lmd", CARDS_LMD);
        let (blocks, diags, _) = render_at(
            "```{lmd}\nlocal cards = require(\"components/cards\")\nreturn cards.card(\"Hello\", {=[ from a *module* ]=})\n```\n",
            &tmp.path().join("doc.qmd"),
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected the card div, got {:?}", blocks[0]);
        };
        assert_eq!(div.attr.1, vec!["card".to_string()]);
        let Block::Header(h) = &div.content[0] else {
            panic!("expected a heading, got {:?}", div.content[0]);
        };
        assert_eq!(inlines_text(&h.content), "Hello");
        assert_eq!(para_text(&div.content[1]), "from a *module*");
    }

    #[test]
    fn a_runtime_error_inside_a_module_names_the_module_file_and_line() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_module(
            tmp.path(),
            "boom.lmd",
            "local M = {}\nfunction M.go()\n  error(\"boom\")\nend\nreturn M\n",
        );
        let (_, diags, sources) = render_at(
            // Not `return m.go()`: a tail call drops the cell's frame from
            // the traceback, and then the whole cell is the honest location.
            "```{lmd}\nlocal m = require(\"boom\")\nlocal r = m.go()\nreturn r\n```\n",
            &tmp.path().join("doc.qmd"),
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-26-4"));
        let problem = problem_text(&diags[0]);
        // The module's own position leads the message…
        assert!(problem.contains("boom.lmd:3: boom"), "{problem}");
        // …and the cell's frame in the traceback locates the diagnostic.
        assert_eq!(
            located_text(&diags[0], &sources),
            "local r = m.go()",
            "{problem}"
        );
    }

    #[test]
    fn a_desugar_failure_inside_a_module_is_reported_at_the_require() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_module(
            tmp.path(),
            "bad.lmd",
            "local M = {}\nfunction M.go()\n  return {=: pages ' :=}\nend\nreturn M\n",
        );
        let (_, diags, sources) = render_at(
            "```{lmd}\nlocal m = require(\"bad\")\nreturn m.go()\n```\n",
            &tmp.path().join("doc.qmd"),
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-26-8"), "{diags:?}");
        let problem = problem_text(&diags[0]);
        assert!(
            problem.contains("module 'bad' could not be compiled from lmd"),
            "{problem}"
        );
        // The reader's own title is in the message; the hint is the fix.
        assert!(problem.contains("Quote"), "{problem}");
        let hints = format!("{:?}", diags[0].hints);
        assert!(hints.contains("Fix the module file"), "{hints}");
        assert!(problem.contains("bad.lmd"), "{problem}");
        assert!(
            problem.contains("markdown fragment failed to parse"),
            "{problem}"
        );
        assert_eq!(
            located_text(&diags[0], &sources),
            "local m = require(\"bad\")"
        );
    }

    #[test]
    fn a_module_may_require_a_sibling_lmd_module() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_module(tmp.path(), "ui/cards.lmd", CARDS_LMD);
        write_module(
            tmp.path(),
            "ui/page.lmd",
            "local cards = require(\"cards\")\nreturn { hero = function(t) return cards.card(t, {=[ hero ]=}) end }\n",
        );
        let (blocks, diags, _) = render_at(
            "```{lmd}\nreturn require(\"ui/page\").hero(\"Top\")\n```\n",
            &tmp.path().join("doc.qmd"),
        );
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Div(div) = &blocks[0] else {
            panic!("expected the card div, got {:?}", blocks[0]);
        };
        let Block::Header(h) = &div.content[0] else {
            panic!("expected a heading");
        };
        assert_eq!(inlines_text(&h.content), "Top");
    }

    #[test]
    fn an_lmd_module_wins_over_a_lua_file_of_the_same_name_with_a_warning() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_module(tmp.path(), "cards.lmd", CARDS_LMD);
        write_module(
            tmp.path(),
            "cards.lua",
            "return { card = function() error(\"stale\") end }\n",
        );
        let (blocks, diags, _) = render_at(
            "```{lmd}\nreturn require(\"cards\").card(\"Hello\", {=[ x ]=})\n```\n",
            &tmp.path().join("doc.qmd"),
        );
        assert!(matches!(blocks[0], Block::Div(_)), "{blocks:?}");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].title.contains("cards.lmd") && diags[0].title.contains("cards.lua"),
            "{diags:?}"
        );
    }

    // ---- autoload: `lmd: modules:` (bd-76ogq4ew, phase 3) ----

    #[test]
    fn autoloaded_modules_are_globals_before_the_first_cell() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_module(tmp.path(), "components/cards.lmd", CARDS_LMD);
        let (blocks, diags, _) = render_at(
            "---\nlmd:\n  modules:\n    - components/cards\n    - ui: components/cards\n---\n\n```{lmd}\nreturn { cards.card(\"A\", {=[ one ]=}), ui.card(\"B\", {=[ two ]=}) }\n```\n",
            &tmp.path().join("doc.qmd"),
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert!(
            blocks.iter().all(|b| matches!(b, Block::Div(_))),
            "{blocks:?}"
        );
    }

    #[test]
    fn a_failing_autoload_entry_is_q_26_9_and_leaves_every_cell_as_written() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (blocks, diags, sources) = render_at(
            "---\nlmd:\n  modules:\n    - components/nowhere\n---\n\n```{lmd}\nreturn nowhere.thing()\n```\n\nInline `{lmd} nowhere.x` too.\n",
            &tmp.path().join("doc.qmd"),
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-26-9"), "{diags:?}");
        assert_eq!(located_text(&diags[0], &sources), "components/nowhere");
        assert!(
            problem_text(&diags[0]).contains("components/nowhere"),
            "{diags:?}"
        );
        // Both cells are displayed, not run: no Q-26-4 for the missing global.
        let cell = match &blocks[0] {
            Block::CodeBlock(cb) => cb,
            other => panic!("expected the displayed cell, got {other:?}"),
        };
        assert_eq!(cell.attr.1, vec!["lmd".to_string()]);
        assert!(cell.attr.2.get(LMD_PENDING_ATTR).is_none());
        let Block::Paragraph(p) = &blocks[1] else {
            panic!("{blocks:?}")
        };
        assert!(
            p.content
                .iter()
                .any(|i| matches!(i, Inline::Code(c) if c.attr.1 == vec!["lmd".to_string()])),
            "{:?}",
            p.content
        );
    }

    #[test]
    fn a_malformed_autoload_entry_is_reported_and_the_others_still_load() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_module(tmp.path(), "components/cards.lmd", CARDS_LMD);
        let (blocks, diags, _) = render_at(
            "---\nlmd:\n  modules:\n    - components/cards\n    - my-module\n    - [1, 2]\n---\n\n```{lmd}\nreturn cards.card(\"A\", {=[ one ]=})\n```\n",
            &tmp.path().join("doc.qmd"),
        );
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(
            diags.iter().all(|d| d.code.as_deref() == Some("Q-26-9")),
            "{diags:?}"
        );
        assert!(
            problem_text(&diags[0]).contains("not a Lua name"),
            "{diags:?}"
        );
        assert!(matches!(blocks[0], Block::Div(_)), "{blocks:?}");
    }

    // ---- fragment gaps (bd-76ogq4ew, phase 2) ----

    /// A footnote defined inside a fragment runs end to end: the cell's
    /// blocks hold an inline `Note`, which the document's footnote
    /// transforms number later like any other note.
    #[test]
    fn a_footnote_defined_in_a_fragment_is_an_inline_note_in_the_cells_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (blocks, diags, _) = render_at(
            "```{lmd}\nreturn {=:\nThe claim.[^src]\n\n[^src]: The *source*.\n:=}\n```\n",
            &tmp.path().join("doc.qmd"),
        );
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("{blocks:?}")
        };
        let note = p
            .content
            .iter()
            .find_map(|i| match i {
                Inline::Note(n) => Some(n),
                _ => None,
            })
            .expect("a Note inline");
        let Block::Paragraph(inner) = &note.content[0] else {
            panic!("{:?}", note.content)
        };
        assert_eq!(inlines_text(&inner.content), "The *source*.");
    }

    /// `quarto.markdown(str)` in a cell: a string becomes `Nodes`, which a
    /// block cell takes as blocks and an inline cell as inlines.
    #[test]
    fn quarto_markdown_turns_a_string_into_nodes_for_either_kind_of_cell() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (blocks, diags, _) = render_at(
            "```{lmd}\nreturn quarto.markdown(\"One *em*.\\n\\nTwo.\")\n```\n\nAnd `{lmd} quarto.markdown(\"**bold** inline\")` here.\n",
            &tmp.path().join("doc.qmd"),
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert!(
            matches!(&blocks[0], Block::Paragraph(p) if p.content.iter().any(|i| matches!(i, Inline::Emph(_)))),
            "{blocks:?}"
        );
        assert!(matches!(&blocks[1], Block::Paragraph(_)), "{blocks:?}");
        let Block::Paragraph(p) = &blocks[2] else {
            panic!("{blocks:?}")
        };
        assert!(
            p.content.iter().any(|i| matches!(i, Inline::Strong(_))),
            "{:?}",
            p.content
        );
        assert_eq!(inlines_text(&p.content), "And **bold** inline here.");
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

    // ----- the structural desugaring set over cell results (bd-7gqr148v) -----

    const LIST_TABLE_DIV: &str = "pandoc.Div({ pandoc.BulletList({ { pandoc.BulletList({ { pandoc.Plain({ pandoc.Str('k') }) }, { pandoc.Plain({ pandoc.Str('v') }) } }) } }) }, pandoc.Attr('', {'list-table'}))";

    #[test]
    fn a_cell_built_list_table_div_becomes_a_table() {
        let (blocks, diags) = render(&format!("```{{lmd}}\nreturn {LIST_TABLE_DIV}\n```\n"));
        assert!(diags.is_empty(), "{diags:?}");
        assert!(matches!(blocks.as_slice(), [Block::Table(_)]), "{blocks:?}");
    }

    #[test]
    fn a_cell_built_definition_list_div_becomes_a_definition_list() {
        let qmd = "```{lmd}\nreturn pandoc.Div({ pandoc.BulletList({ { pandoc.Plain({ pandoc.Str('t') }), pandoc.BulletList({ { pandoc.Plain({ pandoc.Str('d') }) } }) } }) }, pandoc.Attr('', {'definition-list'}))\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert!(
            matches!(blocks.as_slice(), [Block::DefinitionList(_)]),
            "{blocks:?}"
        );
    }

    #[test]
    fn a_cell_built_single_image_para_becomes_a_figure() {
        let qmd = "```{lmd}\nreturn pandoc.Para({ pandoc.Image({ pandoc.Str('alt') }, 'x.png', '', pandoc.Attr('fig-x')) })\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        let [Block::Figure(figure)] = blocks.as_slice() else {
            panic!("expected Figure, got {blocks:?}");
        };
        assert_eq!(figure.attr.0, "fig-x");
    }

    #[test]
    fn an_invalid_cell_built_list_table_warns_at_the_cell() {
        let qmd = "Before\n\n```{lmd}\nreturn pandoc.Div({ pandoc.Para({ pandoc.Str('x') }) }, pandoc.Attr('', {'list-table'}))\n```\n";
        let (blocks, diags, sources) = render_with_sources(qmd);
        assert!(
            matches!(blocks.as_slice(), [_, Block::Div(_)]),
            "{blocks:?}"
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-2-35"));
        assert_eq!(
            diags[0].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
        // Located on the cell's text, not on the Lua chunk position the
        // constructed div carries.
        assert_eq!(
            located_text(&diags[0], &sources),
            "return pandoc.Div({ pandoc.Para({ pandoc.Str('x') }) }, pandoc.Attr('', {'list-table'}))"
        );
    }

    #[test]
    fn an_inline_cell_note_holding_a_list_table_is_desugared() {
        let qmd = format!("A note`{{lmd}} pandoc.Note({{ {LIST_TABLE_DIV} }})` here.\n");
        let (blocks, diags) = render(&qmd);
        assert!(diags.is_empty(), "{diags:?}");
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("{blocks:?}");
        };
        let note = p
            .content
            .iter()
            .find_map(|i| match i {
                Inline::Note(n) => Some(n),
                _ => None,
            })
            .expect("the note is spliced in");
        assert!(
            matches!(note.content.as_slice(), [Block::Table(_)]),
            "{:?}",
            note.content
        );
    }

    #[test]
    fn a_cell_nested_in_a_fragment_is_covered_by_the_parent_pass() {
        let qmd = format!(
            "````{{lmd}}\nreturn {{=:\nIntro\n\n```{{lmd}}\nreturn {LIST_TABLE_DIV}\n```\n:=}}\n````\n"
        );
        let (blocks, diags) = render(&qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert!(
            matches!(blocks.as_slice(), [Block::Paragraph(_), Block::Table(_)]),
            "{blocks:?}"
        );
    }

    /// bd-7gqr148v: a table written inside a fragment runs and is spliced
    /// in, whether it was a pipe table or a list-table the reader desugared.
    #[test]
    fn tables_in_fragments_run_and_are_spliced_in() {
        let qmd = "```{lmd}\nreturn {=:\n| a | b |\n|---|--:|\n| 1 | 2 |\n\n::: {.list-table}\n* - k\n  - v\n* - x\n  - y\n:::\n:=}\n```\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        let [Block::Table(pipe), Block::Table(list)] = blocks.as_slice() else {
            panic!("expected two tables, got {blocks:?}");
        };
        assert_eq!(pipe.head.rows.len(), 1);
        assert_eq!(pipe.bodies[0].body.len(), 1);
        assert_eq!(
            pipe.colspec[1].0,
            quarto_pandoc_types::table::Alignment::Right
        );
        assert_eq!(
            list.head.rows.len(),
            1,
            "bare list-table promotes its first row"
        );
        assert_eq!(list.bodies[0].body.len(), 1);
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

    /// bd-iqd6niat: metadata numbers and booleans are Lua numbers and
    /// booleans (divergence D-num), and an inline cell renders them as
    /// `tostring` would, so `version: 12.1` needs no `tostring()`.
    #[test]
    fn inline_cells_render_metadata_numbers_and_booleans() {
        let qmd = "---\nversion: 12.1\ncount: 3\nflag: true\n---\n\nVersion `{lmd} meta.version`, count `{lmd} meta.count`, twice `{lmd} meta.count * 2`, flag `{lmd} meta.flag`, pair `{lmd} {meta.count, ' of ', meta.version}`.\n";
        let (blocks, diags) = render(qmd);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(
            para_text(&blocks[0]),
            "Version 12.1, count 3, twice 6, flag true, pair 3 of 12.1."
        );
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
    fn a_wrongly_typed_echo_option_is_reported_on_its_value() {
        let qmd = "```{lmd}\n--| echo: yes please\nreturn nil\n```\n";
        let (blocks, diags, sources) = render_with_sources(qmd);
        assert!(blocks.is_empty(), "the cell still runs: {blocks:?}");
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code.as_deref(), Some("Q-26-7"), "{:?}", diags[0]);
        // The value's span is composed through the YAML parse; map its start
        // and check it lands on `yes please` (row 1, after `--| echo: `).
        let loc = diags[0].location.as_ref().expect("located");
        let mapped = loc.map_offset(0, &sources).expect("maps");
        assert_eq!(
            (mapped.location.row, mapped.location.column),
            (1, 10),
            "{mapped:?}"
        );
    }

    #[test]
    fn cell_option_errors_are_located_on_the_option_line() {
        let qmd = "```{lmd}\n--| echo: [\nreturn nil\n```\n";
        let (_blocks, diags, sources) = render_with_sources(qmd);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(
            diags[0].title.contains("Invalid lmd cell options"),
            "{:?}",
            diags[0]
        );
        let loc = diags[0].location.as_ref().expect("located");
        let mapped = loc.map_offset(0, &sources).expect("maps");
        assert_eq!(mapped.location.row, 1, "{mapped:?}");
    }
}
