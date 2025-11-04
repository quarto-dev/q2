# Pipe Table Cell Space Trimming - Investigation and Plan

## Date: 2025-11-03

## Issue
Test `tests/pandoc-match-corpus/markdown/pipe-table-code-span.qmd` fails because pipe table cells have leading spaces that Pandoc doesn't include.

## Root Cause Analysis

### The Problem
Input:
```markdown
| a | b |
|---|---|
| `|` | oh no |
```

**Pandoc output** for first data cell:
```
[ Plain [ Code ( "" , [] , [] ) "|" ] ]
```

**Our parser output** for first data cell:
```
[Plain [Space, Code ( "" , [] , [] ) "|"]]
```

There's an extra `Space` before the `Code` element.

### Why the Space Exists

1. **Grammar captures delimiter spaces**: The tree-sitter grammar correctly identifies that the code span delimiter includes a leading space:
   ```
   pipe_table_cell: {Node pipe_table_cell (2, 1) - (2, 6)}
     pandoc_code_span: {Node pandoc_code_span (2, 1) - (2, 5)}
       code_span_delimiter: {Node code_span_delimiter (2, 1) - (2, 3)}  # " `" (space + backtick)
       content: {Node content (2, 3) - (2, 4)}                          # "|"
       code_span_delimiter: {Node code_span_delimiter (2, 4) - (2, 5)}  # "`"
     pandoc_space: {Node pandoc_space (2, 5) - (2, 6)}
   ```

2. **Code span processor emits Space nodes**: The `process_pandoc_code_span` function in `code_span_helpers.rs` (lines 104-108) intentionally creates Space inline elements when delimiters contain spaces:
   ```rust
   if has_leading_space {
       result.push(Inline::Space(Space {
           source_info: node_source_info_with_context(node, context),
       }));
   }
   ```

3. **Pipe table processor only trims trailing spaces**: The `process_pipe_table_cell` function in `pipe_table.rs` (lines 132-139) manually removes trailing spaces:
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
   But it doesn't remove leading spaces!

## Why This Is Expected Behavior (Outside Tables)

The grammar and code span processor correctly handle spaces in delimiters. This is analogous to how emphasis/strong delimiters work (e.g., `* * bold * *` with spaces).

For regular paragraphs, this is correct behavior. But in pipe table cells, Pandoc strips both leading AND trailing spaces.

## Solution: Use Existing trim_inlines Function

There's already a perfect function for this in `postprocess.rs`:

```rust
/// Trim leading and trailing spaces from inlines
pub fn trim_inlines(inlines: Inlines) -> (Inlines, bool) {
    let mut result: Inlines = Vec::new();
    let mut at_start = true;
    let mut space_run: Inlines = Vec::new();
    let mut changed = false;
    for inline in inlines {
        match &inline {
            Inline::Space(_) if at_start => {
                // skip leading spaces
                changed = true;
                continue;
            }
            Inline::Space(_) => {
                // collect spaces
                space_run.push(inline);
                continue;
            }
            _ => {
                result.extend(space_run.drain(..));
                result.push(inline);
                at_start = false;
            }
        }
    }
    if space_run.len() > 0 {
        changed = true;
    }
    (result, changed)
}
```

This function:
- ✅ Strips leading spaces
- ✅ Strips trailing spaces
- ✅ Preserves spaces in the middle
- ✅ Already used throughout the codebase

## Implementation Plan

### Step 1: Import trim_inlines
In `pipe_table.rs`, add import:
```rust
use crate::pandoc::treesitter_utils::postprocess::trim_inlines;
```

### Step 2: Replace Manual Trailing Trim with trim_inlines
In `process_pipe_table_cell` function (around line 132), replace:
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

With:
```rust
// Trim leading and trailing spaces from cell content to match Pandoc behavior
plain_content = trim_inlines(plain_content).0;
```

### Step 3: Test
Run the failing test:
```bash
cargo test unit_test_corpus_matches_pandoc_markdown
```

### Step 4: Verify Other Tests Still Pass
```bash
cargo test
```

## Expected Impact

### Tests That Will Be Fixed
- `tests/pandoc-match-corpus/markdown/pipe-table-code-span.qmd`
- Possibly other pipe table tests with spaces in cells

### Potential Regressions
Unlikely, because:
1. We're replacing less comprehensive manual trimming with more comprehensive automatic trimming
2. The `trim_inlines` function is already widely used in the codebase
3. Pandoc's behavior is to trim both leading and trailing spaces in table cells

## Files to Modify

1. **crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/pipe_table.rs**
   - Add import for `trim_inlines`
   - Replace manual trailing space trimming with `trim_inlines` call (around line 132)

## Edge Cases to Consider

1. **Empty cells**: Should work fine - trimming an empty Vec returns empty Vec
2. **Cells with only spaces**: Will be trimmed to empty, matching Pandoc
3. **Cells with code spans**: Will have leading/trailing spaces properly trimmed (the failing case)
4. **Cells with emphasis/strong**: Will have leading/trailing spaces properly trimmed
5. **Spaces between words**: Will be preserved (trim_inlines keeps middle spaces)

## Test Case Analysis

Input: `| `|` |`
- Grammar parses: Space + Code + Space
- Current behavior: Outputs Space + Code
- After fix: Will output just Code ✅ (matches Pandoc)

Input: `| a b |`
- Grammar parses: Str + Space + Str
- Current behavior: Outputs Str + Space + Str (correct)
- After fix: Same (middle spaces preserved) ✅

Input: `|  |` (empty cell with spaces)
- Grammar parses: Space + Space
- Current behavior: Outputs Space (one trailing space removed)
- After fix: Will output nothing ✅ (matches Pandoc)
