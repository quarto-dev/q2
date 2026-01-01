/*
 * html_source.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Parallel walk of AST and JSON to build source info map for HTML writer.
 */

use crate::pandoc::{Block, Inline, Inlines, Pandoc};
use std::collections::HashMap;

use super::html::{ResolvedLocation, SourceNodeInfo};

/// Extract source node info from a JSON object.
///
/// Looks for "s" (pool ID) and "l" (resolved location) fields.
pub fn extract_source_node_info(json: &serde_json::Value) -> Option<SourceNodeInfo> {
    let pool_id = json.get("s")?.as_u64()? as usize;

    let location = json.get("l").and_then(|l| {
        Some(ResolvedLocation {
            file_id: l.get("f")?.as_u64()? as usize,
            start_line: l.get("b")?.get("l")?.as_u64()? as usize,
            start_col: l.get("b")?.get("c")?.as_u64()? as usize,
            end_line: l.get("e")?.get("l")?.as_u64()? as usize,
            end_col: l.get("e")?.get("c")?.as_u64()? as usize,
        })
    });

    Some(SourceNodeInfo { pool_id, location })
}

/// Build a source map by walking AST and JSON in parallel.
///
/// The JSON must have been generated from the same `&Pandoc` reference,
/// so the structures are guaranteed to be parallel.
pub fn build_source_map(
    pandoc: &Pandoc,
    json: &serde_json::Value,
) -> HashMap<*const (), SourceNodeInfo> {
    let mut map = HashMap::new();

    // Walk blocks
    if let Some(blocks_json) = json.get("blocks").and_then(|v| v.as_array()) {
        walk_blocks(&pandoc.blocks, blocks_json, &mut map);
    }

    map
}

/// Walk a sequence of blocks in parallel with JSON array.
fn walk_blocks(
    blocks: &[Block],
    json_blocks: &[serde_json::Value],
    map: &mut HashMap<*const (), SourceNodeInfo>,
) {
    for (block, json) in blocks.iter().zip(json_blocks.iter()) {
        walk_block(block, json, map);
    }
}

/// Walk a single block and its JSON representation.
fn walk_block(
    block: &Block,
    json: &serde_json::Value,
    map: &mut HashMap<*const (), SourceNodeInfo>,
) {
    // Extract source info from JSON and store with block pointer as key
    if let Some(info) = extract_source_node_info(json) {
        let key = block as *const Block as *const ();
        map.insert(key, info);
    }

    // Recurse into children based on block type
    match block {
        Block::Plain(plain) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&plain.content, inlines, map);
            }
        }
        Block::Paragraph(para) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&para.content, inlines, map);
            }
        }
        Block::LineBlock(lineblock) => {
            // JSON: {"t": "LineBlock", "c": [[inlines], [inlines], ...]}
            if let Some(lines) = json.get("c").and_then(|v| v.as_array()) {
                for (line, line_json) in lineblock.content.iter().zip(lines.iter()) {
                    if let Some(inlines) = line_json.as_array() {
                        walk_inlines(line, inlines, map);
                    }
                }
            }
        }
        Block::CodeBlock(_) => {
            // No children to walk
        }
        Block::RawBlock(_) => {
            // No children to walk
        }
        Block::BlockQuote(quote) => {
            // JSON: {"t": "BlockQuote", "c": [blocks]}
            if let Some(blocks) = json.get("c").and_then(|v| v.as_array()) {
                walk_blocks(&quote.content, blocks, map);
            }
        }
        Block::OrderedList(list) => {
            // JSON: {"t": "OrderedList", "c": [attr, [[blocks], [blocks], ...]]}
            if let Some(items) = json
                .get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_array())
            {
                for (item_blocks, item_json) in list.content.iter().zip(items.iter()) {
                    if let Some(blocks) = item_json.as_array() {
                        walk_blocks(item_blocks, blocks, map);
                    }
                }
            }
        }
        Block::BulletList(list) => {
            // JSON: {"t": "BulletList", "c": [[blocks], [blocks], ...]}
            if let Some(items) = json.get("c").and_then(|v| v.as_array()) {
                for (item_blocks, item_json) in list.content.iter().zip(items.iter()) {
                    if let Some(blocks) = item_json.as_array() {
                        walk_blocks(item_blocks, blocks, map);
                    }
                }
            }
        }
        Block::DefinitionList(deflist) => {
            // JSON: {"t": "DefinitionList", "c": [[[inlines], [[blocks], ...]], ...]}
            if let Some(items) = json.get("c").and_then(|v| v.as_array()) {
                for ((term, defs), item_json) in deflist.content.iter().zip(items.iter()) {
                    if let Some(item_arr) = item_json.as_array() {
                        // First element is term inlines
                        if let Some(term_json) = item_arr.first().and_then(|v| v.as_array()) {
                            walk_inlines(term, term_json, map);
                        }
                        // Second element is array of definitions
                        if let Some(defs_json) = item_arr.get(1).and_then(|v| v.as_array()) {
                            for (def_blocks, def_json) in defs.iter().zip(defs_json.iter()) {
                                if let Some(blocks) = def_json.as_array() {
                                    walk_blocks(def_blocks, blocks, map);
                                }
                            }
                        }
                    }
                }
            }
        }
        Block::Header(header) => {
            // JSON: {"t": "Header", "c": [level, attr, [inlines]]}
            if let Some(inlines) = json
                .get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(2))
                .and_then(|v| v.as_array())
            {
                walk_inlines(&header.content, inlines, map);
            }
        }
        Block::HorizontalRule(_) => {
            // No children
        }
        Block::Table(table) => {
            // Table structure is complex, walk caption and cells
            // JSON: {"t": "Table", "c": [attr, caption, colspecs, head, bodies, foot]}
            if let Some(arr) = json.get("c").and_then(|v| v.as_array()) {
                // Caption: [short, long] where long is blocks
                if let (Some(long_caption), Some(caption_json)) =
                    (&table.caption.long, arr.get(1).and_then(|v| v.as_array()))
                    && let Some(long_json) = caption_json.get(1).and_then(|v| v.as_array())
                {
                    walk_blocks(long_caption, long_json, map);
                }
                // Head, bodies, foot contain rows with cells containing blocks
                // This is simplified - full implementation would walk all cells
            }
        }
        Block::Figure(figure) => {
            // JSON: {"t": "Figure", "c": [attr, caption, [blocks]]}
            if let Some(arr) = json.get("c").and_then(|v| v.as_array()) {
                if let Some(blocks) = arr.get(2).and_then(|v| v.as_array()) {
                    walk_blocks(&figure.content, blocks, map);
                }
                // Caption
                if let (Some(long_caption), Some(caption_json)) =
                    (&figure.caption.long, arr.get(1).and_then(|v| v.as_array()))
                    && let Some(long_json) = caption_json.get(1).and_then(|v| v.as_array())
                {
                    walk_blocks(long_caption, long_json, map);
                }
            }
        }
        Block::Div(div) => {
            // JSON: {"t": "Div", "c": [attr, [blocks]]}
            if let Some(blocks) = json
                .get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_array())
            {
                walk_blocks(&div.content, blocks, map);
            }
        }
        Block::BlockMetadata(_) => {
            // No children
        }
        Block::NoteDefinitionPara(note) => {
            // Has inlines
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&note.content, inlines, map);
            }
        }
        Block::NoteDefinitionFencedBlock(note) => {
            // Has blocks
            if let Some(blocks) = json.get("c").and_then(|v| v.as_array()) {
                walk_blocks(&note.content, blocks, map);
            }
        }
        Block::CaptionBlock(caption) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&caption.content, inlines, map);
            }
        }
        Block::Custom(_custom) => {
            // Custom nodes are wrapped in Div in JSON
            // The "l" field is still present on the wrapper
            // Walk into slots
            if let Some(content) = json
                .get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_array())
            {
                // Each slot is wrapped in a Div with data-slot-name
                for slot_wrapper in content {
                    // Get the slot content (second element of the wrapper Div's content)
                    if let Some(slot_content) = slot_wrapper
                        .get("c")
                        .and_then(|v| v.as_array())
                        .and_then(|arr| arr.get(1))
                        .and_then(|v| v.as_array())
                    {
                        // Match by position with the custom node's slots
                        // This is a simplified approach - ideally we'd match by slot name
                        for block_json in slot_content {
                            // We don't have a direct mapping from JSON back to which slot
                            // For now, we just skip walking custom node children
                            // The custom node itself still gets source info
                            let _ = block_json;
                        }
                    }
                }
            }
        }
    }
}

/// Walk a sequence of inlines in parallel with JSON array.
fn walk_inlines(
    inlines: &Inlines,
    json_inlines: &[serde_json::Value],
    map: &mut HashMap<*const (), SourceNodeInfo>,
) {
    for (inline, json) in inlines.iter().zip(json_inlines.iter()) {
        walk_inline(inline, json, map);
    }
}

/// Walk a single inline and its JSON representation.
fn walk_inline(
    inline: &Inline,
    json: &serde_json::Value,
    map: &mut HashMap<*const (), SourceNodeInfo>,
) {
    // Extract source info from JSON and store with inline pointer as key
    if let Some(info) = extract_source_node_info(json) {
        let key = inline as *const Inline as *const ();
        map.insert(key, info);
    }

    // Recurse into children based on inline type
    match inline {
        Inline::Str(_) | Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => {
            // No children
        }
        Inline::Emph(e) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&e.content, inlines, map);
            }
        }
        Inline::Strong(s) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&s.content, inlines, map);
            }
        }
        Inline::Underline(u) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&u.content, inlines, map);
            }
        }
        Inline::Strikeout(s) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&s.content, inlines, map);
            }
        }
        Inline::Superscript(s) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&s.content, inlines, map);
            }
        }
        Inline::Subscript(s) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&s.content, inlines, map);
            }
        }
        Inline::SmallCaps(s) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&s.content, inlines, map);
            }
        }
        Inline::Quoted(q) => {
            // JSON: {"t": "Quoted", "c": [quote_type, [inlines]]}
            if let Some(inlines) = json
                .get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_array())
            {
                walk_inlines(&q.content, inlines, map);
            }
        }
        Inline::Code(_) | Inline::Math(_) | Inline::RawInline(_) => {
            // No inline children
        }
        Inline::Link(link) => {
            // JSON: {"t": "Link", "c": [attr, [inlines], target]}
            if let Some(inlines) = json
                .get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_array())
            {
                walk_inlines(&link.content, inlines, map);
            }
        }
        Inline::Image(image) => {
            // JSON: {"t": "Image", "c": [attr, [inlines], target]}
            if let Some(inlines) = json
                .get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_array())
            {
                walk_inlines(&image.content, inlines, map);
            }
        }
        Inline::Span(span) => {
            // JSON: {"t": "Span", "c": [attr, [inlines]]}
            if let Some(inlines) = json
                .get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_array())
            {
                walk_inlines(&span.content, inlines, map);
            }
        }
        Inline::Note(note) => {
            // JSON: {"t": "Note", "c": [blocks]}
            if let Some(blocks) = json.get("c").and_then(|v| v.as_array()) {
                walk_blocks(&note.content, blocks, map);
            }
        }
        Inline::Cite(cite) => {
            // JSON: {"t": "Cite", "c": [[citations], [inlines]]}
            if let Some(arr) = json.get("c").and_then(|v| v.as_array()) {
                // Walk citation prefix/suffix inlines
                if let Some(citations_json) = arr.first().and_then(|v| v.as_array()) {
                    for (citation, citation_json) in
                        cite.citations.iter().zip(citations_json.iter())
                    {
                        // Each citation has prefix and suffix inlines
                        if let Some(cit_arr) = citation_json
                            .get("citationPrefix")
                            .and_then(|v| v.as_array())
                        {
                            walk_inlines(&citation.prefix, cit_arr, map);
                        }
                        if let Some(cit_arr) = citation_json
                            .get("citationSuffix")
                            .and_then(|v| v.as_array())
                        {
                            walk_inlines(&citation.suffix, cit_arr, map);
                        }
                    }
                }
                // Walk content inlines
                if let Some(content_json) = arr.get(1).and_then(|v| v.as_array()) {
                    walk_inlines(&cite.content, content_json, map);
                }
            }
        }
        Inline::Shortcode(_) | Inline::NoteReference(_) | Inline::Attr(_, _) => {
            // Quarto extensions - no children to walk
        }
        Inline::Insert(ins) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&ins.content, inlines, map);
            }
        }
        Inline::Delete(del) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&del.content, inlines, map);
            }
        }
        Inline::Highlight(h) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&h.content, inlines, map);
            }
        }
        Inline::EditComment(c) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&c.content, inlines, map);
            }
        }
        Inline::Custom(_) => {
            // Custom inlines are wrapped in Span in JSON
            // Similar to Custom blocks - simplified handling
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_source_node_info_with_location() {
        let json = json!({
            "t": "Para",
            "c": [],
            "s": 42,
            "l": {
                "f": 0,
                "b": {"o": 10, "l": 5, "c": 1},
                "e": {"o": 50, "l": 5, "c": 41}
            }
        });

        let info = extract_source_node_info(&json).unwrap();
        assert_eq!(info.pool_id, 42);
        let loc = info.location.unwrap();
        assert_eq!(loc.file_id, 0);
        assert_eq!(loc.start_line, 5);
        assert_eq!(loc.start_col, 1);
        assert_eq!(loc.end_line, 5);
        assert_eq!(loc.end_col, 41);
    }

    #[test]
    fn test_extract_source_node_info_without_location() {
        let json = json!({
            "t": "Str",
            "c": "hello",
            "s": 7
        });

        let info = extract_source_node_info(&json).unwrap();
        assert_eq!(info.pool_id, 7);
        assert!(info.location.is_none());
    }

    #[test]
    fn test_extract_source_node_info_missing() {
        let json = json!({
            "t": "Str",
            "c": "hello"
        });

        assert!(extract_source_node_info(&json).is_none());
    }

    #[test]
    fn test_resolved_location_to_data_loc() {
        let loc = ResolvedLocation {
            file_id: 0,
            start_line: 5,
            start_col: 1,
            end_line: 5,
            end_col: 41,
        };
        assert_eq!(loc.to_data_loc(), "0:5:1-5:41");
    }
}
