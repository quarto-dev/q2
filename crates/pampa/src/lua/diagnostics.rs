/*
 * lua/diagnostics.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Diagnostic functions for Lua filters.
 *
 * This module provides `quarto.warn()` and `quarto.error()` functions that allow
 * filter authors to emit diagnostic messages during filter execution.
 */

use mlua::{Error, Lua, MultiValue, Result, Table, Value};
use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::{FileId, SourceInfo, SourcePiece};
use std::sync::Arc;

use super::types::{LuaBlock, LuaInline};
use crate::pandoc::{Block, Inline};

/// Register the quarto namespace with diagnostic functions
pub fn register_quarto_namespace(lua: &Lua) -> Result<()> {
    let quarto = lua.create_table()?;

    // Initialize the diagnostics storage table
    let diagnostics = lua.create_table()?;
    quarto.set("_diagnostics", diagnostics)?;

    // Register quarto.warn
    let quarto_ref = quarto.clone();
    quarto.set(
        "warn",
        lua.create_function(move |lua, args: MultiValue| {
            add_diagnostic(lua, &quarto_ref, "warning", args)
        })?,
    )?;

    // Register quarto.error
    let quarto_ref2 = quarto.clone();
    quarto.set(
        "error",
        lua.create_function(move |lua, args: MultiValue| {
            add_diagnostic(lua, &quarto_ref2, "error", args)
        })?,
    )?;

    // Set as global
    lua.globals().set("quarto", quarto)?;

    Ok(())
}

// ============================================================================
// SourceInfo <-> Lua Table Serialization
// ============================================================================

/// Serialize a SourceInfo to a Lua table
///
/// The table format uses a "t" field for the variant type:
/// - Original: { t = "Original", file_id = N, start_offset = N, end_offset = N }
/// - Substring: { t = "Substring", parent = {...}, start_offset = N, end_offset = N }
/// - Concat: { t = "Concat", pieces = [{source_info = {...}, offset_in_concat = N, length = N}, ...] }
/// - FilterProvenance: { t = "FilterProvenance", filter_path = "...", line = N }
fn source_info_to_lua_table(lua: &Lua, si: &SourceInfo) -> Result<Table> {
    let table = lua.create_table()?;
    match si {
        SourceInfo::Original {
            file_id,
            start_offset,
            end_offset,
        } => {
            table.set("t", "Original")?;
            table.set("file_id", file_id.0)?;
            table.set("start_offset", *start_offset)?;
            table.set("end_offset", *end_offset)?;
        }
        SourceInfo::Substring {
            parent,
            start_offset,
            end_offset,
        } => {
            table.set("t", "Substring")?;
            table.set("parent", source_info_to_lua_table(lua, parent)?)?;
            table.set("start_offset", *start_offset)?;
            table.set("end_offset", *end_offset)?;
        }
        SourceInfo::Concat { pieces } => {
            table.set("t", "Concat")?;
            let pieces_table = lua.create_table()?;
            for (i, piece) in pieces.iter().enumerate() {
                let piece_table = lua.create_table()?;
                piece_table.set(
                    "source_info",
                    source_info_to_lua_table(lua, &piece.source_info)?,
                )?;
                piece_table.set("offset_in_concat", piece.offset_in_concat)?;
                piece_table.set("length", piece.length)?;
                pieces_table.set(i + 1, piece_table)?;
            }
            table.set("pieces", pieces_table)?;
        }
        SourceInfo::FilterProvenance { filter_path, line } => {
            table.set("t", "FilterProvenance")?;
            table.set("filter_path", filter_path.clone())?;
            table.set("line", *line)?;
        }
    }
    Ok(table)
}

/// Deserialize a SourceInfo from a Lua table
fn source_info_from_lua_table(table: &Table) -> Result<SourceInfo> {
    let t: String = table.get("t")?;
    match t.as_str() {
        "Original" => Ok(SourceInfo::Original {
            file_id: FileId(table.get::<usize>("file_id")?),
            start_offset: table.get("start_offset")?,
            end_offset: table.get("end_offset")?,
        }),
        "Substring" => {
            let parent_table: Table = table.get("parent")?;
            Ok(SourceInfo::Substring {
                parent: Arc::new(source_info_from_lua_table(&parent_table)?),
                start_offset: table.get("start_offset")?,
                end_offset: table.get("end_offset")?,
            })
        }
        "Concat" => {
            let pieces_table: Table = table.get("pieces")?;
            let mut pieces = Vec::new();
            for i in 1..=pieces_table.raw_len() {
                let piece_table: Table = pieces_table.get(i)?;
                let si_table: Table = piece_table.get("source_info")?;
                pieces.push(SourcePiece {
                    source_info: source_info_from_lua_table(&si_table)?,
                    offset_in_concat: piece_table.get("offset_in_concat")?,
                    length: piece_table.get("length")?,
                });
            }
            Ok(SourceInfo::Concat { pieces })
        }
        "FilterProvenance" => Ok(SourceInfo::FilterProvenance {
            filter_path: table.get("filter_path")?,
            line: table.get("line")?,
        }),
        _ => Err(Error::runtime(format!("Unknown SourceInfo type: {}", t))),
    }
}

// ============================================================================
// Helper Functions for Extracting SourceInfo from Elements
// ============================================================================

/// Extract SourceInfo from an Inline element
///
/// Returns None for element types that don't have source_info (Shortcode, Attr)
fn get_inline_source_info(inline: &Inline) -> Option<SourceInfo> {
    match inline {
        Inline::Str(s) => Some(s.source_info.clone()),
        Inline::Emph(e) => Some(e.source_info.clone()),
        Inline::Underline(u) => Some(u.source_info.clone()),
        Inline::Strong(s) => Some(s.source_info.clone()),
        Inline::Strikeout(s) => Some(s.source_info.clone()),
        Inline::Superscript(s) => Some(s.source_info.clone()),
        Inline::Subscript(s) => Some(s.source_info.clone()),
        Inline::SmallCaps(s) => Some(s.source_info.clone()),
        Inline::Quoted(q) => Some(q.source_info.clone()),
        Inline::Cite(c) => Some(c.source_info.clone()),
        Inline::Code(c) => Some(c.source_info.clone()),
        Inline::Space(s) => Some(s.source_info.clone()),
        Inline::SoftBreak(s) => Some(s.source_info.clone()),
        Inline::LineBreak(l) => Some(l.source_info.clone()),
        Inline::Math(m) => Some(m.source_info.clone()),
        Inline::RawInline(r) => Some(r.source_info.clone()),
        Inline::Link(l) => Some(l.source_info.clone()),
        Inline::Image(i) => Some(i.source_info.clone()),
        Inline::Note(n) => Some(n.source_info.clone()),
        Inline::Span(s) => Some(s.source_info.clone()),
        Inline::Insert(i) => Some(i.source_info.clone()),
        Inline::Delete(d) => Some(d.source_info.clone()),
        Inline::Highlight(h) => Some(h.source_info.clone()),
        Inline::EditComment(e) => Some(e.source_info.clone()),
        Inline::NoteReference(n) => Some(n.source_info.clone()),
        Inline::Custom(c) => Some(c.source_info.clone()),
        // These element types don't have source_info
        Inline::Shortcode(_) => None,
        Inline::Attr(_, _) => None,
    }
}

/// Extract SourceInfo from a Block element
fn get_block_source_info(block: &Block) -> SourceInfo {
    match block {
        Block::Plain(p) => p.source_info.clone(),
        Block::Paragraph(p) => p.source_info.clone(),
        Block::LineBlock(l) => l.source_info.clone(),
        Block::CodeBlock(c) => c.source_info.clone(),
        Block::RawBlock(r) => r.source_info.clone(),
        Block::BlockQuote(b) => b.source_info.clone(),
        Block::OrderedList(o) => o.source_info.clone(),
        Block::BulletList(b) => b.source_info.clone(),
        Block::DefinitionList(d) => d.source_info.clone(),
        Block::Header(h) => h.source_info.clone(),
        Block::HorizontalRule(h) => h.source_info.clone(),
        Block::Table(t) => t.source_info.clone(),
        Block::Figure(f) => f.source_info.clone(),
        Block::Div(d) => d.source_info.clone(),
        Block::BlockMetadata(b) => b.source_info.clone(),
        Block::NoteDefinitionPara(n) => n.source_info.clone(),
        Block::NoteDefinitionFencedBlock(n) => n.source_info.clone(),
        Block::CaptionBlock(c) => c.source_info.clone(),
        Block::Custom(c) => c.source_info.clone(),
    }
}

/// Extract SourceInfo from an AST element (Inline or Block) and convert to Lua table
fn extract_source_info_from_element(lua: &Lua, elem: &Value) -> Result<Option<Table>> {
    if let Value::UserData(ud) = elem {
        // Try to extract source info from Inline element
        if let Ok(lua_inline) = ud.borrow::<LuaInline>() {
            if let Some(si) = get_inline_source_info(&lua_inline.0) {
                return Ok(Some(source_info_to_lua_table(lua, &si)?));
            }
            // Element type without source_info (Shortcode, Attr) - return None
            return Ok(None);
        }
        // Try to extract source info from Block element
        if let Ok(lua_block) = ud.borrow::<LuaBlock>() {
            let si = get_block_source_info(&lua_block.0);
            return Ok(Some(source_info_to_lua_table(lua, &si)?));
        }
    }
    // Not a recognized element type
    Ok(None)
}

/// Get SourceInfo for the Lua caller location (for stack-based fallback)
fn get_caller_source_info(lua: &Lua) -> SourceInfo {
    let (source, line) = get_caller_location(lua);
    let source_path = source.strip_prefix('@').unwrap_or(&source);
    SourceInfo::filter_provenance(source_path, line.max(0) as usize)
}

/// Add a diagnostic to the quarto._diagnostics table
fn add_diagnostic(lua: &Lua, quarto: &Table, kind: &str, args: MultiValue) -> Result<()> {
    let diagnostics: Table = quarto.get("_diagnostics")?;

    let mut iter = args.into_iter();

    // First argument: message (required)
    let message = match iter.next() {
        Some(Value::String(s)) => s.to_str()?.to_string(),
        Some(_) => {
            return Err(Error::runtime(
                "quarto.warn/error requires a string message as first argument",
            ));
        }
        None => {
            return Err(Error::runtime(
                "quarto.warn/error requires a message argument",
            ));
        }
    };

    // Second argument: optional AST element for source location
    // Extract SourceInfo and serialize to Lua table (don't resolve yet!)
    let source_info_table: Option<Table> = if let Some(elem) = iter.next() {
        // Try to extract SourceInfo from the element
        match extract_source_info_from_element(lua, &elem)? {
            Some(table) => Some(table),
            // Element was provided but had no source_info - fall back to stack location
            None => Some(source_info_to_lua_table(lua, &get_caller_source_info(lua))?),
        }
    } else {
        // No element provided - use Lua stack location
        Some(source_info_to_lua_table(lua, &get_caller_source_info(lua))?)
    };

    // Create diagnostic entry
    let entry = lua.create_table()?;
    entry.set("kind", kind)?;
    entry.set("message", message)?;
    if let Some(si_table) = source_info_table {
        entry.set("source_info", si_table)?;
    }

    // Add to diagnostics table (Lua arrays are 1-indexed)
    let len = diagnostics.raw_len();
    diagnostics.set(len + 1, entry)?;

    Ok(())
}

/// Get source location from the Lua call stack
///
/// Walks up the stack looking for the first Lua function call (not a C function).
/// Returns (source_path, line_number).
fn get_caller_location(lua: &Lua) -> (String, i64) {
    // Walk up the stack looking for filter code
    // Level 0 is the current function, level 1 is the caller, etc.
    // We start at level 1 to find the actual caller
    for level in 1..=10 {
        if let Some(result) = lua.inspect_stack(level, |debug| {
            let source: mlua::DebugSource = debug.source();
            let line = debug.current_line();

            // Skip C functions (internal mlua calls)
            // Accept "Lua", "main" (for main chunks), and any other non-C sources
            if source.what != "C"
                && let Some(src) = source.source
            {
                // Only return if it looks like a real source (has meaningful content)
                let src_str: String = src.to_string();
                if !src_str.is_empty() && src_str != "=[C]" {
                    return Some((src_str, line.unwrap_or(0) as i64));
                }
            }
            None
        }) {
            if let Some(location) = result {
                return location;
            }
        }
    }
    ("unknown".to_string(), 0)
}

/// Extract diagnostics from the Lua state after filter execution
///
/// Returns a vector of DiagnosticMessage objects that were collected
/// during filter execution via quarto.warn() and quarto.error().
pub fn extract_lua_diagnostics(lua: &Lua) -> Result<Vec<DiagnosticMessage>> {
    let quarto: Table = lua.globals().get("quarto")?;
    let diagnostics: Table = quarto.get("_diagnostics")?;

    let mut result = Vec::new();
    let len = diagnostics.raw_len();

    for i in 1..=len {
        let entry: Table = diagnostics.get(i)?;
        let kind: String = entry.get("kind")?;
        let message: String = entry.get("message")?;

        // Get SourceInfo from Lua table (deserialize)
        let source_info: Option<SourceInfo> = entry
            .get::<Option<Table>>("source_info")?
            .map(|t| source_info_from_lua_table(&t))
            .transpose()?;

        // Create the diagnostic message
        let diag = if kind == "error" {
            let mut builder = quarto_error_reporting::DiagnosticMessageBuilder::error(&message)
                .with_code("Q-11-1");
            if let Some(si) = source_info {
                builder = builder.with_location(si);
            }
            builder.build()
        } else {
            let mut builder = quarto_error_reporting::DiagnosticMessageBuilder::warning(&message)
                .with_code("Q-11-1");
            if let Some(si) = source_info {
                builder = builder.with_location(si);
            }
            builder.build()
        };

        result.push(diag);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_quarto_namespace() {
        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Check that quarto table exists
        let quarto: Table = lua.globals().get("quarto").unwrap();

        // Check that _diagnostics table exists
        let _diagnostics: Table = quarto.get("_diagnostics").unwrap();

        // Check that warn function exists
        let _warn: mlua::Function = quarto.get("warn").unwrap();

        // Check that error function exists
        let _error: mlua::Function = quarto.get("error").unwrap();
    }

    #[test]
    fn test_quarto_warn_basic() {
        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Call quarto.warn
        lua.load(r#"quarto.warn("Test warning message")"#)
            .exec()
            .unwrap();

        // Extract diagnostics
        let diagnostics = extract_lua_diagnostics(&lua).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
        assert!(diagnostics[0].title.contains("Test warning message"));
    }

    #[test]
    fn test_quarto_error_basic() {
        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Call quarto.error
        lua.load(r#"quarto.error("Test error message")"#)
            .exec()
            .unwrap();

        // Extract diagnostics
        let diagnostics = extract_lua_diagnostics(&lua).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Error
        );
        assert!(diagnostics[0].title.contains("Test error message"));
    }

    #[test]
    fn test_multiple_diagnostics() {
        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Call both warn and error multiple times
        lua.load(
            r#"
            quarto.warn("First warning")
            quarto.warn("Second warning")
            quarto.error("An error occurred")
            quarto.warn("Third warning")
        "#,
        )
        .exec()
        .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();

        assert_eq!(diagnostics.len(), 4);
        assert_eq!(
            diagnostics[0].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
        assert_eq!(
            diagnostics[1].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
        assert_eq!(
            diagnostics[2].kind,
            quarto_error_reporting::DiagnosticKind::Error
        );
        assert_eq!(
            diagnostics[3].kind,
            quarto_error_reporting::DiagnosticKind::Warning
        );
    }

    #[test]
    fn test_quarto_warn_requires_message() {
        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Call quarto.warn without arguments should fail
        let result = lua.load(r#"quarto.warn()"#).exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_quarto_warn_requires_string_message() {
        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Call quarto.warn with non-string should fail
        let result = lua.load(r#"quarto.warn(123)"#).exec();
        assert!(result.is_err());
    }

    #[test]
    fn test_source_location_captured() {
        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Load script with a name so we can verify source info
        lua.load(r#"quarto.warn("Warning at line 1")"#)
            .set_name("@test_filter.lua")
            .exec()
            .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();

        assert_eq!(diagnostics.len(), 1);
        // Verify source location was captured
        assert!(diagnostics[0].location.is_some());

        if let Some(SourceInfo::FilterProvenance { filter_path, line }) = &diagnostics[0].location {
            // The path should contain the filter name (@ prefix is stripped)
            assert!(
                filter_path.contains("test_filter.lua"),
                "Expected path to contain 'test_filter.lua', got '{}'",
                filter_path
            );
            assert_eq!(*line, 1);
        } else {
            panic!("Expected FilterProvenance source info");
        }
    }

    #[test]
    fn test_quarto_warn_preserves_original_source_info() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::FileId;

        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Create an Inline::Str element with SourceInfo::Original
        // This simulates an element from the original document (not created by a filter)
        let original_source_info = SourceInfo::Original {
            file_id: FileId(42),
            start_offset: 100,
            end_offset: 110,
        };

        let str_inline = Inline::Str(Str {
            text: "TODO".to_string(),
            source_info: original_source_info.clone(),
        });

        // Register the element as Lua userdata
        let lua_inline = LuaInline(str_inline);
        lua.globals()
            .set("test_elem", lua.create_userdata(lua_inline).unwrap())
            .unwrap();

        // Call quarto.warn with this element
        lua.load(r#"quarto.warn("Found TODO in document", test_elem)"#)
            .set_name("@linter.lua")
            .exec()
            .unwrap();

        // Extract diagnostics
        let diagnostics = extract_lua_diagnostics(&lua).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].location.is_some());

        // The key assertion: the SourceInfo should be Original, not FilterProvenance
        // This is the bug we're fixing - currently it falls back to FilterProvenance
        match &diagnostics[0].location {
            Some(SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            }) => {
                assert_eq!(file_id.0, 42, "file_id should be preserved");
                assert_eq!(*start_offset, 100, "start_offset should be preserved");
                assert_eq!(*end_offset, 110, "end_offset should be preserved");
            }
            Some(SourceInfo::FilterProvenance { filter_path, line }) => {
                panic!(
                    "Expected SourceInfo::Original, but got FilterProvenance({}, {}). \
                     This is the bug we're fixing!",
                    filter_path, line
                );
            }
            other => {
                panic!("Expected SourceInfo::Original, got {:?}", other);
            }
        }
    }

    #[test]
    fn test_quarto_warn_preserves_substring_source_info() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::FileId;

        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Create a Substring SourceInfo (a substring of an Original)
        let parent = SourceInfo::Original {
            file_id: FileId(10),
            start_offset: 0,
            end_offset: 100,
        };
        let substring_source_info = SourceInfo::substring(parent, 20, 40);

        let str_inline = Inline::Str(Str {
            text: "substring text".to_string(),
            source_info: substring_source_info,
        });

        let lua_inline = LuaInline(str_inline);
        lua.globals()
            .set("test_elem", lua.create_userdata(lua_inline).unwrap())
            .unwrap();

        lua.load(r#"quarto.warn("Warning about substring", test_elem)"#)
            .set_name("@filter.lua")
            .exec()
            .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();
        assert_eq!(diagnostics.len(), 1);

        match &diagnostics[0].location {
            Some(SourceInfo::Substring {
                parent,
                start_offset,
                end_offset,
            }) => {
                // Verify the Substring structure is preserved
                assert_eq!(*start_offset, 20);
                assert_eq!(*end_offset, 40);
                // Verify the parent is an Original
                match parent.as_ref() {
                    SourceInfo::Original {
                        file_id,
                        start_offset: parent_start,
                        end_offset: parent_end,
                    } => {
                        assert_eq!(file_id.0, 10);
                        assert_eq!(*parent_start, 0);
                        assert_eq!(*parent_end, 100);
                    }
                    _ => panic!("Expected parent to be Original"),
                }
            }
            other => panic!("Expected SourceInfo::Substring, got {:?}", other),
        }
    }

    #[test]
    fn test_quarto_warn_preserves_concat_source_info() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Str;
        use quarto_source_map::FileId;

        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Create a Concat SourceInfo (combining two Originals)
        let piece1 = SourceInfo::Original {
            file_id: FileId(1),
            start_offset: 10,
            end_offset: 20,
        };
        let piece2 = SourceInfo::Original {
            file_id: FileId(2),
            start_offset: 30,
            end_offset: 45,
        };
        let concat_source_info = SourceInfo::concat(vec![(piece1, 10), (piece2, 15)]);

        let str_inline = Inline::Str(Str {
            text: "concatenated text".to_string(),
            source_info: concat_source_info,
        });

        let lua_inline = LuaInline(str_inline);
        lua.globals()
            .set("test_elem", lua.create_userdata(lua_inline).unwrap())
            .unwrap();

        lua.load(r#"quarto.warn("Warning about concat", test_elem)"#)
            .set_name("@filter.lua")
            .exec()
            .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();
        assert_eq!(diagnostics.len(), 1);

        match &diagnostics[0].location {
            Some(SourceInfo::Concat { pieces }) => {
                assert_eq!(pieces.len(), 2);
                // Verify first piece
                assert_eq!(pieces[0].offset_in_concat, 0);
                assert_eq!(pieces[0].length, 10);
                match &pieces[0].source_info {
                    SourceInfo::Original { file_id, .. } => {
                        assert_eq!(file_id.0, 1);
                    }
                    _ => panic!("Expected piece 0 to be Original"),
                }
                // Verify second piece
                assert_eq!(pieces[1].offset_in_concat, 10);
                assert_eq!(pieces[1].length, 15);
                match &pieces[1].source_info {
                    SourceInfo::Original { file_id, .. } => {
                        assert_eq!(file_id.0, 2);
                    }
                    _ => panic!("Expected piece 1 to be Original"),
                }
            }
            other => panic!("Expected SourceInfo::Concat, got {:?}", other),
        }
    }

    #[test]
    fn test_quarto_warn_with_block_element() {
        use crate::pandoc::Block;
        use crate::pandoc::block::Paragraph;
        use quarto_source_map::FileId;

        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Create a Block element with SourceInfo::Original
        let original_source_info = SourceInfo::Original {
            file_id: FileId(99),
            start_offset: 500,
            end_offset: 600,
        };

        let para_block = Block::Paragraph(Paragraph {
            content: vec![],
            source_info: original_source_info,
        });

        let lua_block = LuaBlock(para_block);
        lua.globals()
            .set("test_block", lua.create_userdata(lua_block).unwrap())
            .unwrap();

        lua.load(r#"quarto.warn("Warning about paragraph", test_block)"#)
            .set_name("@filter.lua")
            .exec()
            .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();
        assert_eq!(diagnostics.len(), 1);

        match &diagnostics[0].location {
            Some(SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            }) => {
                assert_eq!(file_id.0, 99);
                assert_eq!(*start_offset, 500);
                assert_eq!(*end_offset, 600);
            }
            other => panic!("Expected SourceInfo::Original, got {:?}", other),
        }
    }

    #[test]
    fn test_source_info_roundtrip_serialization() {
        // Test that all SourceInfo variants can be serialized to Lua and back
        use quarto_source_map::FileId;

        let lua = Lua::new();

        // Test Original
        let original = SourceInfo::Original {
            file_id: FileId(42),
            start_offset: 100,
            end_offset: 200,
        };
        let table = source_info_to_lua_table(&lua, &original).unwrap();
        let roundtrip = source_info_from_lua_table(&table).unwrap();
        assert_eq!(original, roundtrip);

        // Test Substring
        let substring = SourceInfo::substring(
            SourceInfo::Original {
                file_id: FileId(1),
                start_offset: 0,
                end_offset: 1000,
            },
            50,
            100,
        );
        let table = source_info_to_lua_table(&lua, &substring).unwrap();
        let roundtrip = source_info_from_lua_table(&table).unwrap();
        assert_eq!(substring, roundtrip);

        // Test Concat
        let concat = SourceInfo::concat(vec![
            (
                SourceInfo::Original {
                    file_id: FileId(1),
                    start_offset: 0,
                    end_offset: 10,
                },
                10,
            ),
            (
                SourceInfo::Original {
                    file_id: FileId(2),
                    start_offset: 20,
                    end_offset: 35,
                },
                15,
            ),
        ]);
        let table = source_info_to_lua_table(&lua, &concat).unwrap();
        let roundtrip = source_info_from_lua_table(&table).unwrap();
        assert_eq!(concat, roundtrip);

        // Test FilterProvenance
        let filter_prov = SourceInfo::filter_provenance("/path/to/filter.lua", 42);
        let table = source_info_to_lua_table(&lua, &filter_prov).unwrap();
        let roundtrip = source_info_from_lua_table(&table).unwrap();
        assert_eq!(filter_prov, roundtrip);
    }

    // =========================================================================
    // Tests for get_inline_source_info - covering all Inline variants
    // =========================================================================

    #[test]
    fn test_get_inline_source_info_emph() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Emph;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(1),
            start_offset: 10,
            end_offset: 20,
        };
        let emph = Inline::Emph(Emph {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&emph), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_underline() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Underline;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(2),
            start_offset: 20,
            end_offset: 30,
        };
        let underline = Inline::Underline(Underline {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&underline), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_strong() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Strong;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(3),
            start_offset: 30,
            end_offset: 40,
        };
        let strong = Inline::Strong(Strong {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&strong), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_strikeout() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Strikeout;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(4),
            start_offset: 40,
            end_offset: 50,
        };
        let strikeout = Inline::Strikeout(Strikeout {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&strikeout), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_superscript() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Superscript;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(5),
            start_offset: 50,
            end_offset: 60,
        };
        let superscript = Inline::Superscript(Superscript {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&superscript), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_subscript() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Subscript;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(6),
            start_offset: 60,
            end_offset: 70,
        };
        let subscript = Inline::Subscript(Subscript {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&subscript), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_smallcaps() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::SmallCaps;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(7),
            start_offset: 70,
            end_offset: 80,
        };
        let smallcaps = Inline::SmallCaps(SmallCaps {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&smallcaps), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_quoted() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::{QuoteType, Quoted};
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(8),
            start_offset: 80,
            end_offset: 90,
        };
        let quoted = Inline::Quoted(Quoted {
            quote_type: QuoteType::DoubleQuote,
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&quoted), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_cite() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Cite;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(9),
            start_offset: 90,
            end_offset: 100,
        };
        let cite = Inline::Cite(Cite {
            citations: vec![],
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&cite), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_code() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::inline::Code;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(10),
            start_offset: 100,
            end_offset: 110,
        };
        let code = Inline::Code(Code {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            text: "code".to_string(),
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_inline_source_info(&code), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_space() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Space;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(11),
            start_offset: 110,
            end_offset: 111,
        };
        let space = Inline::Space(Space {
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&space), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_softbreak() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::SoftBreak;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(12),
            start_offset: 120,
            end_offset: 121,
        };
        let softbreak = Inline::SoftBreak(SoftBreak {
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&softbreak), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_linebreak() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::LineBreak;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(13),
            start_offset: 130,
            end_offset: 131,
        };
        let linebreak = Inline::LineBreak(LineBreak {
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&linebreak), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_math() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::{Math, MathType};
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(14),
            start_offset: 140,
            end_offset: 150,
        };
        let math = Inline::Math(Math {
            math_type: MathType::InlineMath,
            text: "x^2".to_string(),
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&math), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_rawinline() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::RawInline;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(15),
            start_offset: 150,
            end_offset: 160,
        };
        let rawinline = Inline::RawInline(RawInline {
            format: "html".to_string(),
            text: "<span>".to_string(),
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&rawinline), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_link() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::{AttrSourceInfo, TargetSourceInfo};
        use crate::pandoc::inline::Link;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(16),
            start_offset: 160,
            end_offset: 170,
        };
        let link = Inline::Link(Link {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            target: ("url".to_string(), "title".to_string()),
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });
        assert_eq!(get_inline_source_info(&link), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_image() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::{AttrSourceInfo, TargetSourceInfo};
        use crate::pandoc::inline::Image;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(17),
            start_offset: 170,
            end_offset: 180,
        };
        let image = Inline::Image(Image {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            target: ("image.png".to_string(), String::new()),
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
            target_source: TargetSourceInfo::empty(),
        });
        assert_eq!(get_inline_source_info(&image), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_note() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Note;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(18),
            start_offset: 180,
            end_offset: 190,
        };
        let note = Inline::Note(Note {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&note), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_span() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::inline::Span;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(19),
            start_offset: 190,
            end_offset: 200,
        };
        let span = Inline::Span(Span {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_inline_source_info(&span), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_insert() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::inline::Insert;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(20),
            start_offset: 200,
            end_offset: 210,
        };
        let insert = Inline::Insert(Insert {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_inline_source_info(&insert), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_delete() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::inline::Delete;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(21),
            start_offset: 210,
            end_offset: 220,
        };
        let delete = Inline::Delete(Delete {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_inline_source_info(&delete), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_highlight() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::inline::Highlight;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(22),
            start_offset: 220,
            end_offset: 230,
        };
        let highlight = Inline::Highlight(Highlight {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_inline_source_info(&highlight), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_editcomment() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::inline::EditComment;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(23),
            start_offset: 230,
            end_offset: 240,
        };
        let editcomment = Inline::EditComment(EditComment {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_inline_source_info(&editcomment), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_notereference() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::NoteReference;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(24),
            start_offset: 240,
            end_offset: 250,
        };
        let noteref = Inline::NoteReference(NoteReference {
            id: "note1".to_string(),
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&noteref), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_custom() {
        use crate::pandoc::Inline;
        use crate::pandoc::custom::CustomNode;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(25),
            start_offset: 250,
            end_offset: 260,
        };
        let custom = Inline::Custom(CustomNode {
            type_name: "test".to_string(),
            slots: LinkedHashMap::new(),
            plain_data: serde_json::Value::Null,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: source_info.clone(),
        });
        assert_eq!(get_inline_source_info(&custom), Some(source_info));
    }

    #[test]
    fn test_get_inline_source_info_shortcode_returns_none() {
        use crate::pandoc::Inline;
        use quarto_pandoc_types::shortcode::Shortcode;
        use std::collections::HashMap;

        let shortcode = Inline::Shortcode(Shortcode {
            is_escaped: false,
            name: "test".to_string(),
            positional_args: vec![],
            keyword_args: HashMap::new(),
            source_info: quarto_source_map::SourceInfo::default(),
        });
        // Shortcode now has source_info, but we return None for now
        // (this may change when shortcode resolution is implemented)
        assert_eq!(get_inline_source_info(&shortcode), None);
    }

    #[test]
    fn test_get_inline_source_info_attr_returns_none() {
        use crate::pandoc::Inline;
        use crate::pandoc::attr::AttrSourceInfo;
        use hashlink::LinkedHashMap;

        let attr = Inline::Attr(
            (String::new(), vec![], LinkedHashMap::new()),
            AttrSourceInfo::empty(),
        );
        assert_eq!(get_inline_source_info(&attr), None);
    }

    // =========================================================================
    // Tests for get_block_source_info - covering all Block variants
    // =========================================================================

    #[test]
    fn test_get_block_source_info_plain() {
        use crate::pandoc::Block;
        use crate::pandoc::block::Plain;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(1),
            start_offset: 0,
            end_offset: 10,
        };
        let plain = Block::Plain(Plain {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&plain), source_info);
    }

    #[test]
    fn test_get_block_source_info_lineblock() {
        use crate::pandoc::Block;
        use crate::pandoc::block::LineBlock;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(2),
            start_offset: 10,
            end_offset: 20,
        };
        let lineblock = Block::LineBlock(LineBlock {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&lineblock), source_info);
    }

    #[test]
    fn test_get_block_source_info_codeblock() {
        use crate::pandoc::Block;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::block::CodeBlock;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(3),
            start_offset: 20,
            end_offset: 30,
        };
        let codeblock = Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            text: "code".to_string(),
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_block_source_info(&codeblock), source_info);
    }

    #[test]
    fn test_get_block_source_info_rawblock() {
        use crate::pandoc::Block;
        use crate::pandoc::block::RawBlock;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(4),
            start_offset: 30,
            end_offset: 40,
        };
        let rawblock = Block::RawBlock(RawBlock {
            format: "html".to_string(),
            text: "<div>".to_string(),
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&rawblock), source_info);
    }

    #[test]
    fn test_get_block_source_info_blockquote() {
        use crate::pandoc::Block;
        use crate::pandoc::block::BlockQuote;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(5),
            start_offset: 40,
            end_offset: 50,
        };
        let blockquote = Block::BlockQuote(BlockQuote {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&blockquote), source_info);
    }

    #[test]
    fn test_get_block_source_info_orderedlist() {
        use crate::pandoc::Block;
        use crate::pandoc::block::OrderedList;
        use crate::pandoc::list::{ListNumberDelim, ListNumberStyle};
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(6),
            start_offset: 50,
            end_offset: 60,
        };
        let orderedlist = Block::OrderedList(OrderedList {
            attr: (1, ListNumberStyle::Default, ListNumberDelim::Default),
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&orderedlist), source_info);
    }

    #[test]
    fn test_get_block_source_info_bulletlist() {
        use crate::pandoc::Block;
        use crate::pandoc::block::BulletList;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(7),
            start_offset: 60,
            end_offset: 70,
        };
        let bulletlist = Block::BulletList(BulletList {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&bulletlist), source_info);
    }

    #[test]
    fn test_get_block_source_info_definitionlist() {
        use crate::pandoc::Block;
        use crate::pandoc::block::DefinitionList;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(8),
            start_offset: 70,
            end_offset: 80,
        };
        let deflist = Block::DefinitionList(DefinitionList {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&deflist), source_info);
    }

    #[test]
    fn test_get_block_source_info_header() {
        use crate::pandoc::Block;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::block::Header;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(9),
            start_offset: 80,
            end_offset: 90,
        };
        let header = Block::Header(Header {
            level: 1,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_block_source_info(&header), source_info);
    }

    #[test]
    fn test_get_block_source_info_horizontalrule() {
        use crate::pandoc::Block;
        use crate::pandoc::block::HorizontalRule;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(10),
            start_offset: 90,
            end_offset: 93,
        };
        let hrule = Block::HorizontalRule(HorizontalRule {
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&hrule), source_info);
    }

    #[test]
    fn test_get_block_source_info_table() {
        use crate::pandoc::Block;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::caption::Caption;
        use crate::pandoc::table::{Table, TableFoot, TableHead};
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(11),
            start_offset: 100,
            end_offset: 200,
        };
        let table = Block::Table(Table {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: None,
                source_info: source_info.clone(),
            },
            colspec: vec![],
            head: TableHead {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                rows: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            },
            bodies: vec![],
            foot: TableFoot {
                attr: (String::new(), vec![], LinkedHashMap::new()),
                rows: vec![],
                source_info: source_info.clone(),
                attr_source: AttrSourceInfo::empty(),
            },
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_block_source_info(&table), source_info);
    }

    #[test]
    fn test_get_block_source_info_figure() {
        use crate::pandoc::Block;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::block::Figure;
        use crate::pandoc::caption::Caption;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(12),
            start_offset: 200,
            end_offset: 300,
        };
        let figure = Block::Figure(Figure {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            caption: Caption {
                short: None,
                long: None,
                source_info: source_info.clone(),
            },
            content: vec![],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_block_source_info(&figure), source_info);
    }

    #[test]
    fn test_get_block_source_info_div() {
        use crate::pandoc::Block;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::block::Div;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(13),
            start_offset: 300,
            end_offset: 400,
        };
        let div = Block::Div(Div {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            content: vec![],
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        assert_eq!(get_block_source_info(&div), source_info);
    }

    #[test]
    fn test_get_block_source_info_blockmetadata() {
        use crate::pandoc::Block;
        use crate::pandoc::block::MetaBlock;
        use crate::pandoc::config_value::ConfigValue;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(14),
            start_offset: 400,
            end_offset: 500,
        };
        let metablock = Block::BlockMetadata(MetaBlock {
            meta: ConfigValue::null(source_info.clone()),
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&metablock), source_info);
    }

    #[test]
    fn test_get_block_source_info_notedefinitionpara() {
        use crate::pandoc::Block;
        use crate::pandoc::block::NoteDefinitionPara;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(15),
            start_offset: 500,
            end_offset: 600,
        };
        let notedefpara = Block::NoteDefinitionPara(NoteDefinitionPara {
            id: "note1".to_string(),
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&notedefpara), source_info);
    }

    #[test]
    fn test_get_block_source_info_notedefinitionfencedblock() {
        use crate::pandoc::Block;
        use crate::pandoc::block::NoteDefinitionFencedBlock;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(16),
            start_offset: 600,
            end_offset: 700,
        };
        let notedeffenced = Block::NoteDefinitionFencedBlock(NoteDefinitionFencedBlock {
            id: "note2".to_string(),
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&notedeffenced), source_info);
    }

    #[test]
    fn test_get_block_source_info_captionblock() {
        use crate::pandoc::Block;
        use crate::pandoc::block::CaptionBlock;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(17),
            start_offset: 700,
            end_offset: 800,
        };
        let captionblock = Block::CaptionBlock(CaptionBlock {
            content: vec![],
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&captionblock), source_info);
    }

    #[test]
    fn test_get_block_source_info_custom() {
        use crate::pandoc::Block;
        use crate::pandoc::custom::CustomNode;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let source_info = SourceInfo::Original {
            file_id: FileId(18),
            start_offset: 800,
            end_offset: 900,
        };
        let custom = Block::Custom(CustomNode {
            type_name: "callout".to_string(),
            slots: LinkedHashMap::new(),
            plain_data: serde_json::Value::Null,
            attr: (String::new(), vec![], LinkedHashMap::new()),
            source_info: source_info.clone(),
        });
        assert_eq!(get_block_source_info(&custom), source_info);
    }

    // =========================================================================
    // Tests for error paths and edge cases
    // =========================================================================

    #[test]
    fn test_source_info_from_lua_table_unknown_type_error() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("t", "Unknown").unwrap();

        let result = source_info_from_lua_table(&table);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Unknown SourceInfo type"));
    }

    #[test]
    fn test_extract_source_info_non_userdata_returns_none() {
        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Pass a non-userdata value (a table) as the element argument
        lua.load(
            r#"
            local t = {}
            quarto.warn("Test warning", t)
        "#,
        )
        .set_name("@test.lua")
        .exec()
        .unwrap();

        // Should still work, falling back to stack location
        let diagnostics = extract_lua_diagnostics(&lua).unwrap();
        assert_eq!(diagnostics.len(), 1);
        // Should have FilterProvenance since the element wasn't recognized
        match &diagnostics[0].location {
            Some(SourceInfo::FilterProvenance { .. }) => {}
            other => panic!(
                "Expected FilterProvenance for non-userdata element, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_quarto_warn_with_shortcode_element_falls_back_to_stack() {
        use crate::pandoc::Inline;
        use quarto_pandoc_types::shortcode::Shortcode;
        use std::collections::HashMap;

        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Shortcode has source_info but get_inline_source_info returns None for it,
        // so this test verifies fallback to stack location
        let shortcode = Inline::Shortcode(Shortcode {
            is_escaped: false,
            name: "test".to_string(),
            positional_args: vec![],
            keyword_args: HashMap::new(),
            source_info: quarto_source_map::SourceInfo::default(),
        });
        let lua_inline = LuaInline(shortcode);
        lua.globals()
            .set("test_shortcode", lua.create_userdata(lua_inline).unwrap())
            .unwrap();

        lua.load(r#"quarto.warn("Warning about shortcode", test_shortcode)"#)
            .set_name("@shortcode_filter.lua")
            .exec()
            .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();
        assert_eq!(diagnostics.len(), 1);

        // Should fall back to FilterProvenance since Shortcode returns None for source_info
        match &diagnostics[0].location {
            Some(SourceInfo::FilterProvenance { filter_path, .. }) => {
                assert!(filter_path.contains("shortcode_filter.lua"));
            }
            other => panic!(
                "Expected FilterProvenance for Shortcode element, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_quarto_warn_with_more_inline_variants_in_lua() {
        use crate::pandoc::Inline;
        use crate::pandoc::inline::Emph;
        use quarto_source_map::FileId;

        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        let source_info = SourceInfo::Original {
            file_id: FileId(77),
            start_offset: 100,
            end_offset: 150,
        };
        let emph = Inline::Emph(Emph {
            content: vec![],
            source_info: source_info.clone(),
        });
        let lua_inline = LuaInline(emph);
        lua.globals()
            .set("test_emph", lua.create_userdata(lua_inline).unwrap())
            .unwrap();

        lua.load(r#"quarto.warn("Warning about emph", test_emph)"#)
            .exec()
            .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();
        assert_eq!(diagnostics.len(), 1);

        match &diagnostics[0].location {
            Some(SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            }) => {
                assert_eq!(file_id.0, 77);
                assert_eq!(*start_offset, 100);
                assert_eq!(*end_offset, 150);
            }
            other => panic!("Expected Original source info, got {:?}", other),
        }
    }

    #[test]
    fn test_quarto_warn_with_block_codeblock() {
        use crate::pandoc::Block;
        use crate::pandoc::attr::AttrSourceInfo;
        use crate::pandoc::block::CodeBlock;
        use hashlink::LinkedHashMap;
        use quarto_source_map::FileId;

        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        let source_info = SourceInfo::Original {
            file_id: FileId(88),
            start_offset: 200,
            end_offset: 300,
        };
        let codeblock = Block::CodeBlock(CodeBlock {
            attr: (String::new(), vec![], LinkedHashMap::new()),
            text: "print('hello')".to_string(),
            source_info: source_info.clone(),
            attr_source: AttrSourceInfo::empty(),
        });
        let lua_block = LuaBlock(codeblock);
        lua.globals()
            .set("test_codeblock", lua.create_userdata(lua_block).unwrap())
            .unwrap();

        lua.load(r#"quarto.warn("Warning about code block", test_codeblock)"#)
            .exec()
            .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();
        assert_eq!(diagnostics.len(), 1);

        match &diagnostics[0].location {
            Some(SourceInfo::Original {
                file_id,
                start_offset,
                end_offset,
            }) => {
                assert_eq!(file_id.0, 88);
                assert_eq!(*start_offset, 200);
                assert_eq!(*end_offset, 300);
            }
            other => panic!("Expected Original source info, got {:?}", other),
        }
    }
}
