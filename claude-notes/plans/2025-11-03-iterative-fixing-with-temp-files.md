# Iterative Fixing with Temporary Files for qmd-syntax-helper

**Date**: 2025-11-03
**Issue**: test_div_whitespace_conversion failing due to parser only reliably detecting one error at a time
**Approach**: Global iteration using temporary files as working copies

---

## Problem Analysis

### Root Cause

The parser can only reliably report **one error at a time**. After encountering the first parse error, the parser enters error recovery mode and subsequent error reports have incorrect/unreliable locations.

**Example with multiple `:::{` errors:**

```
Initial state: :::{.class}\n...\n:::{#id}\n...\n:::{}

Parser encounters :::{.class}
├─ Reports: "Missing Space After Div Fence" at offset 16 ✓ (accurate)
├─ Gets confused, enters error recovery
└─ Reports: "Parse error" at offset 47 ✗ (wrong location)
    Reports: "Parse error" at offset 77 ✗ (wrong location)

After fixing :::{.class} → ::: {.class} and REPARSING
New state: ::: {.class}\n...\n:::{#id}\n...\n:::{}

Parser re-parses cleanly through first div
├─ Now encounters :::{#id}
└─ Reports: "Missing Space After Div Fence" at correct location ✓

After fixing :::{#id} → ::: {#id} and REPARSING
New state: ::: {.class}\n...\n::: {#id}\n...\n:::{}

Parser re-parses cleanly through both divs
└─ Now encounters :::{}
    Reports: Accurate error for this one ✓
```

**Key insight**: We need to **reparse after each fix** to get accurate error locations. This requires iteration.

### Current Architecture Limitation

Looking at `main.rs:179-224` and `div_whitespace.rs:284-322`, the current architecture has a critical limitation:

```rust
// Current flow
for file in files {
    for rule in rules {
        rule.convert(file_path, in_place, ...)?;  // Reads from disk
    }
}
```

**The problem**: Each `convert()` call **always reads from the original file path on disk**.

- If `in_place=true`: Changes are written back to disk, next iteration rereads ✓
- If `in_place=false`: Changes returned in `message`, but file on disk never changes ✗

**Result**: Iteration only works for `in_place=true` mode. Non-in-place mode would infinitely report the same error.

---

## Solution: Temporary File as Working Copy

### Core Idea

**Always work on a temporary copy of the file, regardless of mode:**

1. **Copy** original → temporary file
2. **Iterate** on temporary file (all rules, all passes, actual writes to temp)
3. **Finalize**:
   - If `--in-place`: Copy temp → original (atomic)
   - If not `--in-place`: Print temp to stdout
   - Original file only touched at the very end (or not at all)

### Why This Works

| Mode | Old Behavior | New Behavior |
|------|-------------|--------------|
| `--in-place` | Rules modify file directly | Rules modify temp, then temp → original |
| Not `--in-place` | Rules return content, no disk writes | Rules modify temp, print temp at end |
| `--check` | Rules simulate changes, no writes | Rules modify temp, discard temp at end |

**Key benefit**: Both in-place and non-in-place modes use **identical iteration logic** because both write to the temp file.

---

## Architecture Comparison

### Before (Current)

```
file1.qmd
    ↓
    ├─ Rule 1: convert(file1, in_place=true) → writes to file1
    ├─ Rule 2: convert(file1, in_place=true) → writes to file1
    └─ Done (each rule runs once)

file2.qmd
    ↓
    ├─ Rule 1: convert(file2, in_place=false) → returns content in message
    ├─ Rule 2: convert(file2, in_place=false) → returns content in message
    └─ Done (file2 never modified, can't iterate)
```

**Limitation**: Non-in-place mode can't iterate because file is never modified.

### After (Proposed)

```
file1.qmd
    ↓
    Copy to temp1.qmd
    ↓
    Iteration loop:
    ├─ Iteration 1:
    │   ├─ Rule 1: convert(temp1, in_place=true) → writes to temp1
    │   ├─ Rule 2: convert(temp1, in_place=true) → writes to temp1
    ├─ Iteration 2:
    │   ├─ Rule 1: convert(temp1, in_place=true) → writes to temp1
    │   └─ (converged)
    ↓
    If --in-place: copy temp1 → file1
    If not: print temp1 to stdout
    ↓
    Delete temp1
```

**Benefit**: Iteration works identically for both modes. Rules don't need to know about temp files.

---

## Benefits of Temporary File Approach

### 1. **Unified Iteration Logic**
- Both `--in-place` and non-in-place modes iterate the same way
- No special cases in iteration loop

### 2. **No Rule Changes Required**
- Rules still receive a file path
- Rules still read/write using existing logic
- Main.rs handles temp file lifecycle transparently

### 3. **Transactional Semantics**
- Original file only modified at the very end
- If any error occurs, original is untouched
- Temp file automatically cleaned up (even on panic)

### 4. **Check Mode Makes Sense**
- Check mode can iterate to find ALL potential fixes
- Shows: "Would apply 3 fixes across 3 iterations"
- Temp file discarded at end, original untouched
- Semantically correct behavior

### 5. **Clean Output**
- Non-in-place mode outputs final content **once** at the end
- No more triple-printing the entire file
- Verbose mode still shows iteration progress

### 6. **Error Safety**
- Using `tempfile` crate ensures cleanup on panic
- Permissions preserved when copying temp → original
- Atomic file operations where possible

### 7. **Future-Proof**
- Handles inter-rule dependencies naturally
- Easy to add per-file rollback in the future
- Supports future features like "dry-run with diff"

---

## Implementation Plan

### Phase 1: Add tempfile Dependency

**File**: `crates/qmd-syntax-helper/Cargo.toml`

```toml
[dependencies]
# ... existing dependencies ...
tempfile = "3.13"
```

**Rationale**: The `tempfile` crate provides:
- Automatic cleanup on drop (even on panic)
- Unique file naming (handles concurrent processes)
- Atomic persist operation (rename)

---

### Phase 2: Temporary File Infrastructure

**File**: `crates/qmd-syntax-helper/src/main.rs`

Add helper functions for temp file management:

```rust
use tempfile::NamedTempFile;
use std::path::{Path, PathBuf};

/// Create a temporary copy of a file in the same directory
fn create_temp_copy(file_path: &Path) -> Result<NamedTempFile> {
    // Create temp file in same directory as original
    let parent = file_path.parent().unwrap_or(Path::new("."));
    let temp = tempfile::Builder::new()
        .prefix(".qmd-syntax-helper.")
        .suffix(".tmp")
        .tempfile_in(parent)?;

    // Copy original content to temp
    let original_content = std::fs::read_to_string(file_path)?;
    std::fs::write(temp.path(), original_content)?;

    Ok(temp)
}

/// Finalize the temp file based on mode
fn finalize_temp_file(
    temp: NamedTempFile,
    original_path: &Path,
    in_place: bool,
    check_mode: bool,
) -> Result<()> {
    if check_mode {
        // Check mode: just drop temp (auto-deleted)
        drop(temp);
        return Ok(());
    }

    if in_place {
        // Preserve original permissions before persisting
        let metadata = std::fs::metadata(original_path)?;
        let permissions = metadata.permissions();
        std::fs::set_permissions(temp.path(), permissions)?;

        // Atomic rename temp → original
        temp.persist(original_path)?;
    } else {
        // Print final content to stdout
        let final_content = std::fs::read_to_string(temp.path())?;
        print!("{}", final_content);

        // Temp auto-deleted on drop
        drop(temp);
    }

    Ok(())
}
```

**Key decisions**:
- Temp file created in **same directory** as original (same filesystem, enables atomic rename)
- Prefix `.qmd-syntax-helper.` makes it clear what created the file
- Permissions copied from original before persisting
- Check mode simply drops temp file (no output, original untouched)

---

### Phase 3: Add CLI Arguments

**File**: `crates/qmd-syntax-helper/src/main.rs:49-69`

```rust
Convert {
    /// Glob patterns for files to convert
    files: Vec<String>,

    /// Rules to apply (defaults to all rules)
    #[arg(short, long)]
    rule: Vec<String>,

    /// Modify files in-place
    #[arg(short = 'i', long)]
    in_place: bool,

    /// Check mode: show what would be changed without modifying files
    #[arg(short, long)]
    check: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Maximum iterations for fixing (default: 10)
    #[arg(long, default_value = "10")]
    max_iterations: usize,

    /// Disable iterative fixing (run each rule once, like old behavior)
    #[arg(long)]
    no_iteration: bool,
}
```

**Rationale**:
- `--max-iterations`: Safety guard against infinite loops (default 10 is reasonable)
- `--no-iteration`: Backward compatibility for users who want old single-pass behavior
- Iteration **enabled by default** (correct behavior for most users)

---

### Phase 4: Refactor Convert Command with Iteration

**File**: `crates/qmd-syntax-helper/src/main.rs:179-224`

**Current code**:
```rust
Commands::Convert { files, rule: rule_names, in_place, check: check_mode, verbose } => {
    let file_paths = expand_globs(&files)?;
    let rules = resolve_rules(&registry, &rule_names)?;

    for file_path in file_paths {
        // ... apply each rule once
    }
}
```

**New code** (complete implementation):

```rust
Commands::Convert {
    files,
    rule: rule_names,
    in_place,
    check: check_mode,
    verbose,
    max_iterations,
    no_iteration,
} => {
    let file_paths = expand_globs(&files)?;
    let rules = resolve_rules(&registry, &rule_names)?;
    let max_iter = if no_iteration { 1 } else { max_iterations };

    for file_path in file_paths {
        if verbose {
            println!("Processing: {}", file_path.display());
        }

        // Create temporary working copy
        let temp_file = create_temp_copy(&file_path)?;
        let temp_path = temp_file.path().to_path_buf();

        // Iteration loop
        let mut iteration = 0;
        let mut total_fixes_for_file = 0;
        let mut prev_fixes = 0;
        let mut oscillation_count = 0;
        let mut show_iteration_details = false;

        loop {
            iteration += 1;
            let mut fixes_this_iteration = 0;

            // Apply all rules to temp file
            for rule in &rules {
                match rule.convert(&temp_path, true, check_mode, verbose) {
                    Ok(mut result) => {
                        if result.fixes_applied > 0 {
                            fixes_this_iteration += result.fixes_applied;
                            total_fixes_for_file += result.fixes_applied;

                            // Override file_path in result for user-facing reporting
                            result.file_path = file_path.to_string_lossy().to_string();

                            // Show rule progress
                            if verbose || check_mode {
                                let prefix = if show_iteration_details { "    " } else { "  " };
                                println!(
                                    "{}{} {} - {}",
                                    prefix,
                                    if check_mode { "Would fix" } else { "Fixed" },
                                    rule.name(),
                                    result.message.clone().unwrap_or_default()
                                );
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  {} Error converting {}: {}", "✗".red(), rule.name(), e);
                        drop(temp_file); // Clean up temp
                        return Err(e);
                    }
                }
            }

            // Check for convergence
            if fixes_this_iteration == 0 {
                if verbose && show_iteration_details {
                    println!("  Converged after {} iteration(s) ({} total fixes)",
                             iteration, total_fixes_for_file);
                }
                break;
            }

            // Oscillation detection
            if fixes_this_iteration == prev_fixes {
                oscillation_count += 1;
                if oscillation_count >= 2 {
                    eprintln!(
                        "  {} Warning: Possible oscillation detected (same fix count for {} consecutive iterations)",
                        "⚠".yellow(), oscillation_count + 1
                    );
                    eprintln!("  Stopping iteration to prevent infinite loop");
                    break;
                }
            } else {
                oscillation_count = 0;
            }
            prev_fixes = fixes_this_iteration;

            // Check max iterations
            if iteration >= max_iter {
                if !no_iteration {
                    eprintln!(
                        "  {} Warning: Reached max iterations ({}), but file may still have issues",
                        "⚠".yellow(), max_iter
                    );
                }
                break;
            }

            // From iteration 2 onwards, show detailed iteration info
            if iteration == 1 && !no_iteration {
                show_iteration_details = true;
                if verbose {
                    println!("  Iteration {}:", iteration);
                }
            }
            if iteration >= 2 && verbose {
                println!("  Iteration {}:", iteration);
            }
        }

        // Finalize: copy temp to original or print to stdout
        finalize_temp_file(temp_file, &file_path, in_place, check_mode)?;
    }

    Ok(())
}
```

**Key features**:

1. **Temp file lifecycle**:
   - Create temp copy at start
   - All rules work on temp (always `in_place=true`)
   - Finalize at end (persist or print or drop)

2. **Convergence detection**:
   - Stop when `fixes_this_iteration == 0`
   - No more fixes found = converged

3. **Oscillation detection**:
   - If same fix count for 2+ consecutive iterations, warn and stop
   - Prevents infinite loops from inter-rule conflicts

4. **Max iterations guard**:
   - Hard limit (default 10)
   - Warn if reached without convergence

5. **Verbose output**:
   - Only show "Iteration N:" if actually iterating multiple times
   - If converges in 1 pass, output looks like old behavior
   - Clear progress reporting

6. **File path fixup**:
   - Rules report temp path in `ConvertResult`
   - Override to show original path to user
   - User never sees temp file paths

---

### Phase 5: Improve Verbose Output

**Smart iteration display**:

```rust
// Only show iteration details if we actually iterate > 1 time
let mut show_iteration_details = false;

// ... in loop ...
if iteration == 2 && !no_iteration {
    // We're entering iteration 2, so now show details
    show_iteration_details = true;
    // Retroactively label iteration 1
    if verbose {
        println!("  Iteration 1:");
        println!("    ... (show iteration 1 fixes here)");
    }
}
```

**Example outputs**:

**Case 1: Single iteration (converges immediately)**
```
Processing: test.qmd
  ✓ div-whitespace - Fixed 2 div fence(s)
```

**Case 2: Multiple iterations (shows detail)**
```
Processing: test.qmd
  Iteration 1:
    ✓ div-whitespace - Fixed 1 div fence(s)
  Iteration 2:
    ✓ div-whitespace - Fixed 1 div fence(s)
  Iteration 3:
    ✓ div-whitespace - Fixed 1 div fence(s)
  Converged after 3 iterations (3 total fixes)
```

**Case 3: Check mode**
```
Processing: test.qmd
  Iteration 1:
    Would fix div-whitespace - 1 div fence(s)
  Iteration 2:
    Would fix div-whitespace - 1 div fence(s)
  Converged after 2 iterations (2 total fixes)
  (No changes made - check mode)
```

---

## Detailed Edge Cases

### 1. **File with no issues**
```rust
// Iteration 1: 0 fixes → converge immediately
// Output: (nothing, or "No issues found")
```

### 2. **File with 1 issue**
```rust
// Iteration 1: 1 fix → reparse
// Iteration 2: 0 fixes → converge
// Output shows: "Fixed X - Y" (no iteration detail)
```

### 3. **File with multiple issues requiring iteration**
```rust
// Iteration 1: 1 fix
// Iteration 2: 1 fix
// Iteration 3: 1 fix
// Iteration 4: 0 fixes → converge
// Output shows full iteration detail
```

### 4. **Oscillating rules (A↔B)**
```rust
// Iteration 1: 2 fixes (1 from A, 1 from B)
// Iteration 2: 2 fixes (same pattern)
// Iteration 3: 2 fixes (same pattern)
// Oscillation detected → warn and stop
```

### 5. **Check mode with multiple files**
```rust
Processing: file1.qmd
  Would fix div-whitespace - 2 issues
Processing: file2.qmd
  Iteration 1:
    Would fix div-whitespace - 1 issue
  Iteration 2:
    Would fix div-whitespace - 1 issue
  Converged after 2 iterations (2 total fixes)
  (No changes made - check mode)
```

### 6. **Non-in-place mode**
```rust
// All iteration happens on temp file
// At end: print entire final temp file to stdout (once)
// No triple-printing issue
```

### 7. **Multiple rules interaction**
```rust
// Rule A fixes divs
// Rule B fixes tables
// Rule A's fixes might reveal new table issues
// Iteration handles this naturally
```

### 8. **Error during iteration**
```rust
// Iteration 1: success
// Iteration 2: Rule X throws error
// → Temp file dropped (auto-cleanup)
// → Original file untouched
// → Clean error state
```

### 9. **Glob with no matches**
```rust
// expand_globs returns empty vec
// No files processed
// No error (same as current behavior)
```

### 10. **Symlinks**
```rust
// std::fs::read_to_string follows symlinks
// We read/write the target file
// Current behavior preserved
// (Could enhance later to preserve symlink structure)
```

### 11. **Read-only files**
```rust
// create_temp_copy: succeeds (temp is writable)
// finalize_temp_file: fails on persist (original is read-only)
// Error reported, temp cleaned up
// Same behavior as current code attempting to write
```

### 12. **Concurrent invocations**
```rust
// tempfile crate ensures unique temp file names
// Multiple processes can work on same file safely
// Last one to finish wins (same as current race condition)
```

---

## Testing Strategy

### Unit Tests (in `src/main.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_temp_copy() {
        // Create temp file, verify content matches original
    }

    #[test]
    fn test_finalize_in_place() {
        // Modify temp, finalize with in_place=true
        // Verify original is updated
    }

    #[test]
    fn test_finalize_not_in_place() {
        // Modify temp, finalize with in_place=false
        // Verify original is unchanged
    }

    #[test]
    fn test_finalize_check_mode() {
        // Modify temp, finalize with check_mode=true
        // Verify original is unchanged, temp is cleaned
    }

    #[test]
    fn test_permissions_preserved() {
        // Set specific permissions on original
        // Run conversion
        // Verify permissions preserved
    }
}
```

### Integration Tests

#### Test 1: Existing test should now pass

**File**: `crates/qmd-syntax-helper/tests/div_whitespace_test.rs`

The `test_div_whitespace_conversion` test should now pass without modification, because iteration will fix all 3 issues.

```rust
#[test]
fn test_div_whitespace_conversion() {
    // File with 3 div whitespace errors
    // Run convert --in-place
    // Verify all 3 are fixed (requires 3 iterations)
    // This test should now PASS
}
```

#### Test 2: Non-in-place iteration

**File**: `crates/qmd-syntax-helper/tests/iteration_test.rs` (new)

```rust
#[test]
fn test_non_in_place_iteration() {
    // File with 3 errors requiring 3 iterations
    // Run convert WITHOUT --in-place
    // Verify:
    //   - Original file is unchanged
    //   - Output to stdout contains all fixes
    //   - No temp files left behind
}
```

#### Test 3: Check mode iteration

```rust
#[test]
fn test_check_mode_iteration() {
    // File with multiple errors
    // Run convert --check
    // Verify:
    //   - Original file unchanged
    //   - Output shows all potential fixes
    //   - Reports correct iteration count
}
```

#### Test 4: Convergence on first pass

```rust
#[test]
fn test_immediate_convergence() {
    // File with 0 or 1 errors (fixes in one pass)
    // Run convert
    // Verify:
    //   - No "Iteration N:" output
    //   - Clean simple output
    //   - Only 1 iteration occurred
}
```

#### Test 5: Max iterations limit

```rust
#[test]
fn test_max_iterations() {
    // Mock file that would need > 10 iterations
    // Run convert with --max-iterations 3
    // Verify:
    //   - Stops after 3 iterations
    //   - Warning emitted
    //   - Partial fixes applied
}
```

#### Test 6: No-iteration flag

```rust
#[test]
fn test_no_iteration_flag() {
    // File with 3 errors
    // Run convert --no-iteration
    // Verify:
    //   - Only first error fixed (single pass)
    //   - No iteration occurred
    //   - Backward compatibility preserved
}
```

#### Test 7: Oscillation detection

```rust
#[test]
fn test_oscillation_detection() {
    // Mock scenario: Rule A undoes Rule B's fixes
    // Run convert
    // Verify:
    //   - Oscillation warning emitted
    //   - Stops after detecting oscillation
    //   - Doesn't run full max_iterations
}
```

#### Test 8: Multiple rules interaction

```rust
#[test]
fn test_multiple_rules_iteration() {
    // File needing fixes from multiple rules
    // Rule A's fixes reveal issues for Rule B
    // Run convert with both rules
    // Verify:
    //   - Both rules' fixes applied
    //   - Correct total iteration count
    //   - Converges correctly
}
```

#### Test 9: Error handling

```rust
#[test]
fn test_error_during_iteration() {
    // Mock rule that fails on iteration 2
    // Run convert
    // Verify:
    //   - Original file unchanged
    //   - Error reported
    //   - Temp file cleaned up
}
```

#### Test 10: Permissions preservation

```rust
#[test]
#[cfg(unix)]
fn test_permissions_preserved() {
    // Set file to mode 0o644
    // Run convert --in-place
    // Verify file still has mode 0o644
}
```

#### Test 11: Multiple files

```rust
#[test]
fn test_multiple_files_independent() {
    // File1: needs 1 iteration
    // File2: needs 3 iterations
    // Run convert with glob
    // Verify:
    //   - Each file processed independently
    //   - Correct iteration count per file
    //   - All files fixed
}
```

#### Test 12: Glob patterns

```rust
#[test]
fn test_glob_patterns() {
    // Create test files: test1.qmd, test2.qmd, test3.md
    // Run convert with pattern "*.qmd"
    // Verify:
    //   - Only .qmd files processed
    //   - Glob expansion works correctly
}
```

---

## Migration Considerations

### Backward Compatibility

1. **Default behavior changes**:
   - OLD: Each rule runs once
   - NEW: Iteration enabled by default
   - **Migration**: Users wanting old behavior use `--no-iteration`

2. **Output format**:
   - OLD: Simple "Fixed X - Y" messages
   - NEW: May show "Iteration N:" for multi-pass fixes
   - **Impact**: Scripts parsing output may need updates
   - **Mitigation**: Only show iteration details when > 1 iteration

3. **Performance**:
   - NEW: May be slower for files with many issues (multiple reparses)
   - **Mitigation**: `--no-iteration` for performance-critical use cases
   - **Expectation**: Most files converge quickly (1-3 iterations)

### Deployment Strategy

1. **Phase 1**: Implement and test thoroughly
2. **Phase 2**: Beta test with known problematic files
3. **Phase 3**: Document new flags in `--help`
4. **Phase 4**: Update user-facing docs with examples
5. **Phase 5**: Release with clear changelog notes

### Documentation Updates

#### CLI Help Text

```
USAGE:
    qmd-syntax-helper convert [OPTIONS] <FILES>...

OPTIONS:
    -i, --in-place              Modify files in-place
    -c, --check                 Check mode: show what would be changed
    -v, --verbose               Verbose output
    --max-iterations <N>        Maximum iterations for fixing [default: 10]
    --no-iteration              Disable iterative fixing (old behavior)
    -r, --rule <RULE>           Rules to apply [default: all rules]

EXAMPLES:
    # Fix all issues in a file (iterates until all issues resolved)
    qmd-syntax-helper convert --in-place test.qmd

    # Check what would be fixed without modifying the file
    qmd-syntax-helper convert --check test.qmd

    # Limit iterations (for performance)
    qmd-syntax-helper convert --in-place --max-iterations 3 test.qmd

    # Old single-pass behavior (backward compatibility)
    qmd-syntax-helper convert --in-place --no-iteration test.qmd

    # Convert and output to stdout (useful for pipes)
    qmd-syntax-helper convert test.qmd > output.qmd
```

#### User Guide Section

```markdown
## How Iteration Works

qmd-syntax-helper uses an iterative approach to fix issues in your files:

1. **First pass**: The tool finds and fixes the first issue it encounters
2. **Reparse**: The file is re-parsed with the fix applied
3. **Next pass**: The parser can now accurately detect the next issue
4. **Repeat**: This continues until no more issues are found

This approach is necessary because the parser can only reliably detect
one error at a time. After encountering an error, subsequent error
locations may be inaccurate until the file is re-parsed.

### Example

A file with three div syntax errors:
```
:::{.class}        # Error 1
content
:::

:::{#id}           # Error 2
content
:::

:::{}              # Error 3
content
:::
```

The tool will:
- Iteration 1: Fix `:::{.class}` → `::: {.class}`
- Iteration 2: Fix `:::{#id}` → `::: {#id}`
- Iteration 3: Fix `:::{}` → `::: {}`
- Converge: No more issues found

### Performance

Most files converge in 1-3 iterations. Files with many issues may
require more iterations. Use `--max-iterations` to limit this if needed.

For the old single-pass behavior, use `--no-iteration`.
```

---

## Open Questions & Design Decisions

### Decisions Made

1. **Iteration enabled by default** ✓
   - Correct behavior for most users
   - Old behavior available via `--no-iteration`

2. **Max iterations: 10** ✓
   - Reasonable default
   - Configurable via `--max-iterations`
   - Prevents runaway loops

3. **Oscillation detection: 2 consecutive iterations** ✓
   - If same fix count for 2+ iterations, likely oscillating
   - Warn and stop early (don't wait for max_iterations)

4. **Temp file in same directory** ✓
   - Enables atomic rename (same filesystem)
   - Inherits same disk quota/permissions context

5. **Check mode iterates** ✓
   - Shows all potential fixes (useful)
   - Temp file discarded at end
   - Semantically correct

6. **Smart iteration display** ✓
   - Only show "Iteration N:" if actually iterating > 1 time
   - Cleaner output for simple cases

7. **Preserve permissions** ✓
   - Copy permissions from original to temp before persisting
   - Unix: works automatically
   - Windows: best-effort

### Open Questions

1. **Should we show a summary for non-verbose mode when > 3 iterations?**
   - Pro: User knows something complex happened
   - Con: Extra output clutter
   - **Recommendation**: Show if verbose OR if > 3 iterations

2. **Should we cache parse results within an iteration?**
   - Currently: Each rule reparses the file
   - Alternative: Parse once per iteration, share results
   - **Recommendation**: Start simple, optimize later if needed

3. **Should we add a `--diff` mode showing before/after?**
   - Use case: Preview changes
   - Implementation: Keep original, compare with temp
   - **Recommendation**: Future enhancement

4. **Should oscillation threshold be configurable?**
   - Currently: Hardcoded to 2
   - Alternative: `--oscillation-threshold <N>`
   - **Recommendation**: Not needed initially, hardcoded 2 is fine

5. **Should we track and report iteration count in non-verbose mode?**
   - Currently: Only shown in verbose mode
   - Alternative: Always show summary
   - **Recommendation**: Only show if > 1 iteration occurred

6. **Symlink handling**:
   - Currently: Follow symlinks (modify target)
   - Alternative: Preserve symlink structure
   - **Recommendation**: Document current behavior, enhance later if needed

---

## Implementation Checklist

### Code Changes
- [ ] Add `tempfile = "3.13"` to Cargo.toml
- [ ] Add `create_temp_copy()` helper function
- [ ] Add `finalize_temp_file()` helper function
- [ ] Add CLI arguments: `max_iterations`, `no_iteration`
- [ ] Refactor `Commands::Convert` to use temp file + iteration
- [ ] Add oscillation detection logic
- [ ] Add smart iteration display logic
- [ ] Fix file path in `ConvertResult` for user-facing output
- [ ] Add permission preservation before persist

### Testing
- [ ] Verify `test_div_whitespace_conversion` now passes
- [ ] Write `test_non_in_place_iteration`
- [ ] Write `test_check_mode_iteration`
- [ ] Write `test_immediate_convergence`
- [ ] Write `test_max_iterations`
- [ ] Write `test_no_iteration_flag`
- [ ] Write `test_oscillation_detection`
- [ ] Write `test_multiple_rules_iteration`
- [ ] Write `test_error_during_iteration`
- [ ] Write `test_permissions_preserved` (Unix)
- [ ] Write `test_multiple_files_independent`
- [ ] Write `test_glob_patterns`
- [ ] Manual testing with large files
- [ ] Manual testing with edge cases (empty file, no issues, etc.)

### Documentation
- [ ] Update `--help` text for new flags
- [ ] Add examples to `--help`
- [ ] Write user guide section on iteration
- [ ] Add changelog entry
- [ ] Update any existing docs mentioning convert behavior

### Performance & Quality
- [ ] Benchmark: before/after on representative files
- [ ] Verify no temp files left behind after errors
- [ ] Test on Windows (permission handling)
- [ ] Test on macOS (permissions, symlinks)
- [ ] Test on Linux (permissions, symlinks)
- [ ] Code review focusing on error handling paths

---

## Success Criteria

1. **test_div_whitespace_conversion passes** (primary goal)
2. **All new tests pass**
3. **No regressions in existing functionality**
4. **Performance acceptable** (< 2x slowdown for typical files)
5. **Clean error handling** (no leaked temp files)
6. **Clear user-facing output** (iteration details when relevant)
7. **Backward compatibility** (`--no-iteration` provides old behavior)
8. **Documentation complete** (help text, examples, user guide)

---

## Related Issues

- Test failure: `test_div_whitespace_conversion` in `crates/qmd-syntax-helper/tests/div_whitespace_test.rs`
- Root cause: Parser error recovery makes subsequent error locations unreliable after first error
- Solution: Iterate (fix → reparse → fix) until convergence
- Implementation: Temporary file approach enables iteration for all modes

---

## Notes

### Why Temporary Files Are Essential

The key architectural insight: **rules must write to disk to trigger reparsing**.

- Each `convert()` call reads from disk via `read_file(file_path)`
- Each `convert()` call writes to disk via `write_file(file_path, ...)`
- Reparsing happens on next `convert()` call when it reads the updated file

Without temp files:
- `in_place=false` mode never writes to disk
- File never changes between iterations
- Parser reads same content every time
- Infinite loop (same error every iteration)

With temp files:
- Both modes write to temp file on disk
- File changes between iterations
- Parser reads updated content
- Iteration works correctly

### Why This Is Better Than In-Memory Threading

Alternative approach: Refactor all rules to operate on `&str` instead of `&Path`.

Problems:
- Requires changing `Rule` trait (breaking change)
- Requires rewriting all rules (large refactor)
- Parse errors need file paths for error reporting
- Some rules might depend on file metadata

Temp file approach:
- No rule changes needed (use existing interface)
- Parse errors work correctly (real file paths)
- File metadata available (permissions, etc.)
- Minimal changes to existing code

### Similarity to Other Tools

This pattern (iterate until stable) is used by:
- **Prettier**: `--write` may take multiple passes to stabilize
- **ESLint**: `--fix` may iterate for interdependent rules
- **rustfmt**: May reformat multiple times for complex macros

Industry-standard approach for tools dealing with unreliable single-pass behavior.
