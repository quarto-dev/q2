# Lua Filter Diagnostics Implementation Plan

**Date:** 2025-12-03
**Issue:** k-480 (Implement quarto.warn() and quarto.error() Lua functions)
**Status:** ✅ COMPLETED (with one known issue tracked in k-481)

**Related:**
- [Filter Diagnostics Analysis](./2025-12-02-filter-diagnostics-analysis.md)
- [FilterContext Refactoring](./2025-12-02-filter-context-refactoring.md)
- [Lua Filters Design](./2025-11-26-lua-filters-design.md)

## Summary

Implement `quarto.warn()` and `quarto.error()` Lua functions that allow filter authors to emit diagnostic messages. These diagnostics are collected during filter execution and returned to the Rust side, integrating with our existing `FilterContext` and `DiagnosticCollector` infrastructure.

## Implementation Status

### ✅ Phase 1: Core Lua Functions - COMPLETED

| Task | Status | Notes |
|------|--------|-------|
| Create `src/lua/diagnostics.rs` | ✅ Done | Contains `register_quarto_namespace()`, `add_diagnostic()`, `get_caller_location()`, `extract_lua_diagnostics()` |
| Update `src/lua/mod.rs` | ✅ Done | Added `mod diagnostics;` |
| Update `src/lua/constructors.rs` | ✅ Done | Calls `register_quarto_namespace()` in `register_pandoc_namespace()` |

### ✅ Phase 2: Integration with Filter Pipeline - COMPLETED

| Task | Status | Notes |
|------|--------|-------|
| Update `src/lua/filter.rs` return types | ✅ Done | Returns `(Pandoc, ASTContext, Vec<DiagnosticMessage>)` |
| Update `apply_lua_filters()` | ✅ Done | Accumulates diagnostics across multiple filters |
| Update `src/main.rs` | ✅ Done | Outputs diagnostics via text or JSON format |
| Update existing tests | ✅ Done | Updated 16 test call sites + `test_lua_list.rs` |

### ✅ Phase 3: Testing - COMPLETED

| Task | Status | Notes |
|------|--------|-------|
| Unit tests in `diagnostics.rs` | ✅ Done | 7 tests for basic warn/error, multiple diagnostics, source location |
| Integration tests in `filter.rs` | ✅ Done | 4 tests: `test_quarto_warn_in_filter`, `test_quarto_error_in_filter`, `test_multiple_diagnostics_from_filter`, `test_diagnostics_accumulated_across_filters` |
| All 524 tests pass | ✅ Done | Verified |

### ✅ Phase 4: LuaLS Documentation - COMPLETED

| File | Status | Description |
|------|--------|-------------|
| `resources/lua-types/README.md` | ✅ Done | Usage instructions for VS Code, Neovim, project config |
| `resources/lua-types/pandoc/pandoc.lua` | ✅ Done | Main module declaration |
| `resources/lua-types/pandoc/global.lua` | ✅ Done | FORMAT, PANDOC_VERSION, PANDOC_API_VERSION, PANDOC_SCRIPT_FILE |
| `resources/lua-types/pandoc/inlines.lua` | ✅ Done | All 17 inline types (Str, Emph, Strong, Link, Image, Span, etc.) |
| `resources/lua-types/pandoc/blocks.lua` | ✅ Done | All 10 block types (Para, Header, Div, CodeBlock, BulletList, etc.) |
| `resources/lua-types/pandoc/components.lua` | ✅ Done | Attr, Inlines, Blocks types |
| `resources/lua-types/pandoc/List.lua` | ✅ Done | All List methods (at, clone, extend, filter, find, map, walk, etc.) |
| `resources/lua-types/pandoc/utils.lua` | ✅ Done | pandoc.utils.stringify() |
| `resources/lua-types/quarto/quarto.lua` | ✅ Done | Main module declaration |
| `resources/lua-types/quarto/diagnostics.lua` | ✅ Done | quarto.warn(), quarto.error() with full documentation |

## Known Issues

### k-481: Element location doesn't work for original document elements

**Problem:** The `quarto.warn(msg, elem)` and `quarto.error(msg, elem)` functions accept an optional AST element to attach source location. However, this only works for elements **created by filters** (which have `FilterProvenance` source info), not elements **from the original document** (which have `SourceInfo::Original`).

**Impact:** User-provided filters that want to act as linters cannot report warnings/errors about specific locations in the source document.

**Root Cause:** `source_info_to_path_line()` in `diagnostics.rs` only handles `FilterProvenance`. For `Original`, we need access to `SourceContext` to map file IDs and byte offsets to file paths and line numbers.

**Tracked in:** Issue k-481

## Lua API

```lua
-- Warning with automatic source location (from Lua call stack)
quarto.warn("This is a warning message")

-- Warning associated with a specific AST element (partially working - see k-481)
quarto.warn("Invalid URL scheme", elem)

-- Error with automatic source location
quarto.error("Fatal problem in filter")

-- Error associated with a specific AST element (partially working - see k-481)
quarto.error("Required field missing", elem)
```

## Files Created/Modified

### Implementation Files

| File | Action | Description |
|------|--------|-------------|
| `src/lua/diagnostics.rs` | Created | Core diagnostic functions |
| `src/lua/mod.rs` | Modified | Added `mod diagnostics;` |
| `src/lua/constructors.rs` | Modified | Register quarto namespace |
| `src/lua/filter.rs` | Modified | Updated return types, added integration tests |
| `src/main.rs` | Modified | Handle diagnostics output |
| `tests/test_lua_list.rs` | Modified | Updated helper for new return type |

### Documentation Files

| File | Action | Description |
|------|--------|-------------|
| `resources/lua-types/README.md` | Created | IDE configuration instructions |
| `resources/lua-types/pandoc/*.lua` | Created | 7 files for pandoc namespace |
| `resources/lua-types/quarto/*.lua` | Created | 2 files for quarto namespace |

## Success Criteria - Final Status

### Implementation
1. ✅ `quarto.warn("message")` adds a warning to the diagnostics list
2. ✅ `quarto.error("message")` adds an error to the diagnostics list
3. ✅ Source location correctly points to the filter file and line (for stack-based location)
4. ⚠️ Element-based source location works for filter-created elements only (k-481)
5. ✅ Diagnostics are returned from `apply_lua_filter()` and `apply_lua_filters()`
6. ✅ Multiple filters accumulate diagnostics correctly
7. ✅ All tests pass (524 tests)
8. ✅ Existing Lua filter tests continue to pass

### Documentation
9. ✅ LuaLS type annotation files exist in `resources/lua-types/`
10. ✅ README.md provides clear instructions for VS Code and Neovim configuration
11. ✅ Type annotations accurately reflect the implemented API
12. ✅ Documentation notes k-481 limitation for element parameter

## Future Enhancements (Not in Scope)

- Fix element location for original document elements (k-481)
- `quarto.info()` and `quarto.note()` for other diagnostic levels
- Structured diagnostic codes for categorization
- Rich diagnostic messages with problem/details/notes
- Integration with error rendering pipeline
- Documentation for `pandoc.walk()`, `pandoc.read()`, `pandoc.write()` (not yet implemented)
- Documentation for `pandoc.path`, `pandoc.system`, `pandoc.json` modules (not yet implemented)
