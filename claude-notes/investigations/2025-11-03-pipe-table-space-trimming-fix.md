# Pipe Table Cell Space Trimming - Fix Implementation

## Date: 2025-11-03

## Issue
Test `tests/pandoc-match-corpus/markdown/pipe-table-code-span.qmd` was failing because pipe table cells contained leading spaces that Pandoc doesn't include.

## Root Cause
The new tree-sitter grammar correctly identifies spaces in code span delimiters (e.g., ` `` ` - space before backtick). The code span processor (`code_span_helpers.rs`) intentionally emits Space inline elements for these delimiter spaces, which is correct behavior for normal paragraphs.

However, pipe table cells should have both leading AND trailing spaces trimmed to match Pandoc behavior. The existing code in `pipe_table.rs` only trimmed trailing spaces manually, missing leading spaces.

## Solution Implemented

### Files Modified

**crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/pipe_table.rs**

1. **Added import** (line 21):
   ```rust
   use super::postprocess::trim_inlines;
   ```

2. **Replaced manual trimming** (line 134):

   **Before:**
   ```rust
   // Trim trailing spaces from cell content to match Pandoc behavior
   while let Some(last) = plain_content.last() {
       if matches!(last, crate::pandoc::inline::Inline::Space(_)) {
           plain_content.pop();
       } else {
           break;
       }
   }
   ```

   **After:**
   ```rust
   // Trim leading and trailing spaces from cell content to match Pandoc behavior
   plain_content = trim_inlines(plain_content).0;
   ```

## Why This Fix Works

The `trim_inlines` function from `postprocess.rs` already exists and is used throughout the codebase. It:
- ✅ Removes leading spaces
- ✅ Removes trailing spaces
- ✅ Preserves spaces in the middle of content
- ✅ Returns a cleaned Vec<Inline> ready to use

This is a better solution than manual loop-based trimming because:
1. It's more comprehensive (handles both leading and trailing)
2. It's already tested and proven reliable
3. It's more maintainable (single call vs manual loop)
4. It matches the approach used elsewhere in the codebase

## Test Results

### Before Fix
```bash
$ cargo test unit_test_corpus_matches_pandoc_markdown
# FAILED: pipe-table-code-span.qmd does not match pandoc
```

**Our output:**
```
[Plain [Space, Code ( "" , [] , [] ) "|"]]
```

**Pandoc output:**
```
[Plain [Code ( "" , [] , [] ) "|"]]
```

### After Fix
```bash
$ cargo test unit_test_corpus_matches_pandoc_markdown
# test unit_test_corpus_matches_pandoc_markdown ... ok
# test result: ok. 1 passed; 0 failed
```

**Our output now matches Pandoc exactly:**
```
[Plain [Code ( "" , [] , [] ) "|"]]
```

## Impact Analysis

### Tests Fixed
- ✅ `tests/pandoc-match-corpus/markdown/pipe-table-code-span.qmd` - now passes

### No Regressions
The change only affects pipe table cell processing via `process_pipe_table_cell`. All other uses of inline content are unaffected. The test suite shows no new failures from this change.

### Edge Cases Handled
- Empty cells with spaces → Trimmed to empty ✅
- Cells with code spans and delimiter spaces → Leading/trailing trimmed ✅
- Cells with multiple words → Middle spaces preserved ✅
- Cells with emphasis/strong → Leading/trailing trimmed ✅

## Pre-existing Test Failures
Note: The following tests were already failing before this fix (from earlier source map range work):
- test_html_writer
- test_json_writer
- test_qmd_roundtrip_consistency
- unit_test_snapshots_json
- unit_test_snapshots_native

These are unrelated to pipe table processing and existed before our change.

## Summary
Simple 2-line change that leverages existing, well-tested `trim_inlines` function to fix pipe table cell space handling. The fix is minimal, maintainable, and matches Pandoc behavior exactly.
