# Filter Diagnostics Infrastructure Analysis

**Date:** 2025-12-02
**Related:** [Lua Filters Design](./2025-11-26-lua-filters-design.md)
**Issue:** k-409

## Summary

This document analyzes the current filter infrastructure in `quarto-markdown-pandoc` to understand what changes are needed to support diagnostic messages (warnings and errors) from Lua filters.

**Conclusion:** The current filter infrastructure was designed for pure transformations without diagnostic capabilities. Supporting Lua filter diagnostics requires redesigning the filter execution model to include a context object that accumulates diagnostics.

---

## Current State Analysis

### 1. Internal Filters (`src/filters.rs`)

The internal filter system provides a builder pattern for AST transformations:

```rust
pub struct Filter<'a> {
    pub str: Option<Box<dyn FnMut(Str) -> FilterReturn<Str, Inlines> + 'a>>,
    pub paragraph: Option<Box<dyn FnMut(Paragraph) -> FilterReturn<Paragraph, Blocks> + 'a>>,
    // ... one field per AST node type
}

pub enum FilterReturn<T, U> {
    Unchanged(T),
    FilterResult(U, bool), // (new content, should recurse)
}
```

**Characteristics:**
- Pure transformations only
- No diagnostic support
- No context threading through traversal
- `topdown_traverse()` returns just `Pandoc`

**Relevant files:**
- `crates/quarto-markdown-pandoc/src/filters.rs` (1140 lines)

### 2. JSON Filters (`src/json_filter.rs`)

External process filters that communicate via JSON:

```rust
pub fn apply_json_filter(
    pandoc: &Pandoc,
    context: &ASTContext,
    filter_path: &Path,
    target_format: &str,
) -> Result<(Pandoc, ASTContext), JsonFilterError>
```

**Characteristics:**
- Spawns subprocess, JSON on stdin/stdout
- Binary success/failure - no warnings
- Stderr passthrough via `Stdio::inherit()` (unstructured)
- No way for filters to emit structured diagnostics

**Relevant files:**
- `crates/quarto-markdown-pandoc/src/json_filter.rs`

### 3. Reader Pattern (`src/readers/qmd.rs`)

Readers demonstrate the pattern we need for filters:

```rust
pub fn read<T: Write>(
    input_bytes: &[u8],
    // ... other params
) -> Result<
    (Pandoc, ASTContext, Vec<DiagnosticMessage>),  // Success + warnings
    Vec<DiagnosticMessage>,                         // Failure
>
```

**Characteristics:**
- `DiagnosticCollector` passed as `&mut` to parsing functions
- Success case includes accumulated warnings
- Error case returns diagnostics
- Location-aware: `error_at()` and `warn_at()` methods

**Relevant files:**
- `crates/quarto-markdown-pandoc/src/readers/qmd.rs`
- `crates/quarto-markdown-pandoc/src/utils/diagnostic_collector.rs`

### 4. DiagnosticCollector (`src/utils/diagnostic_collector.rs`)

Simple accumulator for diagnostic messages:

```rust
pub struct DiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DiagnosticCollector {
    pub fn add(&mut self, diagnostic: DiagnosticMessage);
    pub fn error(&mut self, message: impl Into<String>);
    pub fn warn(&mut self, message: impl Into<String>);
    pub fn error_at(&mut self, message: impl Into<String>, location: SourceInfo);
    pub fn warn_at(&mut self, message: impl Into<String>, location: SourceInfo);
    pub fn has_errors(&self) -> bool;
    pub fn into_diagnostics(self) -> Vec<DiagnosticMessage>;
}
```

### 5. DiagnosticMessage (`crates/quarto-error-reporting/src/diagnostic.rs`)

```rust
pub enum DiagnosticKind {
    Error,
    Warning,
    Info,
    Note,
}

pub struct DiagnosticMessage {
    pub code: Option<String>,
    pub title: String,
    pub kind: DiagnosticKind,
    pub location: Option<SourceInfo>,
    // ... additional fields for problem, details, notes
}
```

---

## Gap Analysis

| Feature | Readers | Internal Filters | JSON Filters | Lua Filters (needed) |
|---------|---------|-----------------|--------------|---------------------|
| Emit warnings | ✅ `DiagnosticCollector` | ❌ | ❌ | ✅ |
| Emit errors | ✅ | ❌ | ✅ (fail only) | ✅ |
| Source location on diagnostics | ✅ `error_at()` | ❌ | ❌ | ✅ |
| Context threading | ✅ `&mut` param | ❌ | ❌ | ✅ |
| Return diagnostics with result | ✅ in tuple | ❌ | ❌ | ✅ |
| Node-associated diagnostics | ✅ | ❌ | ❌ | ✅ |
| Code-location diagnostics | ✅ | ❌ | ❌ | ✅ |

---

## Requirements for Lua Filter Diagnostics

### Two Types of Diagnostics

1. **Node-associated diagnostics**: Warning about a specific AST node
   - Use the node's `SourceInfo` for location
   - Example: "Invalid link target in this Link element"

2. **Code-location diagnostics**: Warning from filter code itself
   - Use `debug.getinfo()` to capture filter file + line
   - Example: "Deprecated API usage" at `filters/myfilter.lua:42`

### Lua-Side API

```lua
-- Node-associated warning
function Link(elem)
    if not valid_url(elem.target) then
        quarto.warn("Invalid URL scheme", elem)  -- elem provides SourceInfo
    end
    return elem
end

-- Code-location warning (captures caller via debug.getinfo)
function Pandoc(doc)
    quarto.warn("This filter uses deprecated API")
    return doc
end

-- Errors
function Str(elem)
    if problem then
        quarto.error("Fatal problem", elem)  -- Stops filter execution
    end
end
```

### Rust-Side Collection

After filter execution:
1. Extract diagnostics from Lua global table
2. Convert Lua provenance (`{source, line}`) to `SourceInfo`
3. Convert node `SourceInfo` references to proper locations
4. Merge into `DiagnosticCollector`

---

## Proposed Solutions

### Option A: FilterContext (Recommended)

Create a unified context object that threads through all filter types:

```rust
pub struct FilterContext {
    /// Source file registry (for provenance tracking)
    pub source_context: SourceContext,

    /// Accumulated diagnostics (warnings and non-fatal errors)
    pub diagnostics: DiagnosticCollector,

    /// Cache for file path -> FileId mapping
    pub file_cache: HashMap<String, FileId>,

    /// Target format (html, pdf, etc.)
    pub target_format: String,
}

impl FilterContext {
    /// Add a warning with source location
    pub fn warn_at(&mut self, message: impl Into<String>, location: SourceInfo) {
        self.diagnostics.warn_at(message, location);
    }

    /// Add a warning from filter code (captures caller location)
    pub fn warn_from_filter(&mut self, message: impl Into<String>, source: &str, line: usize) {
        let source_info = self.resolve_filter_location(source, line);
        self.diagnostics.warn_at(message, source_info);
    }

    /// Resolve Lua source string to SourceInfo
    fn resolve_filter_location(&mut self, source: &str, line: usize) -> SourceInfo {
        // Parse "@path/to/file.lua" format
        // Lazily register file in source_context
        // Convert line to byte offset
        // Return SourceInfo::Original
    }
}
```

**Filter execution signature:**

```rust
pub fn apply_lua_filter(
    pandoc: Pandoc,
    context: &mut FilterContext,
    filter_path: &Path,
) -> Result<Pandoc, Vec<DiagnosticMessage>>
// Warnings accumulated in context.diagnostics
// Errors returned as Err variant
```

**Benefits:**
- Unified pattern for all filter types
- Matches reader pattern
- Extensible for future needs

### Option B: Lua-Specific Solution

Keep filter infrastructure unchanged, handle Lua diagnostics separately:

```rust
pub fn apply_lua_filter(
    pandoc: Pandoc,
    ast_context: &ASTContext,
    filter_path: &Path,
) -> Result<(Pandoc, Vec<DiagnosticMessage>), Vec<DiagnosticMessage>>
// Returns (result, warnings) on success
// Returns diagnostics on failure
```

**Benefits:**
- Minimal changes to existing code
- Can be done incrementally

**Drawbacks:**
- Inconsistent with internal filters
- Harder to chain filter types

### Option C: Extend FilterReturn

Add diagnostic capability to the existing filter system:

```rust
pub enum FilterReturn<T, U> {
    Unchanged(T),
    FilterResult(U, bool),
    WithDiagnostics(U, bool, Vec<DiagnosticMessage>),  // New variant
}
```

**Benefits:**
- Backward compatible
- Works for internal filters too

**Drawbacks:**
- Awkward API
- Doesn't solve context threading

---

## Recommendation

**Implement Option A (FilterContext)** because:

1. **Matches established pattern**: Readers use `ASTContext` + `DiagnosticCollector`
2. **Unified model**: Works for internal, JSON, and Lua filters
3. **Extensible**: Easy to add more context (metadata, options, etc.)
4. **Clean separation**: Diagnostics accumulate without interrupting transformation

### Migration Path

1. Create `FilterContext` struct
2. Update `topdown_traverse()` to accept `&mut FilterContext`
3. Implement Lua filter with `FilterContext`
4. Optionally update JSON filters to parse diagnostic sideband
5. Optionally update internal filter callbacks to emit diagnostics

### Impact on Existing Code

- `filters.rs`: Add `FilterContext` parameter to traversal functions
- `json_filter.rs`: Update return type, optionally parse diagnostics from filter
- Callers of `topdown_traverse`: Create and pass `FilterContext`

---

## Files to Modify

| File | Changes Needed |
|------|----------------|
| `src/filters.rs` | Add `FilterContext` parameter to traversal functions |
| `src/json_filter.rs` | Update return type to include diagnostics |
| `src/readers/qmd.rs` | Update filter calls to use `FilterContext` |
| `src/utils/mod.rs` | Export `FilterContext` |
| New: `src/filter_context.rs` | Define `FilterContext` struct |
| New: `src/lua/` | Lua filter implementation |

---

## Related Design Decisions

See [Lua Filters Design](./2025-11-26-lua-filters-design.md) for:
- Section 2a: Filter Provenance Tracking (how to capture filter source locations)
- Section 3: Error Handling (Lua errors → FilterError)
