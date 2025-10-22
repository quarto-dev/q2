# Plan: Fix qmd-syntax-helper Summary File Count Bug

## Problem Statement

When running `qmd-syntax-helper check` on multiple files, the summary reports an incorrect file count. It only counts files that have issues, not all files checked.

**Example:**
```bash
$ cargo run --bin qmd-syntax-helper -- check '/Users/cscheid/Desktop/daily-log/2025/10/21/*.qmd'
```

**Files in directory:**
- meta-test-2.qmd (parses successfully)
- meta-test.qmd (parses successfully)
- parse-error.qmd (fails to parse)

**Expected summary:**
```
Total files:         3
Files with issues:   1 ✗
Clean files:         2 ✓
```

**Actual summary:**
```
Total files:         1  <-- WRONG!
Files with issues:   1 ✗
Clean files:         0 ✓
```

The verbose output shows all 3 files are being checked correctly, but the summary only counts 1 file.

## Root Cause Analysis

### Data Flow

1. **File expansion** (`main.rs` line 87):
   - `expand_globs(&files)` correctly returns all 3 file paths

2. **File checking loop** (`main.rs` lines 92-118):
   - Iterates over all 3 files
   - For each file, runs all rules
   - Each rule returns `Vec<CheckResult>`
   - Only results are added to `all_results`

3. **Rule behavior** (e.g., `parse_check.rs` lines 45-46):
   ```rust
   if parses {
       Ok(vec![])  // ← Empty vector for files that parse successfully!
   } else {
       Ok(vec![CheckResult { ... }])
   }
   ```

4. **Summary calculation** (`main.rs` line 221):
   ```rust
   let unique_files: HashSet<&str> = results.iter().map(|r| r.file_path.as_str()).collect();
   let total_files = unique_files.len();
   ```

### The Bug

**`all_results` only contains CheckResult entries where rules found something.**

- meta-test-2.qmd: Parses OK → all rules return `vec![]` → **0 entries in all_results**
- meta-test.qmd: Parses OK → all rules return `vec![]` → **0 entries in all_results**
- parse-error.qmd: Parse fails → parse rule returns `vec![CheckResult]` → **1 entry in all_results**

Therefore:
- `all_results.len() = 1`
- `unique_files.len() = 1` (only "parse-error.qmd")
- `total_files = 1` ❌

## Solution Options

### Option A: Track Files Separately (Recommended)

Change the main loop to track all files checked, independent of results.

**Pros:**
- Clean separation: files checked vs. results found
- Accurate count always
- Simple to implement

**Cons:**
- Need to pass additional data to `print_check_summary()`

**Implementation:**
```rust
// In main.rs, Commands::Check branch
let file_paths = expand_globs(&files)?;
let files_checked = file_paths.clone(); // Track all files

// ... existing checking loop ...

// Print summary with file count
if !json && !all_results.is_empty() {
    print_check_summary(&all_results, files_checked.len());
}
```

Update `print_check_summary`:
```rust
fn print_check_summary(results: &[rule::CheckResult], total_files: usize) {
    // ... existing logic but use `total_files` parameter instead of unique_files.len()
}
```

### Option B: Make Rules Always Return Results

Change all rules to return a `CheckResult` even when no issues found.

**Pros:**
- Consistent - every file gets an entry
- No need to change summary logic

**Cons:**
- Verbose - lots of "no issue" results
- Breaks existing behavior
- More memory usage
- JSON output becomes huge

**Not recommended.**

### Option C: Count Files in Loop

Add file counting directly in the main loop.

**Pros:**
- Simple
- Minimal changes

**Cons:**
- Duplicates logic (tracking in loop AND in summary)

**Implementation:**
```rust
let mut files_checked = 0;
for file_path in file_paths {
    files_checked += 1;
    // ... existing loop
}

if !json && !all_results.is_empty() {
    print_check_summary(&all_results, files_checked);
}
```

## Recommended Solution: Option A

Track file count separately and pass it to summary function.

### Implementation Steps

1. **Store file paths count** before the loop (line 87-88)
2. **Pass count to summary** (line 122)
3. **Update print_check_summary signature** (line 217)
4. **Use passed count instead of deriving from results** (line 222)

### Changes Required

**File:** `crates/qmd-syntax-helper/src/main.rs`

**Change 1:** Track file count (around line 87-92)
```rust
let file_paths = expand_globs(&files)?;
let total_files_checked = file_paths.len(); // NEW
let rules = resolve_rules(&registry, &rule_names)?;

let mut all_results = Vec::new();
```

**Change 2:** Pass count to summary (line 121-122)
```rust
// Print summary if not in JSON mode
if !json {
    print_check_summary(&all_results, total_files_checked); // UPDATED
}
```

**Change 3:** Update function signature (line 217)
```rust
fn print_check_summary(results: &[rule::CheckResult], total_files: usize) { // UPDATED
```

**Change 4:** Use parameter instead of deriving (lines 221-222)
```rust
// Remove this line:
// let unique_files: HashSet<&str> = results.iter().map(|r| r.file_path.as_str()).collect();
// let total_files = unique_files.len();

// total_files is now a parameter - use it directly
```

### Edge Cases

1. **No files match glob:** `total_files_checked = 0` → Summary shows "Total files: 0"
2. **All files clean:** Works correctly now (shows actual file count)
3. **All files have issues:** Works same as before
4. **Empty results:** Currently skips summary (`if !all_results.is_empty()`), but we might want to show summary even with 0 issues?

### Additional Improvement (Optional)

Change line 121 to show summary even when all files are clean:

```rust
// OLD:
if !json && !all_results.is_empty() {
    print_check_summary(&all_results, total_files_checked);
}

// NEW: Show summary even when everything is clean
if !json {
    print_check_summary(&all_results, total_files_checked);
}
```

This way users see:
```
=== Summary ===
Total files:         3
Files with issues:   0 ✓
Clean files:         3 ✓

Total issues found:  0
Success rate:        100.0%
```

Even when there are no issues (which is good feedback!).

## Testing

### Test Case 1: Mixed Results (Current Failing Case)
```bash
cargo run --bin qmd-syntax-helper -- check '/Users/cscheid/Desktop/daily-log/2025/10/21/*.qmd'
```

**Expected:**
```
Total files:         3
Files with issues:   1 ✗
Clean files:         2 ✓
```

### Test Case 2: All Clean
```bash
cargo run --bin qmd-syntax-helper -- check 'some/clean/files/*.qmd'
```

**Expected:**
```
Total files:         5
Files with issues:   0 ✓
Clean files:         5 ✓
```

### Test Case 3: All Have Issues
```bash
cargo run --bin qmd-syntax-helper -- check 'some/broken/files/*.qmd'
```

**Expected:**
```
Total files:         2
Files with issues:   2 ✗
Clean files:         0 ✓
```

## Summary

The bug is in `print_check_summary()` which counts files from `results`, but `results` only contains entries for files with issues. The fix is to track the total file count separately and pass it to the summary function.

**Recommended: Option A** - Track file count separately, pass to summary.
