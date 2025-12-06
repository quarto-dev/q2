# Template Diagnostics file_id Attribution Bug

**Issue ID**: k-5zv5
**Date**: 2025-12-06
**Status**: Analysis complete, awaiting implementation approval

## Problem Statement

Template warnings (e.g., undefined variable warnings from the built-in HTML template) are being incorrectly reported as originating from the `.qmd` file instead of the actual template file.

### Minimal Reproduction

```bash
cargo run --bin quarto-markdown-pandoc -- \
  -i /tmp/test-citeproc/doc.qmd \
  -t html \
  --template html \
  --json-errors
```

With a simple test document `/tmp/test-citeproc/doc.qmd`:
```yaml
---
title: Test
---

Hello world.
```

### Observed Output

```json
{"code":"Q-10-2","kind":"warning","location":{"Original":{"end_offset":71,"file_id":0,"start_offset":65}},"title":"Undefined variable: lang"}
{"code":"Q-10-2","kind":"warning","location":{"Original":{"end_offset":89,"file_id":0,"start_offset":83}},"title":"Undefined variable: lang"}
{"code":"Q-10-2","kind":"warning","location":{"Original":{"end_offset":718,"file_id":0,"start_offset":707}},"title":"Undefined variable: pagetitle"}
```

All warnings show `file_id: 0`, which the main program interprets as the `.qmd` file. However, these warnings originate from the HTML template file (specifically, undefined variables `lang` and `pagetitle` in the built-in HTML template).

## Root Cause Analysis

The bug involves a mismatch between two independent `SourceContext` instances:

### 1. Template Parser Context (quarto-doctemplate)

In `crates/quarto-doctemplate/src/parser.rs:44-51`:

```rust
impl ParserContext {
    /// Create a new parser context for a file.
    pub fn new(filename: &str) -> Self {
        let mut source_context = SourceContext::new();
        let file_id = source_context.add_file(filename.to_string(), None);
        Self {
            source_context,
            file_id,
        }
    }
}
```

Each template (main + partials) gets its **own** `SourceContext`, where the template file is assigned `file_id: 0`.

### 2. Main Program Context (quarto-markdown-pandoc)

In `crates/quarto-markdown-pandoc/src/main.rs`, the `.qmd` file is parsed into its own `ASTContext` which contains a `SourceContext` where the `.qmd` file has `file_id: 0`.

### 3. The Mismatch

When template diagnostics are reported (main.rs:363-376):

```rust
match render_with_bundle(&pandoc, &context, &bundle, body_format) {
    Ok((output, diagnostics)) => {
        // ...
        for diagnostic in &diagnostics {
            eprintln!("{}", diagnostic.to_text(Some(&context.source_context)));
        }
    }
}
```

The `context.source_context` is from the **main document**, but the diagnostics have `file_id: 0` from the **template's** `ParserContext`. The rendering code incorrectly maps the template's `file_id: 0` to the main document's file (also `file_id: 0`).

## Solution Approaches

### Option A: Shared SourceContext (Invasive but Correct)

Pass the main program's `SourceContext` into the template parser so all files share a single context with unique file IDs.

**Pros**:
- Correct file attribution
- Single source of truth for all files

**Cons**:
- Requires threading `SourceContext` through multiple crate boundaries
- Changes API surface of quarto-doctemplate
- May complicate WASM builds

### Option B: Use FilterProvenance for Template Diagnostics (Minimal Change)

When template diagnostics are generated, use `SourceInfo::FilterProvenance` variant instead of `SourceInfo::Original`. This avoids the file_id conflict by using a different mechanism.

**Pros**:
- Minimal changes to existing code
- Works with current architecture

**Cons**:
- FilterProvenance was designed for Lua filters, may need modification
- Semantically different from "original source location"

### Option C: SourceContext Merging (Medium Complexity)

Add the template file(s) to the main program's `SourceContext` before rendering diagnostics, and remap the template diagnostics' file_ids accordingly.

**Pros**:
- Keeps existing architecture mostly intact
- Correct file attribution after merging

**Cons**:
- Requires tracking which files are templates
- Need to remap file_ids in diagnostics (potentially deep in SourceInfo structures)

### Option D: External File Path in Diagnostics (Pragmatic)

Store the actual file path in the diagnostic along with the SourceInfo, and use the path for display when the file_id cannot be resolved in the current SourceContext.

**Pros**:
- No changes to SourceContext architecture
- Backward compatible

**Cons**:
- Duplicates information (path stored twice)
- May not integrate well with Ariadne rendering

## Detailed Analysis of Option A Downsides

The initial assessment listed three downsides for Option A (Shared SourceContext). Upon deeper analysis, these concerns are less significant than initially stated:

### Downside 1: "Requires threading SourceContext through multiple crate boundaries"

**Initial Concern**: Cross-crate dependencies could be problematic for maintenance and stability.

**Deeper Analysis**: This concern is largely unfounded because:

1. **Existing dependency**: `quarto-doctemplate` already depends on `quarto-source-map`:
   ```toml
   # crates/quarto-doctemplate/Cargo.toml line 25
   quarto-source-map = { path = "../quarto-source-map" }
   ```

2. **Already using the types**: The template parser already uses `FileId`, `SourceInfo`, and `SourceContext` from `quarto-source-map`. We're not introducing new coupling—we're using an existing dependency more fully.

3. **Semantic correctness**: Threading `SourceContext` actually makes the relationship between files explicit. Currently, the implicit creation of independent contexts *hides* a design flaw.

**Conclusion**: This is not a real downside. The crate boundary is already crossed; we're just using it correctly.

### Downside 2: "Changes API surface of quarto-doctemplate"

**Initial Concern**: API changes could affect callers and require significant refactoring.

**Deeper Analysis**: The change is minimal and actually improves the API:

1. **Simple signature change**:
   ```rust
   // Current
   impl ParserContext {
       pub fn new(filename: &str) -> Self { ... }
   }

   // Proposed (Option 1: required context)
   impl ParserContext {
       pub fn new(filename: &str, source_context: &mut SourceContext) -> Self { ... }
   }

   // Proposed (Option 2: optional context for backward compatibility)
   impl ParserContext {
       pub fn new(filename: &str, source_context: Option<&mut SourceContext>) -> Self { ... }
   }
   ```

2. **Lifetime semantics are natural**: The constraint that `ParserContext` must not outlive the `SourceContext` it references matches the semantic intent perfectly:
   - `SourceContext` represents "maintaining an interest in reporting location-accurate error messages"
   - `ParserContext` is a short-lived parsing operation within that scope
   - This relationship is already implicit; making it explicit via lifetimes is *better* design

3. **Caller changes are mechanical**:
   - Callers who want integrated behavior pass their `SourceContext`
   - Callers who want standalone behavior (if using `Option`) pass `None`
   - No complex decision-making required

**Conclusion**: The API change is small, improves semantic clarity, and the lifetime management is straightforward.

### Downside 3: "May complicate WASM builds"

**Initial Concern**: WASM builds have constraints that could be violated by this change.

**Deeper Analysis**: After examining the actual WASM architecture, this concern does not apply:

1. **Current WASM structure**:
   ```toml
   # crates/wasm-qmd-parser/Cargo.toml
   [dependencies]
   quarto-markdown-pandoc = { path = "../quarto-markdown-pandoc", default-features = false }
   ```
   The WASM module compiles `quarto-markdown-pandoc` (and transitively `quarto-doctemplate`) into a single WASM module.

2. **Feature flags already handle WASM differences**:
   ```toml
   # crates/quarto-markdown-pandoc/Cargo.toml
   [features]
   default = ["terminal-support", "json-filter", "lua-filter", "template-fs"]
   # Enable filesystem-based template resolution (disable for WASM)
   template-fs = []
   ```
   The `template-fs` feature controls filesystem access, not template parsing. Templates still work in WASM via `TemplateBundle` (in-memory).

3. **No cross-module boundary issues**: All the Rust code runs within a single WASM module. We're not passing `SourceContext` across JavaScript↔WASM FFI boundaries—we're passing Rust references within Rust code that happens to be compiled to WASM.

4. **Option pattern provides flexibility**: Using `Option<&mut SourceContext>`:
   - WASM builds can pass `None` for standalone template parsing (creates internal context, current behavior preserved)
   - Integrated builds pass `Some(&mut ctx)` for correct file attribution
   - This is actually MORE flexible than the current design

**Conclusion**: WASM is not a real concern. The change is transparent to WASM builds and, if anything, provides more flexibility.

## Revised Assessment

Given the deeper analysis, **Option A (Shared SourceContext)** should be reconsidered as the preferred approach:

| Concern | Initial Assessment | Revised Assessment |
|---------|-------------------|-------------------|
| Crate boundaries | Problem | Non-issue (dependency already exists) |
| API changes | Significant | Minimal and improves design |
| WASM | Complication | Non-issue (same module, feature flags handle differences) |

### Option A Implementation Sketch

```rust
// crates/quarto-doctemplate/src/parser.rs

pub struct ParserContext<'a> {
    /// External source context (shared with caller) or internal (standalone)
    source_context: SourceContextRef<'a>,
    /// The current file ID within the source context
    pub file_id: FileId,
}

enum SourceContextRef<'a> {
    /// Borrowed from caller - file IDs are globally unique
    Shared(&'a mut SourceContext),
    /// Owned internally - for standalone use
    Owned(SourceContext),
}

impl<'a> ParserContext<'a> {
    /// Create with shared context (integrated mode)
    pub fn with_context(filename: &str, source_context: &'a mut SourceContext) -> Self {
        let file_id = source_context.add_file(filename.to_string(), None);
        Self {
            source_context: SourceContextRef::Shared(source_context),
            file_id,
        }
    }

    /// Create with internal context (standalone mode, backward compatible)
    pub fn new(filename: &str) -> ParserContext<'static> {
        let mut source_context = SourceContext::new();
        let file_id = source_context.add_file(filename.to_string(), None);
        ParserContext {
            source_context: SourceContextRef::Owned(source_context),
            file_id,
        }
    }
}
```

This provides:
- Backward compatibility via `new()` for standalone use
- Correct behavior via `with_context()` for integrated use
- No changes required for existing callers who don't need shared context

## Recommended Approach (Revised)

**Option A (Shared SourceContext)** is now the recommended approach, as the initially-identified downsides do not hold up under scrutiny.

However, **Option C (SourceContext Merging)** remains a valid fallback if:
- The lifetime management in Option A proves more complex than anticipated in practice
- There are unforeseen issues with the `SourceContextRef` enum approach

### Option C as Fallback

If Option A encounters obstacles:

1. Modify `quarto-doctemplate` to return not just diagnostics but also the template's `SourceContext` or file mappings
2. Merge template file information into the main program's `SourceContext` before rendering diagnostics
3. Remap `file_id` values in the template diagnostics to match the merged context

This preserves the existing architecture while ensuring correct file attribution.

## Files Involved

| File | Role |
|------|------|
| `crates/quarto-doctemplate/src/parser.rs` | Creates template's ParserContext with independent SourceContext |
| `crates/quarto-doctemplate/src/evaluator.rs` | Generates diagnostics with template's file_id |
| `crates/quarto-doctemplate/src/eval_context.rs` | Builds DiagnosticMessage with SourceInfo |
| `crates/quarto-markdown-pandoc/src/template/render.rs` | Passes template diagnostics back to caller |
| `crates/quarto-markdown-pandoc/src/main.rs` | Renders diagnostics using wrong SourceContext |
| `crates/quarto-source-map/src/context.rs` | SourceContext and file_id management |
| `crates/quarto-source-map/src/source_info.rs` | SourceInfo structure with file_id |

## Next Steps

1. Get user approval on recommended approach
2. Design the API changes needed for SourceContext merging
3. Implement changes in quarto-doctemplate
4. Update quarto-markdown-pandoc to use the merged context
5. Add tests to verify correct file attribution
