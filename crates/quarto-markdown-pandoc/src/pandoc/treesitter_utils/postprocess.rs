/*
 * postprocess.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::filters::{
    Filter, FilterReturn::FilterResult, FilterReturn::Unchanged, topdown_traverse,
};
use crate::pandoc::attr::{Attr, is_empty_attr};
use crate::pandoc::block::{Block, Blocks, DefinitionList, Div, Figure, Plain};
use crate::pandoc::caption::Caption;
use crate::pandoc::inline::{Inline, Inlines, Space, Span, Str, Superscript};
use crate::pandoc::location::empty_source_info;
use crate::pandoc::pandoc::Pandoc;
use crate::pandoc::shortcode::shortcode_to_span;
use crate::utils::autoid;
use crate::utils::diagnostic_collector::DiagnosticCollector;
use std::cell::RefCell;
use std::collections::HashMap;

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
            // TODO: Properly merge SourceInfo ranges for coalesced text
            let source_info = start_info;

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
            .with_cite(|mut cite| {
                // Increment citation counter for each Cite element
                citation_counter += 1;
                // Update all citations in this Cite element with the current counter
                for citation in &mut cite.citations {
                    citation.note_num = citation_counter;
                }
                // Return Unchanged to allow recursion into cite content while avoiding re-filtering
                Unchanged(cite)
            })
            .with_superscript(|mut superscript| {
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
            .with_header(move |mut header| {
                let is_last_attr = header
                    .content
                    .last()
                    .map_or(false, |v| matches!(v, Inline::Attr(_)));
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
                        if !is_empty_attr(&attr) {
                            header.attr = attr;
                            FilterResult(vec![Block::Header(header)], true)
                        } else {
                            Unchanged(header)
                        }
                    } else {
                        Unchanged(header)
                    }
                } else {
                    let Some(Inline::Attr(attr)) = header.content.pop() else {
                        panic!("shouldn't happen, header should have an attribute at this point");
                    };
                    header.attr = attr;
                    header.content = trim_inlines(header.content).0;
                    FilterResult(vec![Block::Header(header)], true)
                }
            })
            // attempt to desugar single-image paragraphs into figures
            .with_paragraph(|para| {
                if para.content.len() != 1 {
                    return Unchanged(para);
                }
                let first = para.content.first().unwrap();
                let Inline::Image(image) = first else {
                    return Unchanged(para);
                };
                if image.content.is_empty() {
                    return Unchanged(para);
                }
                let figure_attr: Attr = (image.attr.0.clone(), vec![], HashMap::new());
                let image_attr: Attr = ("".to_string(), image.attr.1.clone(), image.attr.2.clone());
                let mut new_image = image.clone();
                new_image.attr = image_attr;
                // FIXME all source location is broken here
                // TODO: Should propagate from image.source_info and para.source_info
                FilterResult(
                    vec![Block::Figure(Figure {
                        attr: figure_attr,
                        caption: Caption {
                            short: None,
                            long: Some(vec![Block::Plain(Plain {
                                content: image.content.clone(),
                                // TODO: Should derive from image.content inlines
                                source_info: quarto_source_map::SourceInfo::default(),
                            })]),
                        },
                        content: vec![Block::Plain(Plain {
                            content: vec![Inline::Image(new_image)],
                            // TODO: Should use image.source_info
                            source_info: quarto_source_map::SourceInfo::default(),
                        })],
                        // TODO: Should use para.source_info
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    true,
                )
            })
            // Convert definition-list divs to DefinitionList blocks
            .with_div(|div| {
                if is_valid_definition_list_div(&div) {
                    FilterResult(vec![transform_definition_list_div(div)], false)
                } else {
                    Unchanged(div)
                }
            })
            .with_shortcode(|shortcode| {
                FilterResult(vec![Inline::Span(shortcode_to_span(shortcode))], false)
            })
            .with_note_reference(|note_ref| {
                let mut kv = HashMap::new();
                kv.insert("reference-id".to_string(), note_ref.id);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (
                            "".to_string(),
                            vec!["quarto-note-reference".to_string()],
                            kv,
                        ),
                        content: vec![],
                        source_info: quarto_source_map::SourceInfo::default(),
                    })],
                    false,
                )
            })
            .with_insert(|insert| {
                let (content, _changed) = trim_inlines(insert.content);
                let mut classes = vec!["quarto-insert".to_string()];
                classes.extend(insert.attr.1);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (insert.attr.0, classes, insert.attr.2),
                        content,
                        source_info: insert.source_info,
                    })],
                    true,
                )
            })
            .with_delete(|delete| {
                let (content, _changed) = trim_inlines(delete.content);
                let mut classes = vec!["quarto-delete".to_string()];
                classes.extend(delete.attr.1);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (delete.attr.0, classes, delete.attr.2),
                        content,
                        source_info: delete.source_info,
                    })],
                    true,
                )
            })
            .with_highlight(|highlight| {
                let (content, _changed) = trim_inlines(highlight.content);
                let mut classes = vec!["quarto-highlight".to_string()];
                classes.extend(highlight.attr.1);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (highlight.attr.0, classes, highlight.attr.2),
                        content,
                        source_info: highlight.source_info,
                    })],
                    true,
                )
            })
            .with_edit_comment(|edit_comment| {
                let (content, _changed) = trim_inlines(edit_comment.content);
                let mut classes = vec!["quarto-edit-comment".to_string()];
                classes.extend(edit_comment.attr.1);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (edit_comment.attr.0, classes, edit_comment.attr.2),
                        content,
                        source_info: edit_comment.source_info,
                    })],
                    true,
                )
            })
            .with_inlines(|inlines| {
                // Combined filter: Handle Math + Attr pattern, then citation suffix pattern

                // Step 1: Handle Math nodes followed by Attr
                // Pattern: Math, Space (optional), Attr -> Span with "quarto-math-with-attribute" class
                let mut math_processed = vec![];
                let mut i = 0;

                while i < inlines.len() {
                    if let Inline::Math(math) = &inlines[i] {
                        // Check if followed by Space then Attr, or just Attr
                        let has_space =
                            i + 1 < inlines.len() && matches!(inlines[i + 1], Inline::Space(_));
                        let attr_idx = if has_space { i + 2 } else { i + 1 };

                        if attr_idx < inlines.len() {
                            if let Inline::Attr(attr) = &inlines[attr_idx] {
                                // Found Math + (Space?) + Attr pattern
                                // Wrap Math in a Span with the attribute
                                let mut classes = vec!["quarto-math-with-attribute".to_string()];
                                classes.extend(attr.1.clone());

                                math_processed.push(Inline::Span(Span {
                                    attr: (attr.0.clone(), classes, attr.2.clone()),
                                    content: vec![Inline::Math(math.clone())],
                                    // TODO: Should combine() source info from math and attr (see k-82)
                                    source_info: quarto_source_map::SourceInfo::default(),
                                }));

                                // Skip the Math, optional Space, and Attr
                                i = attr_idx + 1;
                                continue;
                            }
                        }
                    }

                    // Not a Math + Attr pattern, add as is
                    math_processed.push(inlines[i].clone());
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
                            if let Inline::Space(_) = inline {
                                // Transition to state 2
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
                                    result.push(Inline::Space(Space {
                                        // Synthetic Space: restore space between cite and invalid span
                                        source_info: quarto_source_map::SourceInfo::default(),
                                    }));
                                    result.push(inline);
                                    state = 0;
                                }
                            } else {
                                // Not a span, emit pending cite, space, and current inline
                                if let Some(cite) = pending_cite.take() {
                                    result.push(Inline::Cite(cite));
                                }
                                result.push(Inline::Space(Space {
                                    // Synthetic Space: restore space between cite and non-span element
                                    source_info: quarto_source_map::SourceInfo::default(),
                                }));
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
                        result.push(Inline::Space(Space {
                            // Synthetic Space: restore trailing space after incomplete citation pattern
                            source_info: quarto_source_map::SourceInfo::default(),
                        }));
                    }
                }

                FilterResult(result, true)
            })
            .with_attr(|attr| {
                // TODO: Add source location when attr has it
                error_collector_ref.borrow_mut().error(format!(
                    "Found attr in postprocess: {:?} - this should have been removed",
                    attr
                ));
                FilterResult(vec![], false)
            })
            .with_blocks(|blocks| {
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

                            if let Some(Inline::Attr(attr)) = caption_content.last() {
                                caption_attr = Some(attr.clone());
                                caption_content.pop(); // Remove the Attr from caption content
                            }

                            // If we found attributes in the caption, merge them with the table's attr
                            if let Some(caption_attr_value) = caption_attr {
                                // Merge: caption attributes override table attributes
                                // table.attr is (id, classes, key_values)
                                // Merge key-value pairs from caption into table
                                for (key, value) in caption_attr_value.2 {
                                    table.attr.2.insert(key, value);
                                }
                                // Merge classes from caption into table
                                for class in caption_attr_value.1 {
                                    if !table.attr.1.contains(&class) {
                                        table.attr.1.push(class);
                                    }
                                }
                                // Use caption id if table doesn't have one
                                if table.attr.0.is_empty() && !caption_attr_value.0.is_empty() {
                                    table.attr.0 = caption_attr_value.0;
                                }
                            }

                            // Attach caption to the table (with Attr removed from content)
                            table.caption = Caption {
                                short: None,
                                long: Some(vec![Block::Plain(Plain {
                                    content: caption_content,
                                    source_info: caption_block.source_info.clone(),
                                })]),
                            };
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
        let pandoc_result = topdown_traverse(doc, &mut filter);

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
    topdown_traverse(
        pandoc,
        &mut Filter::new().with_inlines(|inlines| {
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
                            // TODO k-69: Implement combine for quarto_source_map::SourceInfo
                            // if let Some(ref mut info) = current_source_info {
                            //     *info = info.combine(&s.source_info);
                            // }
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
    )
}
