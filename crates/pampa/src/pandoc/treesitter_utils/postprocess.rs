/*
 * postprocess.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::filter_context::FilterContext;
use crate::filters::{
    Filter, FilterReturn::FilterResult, FilterReturn::Unchanged, topdown_traverse,
};
use crate::pandoc::location::empty_source_info;
use crate::pandoc::shortcode::shortcode_to_span;
use crate::pandoc::{
    Attr, Block, Blocks, Caption, DefinitionList, Div, Figure, Inline, Inlines, Pandoc, Plain,
    Space, Span, Str, Superscript, is_empty_attr,
};
use crate::utils::autoid;
use crate::utils::diagnostic_collector::DiagnosticCollector;
use hashlink::LinkedHashMap;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::AttrSourceInfo;
use quarto_pandoc_types::table::{
    Alignment, Cell, ColSpec, ColWidth, Row, Table, TableBody, TableFoot, TableHead,
};
use quarto_source_map::SourceInfo;
use std::cell::RefCell;
use std::collections::HashMap;

/// Result of validating a list-table div
#[derive(Debug)]
pub enum ListTableValidation {
    /// Div is a valid list-table and can be transformed
    Valid,
    /// Div doesn't have 'list-table' class (not an error, just skip)
    NotListTable,
    /// Div has 'list-table' class but has invalid structure
    Invalid {
        reason: String,
        location: SourceInfo,
    },
}

/// Validate that a div has the structure required for a list-table.
///
/// Valid structure:
/// - Div must have "list-table" class
/// - Div must contain at least one block, with the last block being a BulletList
/// - Blocks before the BulletList (if any) form the caption
/// - The outer BulletList represents rows
/// - Each row item must contain exactly one block which is a BulletList (the cells)
///
/// Returns:
/// - `Valid` if the div can be transformed to a Table
/// - `NotListTable` if the div doesn't have the list-table class
/// - `Invalid` with reason and location if the structure is malformed
fn validate_list_table_div(div: &Div) -> ListTableValidation {
    // Check if div has "list-table" class
    if !div.attr.1.contains(&"list-table".to_string()) {
        return ListTableValidation::NotListTable;
    }

    // Must contain at least one block
    if div.content.is_empty() {
        return ListTableValidation::Invalid {
            reason: "list-table div must contain at least one bullet list".to_string(),
            location: div.source_info.clone(),
        };
    }

    // Last block must be a BulletList (the rows)
    let last_block = div.content.last().unwrap();
    let Block::BulletList(rows_list) = last_block else {
        return ListTableValidation::Invalid {
            reason: "list-table div's last block must be a bullet list (the rows)".to_string(),
            location: get_block_source_info(last_block),
        };
    };

    // Each row item must contain exactly one block which is a BulletList (the cells)
    for (row_idx, row_blocks) in rows_list.content.iter().enumerate() {
        // Each row should have exactly one block
        if row_blocks.len() != 1 {
            let location = if row_blocks.is_empty() {
                rows_list.source_info.clone()
            } else {
                get_block_source_info(&row_blocks[0])
            };
            return ListTableValidation::Invalid {
                reason: format!(
                    "row {} in list-table must contain exactly one bullet list (the cells), found {} blocks",
                    row_idx + 1,
                    row_blocks.len()
                ),
                location,
            };
        }

        // That one block must be a BulletList (the cells)
        let Block::BulletList(_cells_list) = &row_blocks[0] else {
            return ListTableValidation::Invalid {
                reason: format!(
                    "row {} in list-table must contain a bullet list of cells",
                    row_idx + 1
                ),
                location: get_block_source_info(&row_blocks[0]),
            };
        };
    }

    ListTableValidation::Valid
}

/// Helper to get the source info from a Block
fn get_block_source_info(block: &Block) -> SourceInfo {
    match block {
        Block::Plain(b) => b.source_info.clone(),
        Block::Paragraph(b) => b.source_info.clone(),
        Block::LineBlock(b) => b.source_info.clone(),
        Block::CodeBlock(b) => b.source_info.clone(),
        Block::RawBlock(b) => b.source_info.clone(),
        Block::BlockQuote(b) => b.source_info.clone(),
        Block::OrderedList(b) => b.source_info.clone(),
        Block::BulletList(b) => b.source_info.clone(),
        Block::DefinitionList(b) => b.source_info.clone(),
        Block::Header(b) => b.source_info.clone(),
        Block::HorizontalRule(b) => b.source_info.clone(),
        Block::Table(b) => b.source_info.clone(),
        Block::Figure(b) => b.source_info.clone(),
        Block::Div(b) => b.source_info.clone(),
        Block::BlockMetadata(b) => b.source_info.clone(),
        Block::CaptionBlock(b) => b.source_info.clone(),
        Block::NoteDefinitionPara(b) => b.source_info.clone(),
        Block::NoteDefinitionFencedBlock(b) => b.source_info.clone(),
        Block::Custom(b) => b.source_info.clone(),
    }
}

/// Parse alignment string ("l,c,r,d") into a vector of Alignment
fn parse_alignments(aligns_str: &str) -> Vec<Alignment> {
    aligns_str
        .split(',')
        .map(|s| match s.trim() {
            "l" => Alignment::Left,
            "c" => Alignment::Center,
            "r" => Alignment::Right,
            _ => Alignment::Default,
        })
        .collect()
}

/// Parse widths string ("1,2,1") into a vector of ColWidth
fn parse_widths(widths_str: &str) -> Vec<ColWidth> {
    widths_str
        .split(',')
        .map(|s| {
            if let Ok(ratio) = s.trim().parse::<f64>() {
                if ratio > 0.0 {
                    // Widths in list-table are ratios; we'll normalize them later
                    ColWidth::Percentage(ratio)
                } else {
                    ColWidth::Default
                }
            } else {
                ColWidth::Default
            }
        })
        .collect()
}

/// Parse alignment character to Alignment
fn char_to_alignment(c: &str) -> Alignment {
    match c {
        "l" => Alignment::Left,
        "c" => Alignment::Center,
        "r" => Alignment::Right,
        _ => Alignment::Default,
    }
}

/// Cell attributes extracted from an empty span at the start of a cell
struct CellAttrs {
    colspan: usize,
    rowspan: usize,
    alignment: Alignment,
}

/// Extract cell attributes from the first inline of the first block if it's an empty span.
/// Returns the extracted attributes and mutates the blocks to remove the attribute span.
fn extract_cell_attrs(blocks: &mut Blocks) -> CellAttrs {
    let mut attrs = CellAttrs {
        colspan: 1,
        rowspan: 1,
        alignment: Alignment::Default,
    };

    if blocks.is_empty() {
        return attrs;
    }

    // Check if first block has inline content with an empty span at the start
    let first_inline_is_empty_span = match &blocks[0] {
        Block::Plain(plain) => {
            if let Some(Inline::Span(span)) = plain.content.first() {
                span.content.is_empty()
            } else {
                false
            }
        }
        Block::Paragraph(para) => {
            if let Some(Inline::Span(span)) = para.content.first() {
                span.content.is_empty()
            } else {
                false
            }
        }
        _ => false,
    };

    if !first_inline_is_empty_span {
        return attrs;
    }

    // Extract attributes from the span and remove it
    match &mut blocks[0] {
        Block::Plain(plain) => {
            if let Some(Inline::Span(span)) = plain.content.first() {
                // Extract attributes
                for (key, value) in &span.attr.2 {
                    match key.as_str() {
                        "colspan" => {
                            if let Ok(v) = value.parse::<usize>() {
                                attrs.colspan = v.max(1);
                            }
                        }
                        "rowspan" => {
                            if let Ok(v) = value.parse::<usize>() {
                                attrs.rowspan = v.max(1);
                            }
                        }
                        "align" => {
                            attrs.alignment = char_to_alignment(value);
                        }
                        _ => {}
                    }
                }
            }
            // Remove the empty span
            plain.content.remove(0);
            // Also remove any leading space that might follow the span
            if let Some(Inline::Space(_)) = plain.content.first() {
                plain.content.remove(0);
            }
        }
        Block::Paragraph(para) => {
            if let Some(Inline::Span(span)) = para.content.first() {
                // Extract attributes
                for (key, value) in &span.attr.2 {
                    match key.as_str() {
                        "colspan" => {
                            if let Ok(v) = value.parse::<usize>() {
                                attrs.colspan = v.max(1);
                            }
                        }
                        "rowspan" => {
                            if let Ok(v) = value.parse::<usize>() {
                                attrs.rowspan = v.max(1);
                            }
                        }
                        "align" => {
                            attrs.alignment = char_to_alignment(value);
                        }
                        _ => {}
                    }
                }
            }
            // Remove the empty span
            para.content.remove(0);
            // Also remove any leading space that might follow the span
            if let Some(Inline::Space(_)) = para.content.first() {
                para.content.remove(0);
            }
        }
        _ => {}
    }

    attrs
}

/// Create an empty Attr
fn empty_table_attr() -> Attr {
    (String::new(), Vec::new(), LinkedHashMap::new())
}

/// Create an empty AttrSourceInfo
fn empty_attr_source() -> AttrSourceInfo {
    AttrSourceInfo::empty()
}

/// Transform a valid list-table div into a Table block.
///
/// PRECONDITION: div must pass validate_list_table_div() check with Valid result.
fn transform_list_table_div(div: Div) -> Block {
    let source_info = div.source_info.clone();

    // Extract attributes from div (LinkedHashMap iteration returns (&String, &String))
    let header_rows: usize = div
        .attr
        .2
        .get("header-rows")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let aligns_str = div.attr.2.get("aligns").map(|v| v.as_str());
    let widths_str = div.attr.2.get("widths").map(|v| v.as_str());

    // Build table attr from div attr, excluding list-table specific attributes
    let table_id = div.attr.0.clone();
    let table_classes: Vec<String> = div
        .attr
        .1
        .iter()
        .filter(|c| *c != "list-table")
        .cloned()
        .collect();
    let mut table_kvs: LinkedHashMap<String, String> = LinkedHashMap::new();
    for (k, v) in &div.attr.2 {
        if k != "header-rows" && k != "aligns" && k != "widths" {
            table_kvs.insert(k.clone(), v.clone());
        }
    }
    let table_attr = (table_id, table_classes, table_kvs);

    // Split content: caption blocks vs the rows BulletList
    let mut content = div.content;
    let rows_bullet_list = content.pop().unwrap(); // Last block is the BulletList (validated)
    let caption_blocks = content; // Everything else is caption

    // Build caption
    let caption = if caption_blocks.is_empty() {
        Caption {
            short: None,
            long: None,
            source_info: empty_source_info(),
        }
    } else {
        Caption {
            short: None,
            long: Some(caption_blocks),
            source_info: source_info.clone(),
        }
    };

    // Extract the rows from the outer BulletList
    let Block::BulletList(outer_list) = rows_bullet_list else {
        panic!("Expected BulletList after validation");
    };

    // Process each row
    let mut all_rows: Vec<Row> = Vec::new();
    let mut num_cols: usize = 0;

    for row_blocks in outer_list.content {
        // Each row has exactly one block which is a BulletList of cells (validated)
        let Block::BulletList(cells_list) = row_blocks.into_iter().next().unwrap() else {
            panic!("Expected BulletList for cells after validation");
        };

        let row_source_info = cells_list.source_info.clone();
        let mut cells: Vec<Cell> = Vec::new();

        for mut cell_blocks in cells_list.content {
            // Extract cell attributes from empty span if present
            let cell_attrs = extract_cell_attrs(&mut cell_blocks);

            let cell_source_info = if cell_blocks.is_empty() {
                row_source_info.clone()
            } else {
                get_block_source_info(&cell_blocks[0])
            };

            cells.push(Cell {
                attr: empty_table_attr(),
                alignment: cell_attrs.alignment,
                row_span: cell_attrs.rowspan,
                col_span: cell_attrs.colspan,
                content: cell_blocks,
                source_info: cell_source_info,
                attr_source: empty_attr_source(),
            });
        }

        // Track max columns (accounting for colspan)
        let row_cols: usize = cells.iter().map(|c| c.col_span).sum();
        num_cols = num_cols.max(row_cols);

        all_rows.push(Row {
            attr: empty_table_attr(),
            cells,
            source_info: row_source_info,
            attr_source: empty_attr_source(),
        });
    }

    // Build colspec from aligns and widths
    let alignments = aligns_str.map(parse_alignments).unwrap_or_default();
    let widths = widths_str.map(parse_widths).unwrap_or_default();

    // Normalize widths to percentages that sum to 1.0
    let normalized_widths: Vec<ColWidth> = if widths.is_empty() {
        vec![ColWidth::Default; num_cols]
    } else {
        let total: f64 = widths
            .iter()
            .map(|w| match w {
                ColWidth::Percentage(p) => *p,
                ColWidth::Default => 1.0,
            })
            .sum();

        widths
            .iter()
            .map(|w| match w {
                ColWidth::Percentage(p) if total > 0.0 => ColWidth::Percentage(*p / total),
                _ => ColWidth::Default,
            })
            .collect()
    };

    // Build colspec (alignment, width) pairs
    let colspec: Vec<ColSpec> = (0..num_cols)
        .map(|i| {
            let align = alignments.get(i).cloned().unwrap_or(Alignment::Default);
            let width = normalized_widths
                .get(i)
                .cloned()
                .unwrap_or(ColWidth::Default);
            (align, width)
        })
        .collect();

    // Split rows into head and body based on header-rows attribute
    let (head_rows, body_rows) = if header_rows > 0 && header_rows <= all_rows.len() {
        let (head, body) = all_rows.split_at(header_rows);
        (head.to_vec(), body.to_vec())
    } else {
        (Vec::new(), all_rows)
    };

    // Build TableHead
    let head = TableHead {
        attr: empty_table_attr(),
        rows: head_rows,
        source_info: source_info.clone(),
        attr_source: empty_attr_source(),
    };

    // Build TableBody (single body with all non-header rows)
    let bodies = if body_rows.is_empty() {
        Vec::new()
    } else {
        vec![TableBody {
            attr: empty_table_attr(),
            rowhead_columns: 0,
            head: Vec::new(),
            body: body_rows,
            source_info: source_info.clone(),
            attr_source: empty_attr_source(),
        }]
    };

    // Build empty TableFoot
    let foot = TableFoot {
        attr: empty_table_attr(),
        rows: Vec::new(),
        source_info: source_info.clone(),
        attr_source: empty_attr_source(),
    };

    Block::Table(Table {
        attr: table_attr,
        caption,
        colspec,
        head,
        bodies,
        foot,
        source_info,
        attr_source: div.attr_source,
    })
}

/// Trim leading and trailing spaces from inlines
pub fn trim_inlines(inlines: Inlines) -> (Inlines, bool) {
    let mut result: Inlines = Vec::new();
    let mut at_start = true;
    let mut space_run: Inlines = Vec::new();
    let mut changed = false;
    for inline in inlines {
        match &inline {
            Inline::Space(_) if at_start => {
                // skip leading spaces
                changed = true;
                continue;
            }
            Inline::Space(_) => {
                // collect spaces
                space_run.push(inline);
                continue;
            }
            _ => {
                result.extend(space_run.drain(..));
                result.push(inline);
                at_start = false;
            }
        }
    }
    if space_run.len() > 0 {
        changed = true;
    }
    (result, changed)
}

/// Convert trailing LineBreak to literal backslash for CommonMark compatibility.
///
/// Per CommonMark spec (lines 9362-9391), hard line breaks do NOT work at the end
/// of a block element. A backslash at the end of a paragraph or header should
/// produce a literal "\", not a LineBreak.
///
/// Example: `foo\` at end of paragraph → `<p>foo\</p>` (literal backslash)
///
/// Returns true if a conversion was made.
pub fn convert_trailing_linebreak_to_str(inlines: &mut Inlines) -> bool {
    if let Some(Inline::LineBreak(lb)) = inlines.last() {
        let source_info = lb.source_info.clone();
        inlines.pop();
        inlines.push(Inline::Str(Str {
            text: "\\".to_string(),
            source_info,
        }));
        true
    } else {
        false
    }
}

/// List of known abbreviations
const ABBREVIATIONS: &[&str] = &[
    "Mr.", "Mrs.", "Ms.", "Capt.", "Dr.", "Prof.", "Gen.", "Gov.", "e.g.", "i.e.", "Sgt.", "St.",
    "vol.", "vs.", "Sen.", "Rep.", "Pres.", "Hon.", "Rev.", "Ph.D.", "M.D.", "M.A.", "p.", "pp.",
    "ch.", "chap.", "sec.", "cf.", "cp.",
];

/// Check if a text string is a known abbreviation
fn is_abbreviation(text: &str) -> bool {
    ABBREVIATIONS.contains(&text)
}

/// Check if text ends with an abbreviation AND has a valid word boundary before it
/// A valid boundary means the abbreviation is either at the start of the string,
/// or preceded by a non-alphanumeric character (punctuation is OK, letters/digits are not)
fn has_valid_abbrev_boundary(text: &str, abbrev: &str) -> bool {
    if !text.ends_with(abbrev) {
        return false;
    }

    // Check if there's a valid word boundary before the abbreviation
    if text.len() == abbrev.len() {
        return true; // abbreviation is the entire string
    }

    // Get the prefix before the abbreviation
    let prefix = &text[..text.len() - abbrev.len()];

    // Check the last character of the prefix - must not be alphanumeric
    if let Some(last_char) = prefix.chars().last() {
        !last_char.is_alphanumeric()
    } else {
        true
    }
}

/// Check if a text string ends with a known abbreviation
fn ends_with_abbreviation(text: &str) -> bool {
    ABBREVIATIONS
        .iter()
        .any(|abbrev| has_valid_abbrev_boundary(text, abbrev))
}

/// Coalesce Str nodes that end with abbreviations with following words
/// This matches Pandoc's behavior of keeping abbreviations with the next word
/// Returns (result, did_coalesce) tuple
pub fn coalesce_abbreviations(inlines: Vec<Inline>) -> (Vec<Inline>, bool) {
    let mut result: Vec<Inline> = Vec::new();
    let mut i = 0;
    let mut did_coalesce = false;

    while i < inlines.len() {
        if let Inline::Str(ref str_inline) = inlines[i] {
            let mut current_text = str_inline.text.clone();
            let start_info = str_inline.source_info.clone();
            let mut end_info = str_inline.source_info.clone();
            let mut j = i + 1;

            // Check if current text ends with an abbreviation
            if ends_with_abbreviation(&current_text) {
                let original_j = j;
                // Coalesce with following Space + Str
                while j + 1 < inlines.len() {
                    if let (Inline::Space(_space_info), Inline::Str(next_str)) =
                        (&inlines[j], &inlines[j + 1])
                    {
                        // Coalesce with non-breaking space (U+00A0) to match Pandoc
                        current_text.push('\u{00A0}');
                        current_text.push_str(&next_str.text);
                        end_info = next_str.source_info.clone();
                        j += 2;
                        did_coalesce = true;

                        // If this word also ends with an abbreviation, continue coalescing
                        // Otherwise, stop after this word
                        if !ends_with_abbreviation(&current_text) {
                            break;
                        }
                    } else {
                        break;
                    }
                }

                // If we didn't coalesce with any Str nodes but have a Space following
                // the abbreviation, include the space in the abbreviation to match Pandoc
                if j == original_j && j < inlines.len() && matches!(inlines[j], Inline::Space(_)) {
                    if let Inline::Space(space_info) = &inlines[j] {
                        current_text.push('\u{00A0}');
                        end_info = space_info.source_info.clone();
                        j += 1;
                        did_coalesce = true;
                    }
                }
            }

            // Create the Str node (possibly coalesced)
            let source_info = if did_coalesce {
                start_info.combine(&end_info)
            } else {
                start_info
            };

            result.push(Inline::Str(Str {
                text: current_text,
                source_info,
            }));
            i = j;
        } else {
            result.push(inlines[i].clone());
            i += 1;
        }
    }

    (result, did_coalesce)
}

/// Validate that a div has the structure required for a definition list.
///
/// Valid structure:
/// - Div must have "definition-list" class
/// - Div must contain exactly one block, which must be a BulletList
/// - Each item in the BulletList must have:
///   - Exactly two blocks
///   - First block must be Plain or Paragraph (contains the term)
///   - Second block must be a BulletList (contains the definitions)
///
/// Returns true if valid, false otherwise.
fn is_valid_definition_list_div(div: &Div) -> bool {
    // Check if div has "definition-list" class
    if !div.attr.1.contains(&"definition-list".to_string()) {
        return false;
    }

    // Must contain exactly one block
    if div.content.len() != 1 {
        // FUTURE: issue linter warning: "definition-list div must contain exactly one bullet list"
        return false;
    }

    // That block must be a BulletList
    let Block::BulletList(bullet_list) = &div.content[0] else {
        // FUTURE: issue linter warning: "definition-list div must contain a bullet list"
        return false;
    };

    // Check each item in the bullet list
    for item_blocks in &bullet_list.content {
        // Each item must have exactly 2 blocks
        if item_blocks.len() != 2 {
            // FUTURE: issue linter warning: "each definition list item must have a term and a nested bullet list"
            return false;
        }

        // First block must be Plain or Paragraph
        match &item_blocks[0] {
            Block::Plain(_) | Block::Paragraph(_) => {}
            _ => {
                // FUTURE: issue linter warning: "definition list term must be Plain or Paragraph"
                return false;
            }
        }

        // Second block must be BulletList
        if !matches!(&item_blocks[1], Block::BulletList(_)) {
            // FUTURE: issue linter warning: "definitions must be in a nested bullet list"
            return false;
        }
    }

    true
}

/// Transform a valid definition-list div into a DefinitionList block.
///
/// PRECONDITION: div must pass is_valid_definition_list_div() check.
/// This function uses unwrap() liberally since the structure has been pre-validated.
fn transform_definition_list_div(div: Div) -> Block {
    // Extract the bullet list (validated to exist)
    let Block::BulletList(bullet_list) = div.content.into_iter().next().unwrap() else {
        panic!("BulletList expected after validation");
    };

    // Transform each item into (term, definitions) tuple
    let mut definition_items: Vec<(Inlines, Vec<crate::pandoc::block::Blocks>)> = Vec::new();

    for mut item_blocks in bullet_list.content {
        // Extract term from first block (Plain or Paragraph)
        let term_inlines = match item_blocks.remove(0) {
            Block::Plain(plain) => plain.content,
            Block::Paragraph(para) => para.content,
            _ => panic!("Plain or Paragraph expected after validation"),
        };

        // Extract definitions from second block (BulletList)
        let Block::BulletList(definitions_list) = item_blocks.remove(0) else {
            panic!("BulletList expected after validation");
        };

        // Each item in the definitions bullet list is a definition (Vec<Block>)
        definition_items.push((term_inlines, definitions_list.content));
    }

    // Preserve source location from the original div
    Block::DefinitionList(DefinitionList {
        content: definition_items,
        source_info: div.source_info,
    })
}

/// Apply post-processing transformations to the Pandoc AST
pub fn postprocess(doc: Pandoc, error_collector: &mut DiagnosticCollector) -> Result<Pandoc, ()> {
    let result = {
        // Wrap error_collector in RefCell for interior mutability across multiple closures
        let error_collector_ref = RefCell::new(error_collector);

        // Track seen header IDs to avoid duplicates
        let mut seen_ids: HashMap<String, usize> = HashMap::new();
        // Track citation count for numbering
        let mut citation_counter: usize = 0;

        let mut filter = Filter::new()
            .with_cite(|mut cite, _ctx| {
                // Increment citation counter for each Cite element
                citation_counter += 1;
                // Update all citations in this Cite element with the current counter
                for citation in &mut cite.citations {
                    citation.note_num = citation_counter;
                }
                // Return Unchanged to allow recursion into cite content while avoiding re-filtering
                Unchanged(cite)
            })
            .with_superscript(|mut superscript, _ctx| {
                let (content, changed) = trim_inlines(superscript.content);
                if !changed {
                    return Unchanged(Superscript {
                        content,
                        ..superscript
                    });
                } else {
                    superscript.content = content;
                    FilterResult(vec![Inline::Superscript(superscript)], true)
                }
            })
            // add attribute to headers that have them.
            .with_header(move |mut header, _ctx| {
                // Convert trailing LineBreak to literal backslash (CommonMark spec)
                // Per spec, hard line breaks don't work at end of block elements
                let trailing_lb_converted = convert_trailing_linebreak_to_str(&mut header.content);

                let is_last_attr = header
                    .content
                    .last()
                    .map_or(false, |v| matches!(v, Inline::Attr(_, _)));
                if !is_last_attr {
                    let mut attr = header.attr.clone();
                    if attr.0.is_empty() {
                        let base_id = autoid::auto_generated_id(&header.content);

                        // Deduplicate the ID by appending -1, -2, etc. for duplicates
                        let final_id = if let Some(count) = seen_ids.get_mut(&base_id) {
                            *count += 1;
                            format!("{}-{}", base_id, count)
                        } else {
                            seen_ids.insert(base_id.clone(), 0);
                            base_id
                        };

                        attr.0 = final_id;
                        if !is_empty_attr(&attr) || trailing_lb_converted {
                            header.attr = attr;
                            FilterResult(vec![Block::Header(header)], true)
                        } else {
                            Unchanged(header)
                        }
                    } else if trailing_lb_converted {
                        FilterResult(vec![Block::Header(header)], true)
                    } else {
                        Unchanged(header)
                    }
                } else {
                    let Some(Inline::Attr(attr, attr_source)) = header.content.pop() else {
                        panic!("shouldn't happen, header should have an attribute at this point");
                    };
                    header.attr = attr;
                    header.attr_source = attr_source;
                    header.content = trim_inlines(header.content).0;
                    FilterResult(vec![Block::Header(header)], true)
                }
            })
            // attempt to desugar single-image paragraphs into figures
            // also convert trailing LineBreak to literal backslash (CommonMark spec)
            .with_paragraph(|mut para, _ctx| {
                // Convert trailing LineBreak to literal backslash (CommonMark spec)
                // Per spec, hard line breaks don't work at end of block elements
                let trailing_lb_converted = convert_trailing_linebreak_to_str(&mut para.content);

                // Check for single-image paragraph (for figure conversion)
                if para.content.len() == 1 {
                    if let Some(Inline::Image(image)) = para.content.first() {
                        if !image.content.is_empty() {
                            let figure_attr: Attr =
                                (image.attr.0.clone(), vec![], LinkedHashMap::new());
                            let image_attr: Attr =
                                ("".to_string(), image.attr.1.clone(), image.attr.2.clone());

                            // Split attr_source between figure and image
                            let figure_attr_source = crate::pandoc::attr::AttrSourceInfo {
                                id: image.attr_source.id.clone(),
                                classes: vec![],
                                attributes: vec![],
                            };
                            let image_attr_source = crate::pandoc::attr::AttrSourceInfo {
                                id: None,
                                classes: image.attr_source.classes.clone(),
                                attributes: image.attr_source.attributes.clone(),
                            };

                            let mut new_image = image.clone();
                            new_image.attr = image_attr;
                            new_image.attr_source = image_attr_source;

                            // Use proper source info from the original paragraph and image
                            return FilterResult(
                                vec![Block::Figure(Figure {
                                    attr: figure_attr,
                                    caption: Caption {
                                        short: None,
                                        long: Some(vec![Block::Plain(Plain {
                                            content: image.content.clone(),
                                            // Caption text comes from image's alt text
                                            source_info: image.source_info.clone(),
                                        })]),
                                        // Caption as a whole also uses image's source info
                                        source_info: image.source_info.clone(),
                                    },
                                    content: vec![Block::Plain(Plain {
                                        content: vec![Inline::Image(new_image)],
                                        // Content contains the image
                                        source_info: image.source_info.clone(),
                                    })],
                                    // Figure spans the entire paragraph
                                    source_info: para.source_info.clone(),
                                    attr_source: figure_attr_source,
                                })],
                                true,
                            );
                        }
                    }
                }

                // Not a figure conversion case, but may have converted trailing LineBreak
                if trailing_lb_converted {
                    FilterResult(vec![Block::Paragraph(para)], true)
                } else {
                    Unchanged(para)
                }
            })
            // Convert trailing LineBreak in Plain blocks (used in tight lists)
            .with_plain(|mut plain, _ctx| {
                // Convert trailing LineBreak to literal backslash (CommonMark spec)
                // Per spec, hard line breaks don't work at end of block elements
                let trailing_lb_converted = convert_trailing_linebreak_to_str(&mut plain.content);
                if trailing_lb_converted {
                    FilterResult(vec![Block::Plain(plain)], true)
                } else {
                    Unchanged(plain)
                }
            })
            // Convert list-table divs to Table blocks and definition-list divs to DefinitionList blocks
            .with_div(|div, _ctx| {
                // First check for list-table
                match validate_list_table_div(&div) {
                    ListTableValidation::Valid => {
                        FilterResult(vec![transform_list_table_div(div)], false)
                    }
                    ListTableValidation::Invalid { reason, location } => {
                        // Emit warning for malformed list-table div
                        error_collector_ref.borrow_mut().add(
                            DiagnosticMessageBuilder::warning("Invalid List-Table Structure")
                                .with_code("Q-2-35")
                                .with_location(location)
                                .problem(reason)
                                .add_hint(
                                    "Check the list-table documentation for the correct structure?",
                                )
                                .build(),
                        );
                        Unchanged(div) // Leave as-is
                    }
                    ListTableValidation::NotListTable => {
                        // Not a list-table, check for definition-list
                        if is_valid_definition_list_div(&div) {
                            FilterResult(vec![transform_definition_list_div(div)], false)
                        } else {
                            Unchanged(div)
                        }
                    }
                }
            })
            // Remove single empty spans from bullet list items
            // This allows `* []` to create truly empty list items in the AST
            .with_bullet_list(|mut bullet_list, _ctx| {
                let mut changed = false;
                for item in &mut bullet_list.content {
                    // Check if item has exactly one block
                    if item.len() == 1 {
                        // Check if that block is Plain or Paragraph with single empty Span
                        let should_clear = match &item[0] {
                            Block::Plain(plain) => {
                                plain.content.len() == 1
                                    && matches!(&plain.content[0], Inline::Span(span)
                                        if span.content.is_empty() && is_empty_attr(&span.attr))
                            }
                            Block::Paragraph(para) => {
                                para.content.len() == 1
                                    && matches!(&para.content[0], Inline::Span(span)
                                        if span.content.is_empty() && is_empty_attr(&span.attr))
                            }
                            _ => false,
                        };

                        if should_clear {
                            // Clear the content to make it truly empty
                            match &mut item[0] {
                                Block::Plain(plain) => {
                                    plain.content.clear();
                                    changed = true;
                                }
                                Block::Paragraph(para) => {
                                    para.content.clear();
                                    changed = true;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                if changed {
                    FilterResult(vec![Block::BulletList(bullet_list)], true)
                } else {
                    Unchanged(bullet_list)
                }
            })
            // Fix table captions that were parsed as last row (no blank line before caption)
            .with_table(|mut table, _ctx| {
                // Check if caption is empty
                let caption_is_empty = table.caption.long.is_none()
                    || table
                        .caption
                        .long
                        .as_ref()
                        .map_or(true, |blocks| blocks.is_empty());

                if !caption_is_empty || table.bodies.is_empty() {
                    return Unchanged(table);
                }

                // Get the last body and check if it has rows
                let last_body = table.bodies.last_mut().unwrap();
                if last_body.body.is_empty() {
                    return Unchanged(table);
                }

                // Check if last row has exactly one cell
                let last_row = last_body.body.last().unwrap();
                if last_row.cells.len() != 1 {
                    return Unchanged(table);
                }

                // Check if cell has exactly one Plain block
                let cell = &last_row.cells[0];
                if cell.content.len() != 1 {
                    return Unchanged(table);
                }

                let Block::Plain(plain) = &cell.content[0] else {
                    return Unchanged(table);
                };

                // Check if Plain starts with Str that begins with ":"
                if plain.content.is_empty() {
                    return Unchanged(table);
                }

                let starts_with_colon = match &plain.content[0] {
                    Inline::Str(s) => s.text.starts_with(':'),
                    _ => false,
                };

                if !starts_with_colon {
                    return Unchanged(table);
                }

                // Pattern matched! Transform the table.
                // Remove the last row and extract its content
                let caption_row = last_body.body.pop().unwrap();
                let caption_cell = &caption_row.cells[0];
                let Block::Plain(caption_plain) = &caption_cell.content[0] else {
                    unreachable!("Already checked this is Plain");
                };

                let mut caption_inlines = caption_plain.content.clone();

                // Strip leading ":" from first Str
                if let Some(Inline::Str(first_str)) = caption_inlines.first_mut() {
                    // Remove leading ":" and trim whitespace
                    first_str.text = first_str
                        .text
                        .strip_prefix(':')
                        .unwrap_or(&first_str.text)
                        .trim_start()
                        .to_string();

                    // If the string is now empty, remove it
                    if first_str.text.is_empty() {
                        caption_inlines.remove(0);
                        // Also remove following Space if present
                        if matches!(caption_inlines.first(), Some(Inline::Space(_))) {
                            caption_inlines.remove(0);
                        }
                    }
                }

                // Create the caption
                table.caption = Caption {
                    short: None,
                    long: Some(vec![Block::Plain(Plain {
                        content: caption_inlines,
                        source_info: caption_row.source_info.clone(),
                    })]),
                    source_info: caption_row.source_info.clone(),
                };

                // Return the transformed table
                FilterResult(vec![Block::Table(table)], false)
            })
            .with_shortcode(|shortcode, _ctx| {
                FilterResult(vec![Inline::Span(shortcode_to_span(shortcode))], false)
            })
            .with_note_reference(|note_ref, _ctx| {
                let mut kv = LinkedHashMap::new();
                kv.insert("reference-id".to_string(), note_ref.id.clone());
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (
                            "".to_string(),
                            vec!["quarto-note-reference".to_string()],
                            kv,
                        ),
                        content: vec![],
                        source_info: note_ref.source_info,
                        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                    })],
                    false,
                )
            })
            .with_insert(|insert, _ctx| {
                let (content, _changed) = trim_inlines(insert.content);
                let mut classes = vec!["quarto-insert".to_string()];
                classes.extend(insert.attr.1);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (insert.attr.0, classes, insert.attr.2),
                        content,
                        source_info: insert.source_info,
                        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                    })],
                    true,
                )
            })
            .with_delete(|delete, _ctx| {
                let (content, _changed) = trim_inlines(delete.content);
                let mut classes = vec!["quarto-delete".to_string()];
                classes.extend(delete.attr.1);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (delete.attr.0, classes, delete.attr.2),
                        content,
                        source_info: delete.source_info,
                        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                    })],
                    true,
                )
            })
            .with_highlight(|highlight, _ctx| {
                let (content, _changed) = trim_inlines(highlight.content);
                let mut classes = vec!["quarto-highlight".to_string()];
                classes.extend(highlight.attr.1);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (highlight.attr.0, classes, highlight.attr.2),
                        content,
                        source_info: highlight.source_info,
                        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                    })],
                    true,
                )
            })
            .with_edit_comment(|edit_comment, _ctx| {
                let (content, _changed) = trim_inlines(edit_comment.content);
                let mut classes = vec!["quarto-edit-comment".to_string()];
                classes.extend(edit_comment.attr.1);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (edit_comment.attr.0, classes, edit_comment.attr.2),
                        content,
                        source_info: edit_comment.source_info,
                        attr_source: crate::pandoc::attr::AttrSourceInfo::empty(),
                    })],
                    true,
                )
            })
            .with_inlines(|inlines, _ctx| {
                // Combined filter: Handle LineBreak + SoftBreak cleanup, Math + Attr pattern, then citation suffix pattern

                // Step 0: Remove SoftBreaks that immediately follow LineBreaks
                // This fixes an issue where tree-sitter emits both break types when
                // a hard break (backslash-newline) is present. Pandoc only emits LineBreak
                // in this case, so we match that behavior by dropping the redundant SoftBreak.
                let mut break_cleaned = vec![];
                let mut i = 0;

                while i < inlines.len() {
                    let current = &inlines[i];

                    // Check if current is LineBreak and next is SoftBreak
                    if matches!(current, Inline::LineBreak(_))
                        && i + 1 < inlines.len()
                        && matches!(inlines[i + 1], Inline::SoftBreak(_))
                    {
                        // Keep the LineBreak
                        break_cleaned.push(inlines[i].clone());
                        // Skip the SoftBreak (i+1)
                        i += 2;
                    } else {
                        // Keep the current inline
                        break_cleaned.push(inlines[i].clone());
                        i += 1;
                    }
                }

                // Step 1: Handle Math nodes followed by Attr (process on break_cleaned)
                // Pattern: Math, Space (optional), Attr -> Span with "quarto-math-with-attribute" class
                let mut math_processed = vec![];
                let mut i = 0;

                while i < break_cleaned.len() {
                    if let Inline::Math(math) = &break_cleaned[i] {
                        // Check if followed by Space then Attr, or just Attr
                        let has_space = i + 1 < break_cleaned.len()
                            && matches!(break_cleaned[i + 1], Inline::Space(_));
                        let attr_idx = if has_space { i + 2 } else { i + 1 };

                        if attr_idx < break_cleaned.len() {
                            if let Inline::Attr(attr, attr_source) = &break_cleaned[attr_idx] {
                                // Found Math + (Space?) + Attr pattern
                                // Wrap Math in a Span with the attribute
                                let mut classes = vec!["quarto-math-with-attribute".to_string()];
                                classes.extend(attr.1.clone());

                                math_processed.push(Inline::Span(Span {
                                    attr: (attr.0.clone(), classes, attr.2.clone()),
                                    content: vec![Inline::Math(math.clone())],
                                    source_info: if let Some(attr_overall) =
                                        attr_source.combine_all()
                                    {
                                        math.source_info.combine(&attr_overall)
                                    } else {
                                        math.source_info.clone()
                                    },
                                    attr_source: attr_source.clone(),
                                }));

                                // Skip the Math, optional Space, and Attr
                                i = attr_idx + 1;
                                continue;
                            }
                        }
                    }

                    // Not a Math + Attr pattern, add as is
                    math_processed.push(break_cleaned[i].clone());
                    i += 1;
                }

                // Step 2: Handle citation suffix pattern on the math-processed result
                let mut result = vec![];
                // states in this state machine:
                // 0. normal state, where we just collect inlines
                // 1. we just saw a valid cite (only one citation, no prefix or suffix)
                // 2. from 1, we then saw a space
                // 3. from 2, we then saw a span with only Strs and Spaces.
                //
                //    Here, we emit the cite and add the span content to the cite suffix.
                let mut state = 0;
                let mut pending_cite: Option<crate::pandoc::inline::Cite> = None;
                let mut pending_space: Option<crate::pandoc::inline::Space> = None;

                for inline in math_processed {
                    match state {
                        0 => {
                            // Normal state - check if we see a valid cite
                            if let Inline::Cite(cite) = &inline {
                                if cite.citations.len() == 1
                                    && cite.citations[0].prefix.is_empty()
                                    && cite.citations[0].suffix.is_empty()
                                {
                                    // Valid cite - transition to state 1
                                    state = 1;
                                    pending_cite = Some(cite.clone());
                                } else {
                                    // Not a simple cite, just add it
                                    result.push(inline);
                                }
                            } else {
                                result.push(inline);
                            }
                        }
                        1 => {
                            // Just saw a valid cite - check for space
                            if let Inline::Space(space) = inline {
                                // Save the space and transition to state 2
                                pending_space = Some(space);
                                state = 2;
                            } else {
                                // Not a space, emit pending cite and reset
                                if let Some(cite) = pending_cite.take() {
                                    result.push(Inline::Cite(cite));
                                }
                                result.push(inline);
                                state = 0;
                            }
                        }
                        2 => {
                            // After cite and space - check for span with only Strs and Spaces
                            if let Inline::Span(span) = &inline {
                                // Check if span contains only Str and Space inlines
                                let is_valid_suffix = span
                                    .content
                                    .iter()
                                    .all(|i| matches!(i, Inline::Str(_) | Inline::Space(_)));

                                if is_valid_suffix {
                                    // State 3 - merge span content into cite suffix
                                    if let Some(mut cite) = pending_cite.take() {
                                        // Add span content to the citation's suffix
                                        cite.citations[0].suffix = span.content.clone();

                                        // Update the content field to include the rendered suffix with brackets
                                        // Pandoc breaks up the bracketed suffix text by spaces, with the opening
                                        // bracket attached to the first word and closing bracket to the last word
                                        // e.g., "@knuth [p. 33]" becomes: Str("@knuth"), Space, Str("[p."), Space, Str("33]")
                                        cite.content.push(Inline::Space(Space {
                                            // Synthetic Space: inserted to separate citation from suffix
                                            source_info: quarto_source_map::SourceInfo::default(),
                                        }));

                                        // The span content may have been merged into a single string, so we need to
                                        // intelligently break it up to match Pandoc's behavior
                                        let mut bracketed_content: Vec<Inline> = vec![];
                                        for inline in &span.content {
                                            if let Inline::Str(s) = inline {
                                                // Split the string by spaces and create Str/Space inlines
                                                let words: Vec<&str> = s.text.split(' ').collect();
                                                for (i, word) in words.iter().enumerate() {
                                                    if i > 0 {
                                                        bracketed_content.push(Inline::Space(
                                                            Space {
                                                                source_info: empty_source_info(),
                                                            },
                                                        ));
                                                    }
                                                    if !word.is_empty() {
                                                        bracketed_content.push(Inline::Str(Str {
                                                            text: word.to_string(),
                                                            source_info: s.source_info.clone(),
                                                        }));
                                                    }
                                                }
                                            } else {
                                                bracketed_content.push(inline.clone());
                                            }
                                        }

                                        // Now add brackets to the first and last Str elements
                                        if !bracketed_content.is_empty() {
                                            // Prepend "[" to the first Str element
                                            if let Some(Inline::Str(first_str)) =
                                                bracketed_content.first_mut()
                                            {
                                                first_str.text = format!("[{}", first_str.text);
                                            }
                                            // Append "]" to the last Str element (search from the end)
                                            for i in (0..bracketed_content.len()).rev() {
                                                if let Inline::Str(last_str) =
                                                    &mut bracketed_content[i]
                                                {
                                                    last_str.text = format!("{}]", last_str.text);
                                                    break;
                                                }
                                            }
                                        }

                                        cite.content.extend(bracketed_content);
                                        result.push(Inline::Cite(cite));
                                    }
                                    state = 0;
                                } else {
                                    // Invalid span, emit pending cite, space, and span
                                    if let Some(cite) = pending_cite.take() {
                                        result.push(Inline::Cite(cite));
                                    }
                                    if let Some(space) = pending_space.take() {
                                        result.push(Inline::Space(space));
                                    }
                                    result.push(inline);
                                    state = 0;
                                }
                            } else {
                                // Not a span, emit pending cite, space, and current inline
                                if let Some(cite) = pending_cite.take() {
                                    result.push(Inline::Cite(cite));
                                }
                                if let Some(space) = pending_space.take() {
                                    result.push(Inline::Space(space));
                                }
                                result.push(inline);
                                state = 0;
                            }
                        }
                        _ => unreachable!("Invalid state: {}", state),
                    }
                }

                // Handle any pending cite at the end
                if let Some(cite) = pending_cite {
                    result.push(Inline::Cite(cite));
                    if state == 2 {
                        if let Some(space) = pending_space {
                            result.push(Inline::Space(space));
                        }
                    }
                }

                FilterResult(result, true)
            })
            .with_attr(|attr, _ctx| {
                // TODO: Add source location when attr has it
                error_collector_ref.borrow_mut().error(format!(
                    "Found attr in postprocess: {:?} - this should have been removed",
                    attr
                ));
                FilterResult(vec![], false)
            })
            .with_blocks(|blocks, _ctx| {
                // Process CaptionBlock nodes: attach to preceding tables or issue warnings
                let mut result: Blocks = Vec::new();

                for block in blocks {
                    // Check if current block is a CaptionBlock
                    if let Block::CaptionBlock(caption_block) = block {
                        // Look for a preceding Table
                        if let Some(Block::Table(table)) = result.last_mut() {
                            // Extract any trailing Inline::Attr from caption content
                            let mut caption_content = caption_block.content.clone();
                            let mut caption_attr: Option<Attr> = None;
                            let mut caption_attr_source: Option<
                                crate::pandoc::attr::AttrSourceInfo,
                            > = None;

                            if let Some(Inline::Attr(attr, attr_source)) = caption_content.last() {
                                caption_attr = Some(attr.clone());
                                caption_attr_source = Some(attr_source.clone());
                                caption_content.pop(); // Remove the Attr from caption content

                                // Trim trailing space before the attribute
                                caption_content = trim_inlines(caption_content).0;
                            }

                            // If we found attributes in the caption, merge them with the table's attr
                            if let Some(caption_attr_value) = caption_attr {
                                // Merge: caption attributes override table attributes
                                // table.attr is (id, classes, key_values)

                                // Merge key-value pairs (both values and sources)
                                if let Some(ref caption_attr_source_value) = caption_attr_source {
                                    for ((key, value), (key_source, value_source)) in
                                        caption_attr_value
                                            .2
                                            .iter()
                                            .zip(caption_attr_source_value.attributes.iter())
                                    {
                                        table.attr.2.insert(key.clone(), value.clone());
                                        table
                                            .attr_source
                                            .attributes
                                            .push((key_source.clone(), value_source.clone()));
                                    }
                                } else {
                                    // Fallback: merge values without sources
                                    for (key, value) in caption_attr_value.2 {
                                        table.attr.2.insert(key, value);
                                    }
                                }

                                // Merge classes (both values and sources)
                                if let Some(ref caption_attr_source_value) = caption_attr_source {
                                    for (class, class_source) in caption_attr_value
                                        .1
                                        .iter()
                                        .zip(caption_attr_source_value.classes.iter())
                                    {
                                        if !table.attr.1.contains(class) {
                                            table.attr.1.push(class.clone());
                                            table.attr_source.classes.push(class_source.clone());
                                        }
                                    }
                                } else {
                                    // Fallback: merge classes without sources
                                    for class in caption_attr_value.1 {
                                        if !table.attr.1.contains(&class) {
                                            table.attr.1.push(class);
                                        }
                                    }
                                }

                                // Use caption id if table doesn't have one (merge both value and source)
                                if table.attr.0.is_empty() && !caption_attr_value.0.is_empty() {
                                    table.attr.0 = caption_attr_value.0;
                                    // Also merge the source location
                                    if let Some(caption_attr_source_value) = caption_attr_source {
                                        if table.attr_source.id.is_none() {
                                            table.attr_source.id = caption_attr_source_value.id;
                                        }
                                    }
                                }
                            }

                            // Attach caption to the table (with Attr removed from content)
                            table.caption = Caption {
                                short: None,
                                long: Some(vec![Block::Plain(Plain {
                                    content: caption_content,
                                    source_info: caption_block.source_info.clone(),
                                })]),
                                source_info: caption_block.source_info.clone(),
                            };

                            // Extend table's source range to include the caption
                            // This ensures that caption attributes are within the table's bounds
                            table.source_info =
                                table.source_info.combine(&caption_block.source_info);

                            // Don't add the CaptionBlock to the result (it's now attached)
                        } else {
                            // Issue a warning when caption has no preceding table
                            error_collector_ref.borrow_mut().warn_at(
                                "Caption found without a preceding table".to_string(),
                                caption_block.source_info.clone(),
                            );
                            // Remove the caption from the output (don't add to result)
                        }
                    } else {
                        // Not a CaptionBlock, add it to result
                        result.push(block);
                    }
                }

                FilterResult(result, true)
            });
        let mut ctx = FilterContext::new();
        let pandoc_result = topdown_traverse(doc, &mut filter, &mut ctx);

        // Check if any errors were collected (before moving out of RefCell)
        let has_errors = error_collector_ref.borrow().has_errors();

        (pandoc_result, has_errors)
    };

    // Return based on whether errors were found
    if result.1 { Err(()) } else { Ok(result.0) }
}

/// Convert smart typography strings
fn as_smart_str(s: String) -> String {
    if s == "..." {
        "…".to_string()
    } else if s == "--" {
        "–".to_string()
    } else if s == "---" {
        "—".to_string()
    } else {
        s
    }
}

/// Merge consecutive Str inlines and apply smart typography
pub fn merge_strs(pandoc: Pandoc) -> Pandoc {
    let mut ctx = FilterContext::new();
    topdown_traverse(
        pandoc,
        &mut Filter::new().with_inlines(|inlines, _ctx| {
            let mut current_str: Option<String> = None;
            let mut current_source_info: Option<quarto_source_map::SourceInfo> = None;
            let mut result: Inlines = Vec::new();
            let mut did_merge = false;
            for inline in inlines {
                match inline {
                    Inline::Str(s) => {
                        let str_text = as_smart_str(s.text.clone());
                        if let Some(ref mut current) = current_str {
                            current.push_str(&str_text);
                            if let Some(ref mut info) = current_source_info {
                                *info = info.combine(&s.source_info);
                            }
                            did_merge = true;
                        } else {
                            current_str = Some(str_text);
                            current_source_info = Some(s.source_info);
                        }
                    }
                    _ => {
                        if let Some(current) = current_str.take() {
                            result.push(Inline::Str(Str {
                                text: current,
                                source_info: current_source_info
                                    .take()
                                    .unwrap_or_else(empty_source_info),
                            }));
                        }
                        result.push(inline);
                    }
                }
            }
            if let Some(current) = current_str {
                result.push(Inline::Str(Str {
                    text: current,
                    source_info: current_source_info.unwrap_or_else(empty_source_info),
                }));
            }

            // Apply abbreviation coalescing after merging strings
            let (coalesced_result, did_coalesce) = coalesce_abbreviations(result);
            did_merge = did_merge || did_coalesce;

            if did_merge {
                FilterResult(coalesced_result, true)
            } else {
                Unchanged(coalesced_result)
            }
        }),
        &mut ctx,
    )
}
