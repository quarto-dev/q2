# Plan: Fix div-whitespace Performance Problem

## Problem Statement

The `div-whitespace` rule is very slow compared to other rules in `qmd-syntax-helper`.

## Root Cause Analysis

### The Performance Bug

**File:** `crates/qmd-syntax-helper/src/conversions/div_whitespace.rs`

**Lines 82-86 (inside a loop):**
```rust
let line_start = content
    .lines()
    .take(line_idx)
    .map(|l| l.len() + 1) // +1 for newline
    .sum::<usize>();
```

**This code is O(N²)!**

### Why It's Slow

The function `find_div_whitespace_errors()` processes each error:
1. For each error, it checks 1-2 lines (lines 69-95)
2. For each line checked, it calculates `line_start` by:
   - **Re-iterating through the entire content from the beginning**
   - Calling `content.lines()` (creates a new iterator)
   - Taking the first `line_idx` lines
   - Summing their lengths

**Time Complexity:**
- If there are E errors and the file has N lines:
  - Outer loop: O(E) iterations
  - Inner loop (lines_to_check): O(2) = O(1)
  - **line_start calculation: O(line_idx)** ← This is the problem!

- For a file with errors throughout, this becomes **O(E × N)** or **O(N²)** in the worst case.

**Concrete Example:**
- File with 1000 lines
- 10 errors evenly distributed (lines 100, 200, 300, ..., 1000)
- Calculations:
  - Error 1: iterate through 100 lines
  - Error 2: iterate through 200 lines
  - Error 3: iterate through 300 lines
  - ...
  - Error 10: iterate through 1000 lines
  - **Total: 100 + 200 + ... + 1000 = 5,500 line iterations**

For a file with 10,000 lines and 100 errors, this could mean **500,000+ line iterations**!

### Why This Wasn't Noticed

Most test files are small, so the O(N²) behavior doesn't show up in testing. But on larger files (like the quarto-web docs), the performance degrades significantly.

## Solution

### Option A: Pre-compute Line Start Offsets

Calculate all line start positions once at the beginning of the function.

```rust
fn find_div_whitespace_errors(&self, content: &str, errors: &[...]) -> Vec<usize> {
    let mut fix_positions = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    // Pre-compute line start offsets (O(N) once)
    let mut line_starts = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        line_starts.push(offset);
        offset += line.len() + 1; // +1 for newline
    }

    for error in errors {
        // ... existing logic ...

        for &line_idx in &lines_to_check {
            if line_idx >= lines.len() {
                continue;
            }

            let line = lines[line_idx];
            let trimmed = line.trim_start();
            if let Some(after_colon) = trimmed.strip_prefix(":::") {
                if after_colon.starts_with('{') {
                    // Use pre-computed offset (O(1) lookup)
                    let line_start = line_starts[line_idx];
                    let indent_bytes = line.len() - trimmed.len();
                    let fix_pos = line_start + indent_bytes + 3;

                    fix_positions.push(fix_pos);
                    break;
                }
            }
        }
    }

    // ... rest of function ...
}
```

**Pros:**
- Simple change
- Reduces from O(N²) to O(N)
- Clear and understandable
- Minimal memory overhead (one Vec<usize>)

**Cons:**
- Uses extra memory for line_starts array
- Memory: O(N) for N lines

### Option B: Calculate Offset Incrementally

Track the offset as we iterate through errors, assuming errors are processed in line order.

**Cons:**
- Requires errors to be sorted by line number
- More complex logic
- Doesn't work well if errors jump around

### Option C: Use a Different Approach Entirely

Parse once, track byte offsets differently.

**Cons:**
- Major refactoring
- Not worth it for this specific issue

## Recommended Solution: Option A

Pre-computing line start offsets is the clear winner:
1. **Simple change** - just add offset calculation at the start
2. **Fixes the O(N²) problem** - reduces to O(N)
3. **Minimal memory cost** - one `usize` per line
4. **No logic changes** - the rest of the code stays the same

### Implementation Details

**File:** `crates/qmd-syntax-helper/src/conversions/div_whitespace.rs`

**Function:** `find_div_whitespace_errors()` (lines 42-103)

**Changes:**

1. **Add line start offset calculation after line 44:**
   ```rust
   let lines: Vec<&str> = content.lines().collect();

   // Pre-compute line start offsets for O(1) lookup
   let mut line_starts = Vec::with_capacity(lines.len());
   let mut offset = 0;
   for line in &lines {
       line_starts.push(offset);
       offset += line.len() + 1; // +1 for newline
   }
   ```

2. **Replace lines 82-86 with O(1) lookup:**
   ```rust
   // Calculate the position right after :::
   // We need byte offset, not char offset
   let line_start = line_starts[line_idx];
   ```

### Performance Impact

**Before:**
- For file with N lines and E errors: O(E × N) ≈ O(N²)
- Example: 1000 lines, 10 errors = ~5,500 line iterations

**After:**
- Pre-computation: O(N)
- Error processing: O(E × 1) = O(E)
- Total: O(N + E) ≈ O(N)
- Example: 1000 lines, 10 errors = 1000 + 10 = 1,010 operations

**Speedup:** ~5.5x for small files, potentially 100x+ for large files with many errors.

## Testing

After implementing the fix, test on:

1. **Small file with no errors** - ensure no regression
2. **Small file with errors** - ensure correct behavior
3. **Large file** - measure performance improvement
   - Try: `time cargo run --bin qmd-syntax-helper -- check --rule div-whitespace 'external-sites/**/*.qmd'`
   - Compare before/after timing

4. **Existing tests** - ensure all pass:
   ```bash
   cargo test --package qmd-syntax-helper
   ```

## Edge Cases

**Empty file:**
- `lines` will be empty
- `line_starts` will be empty
- No errors to process
- Should work fine ✓

**File with no newlines:**
- Single line, single entry in `line_starts`
- Works correctly ✓

**File ending without newline:**
- The last line won't have a +1 for newline, but that's OK
- We're only using these offsets for lines that actually exist
- The calculation is relative, not absolute
- Works correctly ✓

**Unicode/multi-byte characters:**
- We're using `.len()` which gives byte length (not char count)
- This is correct because we need byte offsets for string slicing
- Works correctly ✓

## Summary

The div-whitespace rule has an O(N²) performance bug caused by recalculating line start offsets for every error. The fix is to pre-compute these offsets once at the start of the function, reducing the complexity from O(N²) to O(N).

This is a simple, low-risk change with potentially dramatic performance improvements on large files.
