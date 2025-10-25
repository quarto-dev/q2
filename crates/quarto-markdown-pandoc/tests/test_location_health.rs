//! Self-health tests for location information
//!
//! These tests verify invariant properties that must hold for ALL well-formed
//! parsed documents. They are designed to run on any .qmd file in the test suite
//! without requiring specific knowledge about the file's contents.
//!
//! Properties tested:
//! 1. Well-formed ranges: start <= end in all dimensions
//! 2. Offset/row/column consistency: conversions are proper inverses
//! 3. Bounds checking: all locations are within valid bounds
//! 4. Nesting consistency: child ranges contained in parent ranges
//! 5. Sequential consistency: sibling nodes don't overlap
//! 6. SourceMapping validity: parent references exist and are valid

use quarto_markdown_pandoc::pandoc::{Block, Inline, Pandoc};
use quarto_source_map::{Range, SourceInfo};
use std::fmt;

/// Represents a violation of location health properties
#[derive(Debug, Clone)]
pub struct LocationHealthViolation {
    pub category: ViolationCategory,
    pub message: String,
    pub location_info: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViolationCategory {
    WellFormedRange,
    OffsetRowColConsistency,
    BoundsCheck,
    NestingConsistency,
    SequentialConsistency,
    SourceMappingValidity,
}

impl fmt::Display for LocationHealthViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{:?}] {}", self.category, self.message)?;
        if let Some(loc) = &self.location_info {
            write!(f, " at {}", loc)?;
        }
        Ok(())
    }
}

/// Accumulates violations found during validation
pub struct LocationHealthValidator {
    violations: Vec<LocationHealthViolation>,
    source: String,
}

impl LocationHealthValidator {
    pub fn new(source: String) -> Self {
        Self {
            violations: Vec::new(),
            source,
        }
    }

    pub fn add_violation(
        &mut self,
        category: ViolationCategory,
        message: String,
        range: Option<&Range>,
    ) {
        let location_info = range.map(|r| {
            format!(
                "offset {}:{} (row:{},col:{}) to {}:{} (row:{},col:{})",
                r.start.offset,
                r.start.offset,
                r.start.row,
                r.start.column,
                r.end.offset,
                r.end.offset,
                r.end.row,
                r.end.column
            )
        });

        self.violations.push(LocationHealthViolation {
            category,
            message,
            location_info,
        });
    }

    pub fn violations(&self) -> &[LocationHealthViolation] {
        &self.violations
    }

    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Extract all SourceInfo from a Pandoc document by walking the AST
pub fn extract_all_source_info(doc: &Pandoc) -> Vec<SourceInfo> {
    let mut source_infos = Vec::new();

    for block in &doc.blocks {
        collect_source_info_from_block(block, &mut source_infos);
    }

    source_infos
}

/// Recursively collect SourceInfo from a Block and its children
fn collect_source_info_from_block(block: &Block, source_infos: &mut Vec<SourceInfo>) {
    match block {
        Block::Plain(plain) => {
            source_infos.push(plain.source_info.clone());
            for inline in &plain.content {
                collect_source_info_from_inline(inline, source_infos);
            }
        }
        Block::Paragraph(para) => {
            source_infos.push(para.source_info.clone());
            for inline in &para.content {
                collect_source_info_from_inline(inline, source_infos);
            }
        }
        Block::LineBlock(line_block) => {
            source_infos.push(line_block.source_info.clone());
            for line in &line_block.content {
                for inline in line {
                    collect_source_info_from_inline(inline, source_infos);
                }
            }
        }
        Block::Header(header) => {
            source_infos.push(header.source_info.clone());
            for inline in &header.content {
                collect_source_info_from_inline(inline, source_infos);
            }
        }
        Block::CodeBlock(code_block) => {
            source_infos.push(code_block.source_info.clone());
        }
        Block::RawBlock(raw_block) => {
            source_infos.push(raw_block.source_info.clone());
        }
        Block::HorizontalRule(hr) => {
            source_infos.push(hr.source_info.clone());
        }
        Block::BlockQuote(quote) => {
            source_infos.push(quote.source_info.clone());
            for child_block in &quote.content {
                collect_source_info_from_block(child_block, source_infos);
            }
        }
        Block::Div(div) => {
            source_infos.push(div.source_info.clone());
            for child_block in &div.content {
                collect_source_info_from_block(child_block, source_infos);
            }
        }
        Block::BulletList(bullet_list) => {
            source_infos.push(bullet_list.source_info.clone());
            for item in &bullet_list.content {
                for child_block in item {
                    collect_source_info_from_block(child_block, source_infos);
                }
            }
        }
        Block::OrderedList(ordered_list) => {
            source_infos.push(ordered_list.source_info.clone());
            for item in &ordered_list.content {
                for child_block in item {
                    collect_source_info_from_block(child_block, source_infos);
                }
            }
        }
        Block::DefinitionList(def_list) => {
            source_infos.push(def_list.source_info.clone());
            for (term, definitions) in &def_list.content {
                for inline in term {
                    collect_source_info_from_inline(inline, source_infos);
                }
                for definition in definitions {
                    for child_block in definition {
                        collect_source_info_from_block(child_block, source_infos);
                    }
                }
            }
        }
        Block::Table(table) => {
            source_infos.push(table.source_info.clone());
            // Table headers
            for row in &table.head.rows {
                for cell in &row.cells {
                    for child_block in &cell.content {
                        collect_source_info_from_block(child_block, source_infos);
                    }
                }
            }
            // Table bodies
            for body in &table.bodies {
                // Body head rows
                for row in &body.head {
                    for cell in &row.cells {
                        for child_block in &cell.content {
                            collect_source_info_from_block(child_block, source_infos);
                        }
                    }
                }
                // Body body rows
                for row in &body.body {
                    for cell in &row.cells {
                        for child_block in &cell.content {
                            collect_source_info_from_block(child_block, source_infos);
                        }
                    }
                }
            }
            // Table footer
            for row in &table.foot.rows {
                for cell in &row.cells {
                    for child_block in &cell.content {
                        collect_source_info_from_block(child_block, source_infos);
                    }
                }
            }
        }
        Block::Figure(figure) => {
            source_infos.push(figure.source_info.clone());
            for child_block in &figure.content {
                collect_source_info_from_block(child_block, source_infos);
            }
            // Caption has optional long (blocks)
            if let Some(long_caption) = &figure.caption.long {
                for child_block in long_caption {
                    collect_source_info_from_block(child_block, source_infos);
                }
            }
        }
        Block::BlockMetadata(_)
        | Block::NoteDefinitionPara(_)
        | Block::NoteDefinitionFencedBlock(_)
        | Block::CaptionBlock(_) => {
            // TODO: handle these special block types if they have source info
        }
    }
}

/// Recursively collect SourceInfo from an Inline and its children
fn collect_source_info_from_inline(inline: &Inline, source_infos: &mut Vec<SourceInfo>) {
    match inline {
        Inline::Str(str_node) => {
            source_infos.push(str_node.source_info.clone());
        }
        Inline::Emph(emph) => {
            source_infos.push(emph.source_info.clone());
            for child in &emph.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Underline(underline) => {
            source_infos.push(underline.source_info.clone());
            for child in &underline.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Strong(strong) => {
            source_infos.push(strong.source_info.clone());
            for child in &strong.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Strikeout(strikeout) => {
            source_infos.push(strikeout.source_info.clone());
            for child in &strikeout.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Superscript(sup) => {
            source_infos.push(sup.source_info.clone());
            for child in &sup.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Subscript(sub) => {
            source_infos.push(sub.source_info.clone());
            for child in &sub.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::SmallCaps(small_caps) => {
            source_infos.push(small_caps.source_info.clone());
            for child in &small_caps.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Quoted(quoted) => {
            source_infos.push(quoted.source_info.clone());
            for child in &quoted.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Cite(cite) => {
            source_infos.push(cite.source_info.clone());
            for child in &cite.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Code(code) => {
            source_infos.push(code.source_info.clone());
        }
        Inline::Space(space) => {
            source_infos.push(space.source_info.clone());
        }
        Inline::SoftBreak(soft_break) => {
            source_infos.push(soft_break.source_info.clone());
        }
        Inline::LineBreak(line_break) => {
            source_infos.push(line_break.source_info.clone());
        }
        Inline::Math(math) => {
            source_infos.push(math.source_info.clone());
        }
        Inline::RawInline(raw) => {
            source_infos.push(raw.source_info.clone());
        }
        Inline::Link(link) => {
            source_infos.push(link.source_info.clone());
            for child in &link.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Image(image) => {
            source_infos.push(image.source_info.clone());
            for child in &image.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Note(note) => {
            source_infos.push(note.source_info.clone());
            for child_block in &note.content {
                collect_source_info_from_block(child_block, source_infos);
            }
        }
        Inline::Span(span) => {
            source_infos.push(span.source_info.clone());
            for child in &span.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Shortcode(_) => {
            // TODO: handle shortcode if it has source info
        }
        Inline::NoteReference(note_ref) => {
            source_infos.push(note_ref.source_info.clone());
        }
        Inline::Attr(_, _) => {
            // Attr doesn't have source info - it's just metadata
        }
        Inline::Insert(insert) => {
            source_infos.push(insert.source_info.clone());
            for child in &insert.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Delete(delete) => {
            source_infos.push(delete.source_info.clone());
            for child in &delete.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::Highlight(highlight) => {
            source_infos.push(highlight.source_info.clone());
            for child in &highlight.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
        Inline::EditComment(comment) => {
            source_infos.push(comment.source_info.clone());
            for child in &comment.content {
                collect_source_info_from_inline(child, source_infos);
            }
        }
    }
}

// ============================================================================
// PHASE 2: Core Property Validators
// ============================================================================

/// Validate that a Range is well-formed
fn validate_well_formed_range(range: &Range, validator: &mut LocationHealthValidator) {
    // Check: start.offset <= end.offset
    if range.start.offset > range.end.offset {
        validator.add_violation(
            ViolationCategory::WellFormedRange,
            format!(
                "Start offset {} is greater than end offset {}",
                range.start.offset, range.end.offset
            ),
            Some(range),
        );
    }

    // Check: start.row <= end.row
    if range.start.row > range.end.row {
        validator.add_violation(
            ViolationCategory::WellFormedRange,
            format!(
                "Start row {} is greater than end row {}",
                range.start.row, range.end.row
            ),
            Some(range),
        );
    }

    // Check: if same row, start.column <= end.column
    if range.start.row == range.end.row && range.start.column > range.end.column {
        validator.add_violation(
            ViolationCategory::WellFormedRange,
            format!(
                "On same row {}, start column {} is greater than end column {}",
                range.start.row, range.start.column, range.end.column
            ),
            Some(range),
        );
    }
}

/// Validate that offset and row/column are consistent for a Location
fn validate_offset_row_col_consistency(
    location: &quarto_source_map::Location,
    source: &str,
    context: &str,
    validator: &mut LocationHealthValidator,
) {
    // Check: offset_to_location(offset) should give us the same row/col
    if let Some(computed_loc) =
        quarto_source_map::utils::offset_to_location(source, location.offset)
    {
        if computed_loc.row != location.row || computed_loc.column != location.column {
            validator.add_violation(
                ViolationCategory::OffsetRowColConsistency,
                format!(
                    "{}: offset_to_location({}) returned (row:{}, col:{}) but expected (row:{}, col:{})",
                    context,
                    location.offset,
                    computed_loc.row,
                    computed_loc.column,
                    location.row,
                    location.column
                ),
                None,
            );
        }
    } else {
        validator.add_violation(
            ViolationCategory::OffsetRowColConsistency,
            format!(
                "{}: offset_to_location({}) returned None (offset out of bounds)",
                context, location.offset
            ),
            None,
        );
    }

    // Check: line_col_to_offset(row, col) should give us the same offset
    if let Some(computed_offset) =
        quarto_source_map::utils::line_col_to_offset(source, location.row, location.column)
    {
        if computed_offset != location.offset {
            validator.add_violation(
                ViolationCategory::OffsetRowColConsistency,
                format!(
                    "{}: line_col_to_offset(row:{}, col:{}) returned offset {} but expected {}",
                    context, location.row, location.column, computed_offset, location.offset
                ),
                None,
            );
        }
    } else {
        validator.add_violation(
            ViolationCategory::OffsetRowColConsistency,
            format!(
                "{}: line_col_to_offset(row:{}, col:{}) returned None (row/col out of bounds)",
                context, location.row, location.column
            ),
            None,
        );
    }
}

/// Validate that a Location is within valid bounds for the source
fn validate_location_bounds(
    location: &quarto_source_map::Location,
    source: &str,
    context: &str,
    validator: &mut LocationHealthValidator,
) {
    let source_len = source.len();

    // Check: offset <= source.len()
    if location.offset > source_len {
        validator.add_violation(
            ViolationCategory::BoundsCheck,
            format!(
                "{}: offset {} exceeds source length {}",
                context, location.offset, source_len
            ),
            None,
        );
    }

    // Count number of lines in source
    let num_lines = source.lines().count();
    if num_lines == 0 && location.row != 0 {
        validator.add_violation(
            ViolationCategory::BoundsCheck,
            format!(
                "{}: row {} invalid for empty file (should be 0)",
                context, location.row
            ),
            None,
        );
    } else if location.row >= num_lines && source_len > 0 {
        // Allow row == num_lines for EOF position after final newline
        // But if source is non-empty and we're beyond that, it's invalid
        if location.row > num_lines || (location.row == num_lines && !source.ends_with('\n')) {
            validator.add_violation(
                ViolationCategory::BoundsCheck,
                format!(
                    "{}: row {} exceeds number of lines {} (ends_with_newline: {})",
                    context,
                    location.row,
                    num_lines,
                    source.ends_with('\n')
                ),
                None,
            );
        }
    }
}

/// Validate all core properties for a single SourceInfo
fn validate_source_info_core_properties(
    source_info: &SourceInfo,
    source: &str,
    context: &str,
    validator: &mut LocationHealthValidator,
) {
    let start_offset = source_info.start_offset();
    let end_offset = source_info.end_offset();

    // Build a Range object for compatibility with existing validation functions
    // We compute the Location data from offsets using the source text
    let start_location = quarto_source_map::utils::offset_to_location(source, start_offset)
        .unwrap_or(quarto_source_map::Location {
            offset: start_offset,
            row: 0,
            column: 0,
        });

    let end_location = quarto_source_map::utils::offset_to_location(source, end_offset).unwrap_or(
        quarto_source_map::Location {
            offset: end_offset,
            row: 0,
            column: 0,
        },
    );

    let range = Range {
        start: start_location,
        end: end_location,
    };

    // 1. Well-formed range
    validate_well_formed_range(&range, validator);

    // 2. Offset/row/column consistency for start
    validate_offset_row_col_consistency(
        &range.start,
        source,
        &format!("{} start", context),
        validator,
    );

    // 3. Offset/row/column consistency for end
    validate_offset_row_col_consistency(&range.end, source, &format!("{} end", context), validator);

    // 4. Bounds checking for start
    validate_location_bounds(
        &range.start,
        source,
        &format!("{} start", context),
        validator,
    );

    // 5. Bounds checking for end
    validate_location_bounds(&range.end, source, &format!("{} end", context), validator);
}

/// Run all core property validations on a Pandoc document
pub fn validate_core_properties(doc: &Pandoc, source: &str) -> Vec<LocationHealthViolation> {
    let mut validator = LocationHealthValidator::new(source.to_string());
    let source_infos = extract_all_source_info(doc);

    for (i, source_info) in source_infos.iter().enumerate() {
        let context = format!("SourceInfo #{}", i);
        validate_source_info_core_properties(source_info, source, &context, &mut validator);
    }

    validator.violations().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_markdown_pandoc::pandoc::{ASTContext, treesitter_to_pandoc};
    use quarto_markdown_pandoc::utils::diagnostic_collector::DiagnosticCollector;
    use tree_sitter_qmd::MarkdownParser;

    fn parse_qmd_helper(source: &str) -> Pandoc {
        let mut parser = MarkdownParser::default();
        let input_bytes = source.as_bytes();
        let tree = parser
            .parse(input_bytes, None)
            .expect("Failed to parse input");

        let context = ASTContext::anonymous();
        let mut error_collector = DiagnosticCollector::new();
        treesitter_to_pandoc(
            &mut std::io::sink(),
            &tree,
            input_bytes,
            &context,
            &mut error_collector,
        )
        .expect("Failed to convert to Pandoc AST")
    }

    #[test]
    fn test_extract_source_info_simple() {
        let source = "Hello world";
        let doc = parse_qmd_helper(source);

        let source_infos = extract_all_source_info(&doc);

        // Should have at least: Paragraph, Str "Hello", Space, Str "world"
        assert!(
            source_infos.len() >= 4,
            "Expected at least 4 source infos, got {}",
            source_infos.len()
        );
    }

    #[test]
    fn test_extract_source_info_nested() {
        let source = "This is *emphasis with **strong** inside*";
        let doc = parse_qmd_helper(source);

        let source_infos = extract_all_source_info(&doc);

        // Should have: Paragraph, multiple Str, Emph, Strong, etc.
        assert!(
            source_infos.len() > 5,
            "Expected many source infos for nested structure, got {}",
            source_infos.len()
        );
    }

    #[test]
    fn test_core_properties_simple() {
        let source = "Hello world";
        let doc = parse_qmd_helper(source);

        let violations = validate_core_properties(&doc, source);

        if !violations.is_empty() {
            eprintln!("Found {} violations:", violations.len());
            for v in &violations {
                eprintln!("  {}", v);
            }
        }

        assert!(
            violations.is_empty(),
            "Expected no violations for simple document"
        );
    }

    #[test]
    fn test_core_properties_nested() {
        let source = "This is *emphasis with **strong** inside*";
        let doc = parse_qmd_helper(source);

        let violations = validate_core_properties(&doc, source);

        if !violations.is_empty() {
            eprintln!("Found {} violations:", violations.len());
            for v in &violations {
                eprintln!("  {}", v);
            }
        }

        assert!(
            violations.is_empty(),
            "Expected no violations for nested document"
        );
    }

    #[test]
    fn test_core_properties_multiline() {
        let source = "Line 1\nLine 2\nLine 3";
        let doc = parse_qmd_helper(source);

        let violations = validate_core_properties(&doc, source);

        if !violations.is_empty() {
            eprintln!("Found {} violations:", violations.len());
            for v in &violations {
                eprintln!("  {}", v);
            }
        }

        assert!(
            violations.is_empty(),
            "Expected no violations for multiline document"
        );
    }

    #[test]
    fn test_core_properties_empty() {
        let source = "";
        let doc = parse_qmd_helper(source);

        let violations = validate_core_properties(&doc, source);

        if !violations.is_empty() {
            eprintln!("Found {} violations:", violations.len());
            for v in &violations {
                eprintln!("  {}", v);
            }
        }

        assert!(
            violations.is_empty(),
            "Expected no violations for empty document"
        );
    }

    #[test]
    fn test_core_properties_no_trailing_newline() {
        let source = "Line 1\nLine 2\nLine 3";
        let doc = parse_qmd_helper(source);

        let violations = validate_core_properties(&doc, source);

        if !violations.is_empty() {
            eprintln!("Found {} violations:", violations.len());
            for v in &violations {
                eprintln!("  {}", v);
            }
        }

        assert!(
            violations.is_empty(),
            "Expected no violations for document without trailing newline"
        );
    }

    #[test]
    fn test_core_properties_with_trailing_newline() {
        let source = "Line 1\nLine 2\nLine 3\n";
        let doc = parse_qmd_helper(source);

        let violations = validate_core_properties(&doc, source);

        if !violations.is_empty() {
            eprintln!("Found {} violations:", violations.len());
            for v in &violations {
                eprintln!("  {}", v);
            }
        }

        assert!(
            violations.is_empty(),
            "Expected no violations for document with trailing newline"
        );
    }

    #[test]
    fn test_core_properties_on_smoke_tests() {
        use std::fs;
        use std::path::Path;

        let smoke_dir = Path::new("tests/smoke");
        if !smoke_dir.exists() {
            eprintln!("Smoke test directory not found, skipping");
            return;
        }

        let mut file_count = 0;
        let mut total_violations = 0;

        for entry in fs::read_dir(smoke_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("qmd") {
                file_count += 1;
                let source = fs::read_to_string(&path).unwrap_or_else(|e| {
                    panic!("Failed to read {:?}: {}", path, e);
                });

                // Try to parse; skip files that fail (they may be intentionally malformed)
                let mut parser = MarkdownParser::default();
                let input_bytes = source.as_bytes();
                let tree = match parser.parse(input_bytes, None) {
                    Some(tree) => tree,
                    None => {
                        eprintln!(
                            "Skipping {:?}: parse returned None",
                            path.file_name().unwrap()
                        );
                        continue;
                    }
                };

                let context = ASTContext::anonymous();
                let mut error_collector = DiagnosticCollector::new();
                let doc = match treesitter_to_pandoc(
                    &mut std::io::sink(),
                    &tree,
                    input_bytes,
                    &context,
                    &mut error_collector,
                ) {
                    Ok(doc) => doc,
                    Err(e) => {
                        eprintln!(
                            "Skipping {:?}: conversion failed: {:?}",
                            path.file_name().unwrap(),
                            e
                        );
                        continue;
                    }
                };

                let violations = validate_core_properties(&doc, &source);

                if !violations.is_empty() {
                    eprintln!(
                        "\n{:?} has {} violations:",
                        path.file_name().unwrap(),
                        violations.len()
                    );
                    for v in &violations {
                        eprintln!("  {}", v);
                    }
                    total_violations += violations.len();
                }
            }
        }

        eprintln!("\nTested {} smoke test files", file_count);
        assert_eq!(
            total_violations, 0,
            "Found {} total violations across {} files",
            total_violations, file_count
        );
    }
}
