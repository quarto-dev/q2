# Migration Plan: quarto-markdown-pandoc Error Handling → quarto-error-reporting

<!-- quarto-error-code-audit-ignore-file -->

## Executive Summary

This document outlines the plan to migrate quarto-markdown-pandoc's error handling from its current custom `ErrorCollector` trait to use the standardized `quarto-error-reporting` crate. This migration will provide richer, more structured error messages while maintaining the existing text/JSON output flexibility.

## Current State Analysis

### Current Error Handling in quarto-markdown-pandoc

**Location**: `src/utils/error_collector.rs`

#### Current Architecture

```rust
// Simple source location
pub struct SourceInfo {
    pub row: usize,
    pub column: usize,
}

// Trait for collecting errors/warnings
pub trait ErrorCollector {
    fn warn(&mut self, message: String, location: Option<&SourceInfo>);
    fn error(&mut self, message: String, location: Option<&SourceInfo>);
    fn has_errors(&self) -> bool;
    fn messages(&self) -> Vec<String>;
    fn into_messages(self) -> Vec<String>;
}

// Two implementations:
// 1. TextErrorCollector - produces "Error: message at row:col"
// 2. JsonErrorCollector - produces {"title": "Error", "message": "...", "location": {...}}
```

#### Current Usage Pattern

**Error Collection During Parsing**:
- `postprocess.rs` uses `ErrorCollector` to collect warnings and errors during AST transformation
- Example warnings: "Caption found without a preceding table at 35:1"
- Example errors: "Found attr in postprocess: {...} - this should have been removed"

**Top-level Error Handling**:
- `qmd.rs` reader creates either `TextErrorCollector` or `JsonErrorCollector` based on `error_formatter` parameter
- Warnings are output to stderr
- Errors cause parsing to fail

**Parse Error Messages**:
- Separate system in `qmd_error_messages.rs` using ariadne for tree-sitter parse errors
- Uses error message table loaded from JSON corpus
- **This is separate from ErrorCollector and already has good structure**

### quarto-error-reporting Capabilities

**Location**: `crates/quarto-error-reporting/src/`

#### Current API (Phase 1 + 4)

```rust
// Rich diagnostic message structure
pub struct DiagnosticMessage {
    pub code: Option<String>,          // e.g., "Q-1-1"
    pub title: String,                  // Brief error message
    pub kind: DiagnosticKind,           // Error, Warning, Info
    pub problem: Option<MessageContent>,// What went wrong
    pub details: Vec<DetailItem>,       // Specific information (max 5)
    pub hints: Vec<MessageContent>,     // Guidance for fixing
}

// Builder API (tidyverse-style)
DiagnosticMessageBuilder::error("Incompatible types")
    .with_code("Q-1-2")
    .problem("Cannot combine date and datetime types")
    .add_detail("`x` has type `date`")
    .add_detail("`y` has type `datetime`")
    .add_hint("Convert both to the same type?")
    .build()
```

#### Missing Functionality (Needed for Migration)

1. **Rendering to text/JSON** (Phase 2 - not yet implemented)
   - Need `.to_text()` method for human-readable output
   - Need `.to_json()` method for machine-readable output
   - Currently have structure but no output methods

2. **Source location integration** (Phase 2)
   - DiagnosticMessage has comments for `source_spans` but not implemented
   - Need to integrate with quarto-source-map

3. **ariadne integration** (Phase 2)
   - Planned but not implemented
   - `to_ariadne_report()` method mentioned in docs

## Gap Analysis

### What Works Well

✅ **Current ErrorCollector**:
- Simple, effective trait-based design
- Clear separation of text vs JSON output
- Works well for simple error messages
- Easy to use with RefCell pattern for multiple closures

✅ **quarto-error-reporting structure**:
- Rich, semantic error structure following best practices
- Builder API encourages good error message design
- Extensible with error codes, hints, details

### What Needs Work

❌ **Impedance Mismatch**:
- ErrorCollector is callback-based (collect errors, output later)
- quarto-error-reporting is value-based (build messages, render on demand)

❌ **Missing Rendering**:
- quarto-error-reporting can build DiagnosticMessage but can't output it yet
- Need Phase 2 implementation (rendering) before we can use it

❌ **Source Location Mismatch**:
- ErrorCollector uses simple row/column
- quarto-error-reporting needs full SourceSpan with file info
- quarto-source-map exists but not integrated yet

## Migration Strategy

### Recommended Approach: Phased Migration

#### Phase A: Implement Rendering in quarto-error-reporting (PREREQUISITE)

**Must be done first** before migrating quarto-markdown-pandoc.

1. **Add rendering traits/methods**:
   ```rust
   impl DiagnosticMessage {
       pub fn to_text(&self) -> String { /* format as human-readable */ }
       pub fn to_json(&self) -> String { /* format as JSON */ }
   }
   ```

2. **Design decisions**:
   - Text format: Follow tidyverse style (x/i bullets, etc.)
   - JSON format: Structured JSON with all fields
   - Keep it simple for now (fancy ariadne integration can come later)

#### Phase B: Create Bridge Collector

Create a new `DiagnosticCollector` that implements the `ErrorCollector` trait but builds `DiagnosticMessage` objects:

```rust
// In quarto-markdown-pandoc/src/utils/diagnostic_collector.rs
pub struct DiagnosticCollector {
    messages: Vec<DiagnosticMessage>,
    format: OutputFormat,
}

enum OutputFormat {
    Text,
    Json,
}

impl ErrorCollector for DiagnosticCollector {
    fn warn(&mut self, message: String, location: Option<&SourceInfo>) {
        let diag = DiagnosticMessageBuilder::warning(message)
            // TODO: Add location when we integrate quarto-source-map
            .build();
        self.messages.push(diag);
    }

    fn error(&mut self, message: String, location: Option<&SourceInfo>) {
        let diag = DiagnosticMessageBuilder::error(message)
            .build();
        self.messages.push(diag);
    }

    fn into_messages(self) -> Vec<String> {
        self.messages
            .into_iter()
            .map(|msg| match self.format {
                OutputFormat::Text => msg.to_text(),
                OutputFormat::Json => msg.to_json(),
            })
            .collect()
    }
}
```

**Benefits**:
- Drop-in replacement for current TextErrorCollector/JsonErrorCollector
- No changes to calling code
- Incremental path to richer error messages

#### Phase C: Gradual Enhancement

Once bridge collector is in place, gradually enhance error messages:

1. **Add error codes** where appropriate
2. **Add problem statements** to make errors clearer
3. **Add hints** to guide users
4. **Use semantic markup** (`` `x`{.arg} ``, etc.)

Example migration:
```rust
// Before
error_collector.error(
    "Found attr in postprocess".to_string(),
    Some(&location)
)

// After (Phase C - using generic error)
error_collector.error(
    "Found attr in postprocess".to_string(),
    Some(&location)
)
// ^ Same interface, but DiagnosticCollector internally creates:
// DiagnosticMessageBuilder::generic_error("Found attr in postprocess", file!(), line!())

// Future enhancement (separate task, later):
diagnostic_collector.error_with_builder(|b| {
    b.error("Unexpected attribute")
        .with_code("Q-2-15")  // Specific error code assigned
        .problem("Attributes should have been removed by earlier processing")
        .add_detail(format!("Found: {:?}", attr))
        .add_hint("This is likely a bug in the parser")
        .at_location(location)
})
```

#### Phase D: Source Location Integration

Once quarto-source-map is integrated:

1. Replace simple SourceInfo with full source spans
2. Enable rich ariadne-style error output with source context
3. Add file information to all error messages

#### Phase E: Retire Old Collectors

Once all error sites use DiagnosticCollector:

1. Remove TextErrorCollector
2. Remove JsonErrorCollector
3. Remove old SourceInfo type (use quarto-source-map instead)
4. Remove ErrorCollector trait (or make it an alias for new trait)

## Implementation Plan

### Step 1: Implement Basic Rendering (quarto-error-reporting)

**Files to modify**:
- `crates/quarto-error-reporting/src/diagnostic.rs`
- `crates/quarto-error-reporting/src/rendering.rs` (new module)
- `crates/quarto-error-reporting/src/builder.rs`

**Work**:
- Add `to_text()` method with simple formatting
- Add `to_json()` method matching current JSON format
- Add `DiagnosticMessageBuilder::generic_error()` helper that:
  - Uses error code Q-0-99 <!-- quarto-error-code-audit-ignore -->
  - Accepts `file!()`, `line!()` as parameters
  - Creates a generic error message for migration
  - Example: `.generic_error("Found unexpected attr", file!(), line!())`
- Add tests for both formats

**Estimate**: 2-3 hours

### Step 2: Create DiagnosticCollector Bridge

**Files to create**:
- `crates/quarto-markdown-pandoc/src/utils/diagnostic_collector.rs`

**Files to modify**:
- `crates/quarto-markdown-pandoc/src/utils/mod.rs` (export new collector)
- `crates/quarto-markdown-pandoc/Cargo.toml` (add quarto-error-reporting dep)

**Work**:
- Implement DiagnosticCollector that wraps quarto-error-reporting
- Implement ErrorCollector trait for backward compatibility
- Add tests

**Estimate**: 2-3 hours

### Step 3: Switch to DiagnosticCollector

**Files to modify**:
- `crates/quarto-markdown-pandoc/src/readers/qmd.rs`
  - Replace TextErrorCollector with DiagnosticCollector::text()
  - Replace JsonErrorCollector with DiagnosticCollector::json()

**Work**:
- Update reader to use new collector
- Run all tests to verify no regression
- Test both text and JSON output modes

**Estimate**: 1 hour

### Step 4: Gradual Enhancement (Ongoing)

**Approach**:
- As bugs are fixed or features added, use builder API for new error messages
- Gradually convert simple error strings to rich DiagnosticMessage objects
- Add error codes from error catalog
- Add hints and problem statements

**This is ongoing work, not a single task**.

## Testing Strategy

### Critical Tests

1. **Backward compatibility**: All existing tests must pass with new collector
2. **Text output format**: Verify text output is readable and contains all info
3. **JSON output format**: Verify JSON is valid and structured correctly
4. **Error vs Warning**: Verify distinction is maintained
5. **Source locations**: Verify locations appear in output

### Test Files to Update

- `tests/test_json_errors.rs` - Update expectations for new JSON format
- Add new tests for DiagnosticCollector directly
- Test builder API integration

## Dependencies

### Before We Can Start

1. ✅ **quarto-error-reporting moved to crates/** (DONE)
2. ⏳ **Implement Phase 2 rendering** (NEED TO DO)

### For Full Integration

1. ⏳ **quarto-source-map integration** (future)
2. ⏳ **ariadne integration** (future)

## Risks and Mitigations

### Risk: Breaking Changes to Error Format

**Impact**: Tools parsing error output might break

**Mitigation**:
- Keep JSON format compatible (add fields, don't remove)
- Provide version field in JSON output
- Document format changes clearly

### Risk: Performance Regression

**Impact**: Building DiagnosticMessage objects might be slower

**Mitigation**:
- DiagnosticMessage is lightweight (just strings and enums)
- Only build when actually reporting errors (rare case)
- Profile if concerned

### Risk: Incomplete Migration

**Impact**: Mixed old/new error styles

**Mitigation**:
- This is actually OK! Gradual migration is the plan
- Bridge collector makes it seamless
- Eventually retire old collectors when everything is migrated

## Success Criteria

### Phase B Success (Bridge Collector)

- ✅ All existing tests pass with DiagnosticCollector
- ✅ Text output format is readable
- ✅ JSON output is valid and structured
- ✅ No performance regression

### Full Migration Success

- ✅ All errors use DiagnosticMessage
- ✅ Error codes assigned to common errors
- ✅ Hints provided where applicable
- ✅ Source locations show file info
- ✅ ariadne integration provides rich terminal output

## Decisions Made

1. **JSON Format**: Keep it simple
   - Start with current flat format: `{title, kind, message, location}`
   - Can enhance later when needed

2. **Error Codes**: Use generic Q-0-99 for now <!-- quarto-error-code-audit-ignore -->
   - Create builder method that automatically includes `file!()` and `line!()` info
   - This allows us to track where errors originate in code
   - Later: Create separate task to replace with specific error codes

3. **Semantic Markup**: Not applicable for initial migration
   - Focus is on infrastructure change, not message enhancement
   - Can add markup later when enhancing specific errors

4. **ariadne Integration**: Defer until source location is uniform
   - Only add when YAML and Pandoc AST have uniform file location info
   - Not ready yet

5. **Backward Compatibility**: Keep ErrorCollector trait
   - Trait stays as interface
   - Old implementations removed when migration complete

## Timeline Estimate

- **Phase A** (Rendering): 4-6 hours
- **Phase B** (Bridge Collector): 3-4 hours
- **Phase C** (Switch): 1-2 hours
- **Testing**: 2-3 hours

**Total for basic migration**: ~10-15 hours

**Gradual enhancement**: Ongoing as features/bugs are addressed

## Next Steps

1. **Review this plan** with team/user
2. **Implement Phase A** (rendering in quarto-error-reporting)
3. **Implement Phase B** (bridge collector)
4. **Test thoroughly**
5. **Deploy and monitor**
6. **Begin gradual enhancement**
