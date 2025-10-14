/*
 * postprocess.rs
 * Copyright (c) 2025 Posit, PBC
 */

use crate::filters::{
    Filter, FilterReturn::FilterResult, FilterReturn::Unchanged, topdown_traverse,
};
use crate::pandoc::attr::{Attr, is_empty_attr};
use crate::pandoc::block::{Block, Figure, Plain};
use crate::pandoc::caption::Caption;
use crate::pandoc::inline::{Inline, Inlines, Space, Span, Str, Superscript};
use crate::pandoc::location::{Range, SourceInfo, empty_range, empty_source_info};
use crate::pandoc::pandoc::Pandoc;
use crate::pandoc::shortcode::shortcode_to_span;
use crate::utils::autoid;
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
            let source_info = if j > i + 1 {
                SourceInfo::with_range(Range {
                    start: start_info.range.start.clone(),
                    end: end_info.range.end.clone(),
                })
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

/// Apply post-processing transformations to the Pandoc AST
pub fn postprocess(doc: Pandoc) -> Result<Pandoc, Vec<String>> {
    let mut errors = Vec::new();
    let result = {
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
                FilterResult(
                    vec![Block::Figure(Figure {
                        attr: figure_attr,
                        caption: Caption {
                            short: None,
                            long: Some(vec![Block::Plain(Plain {
                                content: image.content.clone(),
                                source_info: SourceInfo::with_range(empty_range()),
                            })]),
                        },
                        content: vec![Block::Plain(Plain {
                            content: vec![Inline::Image(new_image)],
                            source_info: SourceInfo::with_range(empty_range()),
                        })],
                        source_info: SourceInfo::with_range(empty_range()),
                    })],
                    true,
                )
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
                        source_info: empty_source_info(),
                    })],
                    false,
                )
            })
            .with_insert(|insert| {
                let (content, _changed) = trim_inlines(insert.content);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (
                            "".to_string(),
                            vec!["quarto-insert".to_string()],
                            HashMap::new(),
                        ),
                        content,
                        source_info: empty_source_info(),
                    })],
                    true,
                )
            })
            .with_delete(|delete| {
                let (content, _changed) = trim_inlines(delete.content);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (
                            "".to_string(),
                            vec!["quarto-delete".to_string()],
                            HashMap::new(),
                        ),
                        content,
                        source_info: empty_source_info(),
                    })],
                    true,
                )
            })
            .with_highlight(|highlight| {
                let (content, _changed) = trim_inlines(highlight.content);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (
                            "".to_string(),
                            vec!["quarto-highlight".to_string()],
                            HashMap::new(),
                        ),
                        content,
                        source_info: empty_source_info(),
                    })],
                    true,
                )
            })
            .with_edit_comment(|edit_comment| {
                let (content, _changed) = trim_inlines(edit_comment.content);
                FilterResult(
                    vec![Inline::Span(Span {
                        attr: (
                            "".to_string(),
                            vec!["quarto-edit-comment".to_string()],
                            HashMap::new(),
                        ),
                        content,
                        source_info: empty_source_info(),
                    })],
                    true,
                )
            })
            .with_inlines(|inlines| {
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

                for inline in inlines {
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
                                            source_info: SourceInfo::with_range(empty_range()),
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
                                                                source_info: SourceInfo::with_range(
                                                                    empty_range(),
                                                                ),
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
                                        source_info: SourceInfo::with_range(empty_range()),
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
                                    source_info: SourceInfo::with_range(empty_range()),
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
                            source_info: SourceInfo::with_range(empty_range()),
                        }));
                    }
                }

                FilterResult(result, true)
            })
            .with_attr(|attr| {
                // TODO in order to do good error messages here, attr will need source mapping
                errors.push(format!(
                    "Found attr in postprocess: {:?} - this should have been removed",
                    attr
                ));
                FilterResult(vec![], false)
            });
        topdown_traverse(doc, &mut filter)
    };
    if !errors.is_empty() {
        Err(errors)
    } else {
        Ok(result)
    }
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
            let mut current_source_info: Option<SourceInfo> = None;
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
    )
}
