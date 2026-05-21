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
use quarto_source_map::{Anchor, AnchorRole, By, FileId, SourceInfo, SourcePiece};
use smallvec::SmallVec;
use std::sync::Arc;

use super::types::{LuaBlock, LuaInline};

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
/// - Generated: { t = "Generated", by = { kind = "...", data = "..." (JSON-encoded) },
///                from = [{role = "Invocation" | "ValueSource" | "Other:<name>",
///                         source_info = {...}}, ...] }
///
/// The reader also accepts the legacy `"FilterProvenance"` tag for back-compat,
/// mapping it onto `Generated { by: filter, from: [] }`.
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
        SourceInfo::Generated { by, from } => {
            table.set("t", "Generated")?;
            table.set("by", by_to_lua_table(lua, by)?)?;
            let from_table = lua.create_table()?;
            for (i, anchor) in from.iter().enumerate() {
                let anchor_table = lua.create_table()?;
                anchor_table.set("role", anchor_role_to_lua_string(&anchor.role))?;
                anchor_table.set(
                    "source_info",
                    source_info_to_lua_table(lua, &anchor.source_info)?,
                )?;
                from_table.set(i + 1, anchor_table)?;
            }
            table.set("from", from_table)?;
        }
    }
    Ok(table)
}

/// Serialize a [`By`] to a Lua table: `{ kind = "...", data = "<json>" }`.
///
/// `data` is JSON-encoded as a string because Lua tables don't carry the
/// `serde_json::Value` discriminator; readers decode it back via
/// [`serde_json::from_str`].
fn by_to_lua_table(lua: &Lua, by: &By) -> Result<Table> {
    let table = lua.create_table()?;
    table.set("kind", by.kind.clone())?;
    if !by.data.is_null() {
        let encoded = serde_json::to_string(&by.data)
            .map_err(|e| Error::runtime(format!("By.data serialize failed: {e}")))?;
        table.set("data", encoded)?;
    }
    Ok(table)
}

/// Serialize an [`AnchorRole`] to a Lua string.
fn anchor_role_to_lua_string(role: &AnchorRole) -> String {
    match role {
        AnchorRole::Invocation => "Invocation".to_string(),
        AnchorRole::ValueSource => "ValueSource".to_string(),
        AnchorRole::Other(name) => format!("Other:{name}"),
    }
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
        "Generated" => {
            let by_table: Table = table.get("by")?;
            let by = by_from_lua_table(&by_table)?;
            let mut from: SmallVec<[Anchor; 2]> = SmallVec::new();
            // The `from` field is optional in serialization; absent means empty.
            if let Ok(from_table) = table.get::<Table>("from") {
                for i in 1..=from_table.raw_len() {
                    let anchor_table: Table = from_table.get(i)?;
                    let role_str: String = anchor_table.get("role")?;
                    let role = anchor_role_from_lua_string(&role_str);
                    let si_table: Table = anchor_table.get("source_info")?;
                    from.push(Anchor {
                        role,
                        source_info: Arc::new(source_info_from_lua_table(&si_table)?),
                    });
                }
            }
            Ok(SourceInfo::Generated { by, from })
        }
        // Legacy back-compat: read the old "FilterProvenance" tag as
        // `Generated { by: filter(...), from: [] }`. Writers never emit
        // this tag after Plan 4 Phase 4.
        "FilterProvenance" => Ok(SourceInfo::Generated {
            by: By::filter(
                table.get::<String>("filter_path")?,
                table.get::<usize>("line")?,
            ),
            from: SmallVec::new(),
        }),
        _ => Err(Error::runtime(format!("Unknown SourceInfo type: {}", t))),
    }
}

/// Deserialize a [`By`] from `{ kind = "...", data = "<json>" }`.
fn by_from_lua_table(table: &Table) -> Result<By> {
    let kind: String = table.get("kind")?;
    let data = match table.get::<String>("data") {
        Ok(encoded) => serde_json::from_str(&encoded)
            .map_err(|e| Error::runtime(format!("By.data parse failed: {e}")))?,
        Err(_) => serde_json::Value::Null,
    };
    Ok(By { kind, data })
}

/// Inverse of [`anchor_role_to_lua_string`].
fn anchor_role_from_lua_string(s: &str) -> AnchorRole {
    if let Some(rest) = s.strip_prefix("Other:") {
        AnchorRole::Other(rest.to_string())
    } else if s == "ValueSource" {
        AnchorRole::ValueSource
    } else {
        AnchorRole::Invocation
    }
}

// ============================================================================
// Helper Functions for Extracting SourceInfo from Elements
// ============================================================================

/// Extract SourceInfo from an AST element (Inline or Block) and convert to Lua table
fn extract_source_info_from_element(lua: &Lua, elem: &Value) -> Result<Option<Table>> {
    if let Value::UserData(ud) = elem {
        // Try to extract source info from Inline element
        if let Ok(lua_inline) = ud.borrow::<LuaInline>() {
            let inline = lua_inline.borrow_inline();
            return Ok(Some(source_info_to_lua_table(lua, inline.source_info())?));
        }
        // Try to extract source info from Block element
        if let Ok(lua_block) = ud.borrow::<LuaBlock>() {
            let block = lua_block.borrow_block();
            return Ok(Some(source_info_to_lua_table(lua, block.source_info())?));
        }
    }
    // Not a recognized element type
    Ok(None)
}

/// Get SourceInfo for the Lua caller location (for stack-based fallback)
fn get_caller_source_info(lua: &Lua) -> SourceInfo {
    let (source, line) = get_caller_location(lua);
    let source_path = source.strip_prefix('@').unwrap_or(&source);
    SourceInfo::Generated {
        by: By::filter(source_path, line.max(0) as usize),
        from: SmallVec::new(),
    }
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

        if let Some(SourceInfo::Generated { by, .. }) = &diagnostics[0].location
            && let Some((filter_path, line)) = by.as_filter()
        {
            // The path should contain the filter name (@ prefix is stripped)
            assert!(
                filter_path.contains("test_filter.lua"),
                "Expected path to contain 'test_filter.lua', got '{}'",
                filter_path
            );
            assert_eq!(line, 1);
        } else {
            panic!("Expected filter-kind Generated source info");
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
        let lua_inline = LuaInline::new(str_inline);
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
            Some(SourceInfo::Generated { by, .. }) if by.is_kind("filter") => {
                let (filter_path, line) = by.as_filter().unwrap();
                panic!(
                    "Expected SourceInfo::Original, but got filter-Generated({}, {}). \
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

        let lua_inline = LuaInline::new(str_inline);
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

        let lua_inline = LuaInline::new(str_inline);
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

        let lua_block = LuaBlock::new(para_block);
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

        // Test filter-kind Generated round-trip
        let filter_prov = SourceInfo::generated(By::filter("/path/to/filter.lua", 42));
        let table = source_info_to_lua_table(&lua, &filter_prov).unwrap();
        let roundtrip = source_info_from_lua_table(&table).unwrap();
        assert_eq!(filter_prov, roundtrip);

        // Test shortcode Generated with an Invocation anchor
        let mut shortcode = SourceInfo::generated(By::shortcode("meta"));
        shortcode.append_anchor(
            AnchorRole::Invocation,
            Arc::new(SourceInfo::Original {
                file_id: FileId(3),
                start_offset: 1,
                end_offset: 9,
            }),
        );
        let table = source_info_to_lua_table(&lua, &shortcode).unwrap();
        let roundtrip = source_info_from_lua_table(&table).unwrap();
        assert_eq!(shortcode, roundtrip);
    }

    #[test]
    fn test_legacy_filter_provenance_tag_reads_as_filter_generated() {
        // Plan 4 Phase 4: writers never emit "FilterProvenance" anymore, but
        // the reader still accepts the legacy tag and maps it to a
        // filter-kind Generated with empty anchor list.
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("t", "FilterProvenance").unwrap();
        table.set("filter_path", "legacy.lua").unwrap();
        table.set("line", 7usize).unwrap();
        let parsed = source_info_from_lua_table(&table).unwrap();
        match parsed {
            SourceInfo::Generated { by, from } => {
                assert_eq!(by.as_filter(), Some(("legacy.lua", 7)));
                assert!(from.is_empty());
            }
            other => panic!("Expected filter-kind Generated, got {:?}", other),
        }
    }

    // =========================================================================
    // Tests for Inline::source_info() and Block::source_info() moved to
    // quarto-pandoc-types. The duplicate free functions they tested have been
    // replaced by the enum methods.
    // =========================================================================

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
        // Should have filter-Generated since the element wasn't recognized
        match &diagnostics[0].location {
            Some(SourceInfo::Generated { by, .. }) if by.is_kind("filter") => {}
            other => panic!(
                "Expected filter-Generated for non-userdata element, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn test_quarto_warn_with_shortcode_element_uses_source_info() {
        use crate::pandoc::Inline;
        use quarto_pandoc_types::shortcode::Shortcode;

        let lua = Lua::new();
        register_quarto_namespace(&lua).unwrap();

        // Shortcode has source_info — Inline::source_info() returns it directly
        let shortcode = Inline::Shortcode(Shortcode {
            is_escaped: false,
            name: "test".to_string(),
            positional_args: vec![],
            keyword_args: hashlink::LinkedHashMap::new(),
            source_info: quarto_source_map::SourceInfo::original(
                quarto_source_map::FileId(0),
                10,
                20,
            ),
        });
        let lua_inline = LuaInline::new(shortcode);
        lua.globals()
            .set("test_shortcode", lua.create_userdata(lua_inline).unwrap())
            .unwrap();

        lua.load(r#"quarto.warn("Warning about shortcode", test_shortcode)"#)
            .set_name("@shortcode_filter.lua")
            .exec()
            .unwrap();

        let diagnostics = extract_lua_diagnostics(&lua).unwrap();
        assert_eq!(diagnostics.len(), 1);

        // Shortcode's own source_info is now used (not fallback to stack)
        match &diagnostics[0].location {
            Some(SourceInfo::Original { file_id, .. }) => {
                assert_eq!(*file_id, quarto_source_map::FileId(0));
            }
            other => panic!(
                "Expected Original source_info for Shortcode element, got {:?}",
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
        let lua_inline = LuaInline::new(emph);
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
        let lua_block = LuaBlock::new(codeblock);
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
