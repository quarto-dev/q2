# FilterContext Refactoring Plan

**Date:** 2025-12-02
**Issue:** k-409
**Related:** [Filter Diagnostics Analysis](./2025-12-02-filter-diagnostics-analysis.md)

## Goal

Refactor the internal filter infrastructure in `crates/quarto-markdown-pandoc/src/filters.rs` to accept a `FilterContext` object that can accumulate diagnostics. This is a prerequisite for Lua filter support with diagnostic capabilities.

## Current State

### Filter Infrastructure (`filters.rs`)

```rust
pub fn topdown_traverse(doc: pandoc::Pandoc, filter: &mut Filter) -> pandoc::Pandoc
```

- `Filter` struct has optional callbacks for each element type
- `FilterReturn<T, U>`: `Unchanged(T)` or `FilterResult(U, bool)`
- No context threading - pure transformations only
- Recursive functions: `topdown_traverse_blocks`, `topdown_traverse_inlines`, etc.

### Current Callers

1. **`readers/qmd.rs:281`** - Used in post-parsing to strip NoteDefinition blocks:
   ```rust
   topdown_traverse(result, &mut filter)
   ```

2. **Internal use** - Within `filters.rs` for recursive traversal

### Existing Patterns

The readers already use a context pattern:
```rust
// From readers/qmd.rs
pub fn read<T: Write>(...) -> Result<
    (Pandoc, ASTContext, Vec<DiagnosticMessage>),  // Success + warnings
    Vec<DiagnosticMessage>,                         // Failure
>
```

## Design Decisions

Based on review feedback:

1. **No `target_format` in FilterContext** - A more general execution options infrastructure will be designed later. Keep FilterContext focused on diagnostics only.

2. **No backward compatibility convenience functions** - All callers should use the same consistent API with explicit context passing.

3. **Filter callbacks receive context** - Internal filter callbacks (the closures in `Filter` struct) should accept `&mut FilterContext` so they can emit diagnostics. This is essential for the design.

## Proposed Design

### 1. FilterContext Struct

Create `src/filter_context.rs`:

```rust
use crate::utils::diagnostic_collector::DiagnosticCollector;
use quarto_source_map::SourceInfo;

/// Context for filter execution, enabling diagnostics and source tracking.
///
/// This context is threaded through filter traversal functions to allow
/// filters to emit warnings and errors with proper source locations.
pub struct FilterContext {
    /// Accumulated diagnostics (warnings and non-fatal errors)
    pub diagnostics: DiagnosticCollector,
}

impl FilterContext {
    /// Create a new empty filter context
    pub fn new() -> Self {
        Self {
            diagnostics: DiagnosticCollector::new(),
        }
    }

    /// Add a warning
    pub fn warn(&mut self, message: impl Into<String>) {
        self.diagnostics.warn(message);
    }

    /// Add a warning with source location
    pub fn warn_at(&mut self, message: impl Into<String>, location: SourceInfo) {
        self.diagnostics.warn_at(message, location);
    }

    /// Add an error
    pub fn error(&mut self, message: impl Into<String>) {
        self.diagnostics.error(message);
    }

    /// Add an error with source location
    pub fn error_at(&mut self, message: impl Into<String>, location: SourceInfo) {
        self.diagnostics.error_at(message, location);
    }

    /// Check if any errors were collected
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }

    /// Consume context and return diagnostics
    pub fn into_diagnostics(self) -> Vec<quarto_error_reporting::DiagnosticMessage> {
        self.diagnostics.into_diagnostics()
    }

    /// Get reference to diagnostics
    pub fn diagnostics(&self) -> &[quarto_error_reporting::DiagnosticMessage] {
        self.diagnostics.diagnostics()
    }
}

impl Default for FilterContext {
    fn default() -> Self {
        Self::new()
    }
}
```

### 2. Update Filter Callback Signatures

Filter callbacks must receive `&mut FilterContext` so they can emit diagnostics:

```rust
// Before
type InlineFilterFn<'a, T> = Box<dyn FnMut(T) -> FilterReturn<T, Inlines> + 'a>;
type BlockFilterFn<'a, T> = Box<dyn FnMut(T) -> FilterReturn<T, Blocks> + 'a>;
type MetaFilterFn<'a> = Box<
    dyn FnMut(
            MetaValueWithSourceInfo,
        ) -> FilterReturn<MetaValueWithSourceInfo, MetaValueWithSourceInfo>
        + 'a,
>;

// After
type InlineFilterFn<'a, T> = Box<dyn FnMut(T, &mut FilterContext) -> FilterReturn<T, Inlines> + 'a>;
type BlockFilterFn<'a, T> = Box<dyn FnMut(T, &mut FilterContext) -> FilterReturn<T, Blocks> + 'a>;
type MetaFilterFn<'a> = Box<
    dyn FnMut(
            MetaValueWithSourceInfo,
            &mut FilterContext,
        ) -> FilterReturn<MetaValueWithSourceInfo, MetaValueWithSourceInfo>
        + 'a,
>;
```

### 3. Update Builder Methods

The `with_*` builder methods need updated type bounds:

```rust
// Before
pub fn with_str<F>(mut self, filter: F) -> Filter<'a>
where
    F: FnMut(pandoc::Str) -> FilterReturn<pandoc::Str, Inlines> + 'a,

// After
pub fn with_str<F>(mut self, filter: F) -> Filter<'a>
where
    F: FnMut(pandoc::Str, &mut FilterContext) -> FilterReturn<pandoc::Str, Inlines> + 'a,
```

The macro `define_filter_with_methods!` will need to be updated to include the context parameter in the type bounds.

### 4. Update Traversal Function Signatures

Add `ctx: &mut FilterContext` to all traversal functions:

```rust
pub fn topdown_traverse(
    doc: pandoc::Pandoc,
    filter: &mut Filter,
    ctx: &mut FilterContext,
) -> pandoc::Pandoc

pub fn topdown_traverse_blocks(
    vec: Blocks,
    filter: &mut Filter,
    ctx: &mut FilterContext,
) -> Blocks

pub fn topdown_traverse_inlines(
    vec: Inlines,
    filter: &mut Filter,
    ctx: &mut FilterContext,
) -> Inlines

// etc.
```

### 5. Files to Modify

| File | Changes |
|------|---------|
| `src/filter_context.rs` | **New file** - FilterContext struct |
| `src/filters.rs` | Update callback types, builder methods, macros, and all traversal functions |
| `src/lib.rs` | Export `filter_context` module |
| `src/readers/qmd.rs` | Update call to pass FilterContext |

## Implementation Steps

### Phase 1: Create FilterContext

1. **Create `src/filter_context.rs`**
   - Define `FilterContext` struct (diagnostics only, no target_format)
   - Implement methods: `new()`, `warn()`, `warn_at()`, `error()`, `error_at()`, `has_errors()`, `into_diagnostics()`, `diagnostics()`
   - Implement `Default` trait
   - Add tests

2. **Export from `src/lib.rs`**
   - Add `pub mod filter_context;`

3. **Verify compilation**
   - Run `cargo check`

### Phase 2: Update callback type signatures

4. **Update type aliases in `filters.rs`**
   - `InlineFilterFn<'a, T>` - add `&mut FilterContext` param
   - `BlockFilterFn<'a, T>` - add `&mut FilterContext` param
   - `MetaFilterFn<'a>` - add `&mut FilterContext` param

5. **Update the `define_filter_with_methods!` macro**
   - Change the type bound to include `&mut FilterContext`

6. **Update manual builder methods**
   - `with_inlines`, `with_blocks`, `with_meta` - update type bounds

### Phase 3: Update internal traversal functions

7. **Update internal traits**
   - `InlineFilterableStructure::filter_structure` - add `ctx: &mut FilterContext`
   - `BlockFilterableStructure::filter_structure` - add `ctx: &mut FilterContext`

8. **Update all trait implementations**
   - `impl_inline_filterable_terminal!` macro implementations
   - `impl_inline_filterable_simple!` macro implementations
   - Manual implementations for `Note`, `Cite`, `Inline`
   - `impl_block_filterable_terminal!` macro implementations
   - `impl_block_filterable_simple!` macro implementations
   - Manual implementations for `Paragraph`, `Plain`, `LineBlock`, `OrderedList`, `BulletList`, `DefinitionList`, `Header`, `Table`, `Figure`, `Block`

9. **Update internal helper functions**
   - `traverse_inline_structure` - add ctx param
   - `traverse_inline_nonterminal` - add ctx param
   - `traverse_block_structure` - add ctx param
   - `traverse_block_nonterminal` - add ctx param
   - `traverse_blocks_vec_nonterminal` - add ctx param
   - `traverse_caption` - add ctx param
   - `traverse_row` - add ctx param
   - `traverse_rows` - add ctx param

### Phase 4: Update macros

10. **Update filter application macros**
    - `handle_inline_filter!` - add ctx param, pass to callback
    - `handle_block_filter!` - add ctx param, pass to callback
    - `inlines_apply_and_maybe_recurse!` - add ctx param, pass to callback
    - `blocks_apply_and_maybe_recurse!` - add ctx param, pass to callback

### Phase 5: Update public traversal functions

11. **Update public functions**
    - `topdown_traverse` - add ctx param
    - `topdown_traverse_blocks` - add ctx param
    - `topdown_traverse_inlines` - add ctx param
    - `topdown_traverse_block` - add ctx param
    - `topdown_traverse_inline` - add ctx param
    - `topdown_traverse_meta` - add ctx param
    - `topdown_traverse_meta_value_with_source_info` - add ctx param

### Phase 6: Update callers

12. **Update `readers/qmd.rs`**
    - Create `FilterContext` before calling `topdown_traverse`
    - Update the closure to accept `_ctx` parameter (unused for now)
    - Pass context to traversal

### Phase 7: Tests and validation

13. **Add tests for FilterContext**
    - Test diagnostic accumulation
    - Test `has_errors()`
    - Test `into_diagnostics()`

14. **Run full test suite**
    - `cargo nextest run`
    - Verify no regressions

## Detailed Code Changes

### Callback invocation in macros

The macros need to pass context to callbacks:

```rust
macro_rules! handle_inline_filter {
    ($variant:ident, $value:ident, $filter_field:ident, $filter:expr, $ctx:expr) => {
        if let Some(f) = &mut $filter.$filter_field {
            return inlines_apply_and_maybe_recurse!($value, f, $filter, $ctx);
        } else if let Some(f) = &mut $filter.inline {
            return inlines_apply_and_maybe_recurse!($value.as_inline(), f, $filter, $ctx);
        } else {
            vec![traverse_inline_structure(Inline::$variant($value), $filter, $ctx)]
        }
    };
}

macro_rules! inlines_apply_and_maybe_recurse {
    ($item:expr, $filter_fn:expr, $filter:expr, $ctx:expr) => {
        match $filter_fn($item, $ctx) {  // Pass ctx to callback!
            FilterReturn::Unchanged(inline) => vec![inline.filter_structure($filter, $ctx)],
            FilterReturn::FilterResult(new_content, recurse) => {
                if !recurse {
                    new_content
                } else {
                    topdown_traverse_inlines(new_content, $filter, $ctx)
                }
            }
        }
    };
}
```

### Internal traits with context

```rust
trait InlineFilterableStructure {
    fn filter_structure(self, filter: &mut Filter, ctx: &mut FilterContext) -> Inline;
}

trait BlockFilterableStructure {
    fn filter_structure(self, filter: &mut Filter, ctx: &mut FilterContext) -> Block;
}
```

### Example trait implementation update

```rust
// Before
impl InlineFilterableStructure for pandoc::Emph {
    fn filter_structure(self, filter: &mut Filter) -> Inline {
        Inline::Emph(pandoc::Emph {
            content: topdown_traverse_inlines(self.content, filter),
            ..self
        })
    }
}

// After
impl InlineFilterableStructure for pandoc::Emph {
    fn filter_structure(self, filter: &mut Filter, ctx: &mut FilterContext) -> Inline {
        Inline::Emph(pandoc::Emph {
            content: topdown_traverse_inlines(self.content, filter, ctx),
            ..self
        })
    }
}
```

### readers/qmd.rs caller update

```rust
// Before:
let mut filter = Filter::new()
    .with_block(|block| {
        if let Block::NoteDefinitionPara(refdef) = &block {
            // ...
        }
        // ...
    });
topdown_traverse(result, &mut filter)

// After:
let mut filter = Filter::new()
    .with_block(|block, _ctx| {  // _ctx unused in this filter
        if let Block::NoteDefinitionPara(refdef) = &block {
            // ...
        }
        // ...
    });
let mut filter_ctx = FilterContext::new();
topdown_traverse(result, &mut filter, &mut filter_ctx)
```

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Compilation errors from missed ctx param | High | Low | Compiler will catch these |
| Test failures | Medium | Low | Existing tests should pass (no behavior change) |
| Performance impact | Low | Low | One extra pointer parameter per call |

## Success Criteria

1. `cargo check` passes
2. `cargo nextest run` passes with no new failures
3. `FilterContext` is available for Lua filter implementation
4. Filter callbacks can emit diagnostics via context
5. No behavior changes to existing functionality
