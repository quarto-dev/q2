//! Unified document analysis for extracting intelligence data.
//!
//! This module runs the same pipeline stages the render path uses, stopping
//! short of rendering. Symbols, folding ranges, and diagnostics are extracted
//! from the post-pipeline AST, so the outline sees cross-referenceable
//! elements with their section-scoped numbers already assigned (by
//! `CrossrefIndexTransform`) and theorem titles already absorbed into
//! their `CustomNode("Theorem")` `title` slot (by `TheoremSugarTransform`).
//!
//! See `claude-notes/plans/2026-04-17-crossref-outline.md` for the design.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pampa::pandoc::Pandoc;
use pampa::pandoc::custom::Slot;
use pampa::pandoc::{Block, CodeBlock, Header, Inline, Inlines};
use quarto_analysis::DocumentAnalysisContext;
use quarto_analysis::transforms::{
    AnalysisTransform, MetaShortcodeTransform, run_analysis_transforms,
};
use quarto_core::crossref::{
    CrossrefTargetView, crossref_target_view, crossref_target_view_inline,
};
use quarto_core::format::Format;
use quarto_core::pipeline::build_analysis_pipeline;
use quarto_core::project::{DocumentInfo, ProjectConfig, ProjectContext};
use quarto_core::render::{BinaryDependencies, RenderContext};
use quarto_core::stage::{LoadedSource, PipelineData, StageContext};
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;
use quarto_system_runtime::{SystemRuntime, default_runtime};

use crate::document::Document;
use crate::types::{
    DetailKind, Diagnostic, DiagnosticDetail, DiagnosticSeverity, DocumentAnalysis, FoldingRange,
    FoldingRangeKind, MessageContent, Position, Range, Symbol, SymbolKind,
};

/// Sentinel level for non-header symbols (code cells, crossref targets).
/// Larger than any legal header level so [`build_symbol_hierarchy`] nests
/// them under whichever header most recently opened.
const LEAF_LEVEL: usize = 7;

/// Analyze a document, extracting symbols, folding ranges, and diagnostics.
///
/// Internally runs `quarto_core::pipeline::build_analysis_pipeline()` —
/// Parse + MetadataMerge + PreEngineSugaring + AstTransforms (analysis
/// subset). Meta shortcodes in headers are resolved after the pipeline so
/// `# {{< meta title >}}` shows its resolved value in the outline.
pub fn analyze_document(doc: &Document) -> DocumentAnalysis {
    pollster::block_on(analyze_document_async(doc))
}

/// Async variant of [`analyze_document`]. The sync entry point blocks on
/// this; it is exposed for callers that are already in an async context.
pub async fn analyze_document_async(doc: &Document) -> DocumentAnalysis {
    // The pipeline wants a concrete path on the filesystem-style VFS. A
    // virtual `/input.qmd` matches the WASM render path's convention and
    // avoids pulling in whatever absolute path the editor uses.
    let virtual_path = PathBuf::from("/input.qmd");

    let project = minimal_project_context(&virtual_path);
    let document = DocumentInfo::from_path(&virtual_path);
    let binaries = BinaryDependencies::new();
    let format = Format::from_format_string("html").unwrap_or_else(|_| Format::default());

    let mut render_ctx = RenderContext::new(&project, &document, &format, &binaries);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(default_runtime());

    let mut stage_ctx = match StageContext::new(
        runtime,
        render_ctx.format.clone(),
        render_ctx.project.clone(),
        render_ctx.document.clone(),
    ) {
        Ok(ctx) => ctx,
        Err(e) => {
            // Creating the stage context should not fail for an in-memory
            // single-file doc, but propagate the failure rather than panic.
            let source_context = doc.create_source_context();
            let diag = DiagnosticMessage::error(format!("analysis setup failed: {e}"));
            return DocumentAnalysis::with_data(
                Vec::new(),
                Vec::new(),
                diagnostics_from_messages(std::slice::from_ref(&diag), &source_context),
                source_context,
            );
        }
    };
    // Artifacts flow through RenderContext in the standard pipeline; we
    // don't use them but we transfer them for symmetry in case a future
    // analysis transform starts emitting artifacts.
    stage_ctx.artifacts = std::mem::take(&mut render_ctx.artifacts);

    let input = PipelineData::LoadedSource(LoadedSource::new(
        virtual_path.clone(),
        doc.content_bytes().to_vec(),
    ));

    let pipeline = build_analysis_pipeline();

    match pipeline.run(input, &mut stage_ctx).await {
        Ok(output) => match output.into_document_ast() {
            Some(doc_ast) => {
                let mut pandoc = doc_ast.ast;
                let source_context = doc_ast.source_context;
                let mut pipeline_diagnostics = doc_ast.warnings;
                pipeline_diagnostics.extend(std::mem::take(&mut stage_ctx.diagnostics));

                // Run meta shortcode resolution after the pipeline so
                // header content like `# {{< meta title >}}` shows the
                // resolved value in the outline. This is kept outside the
                // analysis transform pipeline because `MetaShortcodeTransform`
                // implements `AnalysisTransform` (from `quarto-analysis`),
                // not `AstTransform` (from `quarto-core`).
                let mut analysis_ctx = DocumentAnalysisContext::new();
                let transforms: Vec<&dyn AnalysisTransform> = vec![&MetaShortcodeTransform];
                let _ = run_analysis_transforms(&mut pandoc, &mut analysis_ctx, &transforms);

                let symbols = extract_symbols(&pandoc, &source_context);
                let folding_ranges =
                    extract_folding_ranges(&pandoc, &source_context, doc.content());

                let mut diagnostics =
                    diagnostics_from_messages(&pipeline_diagnostics, &source_context);
                for diag in analysis_ctx.diagnostics() {
                    if let Some(d) = convert_diagnostic(diag, &source_context) {
                        diagnostics.push(d);
                    }
                }

                DocumentAnalysis::with_data(symbols, folding_ranges, diagnostics, source_context)
            }
            None => {
                let source_context = doc.create_source_context();
                let diag = DiagnosticMessage::error(
                    "analysis pipeline did not produce a DocumentAst".to_string(),
                );
                DocumentAnalysis::with_data(
                    Vec::new(),
                    Vec::new(),
                    diagnostics_from_messages(std::slice::from_ref(&diag), &source_context),
                    source_context,
                )
            }
        },
        Err(err) => {
            // Parse errors carry their own source context (attached by the
            // parse stage). The pipeline error type does not expose it
            // directly, so rebuild one from the document. Any parse-time
            // diagnostics surface via `StageContext::diagnostics` which
            // pollster already drained above — except here, where the
            // pipeline returned before diagnostic transfer.
            let source_context = doc.create_source_context();
            let mut diagnostics =
                diagnostics_from_messages(&stage_ctx.diagnostics, &source_context);
            if diagnostics.is_empty() {
                let diag = DiagnosticMessage::error(err.to_string());
                diagnostics.extend(diagnostics_from_messages(
                    std::slice::from_ref(&diag),
                    &source_context,
                ));
            }
            DocumentAnalysis::with_data(Vec::new(), Vec::new(), diagnostics, source_context)
        }
    }
}

fn minimal_project_context(document_path: &Path) -> ProjectContext {
    let dir = document_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/"));
    ProjectContext {
        dir: dir.clone(),
        config: ProjectConfig::default(),
        is_single_file: true,
        files: vec![DocumentInfo::from_path(document_path)],
        output_dir: dir,
    }
}

fn diagnostics_from_messages(
    messages: &[DiagnosticMessage],
    ctx: &SourceContext,
) -> Vec<Diagnostic> {
    messages
        .iter()
        .filter_map(|msg| convert_diagnostic(msg, ctx))
        .collect()
}

// ============================================================================
// Symbol Extraction
// ============================================================================

pub(crate) fn extract_symbols(pandoc: &Pandoc, ctx: &SourceContext) -> Vec<Symbol> {
    let mut flat_symbols: Vec<(usize, Symbol)> = Vec::new();
    collect_symbols_from_blocks(&pandoc.blocks, ctx, &mut flat_symbols);
    build_symbol_hierarchy(flat_symbols)
}

fn collect_symbols_from_blocks(
    blocks: &[Block],
    ctx: &SourceContext,
    symbols: &mut Vec<(usize, Symbol)>,
) {
    for block in blocks {
        // Crossref targets are CustomNodes whose plain_data carries the
        // (ref_type, kind, identifier) triple. `crossref_target_view`
        // unifies FloatRefTarget / Theorem / Proof / crossref-Callout
        // recognition.
        if let Some(view) = crossref_target_view(block) {
            // Q4: crossref targets do not contribute nested outline
            // entries. The inner structure (e.g. `## Line` absorbed into
            // a theorem title) is semantically part of the target.
            // Skip recursion whether or not the id is well-formed.
            if is_well_formed_identifier(view.identifier) {
                if let Some(symbol) = crossref_symbol_from_view(block, view, ctx) {
                    symbols.push((LEAF_LEVEL, symbol));
                }
            }
            continue;
        }

        match block {
            Block::Header(header) => {
                if let Some(symbol) = header_to_symbol(header, ctx) {
                    symbols.push((header.level, symbol));
                }
            }
            Block::CodeBlock(code_block) => {
                if let Some(symbol) = code_block_to_symbol(code_block, ctx) {
                    symbols.push((LEAF_LEVEL, symbol));
                }
            }
            // Paragraphs can contain inline crossref targets — labelled
            // display equations end up as `Inline::Custom("Equation")`
            // inside a paragraph after `EquationLabelTransform` runs.
            Block::Paragraph(p) => {
                collect_crossref_symbols_from_inlines(&p.content, ctx, symbols);
            }
            Block::Plain(p) => {
                collect_crossref_symbols_from_inlines(&p.content, ctx, symbols);
            }
            Block::Div(div) => {
                collect_symbols_from_blocks(&div.content, ctx, symbols);
            }
            Block::BlockQuote(bq) => {
                collect_symbols_from_blocks(&bq.content, ctx, symbols);
            }
            Block::Figure(fig) => {
                collect_symbols_from_blocks(&fig.content, ctx, symbols);
            }
            Block::Custom(custom) => {
                // Non-crossref custom nodes (plain callouts, tabsets, …)
                // may still contain headers we want in the outline.
                for slot in custom.slots.values() {
                    if let Slot::Blocks(bs) = slot {
                        collect_symbols_from_blocks(bs, ctx, symbols);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Scan a list of inlines for inline-level crossref custom nodes
/// (equations) and emit outline symbols for each.
fn collect_crossref_symbols_from_inlines(
    inlines: &Inlines,
    ctx: &SourceContext,
    symbols: &mut Vec<(usize, Symbol)>,
) {
    for inline in inlines {
        let Some(view) = crossref_target_view_inline(inline) else {
            continue;
        };
        if !is_well_formed_identifier(view.identifier) {
            continue;
        }
        let Inline::Custom(node) = inline else {
            continue;
        };
        let Some(range) = source_info_to_range(&node.source_info, ctx) else {
            continue;
        };
        let mut symbol = Symbol::new(view.identifier, SymbolKind::Class, range, range);
        if let Some(d) = format_crossref_detail(node, view.kind) {
            symbol = symbol.with_detail(d);
        }
        symbols.push((LEAF_LEVEL, symbol));
    }
}

/// Return `true` for identifiers whose suffix is non-empty.
///
/// `classify_cite_id` deliberately accepts prefix-only ids like `"fig-"`
/// (see its doc comment) — it treats the emptiness as the caller's
/// diagnostic concern. For the outline, an empty-suffix id is *not* a
/// navigable target: there is no way for an author to reference a `@fig-`.
/// Gate these out so they do not appear in the outline.
fn is_well_formed_identifier(identifier: &str) -> bool {
    match identifier.split_once('-') {
        Some((_prefix, rest)) => !rest.is_empty(),
        None => false,
    }
}

/// Convert a crossref `CustomNode` into an outline symbol.
///
/// - `name` = identifier (e.g. `"fig-one"`)
/// - `detail` = rendered label (e.g. `"Figure 1: This is the caption"`)
/// - `kind` = [`SymbolKind::Class`]
fn crossref_symbol_from_view(
    block: &Block,
    view: CrossrefTargetView<'_>,
    ctx: &SourceContext,
) -> Option<Symbol> {
    let Block::Custom(node) = block else {
        return None;
    };

    let range = source_info_to_range(&node.source_info, ctx)?;
    let detail = format_crossref_detail(node, view.kind);

    let mut symbol = Symbol::new(view.identifier, SymbolKind::Class, range, range);
    if let Some(d) = detail {
        symbol = symbol.with_detail(d);
    }
    Some(symbol)
}

/// Build the user-visible label for a crossref entry.
///
/// Matches the shapes `CrossrefRenderTransform` produces:
///
/// - Float targets (figure/table/listing): `"Figure 1: <caption text>"`,
///   or `"Figure 1"` if the target has no caption.
/// - Theorem/proof: `"Theorem 1: <title text>"`, or `"Theorem 1"` if
///   untitled. `Proof` nodes carry their own optional title inline.
/// - Missing number (unnumbered / index transform not run): fall back to
///   kind-only (`"Figure"` / `"Theorem"`), preserving any caption tail.
fn format_crossref_detail(node: &pampa::pandoc::custom::CustomNode, kind: &str) -> Option<String> {
    let order = node
        .plain_data
        .get("order")
        .and_then(|o| o.get("order"))
        .and_then(|n| n.as_u64());

    let caption_or_title = crossref_label_tail(node);

    let prefix = match order {
        Some(n) => format!("{kind} {n}"),
        None => kind.to_string(),
    };

    match caption_or_title {
        Some(text) if !text.trim().is_empty() => Some(format!("{prefix}: {}", text.trim())),
        _ => {
            // Kind alone carries no extra information when order is also
            // missing — omit detail entirely to avoid the redundant
            // `"Figure"` next to `name = "fig-foo"` in that edge case.
            order.map(|_| prefix)
        }
    }
}

/// Extract the text that comes after the label prefix in a crossref detail.
///
/// For `FloatRefTarget`: prefers `caption_short` (inline slot), falls back to
/// the first `Paragraph` inside `caption_long` (blocks slot).
/// For `Theorem` / `Proof`: reads the `title` inline slot.
fn crossref_label_tail(node: &pampa::pandoc::custom::CustomNode) -> Option<String> {
    if let Some(Slot::Inlines(title)) = node.slots.get("title") {
        if !title.is_empty() {
            return Some(inlines_to_text(title));
        }
    }
    if let Some(Slot::Inlines(short)) = node.slots.get("caption_short") {
        if !short.is_empty() {
            return Some(inlines_to_text(short));
        }
    }
    if let Some(Slot::Blocks(long)) = node.slots.get("caption_long") {
        for b in long {
            if let Block::Paragraph(p) = b {
                return Some(inlines_to_text(&p.content));
            }
        }
    }
    None
}

fn header_to_symbol(header: &Header, ctx: &SourceContext) -> Option<Symbol> {
    let name = inlines_to_text(&header.content);
    if name.is_empty() {
        return None;
    }
    let range = source_info_to_range(&header.source_info, ctx)?;
    Some(Symbol::new(name, SymbolKind::String, range, range))
}

fn code_block_to_symbol(code_block: &CodeBlock, ctx: &SourceContext) -> Option<Symbol> {
    let (id, classes, attrs) = &code_block.attr;
    let executable_languages = [
        "r", "python", "julia", "bash", "sh", "sql", "ojs", "dot", "mermaid",
    ];
    let is_executable = classes.iter().any(|c| {
        let c_lower = c.to_lowercase();
        executable_languages.contains(&c_lower.as_str())
    });
    if !is_executable {
        return None;
    }

    let language = classes
        .iter()
        .find(|c| {
            let c_lower = c.to_lowercase();
            executable_languages.contains(&c_lower.as_str())
        })
        .cloned()
        .unwrap_or_else(|| "code".to_string());

    let name = if let Some(label) = attrs.get("label") {
        format!("{language}: {label}")
    } else if !id.is_empty() {
        format!("{language}: {id}")
    } else {
        format!("{language} cell")
    };

    let range = source_info_to_range(&code_block.source_info, ctx)?;
    Some(
        Symbol::new(name, SymbolKind::Function, range, range)
            .with_detail(format!("{} lines", code_block.text.lines().count())),
    )
}

fn build_symbol_hierarchy(flat_symbols: Vec<(usize, Symbol)>) -> Vec<Symbol> {
    if flat_symbols.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<Symbol> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new();

    for (level, symbol) in flat_symbols {
        while let Some(&(stack_level, _)) = stack.last() {
            if stack_level >= level {
                stack.pop();
            } else {
                break;
            }
        }

        if let Some(&(_, parent_idx)) = stack.last() {
            add_child_to_symbol(&mut result, parent_idx, symbol);
            if level < LEAF_LEVEL {
                let child_idx = get_last_child_index(&result, parent_idx);
                stack.push((level, child_idx));
            }
        } else {
            result.push(symbol);
            if level < LEAF_LEVEL {
                stack.push((level, result.len() - 1));
            }
        }
    }

    result
}

fn add_child_to_symbol(symbols: &mut [Symbol], parent_idx: usize, child: Symbol) {
    if parent_idx < symbols.len() {
        symbols[parent_idx].children.push(child);
    }
}

fn get_last_child_index(_symbols: &[Symbol], parent_idx: usize) -> usize {
    parent_idx
}

// ============================================================================
// Folding Range Extraction
// ============================================================================

pub(crate) fn extract_folding_ranges(
    pandoc: &Pandoc,
    ctx: &SourceContext,
    content: &str,
) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();

    if let Some(range) = extract_yaml_frontmatter_range(content) {
        ranges.push(range);
    }

    extract_folding_ranges_from_blocks(&pandoc.blocks, ctx, content, &mut ranges);
    ranges
}

fn extract_yaml_frontmatter_range(content: &str) -> Option<FoldingRange> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        return None;
    }
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            return Some(FoldingRange::with_kind(
                0,
                i as u32,
                FoldingRangeKind::Region,
            ));
        }
    }
    None
}

fn extract_folding_ranges_from_blocks(
    blocks: &[Block],
    ctx: &SourceContext,
    content: &str,
    ranges: &mut Vec<FoldingRange>,
) {
    let mut header_stack: Vec<(usize, u32)> = Vec::new();

    for block in blocks {
        match block {
            Block::Header(header) => {
                while let Some(&(stack_level, start_line)) = header_stack.last() {
                    if stack_level >= header.level {
                        if let Some(current_line) = get_start_line(&header.source_info, ctx) {
                            if current_line > start_line + 1 {
                                ranges.push(FoldingRange::with_kind(
                                    start_line,
                                    current_line - 1,
                                    FoldingRangeKind::Region,
                                ));
                            }
                        }
                        header_stack.pop();
                    } else {
                        break;
                    }
                }
                if let Some(start_line) = get_start_line(&header.source_info, ctx) {
                    header_stack.push((header.level, start_line));
                }
            }
            Block::CodeBlock(code_block) => {
                if let Some(range) = code_block_to_folding_range(code_block, ctx) {
                    ranges.push(range);
                }
            }
            Block::Div(div) => {
                extract_folding_ranges_from_blocks(&div.content, ctx, content, ranges);
            }
            Block::BlockQuote(bq) => {
                extract_folding_ranges_from_blocks(&bq.content, ctx, content, ranges);
            }
            Block::Figure(fig) => {
                extract_folding_ranges_from_blocks(&fig.content, ctx, content, ranges);
            }
            Block::Custom(custom) => {
                for slot in custom.slots.values() {
                    if let Slot::Blocks(bs) = slot {
                        extract_folding_ranges_from_blocks(bs, ctx, content, ranges);
                    }
                }
            }
            _ => {}
        }
    }

    let total_lines = content.lines().count() as u32;
    for (_, start_line) in header_stack {
        if total_lines > start_line + 1 {
            ranges.push(FoldingRange::with_kind(
                start_line,
                total_lines.saturating_sub(1),
                FoldingRangeKind::Region,
            ));
        }
    }
}

fn code_block_to_folding_range(
    code_block: &CodeBlock,
    ctx: &SourceContext,
) -> Option<FoldingRange> {
    let start = code_block.source_info.map_offset(0, ctx)?;
    let end = code_block
        .source_info
        .map_offset(code_block.source_info.length(), ctx)
        .or_else(|| {
            code_block
                .source_info
                .map_offset(code_block.source_info.length().saturating_sub(1), ctx)
        })?;

    Some(FoldingRange::with_kind(
        start.location.row as u32,
        end.location.row as u32,
        FoldingRangeKind::Region,
    ))
}

fn get_start_line(source_info: &quarto_source_map::SourceInfo, ctx: &SourceContext) -> Option<u32> {
    source_info
        .map_offset(0, ctx)
        .map(|loc| loc.location.row as u32)
}

// ============================================================================
// Diagnostic Conversion
// ============================================================================

fn convert_diagnostic(msg: &DiagnosticMessage, ctx: &SourceContext) -> Option<Diagnostic> {
    let range = if let Some(loc) = &msg.location {
        source_info_to_range_diag(loc, ctx)
    } else {
        Range::default()
    };

    let mut diagnostic = Diagnostic::new(
        range,
        DiagnosticSeverity::from_diagnostic_kind(msg.kind),
        msg.title.clone(),
    );

    if let Some(code) = &msg.code {
        diagnostic = diagnostic.with_code(code.clone());
    }
    if let Some(problem) = &msg.problem {
        diagnostic = diagnostic.with_problem(MessageContent::from(problem));
    }
    for detail in &msg.details {
        let detail_range = detail
            .location
            .as_ref()
            .map(|loc| source_info_to_range_diag(loc, ctx));
        let diag_detail = if let Some(r) = detail_range {
            DiagnosticDetail::with_range(
                DetailKind::from(detail.kind),
                MessageContent::from(&detail.content),
                r,
            )
        } else {
            DiagnosticDetail::new(
                DetailKind::from(detail.kind),
                MessageContent::from(&detail.content),
            )
        };
        diagnostic = diagnostic.with_detail(diag_detail);
    }
    for hint in &msg.hints {
        diagnostic = diagnostic.with_hint(MessageContent::from(hint));
    }

    Some(diagnostic)
}

// ============================================================================
// Source-range and text helpers
// ============================================================================

fn source_info_to_range(
    source_info: &quarto_source_map::SourceInfo,
    ctx: &SourceContext,
) -> Option<Range> {
    let start = source_info.map_offset(0, ctx)?;
    let end = source_info
        .map_offset(source_info.length(), ctx)
        .or_else(|| source_info.map_offset(source_info.length().saturating_sub(1), ctx))
        .unwrap_or_else(|| start.clone());
    Some(Range::new(
        Position::new(start.location.row as u32, start.location.column as u32),
        Position::new(end.location.row as u32, end.location.column as u32),
    ))
}

fn source_info_to_range_diag(loc: &quarto_source_map::SourceInfo, ctx: &SourceContext) -> Range {
    let start_mapped = loc.map_offset(0, ctx);
    let end_mapped = loc
        .map_offset(loc.length(), ctx)
        .or_else(|| {
            if loc.length() > 0 {
                loc.map_offset(loc.length().saturating_sub(1), ctx)
            } else {
                None
            }
        })
        .or_else(|| start_mapped.clone());

    match (start_mapped, end_mapped) {
        (Some(start), Some(end)) => Range::new(
            Position::new(start.location.row as u32, start.location.column as u32),
            Position::new(end.location.row as u32, end.location.column as u32),
        ),
        (Some(start), None) => {
            let pos = Position::new(start.location.row as u32, start.location.column as u32);
            Range::point(pos)
        }
        _ => Range::default(),
    }
}

fn inlines_to_text(inlines: &Inlines) -> String {
    let mut text = String::new();
    for inline in inlines {
        inline_to_text(inline, &mut text);
    }
    text.trim().to_string()
}

fn inline_to_text(inline: &Inline, text: &mut String) {
    match inline {
        Inline::Str(s) => text.push_str(&s.text),
        Inline::Space(_) => text.push(' '),
        Inline::SoftBreak(_) => text.push(' '),
        Inline::LineBreak(_) => {}
        Inline::Emph(emph) => {
            for child in &emph.content {
                inline_to_text(child, text);
            }
        }
        Inline::Strong(strong) => {
            for child in &strong.content {
                inline_to_text(child, text);
            }
        }
        Inline::Strikeout(s) => {
            for child in &s.content {
                inline_to_text(child, text);
            }
        }
        Inline::Superscript(s) => {
            for child in &s.content {
                inline_to_text(child, text);
            }
        }
        Inline::Subscript(s) => {
            for child in &s.content {
                inline_to_text(child, text);
            }
        }
        Inline::SmallCaps(s) => {
            for child in &s.content {
                inline_to_text(child, text);
            }
        }
        Inline::Quoted(q) => {
            for child in &q.content {
                inline_to_text(child, text);
            }
        }
        Inline::Link(link) => {
            for child in &link.content {
                inline_to_text(child, text);
            }
        }
        Inline::Span(span) => {
            for child in &span.content {
                inline_to_text(child, text);
            }
        }
        Inline::Code(code) => {
            text.push_str(&code.text);
        }
        Inline::Math(math) => {
            text.push_str(&math.text);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_document_basic() {
        let doc = Document::new(
            "test.qmd",
            r#"---
title: "Test"
---

# Section 1

Some content.

## Subsection 1.1

More content.

```{python}
print("hello")
```

# Section 2

Final content.
"#,
        );

        let analysis = analyze_document(&doc);
        assert!(!analysis.symbols.is_empty(), "Should have symbols");
        assert!(
            !analysis.folding_ranges.is_empty(),
            "Should have folding ranges"
        );

        let errors: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "Valid document should have no errors");
    }

    #[test]
    fn yaml_frontmatter_folding_range() {
        let content = r#"---
title: "Test"
author: "Author"
---

# Content
"#;
        let range = extract_yaml_frontmatter_range(content);
        let range = range.expect("Should detect YAML frontmatter");
        assert_eq!(range.start_line, 0);
        assert_eq!(range.end_line, 3);
    }

    #[test]
    fn no_yaml_frontmatter() {
        let content = "# Just a header\n\nSome content.";
        assert!(extract_yaml_frontmatter_range(content).is_none());
    }

    #[test]
    fn meta_shortcode_resolved_in_outline() {
        let doc = Document::new(
            "test.qmd",
            r#"---
title: "My Document Title"
author: "Alice"
---

# {{< meta title >}}

Some content.

## Written by {{< meta author >}}

More content.
"#,
        );

        let analysis = analyze_document(&doc);
        assert_eq!(analysis.symbols.len(), 1, "Should have 1 top-level section");
        assert_eq!(analysis.symbols[0].name, "My Document Title");
        assert_eq!(analysis.symbols[0].children.len(), 1);
        assert_eq!(analysis.symbols[0].children[0].name, "Written by Alice");
    }
}
