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
        if let Some(debug) = lua.inspect_stack(level) {
            let source = debug.source();
            let line = debug.curr_line();

            // Skip C functions (internal mlua calls)
            // Accept "Lua", "main" (for main chunks), and any other non-C sources
            if source.what != "C"
                && let Some(src) = source.source
            {
                // Only return if it looks like a real source (has meaningful content)
                let src_str = src.to_string();
                if !src_str.is_empty() && src_str != "=[C]" {
                    return (src_str, line as i64);
                }
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
}
