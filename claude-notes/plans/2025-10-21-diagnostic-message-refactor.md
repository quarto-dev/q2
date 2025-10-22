# Refactor readers::qmd::read() to Return DiagnosticMessages

**Date:** 2025-10-21
**Status:** In Progress

## Problem

The `readers::qmd::read()` function currently takes an `error_formatter` parameter to decide between JSON and text output formats. This violates separation of concerns - the parsing layer is making formatting decisions that should be left to the caller.

Additionally, metadata parsing in `meta.rs` calls `eprintln!` directly, which means:
1. Output format decisions are baked into the parsing logic
2. The `--json-errors` flag is not respected for metadata warnings
3. Warnings cannot be collected and processed by the caller

## Solution

Refactor `read()` to return `DiagnosticMessage` objects instead of formatted strings, and include warnings in the success case.

### New Signature

```rust
pub fn read<T: Write>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
) -> Result<
    (pandoc::Pandoc, ASTContext, Vec<DiagnosticMessage>), // Success + warnings
    Vec<DiagnosticMessage> // Errors
>
```

### Current State Analysis

**read() function (src/readers/qmd.rs:53-59):**
- Takes `error_formatter: Option<F>` parameter
- Returns `Result<(Pandoc, ASTContext), Vec<String>>`
- Lines 85-91: Recursive call when adding newline
- Lines 151-161: Converts diagnostics to strings based on error_formatter
- Lines 164-177: Prints warnings to stderr using eprintln!

**Metadata parsing (src/pandoc/meta.rs):**
- Line 214: `parse_yaml_string_as_markdown()` - prints directly to stderr
- Line 265: `eprintln!` for !md tag errors
- Line 295: `eprintln!` for untagged markdown warnings
- Called from `yaml_to_meta_with_source_info()` (line 328)
- Called from `rawblock_to_meta_with_source_info()` (line 597)

### Call Sites (26 total)

1. **main.rs** (1 site)
   - Line 129: Main entry point - needs to format diagnostics based on json_errors flag

2. **wasm_entry_points/mod.rs** (1 site)
   - Line 41: WASM entry - needs to handle diagnostics appropriately

3. **meta.rs** (3 sites)
   - Lines 223, 668, 759: Recursive parsing calls - need DiagnosticCollector threaded through

4. **Tests** (~20 sites)
   - test_metadata_source_tracking.rs: 2 calls
   - test_nested_yaml_serialization.rs: 4 calls
   - test_json_errors.rs: 3 calls
   - test.rs: 7 calls
   - test_ordered_list_formatting.rs: 2 calls
   - test_warnings.rs: 2 calls
   - fuzz target: 1 call

## Implementation Steps

### Step 1: Update read() function signature
- Remove error_formatter parameter and generic F
- Change return type to include Vec<DiagnosticMessage> in both Ok and Err cases
- Handle recursive call for missing newline case

### Step 2: Update error handling in read()
- Return diagnostics directly instead of converting to strings
- Include collected warnings in success return

### Step 3: Thread DiagnosticCollector through metadata parsing
Functions to update:
- `rawblock_to_meta_with_source_info(&RawBlock, &ASTContext, &mut DiagnosticCollector)`
- `yaml_to_meta_with_source_info(YamlWithSourceInfo, &ASTContext, &mut DiagnosticCollector)`
- `parse_yaml_string_as_markdown(&str, &SourceInfo, &ASTContext, Option<SourceInfo>, &mut DiagnosticCollector)`
- `parse_metadata_strings_with_source_info(MetaValue, &mut Vec<MetaMapEntry>, &mut DiagnosticCollector)`

Replace eprintln! with collector.add(diagnostic)

### Step 4: Update main.rs
- Remove error_formatter construction
- Update read() call
- Format diagnostics based on args.json_errors flag
- Output errors and warnings appropriately

### Step 5: Update call sites
- main.rs: Handle new return type with diagnostics
- wasm_entry_points: Decide on diagnostic handling strategy
- meta.rs: Pass through DiagnosticCollector
- Tests: Update pattern matching, most can ignore warnings vector

### Step 6: Add test
- Test that --json-errors flag properly formats metadata warnings as JSON

### Step 7: Validation
- cargo check
- cargo test
- Manual test: `cargo run --bin quarto-markdown-pandoc -- -i crates/quarto-markdown-pandoc/tests/claude-examples/meta-warning.qmd --json-errors`

## Benefits

1. **Separation of concerns**: Parser builds diagnostics; caller formats them
2. **No premature output**: All diagnostics collected and returned
3. **Warnings with success**: Warnings can be returned alongside successful parse
4. **Respects json-errors flag**: All diagnostics use the same formatting
5. **Future-proof**: Easy to add lints, notes, etc.

## Challenges

1. **Recursive calls**: Metadata parsing calls read() recursively - need to thread DiagnosticCollector
2. **Many call sites**: ~26 call sites need updating
3. **Backward compatibility**: This is a breaking API change for any external users

## Notes

- The DiagnosticCollector infrastructure already exists and supports both to_text() and to_json()
- This aligns with the broader error handling redesign in progress
- After this change, all error/warning output decisions happen at the binary entry point, not in library code
