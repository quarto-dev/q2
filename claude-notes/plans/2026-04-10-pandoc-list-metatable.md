# Plan: Add pandoc.List metatable to all list-like tables in Lua API

**Beads issue**: `bd-y9zl`
**Date**: 2026-04-10

## Overview

In Pandoc's Lua API, every sequence-like value returned from the AST carries the
`pandoc.List` metatable, giving it methods like `:includes()`, `:map()`,
`:filter()`, `:clone()`, etc. In Q2, the core `pandoc.List` metatable and all its
methods are already implemented (`crates/pampa/src/lua/list.rs`), but many tables
returned from element field access are created as plain Lua tables without this
metatable. This breaks compatibility with Quarto 1 filters.

**Example of the bug:**
```lua
function Div(div)
  print(div.classes.includes)  -- prints nil (should print a function)
end
```

## Root Cause

In `crates/pampa/src/lua/types.rs`, sequences are built via `lua.create_table()`
followed by element insertion, but `set_metatable()` is never called on the
resulting table. The `inlines_to_lua_table()` and `blocks_to_lua_table()` helpers
in `list.rs` DO set metatables correctly — but many other list-like returns bypass
these helpers.

## Affected Locations

### 1. `classes` fields (all return `List` of strings)

In `LuaInline::get_field()`:
- Line 122: `Code.classes`
- Line 150: `Link.classes`
- Line 164: `Image.classes`
- Line 178: `Span.classes`
- Line 194: `Insert.classes`
- Line 206: `Delete.classes`
- Line 218: `Highlight.classes`
- Line 230: `EditComment.classes`

In `LuaBlock::get_field()`:
- Line 706: `Header.classes`
- Line 718: `CodeBlock.classes`
- Line 737: `Div.classes`

In `LuaAttr` `__index`:
- Line 1643: `attr.classes`

### 2. Container content fields (outer table is `List`)

In `LuaBlock::get_field()`:
- Line 746: `BulletList.content` — List of Blocks lists
- Line 755: `OrderedList.content` — List of Blocks lists
- Line 782: `LineBlock.content` — List of Inlines lists
- Line 791: `DefinitionList.content` — List of (term, defs) pairs
- Line 798: `DefinitionList` inner defs table — List of Blocks lists

### 3. `LuaInline::set_field()` classes (returns on round-trip)

In `LuaInline::set_field()`:
- Lines 383, 397, 411, 425, 453, 471, 489, 507: various `*.classes` setters
  read classes back from tables — these don't need metatable changes since they
  consume the table, but we should verify the getter path is consistent.

## Work Items

### Phase 1: Helper function and classes fix (TDD)

- [x] Write test: `div.classes:includes("foo")` returns true/false correctly
- [x] Write test: `div.classes:map(function(c) return c:upper() end)` works
- [x] Write test: `attr.classes:includes(...)` works
- [x] Create helper `create_string_list_table(lua, strings)` in `list.rs` that
      creates a table with the List metatable applied
- [x] Create helper `create_list_table(lua, values)` in `list.rs` for
      wrapping `Vec<Value>` in a List table
- [x] Replace all `classes` field construction in `LuaInline::get_field()` with
      the helper (8 locations)
- [x] Replace `classes` field construction in `LuaBlock::get_field()` (3 locations)
- [x] Replace `classes` field construction in `LuaAttr` `__index` (1 location)
- [x] Run failing tests, verify they now pass

### Phase 2: Container content fields (TDD)

- [x] Write test: `bullet_list.content:map(...)` works (outer table is a List)
- [x] Write test: `ordered_list.content:clone(...)` works
- [x] Write test: `line_block.content:map(...)` works
- [x] Write test: `definition_list.content:map(...)` works
- [x] Add List metatable to `BulletList.content` outer table
- [x] Add List metatable to `OrderedList.content` outer table
- [x] Add List metatable to `LineBlock.content` outer table
- [x] Add List metatable to `DefinitionList.content` outer table
- [x] Add List metatable to `DefinitionList` inner defs table
- [x] Run failing tests, verify they now pass

### Phase 3: Full test suite and verification

- [x] Run `cargo nextest run --workspace` — verify no regressions
      (7235 passed, 4 pre-existing pandoc-version failures, 0 regressions)
- [x] Verify WASM build: `cargo xtask verify --skip-rust-tests`
      (all verification steps passed)

## Implementation Notes

### Helper function design

```rust
/// Create a List table from a slice of strings
pub fn create_string_list_table(lua: &Lua, items: &[String]) -> Result<Value> {
    let table = lua.create_table()?;
    for (i, item) in items.iter().enumerate() {
        table.set(i + 1, item.clone())?;
    }
    let mt = get_or_create_list_metatable(lua)?;
    table.set_metatable(Some(mt))?;
    Ok(Value::Table(table))
}
```

For container content fields, we just need to add two lines after creating the
outer table:
```rust
let mt = get_or_create_list_metatable(lua)?;
table.set_metatable(Some(mt))?;
```

### WASM considerations

The List metatable is created identically on native and WASM (it's pure
mlua/Lua, no platform-specific code). The `get_or_create_list_metatable()`
function caches in the Lua registry, so there's no performance concern. No
special WASM handling is needed.

### What NOT to change

- `inlines_to_lua_table()` and `blocks_to_lua_table()` already set the
  Inlines/Blocks metatables correctly — don't touch these.
- The `attributes` field (key-value table) is NOT a List — don't add a
  metatable to it.
- Tables used for structural purposes (e.g., the pair table inside
  DefinitionList items) are NOT Lists — only the outer sequence tables are.
