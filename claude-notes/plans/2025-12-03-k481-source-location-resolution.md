# Plan: Source Location Resolution for quarto.warn/error (k-481)

**Date:** 2025-12-03
**Issue:** k-481 (quarto.warn/error element location doesn't work for original document elements)
**Status:** IMPLEMENTED

## Problem Summary

The `quarto.warn(msg, elem)` and `quarto.error(msg, elem)` functions can accept an AST element to attach source location. However, this only works for elements created by filters (`FilterProvenance` source info), not elements from the original document (`SourceInfo::Original`, `Substring`, or `Concat`).

**Root cause**: The current implementation tries to resolve `SourceInfo` to `(path, line)` immediately in Lua, but `SourceContext` (needed for resolution) isn't available there.

## Key Insight: Don't Resolve in Lua

**We don't need to resolve anything in Lua.** The resolution should happen entirely in Rust.

The correct approach:
1. In Lua: Extract `SourceInfo` from the element and store it (unresolved) in the diagnostics table
2. In Rust: `extract_lua_diagnostics()` retrieves the `SourceInfo` and passes it directly to `DiagnosticMessageBuilder::with_location()`
3. Resolution happens later when the diagnostic is rendered (Rust side has full access to `SourceContext`)

## Current Architecture (Problem)

```
                      quarto.warn(msg, elem)
                              │
                              ▼
    ┌─────────────────────────────────────────────────────────┐
    │  add_diagnostic(lua, quarto, "warning", args)           │
    │                              │                          │
    │                              ▼                          │
    │  get_source_from_element_or_stack(lua, &elem)           │
    │       │                                                 │
    │       └──► TRIES TO RESOLVE to (path, line) ← PROBLEM   │
    │                    │                                    │
    │                    ▼                                    │
    │  Stores: { kind, message, source: String, line: i64 }   │
    │                                                         │
    │  FilterProvenance → works (path/line available)         │
    │  Original/Substring/Concat → fails (needs SourceContext)│
    └─────────────────────────────────────────────────────────┘
```

## Proposed Architecture (Solution)

```
                      quarto.warn(msg, elem)
                              │
                              ▼
    ┌─────────────────────────────────────────────────────────┐
    │  add_diagnostic(lua, quarto, "warning", args)           │
    │       │                                                 │
    │       └──► Extract SourceInfo from elem (no resolution) │
    │                    │                                    │
    │                    ▼                                    │
    │  Stores: { kind, message, source_info: LuaSourceInfo }  │
    └─────────────────────────────────────────────────────────┘
                              │
                              ▼
    ┌─────────────────────────────────────────────────────────┐
    │  extract_lua_diagnostics(lua) → Vec<DiagnosticMessage>  │
    │       │                                                 │
    │       └──► Retrieve SourceInfo from Lua                 │
    │       │                                                 │
    │       └──► DiagnosticMessageBuilder::with_location(     │
    │                source_info  ← UNRESOLVED, passed as-is  │
    │            )                                            │
    └─────────────────────────────────────────────────────────┘
                              │
                              ▼
           Resolution happens in error rendering (Rust)
           where SourceContext is available
```

## Implementation Approach

### Core Idea: Store SourceInfo as a Lua Table

Instead of resolving to `(path, line)` in Lua, we:
1. Serialize `SourceInfo` to a plain Lua table
2. Store it in the diagnostics table
3. Deserialize it back to `SourceInfo` in `extract_lua_diagnostics()`
4. Pass to `DiagnosticMessageBuilder::with_location()`

**Why not userdata?** In Pandoc, userdata types are almost exclusively AST nodes. Some filters may check `type(x) == "userdata"` to identify AST elements. Using userdata for non-AST data could cause compatibility issues.

**Why not thread-local IDs?** If someone serializes diagnostic entries inadvertently, thread-local IDs would become meaningless or cause bugs.

### SourceInfo Structure (for reference)

From `quarto-source-map/src/source_info.rs:22-55`:
```rust
pub enum SourceInfo {
    Original {
        file_id: FileId,        // FileId(usize)
        start_offset: usize,
        end_offset: usize,
    },
    Substring {
        parent: Rc<SourceInfo>,
        start_offset: usize,
        end_offset: usize,
    },
    Concat {
        pieces: Vec<SourcePiece>  // SourcePiece { source_info, offset_in_concat, length }
    },
    FilterProvenance {
        filter_path: String,
        line: usize,
    },
}
```

### Lua Table Representation

```lua
-- Original
{ t = "Original", file_id = 0, start_offset = 42, end_offset = 55 }

-- Substring (nested parent)
{ t = "Substring",
  parent = { t = "Original", file_id = 0, start_offset = 0, end_offset = 100 },
  start_offset = 10,
  end_offset = 20
}

-- Concat
{ t = "Concat",
  pieces = {
    { source_info = { t = "Original", ... }, offset_in_concat = 0, length = 10 },
    { source_info = { t = "Original", ... }, offset_in_concat = 10, length = 15 },
  }
}

-- FilterProvenance
{ t = "FilterProvenance", filter_path = "/path/to/filter.lua", line = 42 }
```

### New Functions: SourceInfo ↔ Lua Table

```rust
/// Serialize SourceInfo to a Lua table
fn source_info_to_lua_table(lua: &Lua, si: &SourceInfo) -> Result<Table> {
    let table = lua.create_table()?;
    match si {
        SourceInfo::Original { file_id, start_offset, end_offset } => {
            table.set("t", "Original")?;
            table.set("file_id", file_id.0)?;
            table.set("start_offset", *start_offset)?;
            table.set("end_offset", *end_offset)?;
        }
        SourceInfo::Substring { parent, start_offset, end_offset } => {
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
                piece_table.set("source_info", source_info_to_lua_table(lua, &piece.source_info)?)?;
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

/// Deserialize SourceInfo from a Lua table
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
                parent: Rc::new(source_info_from_lua_table(&parent_table)?),
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
```

### Modified add_diagnostic()

Current location: `diagnostics.rs:50-89`

```rust
fn add_diagnostic(lua: &Lua, quarto: &Table, kind: &str, args: MultiValue) -> Result<()> {
    let diagnostics: Table = quarto.get("_diagnostics")?;
    let mut iter = args.into_iter();

    // First argument: message (required)
    let message = match iter.next() {
        Some(Value::String(s)) => s.to_str()?.to_string(),
        // ... error handling (unchanged)
    };

    // Second argument: optional AST element
    // Extract SourceInfo and serialize to Lua table (don't resolve!)
    let source_info_table: Option<Table> = if let Some(elem) = iter.next() {
        extract_source_info_from_element(lua, &elem)?
    } else {
        // Fall back to Lua stack location (create FilterProvenance)
        let si = get_caller_source_info(lua);
        Some(source_info_to_lua_table(lua, &si)?)
    };

    // Create diagnostic entry
    let entry = lua.create_table()?;
    entry.set("kind", kind)?;
    entry.set("message", message)?;
    if let Some(si_table) = source_info_table {
        entry.set("source_info", si_table)?;
    }

    diagnostics.set(diagnostics.raw_len() + 1, entry)?;
    Ok(())
}
```

### Modified extract_lua_diagnostics()

Current location: `diagnostics.rs:225-261`

```rust
pub fn extract_lua_diagnostics(lua: &Lua) -> Result<Vec<DiagnosticMessage>> {
    let quarto: Table = lua.globals().get("quarto")?;
    let diagnostics: Table = quarto.get("_diagnostics")?;

    let mut result = Vec::new();
    for i in 1..=diagnostics.raw_len() {
        let entry: Table = diagnostics.get(i)?;
        let kind: String = entry.get("kind")?;
        let message: String = entry.get("message")?;

        // Get SourceInfo from Lua table (deserialize)
        let source_info: Option<SourceInfo> = entry
            .get::<Option<Table>>("source_info")?
            .map(|t| source_info_from_lua_table(&t))
            .transpose()?;

        let diag = if kind == "error" {
            let mut builder = DiagnosticMessageBuilder::error(&message)
                .with_code("Q-0-99");
            if let Some(si) = source_info {
                builder = builder.with_location(si);
            }
            builder.build()
        } else {
            let mut builder = DiagnosticMessageBuilder::warning(&message)
                .with_code("Q-0-99");
            if let Some(si) = source_info {
                builder = builder.with_location(si);
            }
            builder.build()
        };

        result.push(diag);
    }
    Ok(result)
}
```

### Helper: Extract SourceInfo from Element

Replace current `get_source_info_from_inline` (line 141) and `get_source_info_from_block` (line 181):

```rust
/// Extract SourceInfo from an AST element and convert to Lua table
fn extract_source_info_from_element(lua: &Lua, elem: &Value) -> Result<Option<Table>> {
    if let Value::UserData(ud) = elem {
        if let Ok(lua_inline) = ud.borrow::<LuaInline>() {
            let si = get_inline_source_info(&lua_inline.0);
            return Ok(Some(source_info_to_lua_table(lua, &si)?));
        }
        if let Ok(lua_block) = ud.borrow::<LuaBlock>() {
            let si = get_block_source_info(&lua_block.0);
            return Ok(Some(source_info_to_lua_table(lua, &si)?));
        }
    }
    Ok(None)
}

/// Get SourceInfo from an Inline element (returns SourceInfo, not Option<(String, i64)>)
fn get_inline_source_info(inline: &Inline) -> SourceInfo {
    use crate::pandoc::Inline;
    match inline {
        Inline::Str(s) => s.source_info.clone(),
        Inline::Emph(e) => e.source_info.clone(),
        // ... all variants (same as current get_source_info_from_inline but return SourceInfo)
    }
}

/// Get SourceInfo from a Block element
fn get_block_source_info(block: &Block) -> SourceInfo {
    use crate::pandoc::Block;
    match block {
        Block::Plain(p) => p.source_info.clone(),
        Block::Paragraph(p) => p.source_info.clone(),
        // ... all variants
    }
}

/// Get SourceInfo for the Lua caller location (for stack-based fallback)
fn get_caller_source_info(lua: &Lua) -> SourceInfo {
    // Reuse existing get_caller_location logic but return SourceInfo::FilterProvenance
    let (source, line) = get_caller_location(lua);
    let source_path = source.strip_prefix('@').unwrap_or(&source);
    SourceInfo::filter_provenance(source_path, line.max(0) as usize)
}
```

## Implementation Steps

### Phase 1: Add SourceInfo ↔ Lua Table Conversion Functions

In `diagnostics.rs`, add:
1. `source_info_to_lua_table(lua: &Lua, si: &SourceInfo) -> Result<Table>`
2. `source_info_from_lua_table(table: &Table) -> Result<SourceInfo>`

Need to add imports:
```rust
use quarto_source_map::{SourceInfo, SourcePiece};
use crate::pandoc::types::FileId;  // or from quarto_source_map
use std::rc::Rc;
```

### Phase 2: Add Helper Functions

1. `get_inline_source_info(inline: &Inline) -> SourceInfo` - extract SourceInfo from Inline
2. `get_block_source_info(block: &Block) -> SourceInfo` - extract SourceInfo from Block
3. `extract_source_info_from_element(lua: &Lua, elem: &Value) -> Result<Option<Table>>` - try to get SourceInfo from userdata, serialize to table
4. `get_caller_source_info(lua: &Lua) -> SourceInfo` - get FilterProvenance from Lua stack

### Phase 3: Update add_diagnostic()

Current: `diagnostics.rs:50-89`

1. Replace call to `get_source_from_element_or_stack()` with `extract_source_info_from_element()`
2. Store `source_info` (Lua table) instead of `source` (String) and `line` (i64)
3. Fall back to `get_caller_source_info()` when no element provided

### Phase 4: Update extract_lua_diagnostics()

Current: `diagnostics.rs:225-261`

1. Read `source_info` as `Option<Table>` instead of `source`/`line` strings
2. Deserialize with `source_info_from_lua_table()`
3. Pass directly to `DiagnosticMessageBuilder::with_location()`
4. Remove the code that creates `FilterProvenance` from string path/line

### Phase 5: Clean Up

Remove obsolete functions:
1. `get_source_from_element_or_stack()` (line 92)
2. `get_source_info_from_inline()` (line 141) - replace with version returning SourceInfo
3. `get_source_info_from_block()` (line 181) - replace with version returning SourceInfo
4. `source_info_to_path_line()` (line 209)

### Phase 6: Testing

1. Update existing tests to work with new storage format
2. Add test: `quarto.warn` with original document element → verify SourceInfo::Original is preserved and round-trips
3. Add test: `quarto.warn` with filter-created element → verify SourceInfo::FilterProvenance works
4. Add test for Substring and Concat variants
5. Add integration test that renders the diagnostic with SourceContext and verifies correct file/line output

## File Changes

| File | Change |
|------|--------|
| `src/lua/diagnostics.rs` | Major refactor: add serialization functions, change storage format |

## Benefits of This Approach

1. **No SourceContext in Lua**: We don't need to clone or store SourceContext at all
2. **No userdata for non-AST data**: Pure Lua tables, Pandoc-compatible
3. **Serialization-safe**: If diagnostic entries are serialized, they remain valid
4. **Resolution stays in Rust**: DiagnosticMessage rendering handles resolution with full context
5. **All SourceInfo variants work**: Original, Substring, Concat, FilterProvenance all handled uniformly
6. **Self-contained**: SourceInfo carries all info needed for later resolution

## Verification ✓

Confirmed: The error reporting infrastructure correctly handles all `SourceInfo` variants.

In `quarto-error-reporting/src/diagnostic.rs:400`:
```rust
if let Some(mapped) = loc.map_offset(loc.start_offset(), ctx) {
    if let Some(file) = ctx.get_file(mapped.file_id) {
        write!(result, "  at {}:{}:{}\n",
               file.path,
               mapped.location.row + 1,
               mapped.location.column + 1)
```

The `map_offset()` function handles:
- `Original` → direct file lookup
- `Substring` → recursive resolution through parent
- `Concat` → find piece and resolve
- `FilterProvenance` → (not supported by map_offset, but has path/line directly)

For `FilterProvenance`, we may need special handling since it already has path/line and doesn't use `map_offset()`. But that's already working today, so no change needed there.

## Summary

This is a clean, minimal solution:
1. **No SourceContext cloning** - we don't need it in Lua at all
2. **Single new type** - `LuaSourceInfo` wrapper
3. **Pass-through semantics** - SourceInfo goes into Lua, comes back out unchanged
4. **Existing infrastructure** - error rendering already handles resolution

The implementation is straightforward and keeps all resolution logic in Rust.
