# Plan: Update qmd-syntax-helper to use DiagnosticMessage

<!-- quarto-error-code-audit-ignore-file -->

## Problem Statement

The `qmd::read()` function signature changed from 5 parameters to 4 parameters, and the return type changed from `Vec<String>` to `Vec<DiagnosticMessage>`. This causes compilation failures in `qmd-syntax-helper` which still uses the old API.

**Compilation errors:**
1. `crates/qmd-syntax-helper/src/conversions/div_whitespace.rs:42` - Passing 5 arguments instead of 4
2. `crates/qmd-syntax-helper/src/conversions/div_whitespace.rs:66` - Calling `.join("")` on `Vec<DiagnosticMessage>`
3. `crates/qmd-syntax-helper/src/diagnostics/parse_check.rs:22` - Passing 5 arguments instead of 4

## Context

The changes consolidate all error/warning reporting to use structured `DiagnosticMessage` objects instead of formatted strings. See `claude-notes/plans/2025-10-21-consolidate-to-diagnosticmessage.md` for the full migration plan.

**Key changes:**
- Removed 5th parameter (error formatter function)
- Return type changed from `Result<(Pandoc, ASTContext, Vec<String>), Vec<String>>` to `Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), Vec<DiagnosticMessage>>`
- Errors are now structured `DiagnosticMessage` objects with fields like `title`, `kind`, `location`, etc.

## Current State Analysis

**Two files need updating:**

1. **`div_whitespace.rs`** (Lines 42-79):
   - Currently passes 5 parameters (including error formatter function)
   - Expects `Err(Vec<String>)` containing JSON-formatted errors
   - Parses JSON strings to extract error information
   - Uses custom `ParseError` struct to deserialize error details
   - Complexity: HIGH - needs to extract location info from SourceInfo

2. **`parse_check.rs`** (Lines 22-37):
   - Currently passes 5 parameters (including error formatter function)
   - Only checks if parse succeeds (`result.is_ok()`)
   - Doesn't use error details
   - Complexity: LOW - just remove parameter

## Implementation Tasks

### Task 1: Add quarto-error-reporting dependency
- **File**: `crates/qmd-syntax-helper/Cargo.toml`
- **Action**: Add `quarto-error-reporting.workspace = true` to dependencies
- **Reason**: Need access to `DiagnosticMessage` type

### Task 2: Update parse_check.rs (simple case)
- **File**: `crates/qmd-syntax-helper/src/diagnostics/parse_check.rs`
- **Changes**:
  - Remove 5th parameter from `qmd::read()` call (lines 22-35)
  - No other changes needed - only uses `result.is_ok()`

### Task 3: Investigate SourceInfo API
- **File**: `crates/quarto-source-map/src/lib.rs`
- **Action**: Understand how to extract row/column from `SourceInfo`
- **Needed for**: Converting DiagnosticMessage locations to row/column in div_whitespace.rs

### Task 4: Update div_whitespace.rs (complex case)
- **File**: `crates/qmd-syntax-helper/src/conversions/div_whitespace.rs`
- **Changes**:
  1. Remove custom `ParseError` and `ErrorLocation` structs (lines 10-24) - no longer needed
  2. Update `get_parse_errors()` method signature:
     - Return type: `Result<Vec<quarto_error_reporting::DiagnosticMessage>>` instead of `Result<Vec<ParseError>>`
  3. Update `qmd::read()` call (lines 42-55):
     - Remove 5th parameter (error formatter function)
  4. Update error handling (lines 57-79):
     - Change from parsing JSON strings to working directly with DiagnosticMessage
     - Remove `.join("")` call
     - Remove JSON deserialization
  5. Update `find_div_whitespace_errors()` signature:
     - Accept `&[quarto_error_reporting::DiagnosticMessage]` instead of `&[ParseError]`
  6. Update error matching logic (lines 87-130):
     - Access `error.title` directly (same as before)
     - Extract row from `error.location` (SourceInfo) instead of `error.location.row`
     - May need helper function to convert SourceInfo to row/column

### Task 5: Run cargo check
- **Action**: `cargo check` to verify compilation
- **Expected**: All compilation errors resolved

### Task 6: Run cargo test
- **Action**: `cargo test` to verify no regressions
- **Expected**: All tests pass

## DiagnosticMessage Structure

From `crates/quarto-error-reporting/src/diagnostic.rs`:

```rust
pub struct DiagnosticMessage {
    pub code: Option<String>,           // e.g., "Q-1-1"
    pub title: String,                   // Brief error message
    pub kind: DiagnosticKind,           // Error, Warning, Info, Note
    pub problem: Option<MessageContent>, // What went wrong
    pub details: Vec<DetailItem>,       // Specific error details
    pub hints: Vec<MessageContent>,     // Optional hints
    pub location: Option<SourceInfo>,   // Source location
}
```

Methods available:
- `to_json()` - Convert to JSON representation
- `to_text(Option<&SourceContext>)` - Convert to human-readable text

## SourceInfo Structure

From `crates/quarto-source-map/src/lib.rs` (need to investigate):

```rust
pub struct SourceInfo {
    pub mapping: SourceMapping,
    // ... other fields
}
```

Need to determine:
- How to extract row/column from SourceInfo
- Whether we need byte offset or can get row/column directly
- How to handle None case (no location info)

## Benefits of Migration

1. **Structured data**: Work with typed objects instead of parsing JSON
2. **Consistency**: Same error representation across all tools
3. **Richer information**: Access to error codes, hints, details
4. **Future-proof**: When DiagnosticMessage evolves, qmd-syntax-helper gets improvements

## Testing Strategy

1. **Manual test**: Run qmd-syntax-helper on file with div whitespace issue
   - Create test file: `::::{.class}` (missing space)
   - Run: `qmd-syntax-helper check test.qmd`
   - Verify: Error detected and reported correctly

2. **cargo test**: Ensure existing tests pass
   - If tests exist for div_whitespace, verify they still pass
   - If not, consider adding tests

## References

- Main migration plan: `claude-notes/plans/2025-10-21-consolidate-to-diagnosticmessage.md`
- DiagnosticMessage API: `crates/quarto-error-reporting/src/diagnostic.rs`
- SourceInfo API: `crates/quarto-source-map/src/lib.rs`
