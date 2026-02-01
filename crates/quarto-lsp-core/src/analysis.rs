//! Unified document analysis for extracting all intelligence data.
//!
//! This module provides `analyze_document()` which performs a single parse
//! and extracts symbols, folding ranges, and diagnostics together.
//! This is more efficient than calling separate functions when you need
//! multiple pieces of data.
//!
//! ## Analysis Transforms
//!
//! Before extracting symbols, this module runs analysis transforms from
//! `quarto-analysis` that can run at "LSP speed" (no I/O, no code execution).
//! Currently this includes:
//!
//! - `MetaShortcodeTransform` - Resolves `{{< meta key >}}` shortcodes so that
//!   headers like `# {{< meta title >}}` appear correctly in the outline.

use crate::document::Document;
use crate::types::{
    DetailKind, Diagnostic, DiagnosticDetail, DiagnosticSeverity, DocumentAnalysis, FoldingRange,
    FoldingRangeKind, MessageContent, Position, Range, Symbol, SymbolKind,
};
use pampa::pandoc::{Block, CodeBlock, Header, Inline, Inlines, Pandoc};
use quarto_analysis::DocumentAnalysisContext;
use quarto_analysis::transforms::{
    AnalysisTransform, MetaShortcodeTransform, run_analysis_transforms,
};
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceContext;

/// Analyze a document, extracting all intelligence data in a single parse.
///
/// This is the primary entry point for document analysis. It returns:
/// - Symbols for document outline and navigation
/// - Folding ranges for code folding
/// - Diagnostics for errors and warnings
///
/// Before extracting symbols, this function runs analysis transforms to resolve
/// shortcodes and other constructs that affect the document outline.
///
/// # Example
///
/// ```rust,ignore
/// use quarto_lsp_core::{Document, analyze_document};
///
/// let doc = Document::new("test.qmd", content);
/// let analysis = analyze_document(&doc);
///
/// println!("Found {} symbols", analysis.symbols.len());
/// println!("Found {} folding ranges", analysis.folding_ranges.len());
/// println!("Found {} diagnostics", analysis.diagnostics.len());
/// ```
pub fn analyze_document(doc: &Document) -> DocumentAnalysis {
    let source_context = doc.create_source_context();

    // Parse with pampa (single parse for all analysis)
    let result = pampa::readers::qmd::read(
        doc.content_bytes(),
        false, // loose mode
        doc.filename(),
        &mut std::io::sink(), // discard verbose output
        true,                 // prune_errors
        None,                 // parent_source_info
    );

    match result {
        Ok((mut pandoc, _ast_context, warnings)) => {
            // Run analysis transforms to resolve shortcodes, etc.
            let mut analysis_ctx = DocumentAnalysisContext::new(source_context.clone());
            let transforms: Vec<&dyn AnalysisTransform> = vec![&MetaShortcodeTransform];
            let _ = run_analysis_transforms(&mut pandoc, &mut analysis_ctx, &transforms);

            // Extract all data from the transformed AST
            let symbols = extract_symbols(&pandoc, &source_context, doc.content());
            let folding_ranges = extract_folding_ranges(&pandoc, &source_context, doc.content());

            // Collect diagnostics from both parsing and analysis transforms
            let mut diagnostics: Vec<Diagnostic> = warnings
                .iter()
                .filter_map(|msg| convert_diagnostic(msg, &source_context))
                .collect();

            // Add diagnostics from analysis transforms
            for diag in analysis_ctx.diagnostics() {
                if let Some(d) = convert_diagnostic(diag, &source_context) {
                    diagnostics.push(d);
                }
            }

            DocumentAnalysis::with_data(symbols, folding_ranges, diagnostics, source_context)
        }
        Err(errors) => {
            // Parsing failed - return diagnostics but empty symbols/folding ranges
            let diagnostics = errors
                .iter()
                .filter_map(|msg| convert_diagnostic(msg, &source_context))
                .collect();

            DocumentAnalysis::with_data(Vec::new(), Vec::new(), diagnostics, source_context)
        }
    }
}

// ============================================================================
// Symbol Extraction
// ============================================================================

/// Extract symbols from a parsed Pandoc document.
fn extract_symbols(pandoc: &Pandoc, ctx: &SourceContext, content: &str) -> Vec<Symbol> {
    let mut flat_symbols: Vec<(usize, Symbol)> = Vec::new();

    // First pass: collect all symbols with their header levels
    collect_symbols_from_blocks(&pandoc.blocks, ctx, content, &mut flat_symbols);

    // Second pass: build hierarchy based on header levels
    build_symbol_hierarchy(flat_symbols)
}

/// Recursively collect symbols from a list of blocks.
fn collect_symbols_from_blocks(
    blocks: &[Block],
    ctx: &SourceContext,
    content: &str,
    symbols: &mut Vec<(usize, Symbol)>,
) {
    for block in blocks {
        match block {
            Block::Header(header) => {
                if let Some(symbol) = header_to_symbol(header, ctx, content) {
                    symbols.push((header.level, symbol));
                }
            }
            Block::CodeBlock(code_block) => {
                if let Some(symbol) = code_block_to_symbol(code_block, ctx) {
                    // Code blocks are at the deepest level (level 7, below h6)
                    symbols.push((7, symbol));
                }
            }
            // Recursively process nested blocks
            Block::Div(div) => {
                collect_symbols_from_blocks(&div.content, ctx, content, symbols);
            }
            Block::BlockQuote(bq) => {
                collect_symbols_from_blocks(&bq.content, ctx, content, symbols);
            }
            Block::Figure(fig) => {
                collect_symbols_from_blocks(&fig.content, ctx, content, symbols);
            }
            Block::Custom(custom) => {
                for slot in custom.slots.values() {
                    if let pampa::pandoc::custom::Slot::Blocks(blocks) = slot {
                        collect_symbols_from_blocks(blocks, ctx, content, symbols);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Convert a header to a Symbol.
fn header_to_symbol(header: &Header, ctx: &SourceContext, content: &str) -> Option<Symbol> {
    let name = inlines_to_text(&header.content);
    if name.is_empty() {
        return None;
    }

    let range = source_info_to_range(&header.source_info, ctx, content)?;
    let selection_range = range;

    Some(Symbol::new(
        name,
        SymbolKind::String,
        range,
        selection_range,
    ))
}

/// Convert a code block to a Symbol (only for executable code blocks).
fn code_block_to_symbol(code_block: &CodeBlock, ctx: &SourceContext) -> Option<Symbol> {
    let (id, classes, attrs) = &code_block.attr;

    // Known executable languages
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

    // Build the name: prefer label attribute, then id, then just language
    let name = if let Some(label) = attrs.get("label") {
        format!("{}: {}", language, label)
    } else if !id.is_empty() {
        format!("{}: {}", language, id)
    } else {
        format!("{} cell", language)
    };

    let range = code_block_to_range(code_block, ctx)?;
    let selection_range = range;

    Some(
        Symbol::new(name, SymbolKind::Function, range, selection_range)
            .with_detail(format!("{} lines", code_block.text.lines().count())),
    )
}

/// Build a hierarchical symbol structure from flat symbols with levels.
fn build_symbol_hierarchy(flat_symbols: Vec<(usize, Symbol)>) -> Vec<Symbol> {
    if flat_symbols.is_empty() {
        return Vec::new();
    }

    let mut result: Vec<Symbol> = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new(); // (level, index)

    for (level, symbol) in flat_symbols {
        // Pop items from stack that are at same level or deeper
        while let Some(&(stack_level, _)) = stack.last() {
            if stack_level >= level {
                stack.pop();
            } else {
                break;
            }
        }

        if let Some(&(_, parent_idx)) = stack.last() {
            // Add as child to parent
            add_child_to_symbol(&mut result, parent_idx, symbol.clone());
            // Don't push code blocks onto stack (they can't have children)
            if level < 7 {
                let child_idx = get_last_child_index(&result, parent_idx);
                stack.push((level, child_idx));
            }
        } else {
            // Top-level symbol
            result.push(symbol);
            if level < 7 {
                stack.push((level, result.len() - 1));
            }
        }
    }

    result
}

fn add_child_to_symbol(symbols: &mut Vec<Symbol>, parent_idx: usize, child: Symbol) {
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

/// Extract folding ranges from a parsed Pandoc document.
fn extract_folding_ranges(
    pandoc: &Pandoc,
    ctx: &SourceContext,
    content: &str,
) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();

    // Extract YAML frontmatter folding range
    if let Some(range) = extract_yaml_frontmatter_range(content) {
        ranges.push(range);
    }

    // Extract code block and section folding ranges from blocks
    extract_folding_ranges_from_blocks(&pandoc.blocks, ctx, content, &mut ranges);

    ranges
}

/// Extract the YAML frontmatter folding range from document content.
fn extract_yaml_frontmatter_range(content: &str) -> Option<FoldingRange> {
    let lines: Vec<&str> = content.lines().collect();

    // Check if document starts with ---
    if lines.is_empty() || lines[0].trim() != "---" {
        return None;
    }

    // Find the closing ---
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

/// Recursively extract folding ranges from blocks.
fn extract_folding_ranges_from_blocks(
    blocks: &[Block],
    ctx: &SourceContext,
    content: &str,
    ranges: &mut Vec<FoldingRange>,
) {
    // Track headers for section folding
    let mut header_stack: Vec<(usize, u32)> = Vec::new(); // (level, start_line)

    for block in blocks {
        match block {
            Block::Header(header) => {
                // Close any sections at same or deeper level
                while let Some(&(stack_level, start_line)) = header_stack.last() {
                    if stack_level >= header.level {
                        // Get current header's start line
                        if let Some(current_line) = get_start_line(&header.source_info, ctx) {
                            // Section ends at line before current header
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

                // Push current header onto stack
                if let Some(start_line) = get_start_line(&header.source_info, ctx) {
                    header_stack.push((header.level, start_line));
                }
            }
            Block::CodeBlock(code_block) => {
                // Add folding range for code blocks
                if let Some(range) = code_block_to_folding_range(code_block, ctx) {
                    ranges.push(range);
                }
            }
            // Recursively process nested blocks
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
                    if let pampa::pandoc::custom::Slot::Blocks(blocks) = slot {
                        extract_folding_ranges_from_blocks(blocks, ctx, content, ranges);
                    }
                }
            }
            _ => {}
        }
    }

    // Close any remaining open sections at end of document
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

/// Convert a code block to a folding range.
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

/// Get the start line of a source info.
fn get_start_line(source_info: &quarto_source_map::SourceInfo, ctx: &SourceContext) -> Option<u32> {
    source_info
        .map_offset(0, ctx)
        .map(|loc| loc.location.row as u32)
}

// ============================================================================
// Diagnostic Conversion
// ============================================================================

/// Convert a quarto-error-reporting DiagnosticMessage to our Diagnostic type.
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
// Helper Functions
// ============================================================================

/// Convert source info to a Range (for symbols).
fn source_info_to_range(
    source_info: &quarto_source_map::SourceInfo,
    ctx: &SourceContext,
    _content: &str,
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

/// Convert source info to a Range (for diagnostics).
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

/// Convert a code block to a Range.
fn code_block_to_range(code_block: &CodeBlock, ctx: &SourceContext) -> Option<Range> {
    let start = code_block.source_info.map_offset(0, ctx)?;
    let end = code_block
        .source_info
        .map_offset(code_block.source_info.length(), ctx)
        .or_else(|| {
            code_block
                .source_info
                .map_offset(code_block.source_info.length().saturating_sub(1), ctx)
        })
        .unwrap_or_else(|| start.clone());

    Some(Range::new(
        Position::new(start.location.row as u32, start.location.column as u32),
        Position::new(end.location.row as u32, end.location.column as u32),
    ))
}

/// Extract plain text from a list of inlines.
fn inlines_to_text(inlines: &Inlines) -> String {
    let mut text = String::new();
    for inline in inlines {
        inline_to_text(inline, &mut text);
    }
    text.trim().to_string()
}

/// Recursively extract text from an inline element.
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

        // Should have symbols (headers)
        assert!(!analysis.symbols.is_empty(), "Should have symbols");

        // Should have folding ranges (YAML, code block, sections)
        assert!(
            !analysis.folding_ranges.is_empty(),
            "Should have folding ranges"
        );

        // Should have no errors (valid document)
        let errors: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert!(errors.is_empty(), "Valid document should have no errors");
    }

    #[test]
    fn analyze_document_with_errors() {
        let doc = Document::new(
            "test.qmd",
            r#"---
title: "Test
unclosed: {
---

# Content
"#,
        );

        let analysis = analyze_document(&doc);

        // Should have error diagnostics for invalid YAML
        let errors: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .collect();
        assert!(!errors.is_empty(), "Invalid YAML should produce errors");
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
        assert!(range.is_some(), "Should detect YAML frontmatter");
        let range = range.unwrap();
        assert_eq!(range.start_line, 0);
        // Lines: 0=---, 1=title, 2=author, 3=---
        assert_eq!(range.end_line, 3);
    }

    #[test]
    fn no_yaml_frontmatter() {
        let content = "# Just a header\n\nSome content.";
        let range = extract_yaml_frontmatter_range(content);
        assert!(range.is_none(), "Should not detect YAML without ---");
    }

    #[test]
    fn meta_shortcode_resolved_in_outline() {
        // Test that meta shortcodes are resolved in header symbols
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

        // Should have 2 top-level symbols
        assert_eq!(analysis.symbols.len(), 1, "Should have 1 top-level section");

        // First header should have resolved title
        assert_eq!(
            analysis.symbols[0].name, "My Document Title",
            "Meta shortcode should be resolved to 'My Document Title'"
        );

        // Second header should be a child and have resolved author
        assert_eq!(
            analysis.symbols[0].children.len(),
            1,
            "Should have 1 child section"
        );
        assert_eq!(
            analysis.symbols[0].children[0].name, "Written by Alice",
            "Meta shortcode should be resolved to 'Alice'"
        );
    }

    #[test]
    fn meta_shortcode_missing_key_graceful() {
        // Test that missing meta keys produce diagnostics but don't break the outline
        let doc = Document::new(
            "test.qmd",
            r#"---
title: "Test"
---

# {{< meta nonexistent >}}

Content.
"#,
        );

        let analysis = analyze_document(&doc);

        // Should still have a symbol (with error placeholder)
        assert_eq!(analysis.symbols.len(), 1, "Should have 1 symbol");

        // The symbol should contain the error placeholder
        assert!(
            analysis.symbols[0].name.contains("?meta:nonexistent"),
            "Missing key should produce error placeholder, got: {}",
            analysis.symbols[0].name
        );

        // Should have a warning diagnostic
        let warnings: Vec<_> = analysis
            .diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .collect();
        assert!(!warnings.is_empty(), "Should have warning for missing key");
    }
}
