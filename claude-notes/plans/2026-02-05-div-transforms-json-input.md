# Plan: Apply div transforms to JSON input

**Issue**: bd-31lk
**Date**: 2026-02-05

## Overview

The `transform_definition_list_div` and `transform_list_table_div` transforms convert divs with specific classes (`definition-list` and `list-table`) into proper DefinitionList and Table AST nodes. Currently, these transforms are only applied when reading qmd input, not when reading JSON input.

## Problem Analysis

### Current Code Flow

1. **QMD input path** (`main.rs:226-269`):
   - `readers::qmd::read()` → `pandoc::treesitter_to_pandoc()`
   - Inside `treesitter_to_pandoc()` (`treesitter.rs:1103`): calls `postprocess()`
   - `postprocess()` (`postprocess.rs:909-939`): applies div transforms via `.with_div()` handler
   - Then calls `merge_strs()`

2. **JSON input path** (`main.rs:271-280`):
   - `readers::json::read()` → `read_pandoc()` → returns directly
   - **No postprocessing is applied**

### Root Cause

The div transforms are embedded in `postprocess()`, which is only called from the qmd reader path. The JSON reader bypasses all postprocessing.

## Solution Approaches

### Option A: Call postprocess() after JSON reading (NOT recommended)

Call the full `postprocess()` function for JSON input too.

**Problems**:
- Many transforms in `postprocess()` are specific to qmd parsing artifacts (e.g., Inline::Attr handling, header auto-id generation, LineBreak cleanup)
- Some transforms assume qmd-specific parsing behavior that may conflict with Pandoc's JSON output
- May cause unintended side effects

### Option B: Extract div transforms into a separate pass (Recommended)

Create a new function `transform_divs()` that only applies the div transforms (definition-list and list-table), and call it:
1. From `postprocess()` (to maintain current qmd behavior)
2. From `main.rs` after reading JSON input

**Advantages**:
- Minimal code duplication
- Clean separation of concerns
- Easy to maintain
- No risk of unintended side effects from other postprocess transforms

### Option C: Apply transforms in main.rs after any input

Add a generic transform pass in `main.rs` that applies div transforms regardless of input format.

**Problems**:
- Adds complexity to main.rs
- Need to handle diagnostic collection at that level
- Less clean architecture

## Chosen Approach: Option B

Extract the div transforms into a reusable function that can be called from both paths.

## Work Items (Test-Driven Development)

### Phase 1: Write failing tests

- [x] Write test: definition-list div in JSON input transforms to DefinitionList
  - Created `crates/pampa/tests/test_json_div_transforms.rs`
  - Test: `test_definition_list_div_transform_from_json`

- [x] Write test: list-table div in JSON input transforms to Table
  - Test: `test_list_table_div_transform_from_json`
  - Test: `test_list_table_with_header_from_json`

- [x] Run tests and verify they fail as expected
  - All 3 tests failed with "Expected ... but got Div - transform was not applied to JSON input"

### Phase 2: Implement the fix

- [x] Create new function `transform_divs(doc: Pandoc, error_collector: &mut DiagnosticCollector) -> Pandoc` in `postprocess.rs`
  - Added at line 761 in `postprocess.rs`
  - Uses Filter machinery with `.with_div()` handler

- [x] Export `transform_divs()` from the pampa crate
  - Accessible via `crate::pandoc::treesitter_utils::postprocess::transform_divs`

- [x] Apply `transform_divs()` at the call site in `main.rs` (NOT inside `json::read()`)
  - Modified `main.rs` "json" arm to call `transform_divs()` after `json::read()`
  - `json::read()` remains a pure JSON-to-AST parser with no side effects
  - This is critical because `json::read()` is used by multiple callers
    (main.rs, json_filter.rs, lua/readwrite.rs, qmd-syntax-helper's grid table converter)
    and only the binary entry point should apply semantic transforms

**IMPORTANT CORRECTION**: The initial implementation (Phase 2, first attempt) incorrectly
placed `transform_divs()` inside `json::read()`. This broke the grid table converter in
`qmd-syntax-helper` because it relies on `json::read()` preserving list-table divs as-is.
The fix was to move the transform to the call site in `main.rs`, matching the original plan.

### Phase 3: Verify tests pass

- [x] Run all tests and verify the new tests now pass
  - All 3 new tests pass (tests call `transform_divs` explicitly after `json::read()`)
- [x] Run full **workspace** test suite (`cargo nextest run --workspace`) to ensure no regressions
  - All 6180 tests pass (including qmd-syntax-helper grid table tests)

## Implementation Notes

### Function signature

```rust
pub fn transform_divs(
    doc: Pandoc,
    error_collector: &mut DiagnosticCollector
) -> Pandoc
```

### Integration in main.rs

After the JSON reader call (`main.rs:271-280`), add:

```rust
"json" => {
    let result = readers::json::read(&mut input.as_bytes());
    match result {
        Ok((pandoc, context)) => {
            // Apply div transforms
            let mut error_collector = DiagnosticCollector::new();
            let pandoc = transform_divs(pandoc, &mut error_collector);
            // Handle any diagnostics from transform_divs
            // ...
            (pandoc, context)
        }
        Err(e) => { ... }
    }
}
```

### Testing

Create test files in `tests/` that:
1. Feed a JSON document containing a `definition-list` div
2. Verify the output contains a DefinitionList block
3. Feed a JSON document containing a `list-table` div
4. Verify the output contains a Table block
