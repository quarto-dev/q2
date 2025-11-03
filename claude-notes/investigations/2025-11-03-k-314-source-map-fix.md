# k-314: Fix Source Map Range Calculation in Tree-Sitter Processing

## Date: 2025-11-03

## Issue Description
k-314 reported that source info pool ranges were incorrect, causing snapshot test failures after the tree-sitter grammar refactoring.

## Root Cause Analysis
The `process_inline_with_delimiter_spaces` function in `text_helpers.rs` was incorrectly handling source ranges for inline formatting elements (Strong, Emph, Strikeout, Superscript, Subscript).

### Problem
When delimiters captured surrounding whitespace, the function would:
1. Create Space nodes with the parent node's range (incorrect)
2. Create the inline element (Strong, etc.) with the full parent node's range including delimiter spaces (incorrect)

For example, with `**bold** ` (with trailing space in delimiter):
- The Strong element would get range [9,19] (full node)
- The Space node would get range [9,19] (parent node, incorrect)

### Expected Behavior
- The Strong element should get range [10,18] (excluding delimiter spaces)
- The leading Space should get range [9,10] (just the space)
- The trailing Space should get range [18,19] (just the space)

## Solution
Modified `process_inline_with_delimiter_spaces` in `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/text_helpers.rs`:

1. **Track delimiter ranges**: Save both first and last delimiter ranges while scanning
2. **Calculate adjusted range**: Compute the correct range for the inline element by:
   - Starting after any leading whitespace in the first delimiter
   - Ending before any trailing whitespace in the last delimiter
3. **Pass adjusted source info**: Changed function signature to pass `SourceInfo` to the create_inline closure
4. **Update all callers**: Modified all inline element handlers in `treesitter.rs` to accept the new signature

### Files Modified

**crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/text_helpers.rs**:
- Lines 226-329: Updated `process_inline_with_delimiter_spaces` function
  - Changed closure signature from `FnOnce(Vec<Inline>)` to `FnOnce(Vec<Inline>, SourceInfo)`
  - Added delimiter range tracking
  - Calculated adjusted source range excluding delimiter spaces
  - Created proper source info for Space nodes

**crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs**:
- Lines 600-613: Updated pandoc_emph handler
- Lines 618-631: Updated pandoc_strong handler
- Lines 633-646: Updated pandoc_strikeout handler
- Lines 650-663: Updated pandoc_superscript handler
- Lines 665-678: Updated pandoc_subscript handler

All handlers now accept `source_info` parameter in their closures.

## Test Results

### Before Fix
```json
{"d":0,"r":[9,19],"t":0}  // Strong element (incorrect, includes delimiters)
{"d":0,"r":[9,19],"t":0}  // Space (incorrect, wrong range)
```

### After Fix
```json
{"d":0,"r":[9,10],"t":0}   // Leading space (correct)
{"d":0,"r":[12,16],"t":0}  // "bold" content (correct)
{"d":0,"r":[10,18],"t":0}  // Strong element (correct, excludes delimiters)
{"d":0,"r":[18,19],"t":0}  // Trailing space (correct)
```

## Snapshots Updated
All JSON and QMD snapshots were updated to reflect the new, simpler source mapping format:
- 001.snap: Basic strong formatting
- 002.snap-007.snap: Various formatting combinations
- html-comment-*.snap: HTML comment tests (61 files)
- Plus many other test snapshots

The new format is simpler and more accurate - source ranges directly map to byte positions without complex displacement entries.

## Status
✅ **Fixed** - Source map ranges now correctly calculated for all inline formatting elements

## Notes
- One unrelated test failure remains: `table-caption-attr.qmd` has a parse error at offset 73-74
  - This is a separate table parsing issue, not related to source map calculation
  - Error: "unexpected character or token here" in attribute section
- The fix simplifies the source mapping system while maintaining accuracy
