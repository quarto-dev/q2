# Plan: Improve qmd-syntax-helper Output - Show Filename Context

## Problem Statement

When running `qmd-syntax-helper check` in non-verbose mode, issues are printed without showing which file they're from.

**Example:**
```bash
$ cargo run --bin qmd-syntax-helper -- check --rule definition-lists 'external-sites/**/*.qmd'
  ✗ Definition list found
  ✗ Definition list found
  ✗ Definition list found
  ...
```

**User sees:** List of issues with no file context
**User needs:** File name before each issue or group of issues

## Current Behavior Analysis

### Code Flow (main.rs lines 92-119)

```rust
for file_path in file_paths {
    // Only print filename in verbose mode
    if verbose && !json {
        println!("Checking: {}", file_path.display());
    }

    for rule in &rules {
        match rule.check(&file_path, verbose && !json) {
            Ok(results) => {
                for result in results {
                    all_results.push(result.clone());
                    // Print issue immediately (no file context!)
                    if !json && result.has_issue {
                        println!("  {} {}", "✗".red(), result.message.unwrap_or_default());
                    }
                }
            }
            ...
        }
    }
}
```

**Problem:**
- Line 94-96: Filename only printed if `verbose`
- Line 103-108: Issue printed immediately, even when non-verbose
- Result: Issues without context in non-verbose mode

## Desired Behavior

**In verbose mode:** Print filename for every file checked
```
Checking: file1.qmd
  No definition lists found
Checking: file2.qmd
  ✗ Definition list found
  ✗ Definition list found
Checking: file3.qmd
  No definition lists found
```

**In non-verbose mode:** Print filename only for files with issues
```
external-sites/quarto-web/docs/website-navigation.qmd
  ✗ Definition list found
  ✗ Definition list found

external-sites/quarto-web/docs/other-file.qmd
  ✗ Definition list found

=== Summary ===
...
```

## Solution Options

### Option A: Buffer Results, Print Grouped by File

Collect all results for a file before printing anything, then decide whether to print.

**Approach:**
1. Check all rules for a file
2. Collect results in a buffer
3. If buffer has issues OR verbose mode:
   - Print filename
   - Print all issues
4. Add to `all_results` regardless

**Pros:**
- Clean grouped output
- Only prints files with issues in non-verbose
- Shows all issues for a file together

**Cons:**
- Changes output order slightly (groups by file instead of interleaving)
- Need to buffer results per file

### Option B: Track "Printed Filename" Flag

Use a flag to track whether we've printed the filename yet for this file.

**Approach:**
1. Start with `printed_filename = false`
2. When finding first issue:
   - If not verbose and not printed_filename: print filename
   - Set printed_filename = true
   - Print issue
3. Continue printing subsequent issues

**Pros:**
- Prints filename immediately before first issue
- Minimal code change
- Preserves issue order

**Cons:**
- If verbose mode, prints filename twice (once at start, once before first issue)
- More complex flag tracking

### Option C: Separate Verbose and Non-Verbose Paths (Recommended)

Simplify logic by handling verbose and non-verbose modes differently.

**Approach:**

**Verbose mode (current behavior):**
```rust
if verbose && !json {
    println!("Checking: {}", file_path.display());
}
// ... check and print immediately ...
```

**Non-verbose mode:**
```rust
if !verbose && !json {
    // Buffer results for this file
    let file_results = collect_all_results_for_file();

    // Only print if there are issues
    if file_results.iter().any(|r| r.has_issue) {
        println!("{}", file_path.display());
        for result in file_results {
            if result.has_issue {
                println!("  {} {}", "✗".red(), result.message.unwrap_or_default());
            }
        }
        println!(); // Blank line between files
    }
}
```

**Pros:**
- Clear separation of concerns
- Each mode optimized for its use case
- No flag tracking

**Cons:**
- Code duplication (but manageable)

## Recommended Implementation: Option C

### Changes Required

**File:** `crates/qmd-syntax-helper/src/main.rs`

**Current structure (lines 92-119):**
```rust
for file_path in file_paths {
    if verbose && !json {
        println!("Checking: {}", file_path.display());
    }

    for rule in &rules {
        match rule.check(&file_path, verbose && !json) {
            Ok(results) => {
                for result in results {
                    all_results.push(result.clone());
                    if !json && result.has_issue {
                        println!("  {} {}", "✗".red(), result.message.unwrap_or_default());
                    }
                }
            }
            Err(e) => { ... }
        }
    }
}
```

**New structure:**
```rust
for file_path in file_paths {
    // Collect results for this file
    let mut file_results = Vec::new();

    for rule in &rules {
        match rule.check(&file_path, verbose && !json) {
            Ok(results) => {
                file_results.extend(results);
            }
            Err(e) => {
                if !json {
                    eprintln!("  {} Error checking {}: {}", "✗".red(), rule.name(), e);
                }
            }
        }
    }

    // Print results based on mode
    if !json {
        if verbose {
            // Verbose: print filename always
            println!("Checking: {}", file_path.display());
            for result in &file_results {
                if result.has_issue {
                    println!("  {} {}", "✗".red(), result.message.unwrap_or_default());
                } else if verbose {
                    // Could print "No issues found" or similar
                }
            }
        } else {
            // Non-verbose: only print filename if there are issues
            let has_issues = file_results.iter().any(|r| r.has_issue);
            if has_issues {
                println!("{}", file_path.display());
                for result in &file_results {
                    if result.has_issue {
                        println!("  {} {}", "✗".red(), result.message.unwrap_or_default());
                    }
                }
                println!(); // Blank line between files
            }
        }
    }

    // Add to overall results
    all_results.extend(file_results);
}
```

### Key Changes

1. **Line 92-94**: Collect results in `file_results` buffer
2. **Lines 98-110**: Collect results without immediate printing
3. **Lines 112-132**: Handle printing based on verbose mode
   - Verbose: Print filename always, then issues
   - Non-verbose: Print filename only if issues, then issues, then blank line

### Edge Cases

**1. Error during check:**
- Currently prints error immediately (line 113-115)
- Should this count as "has issue" for filename printing?
- **Decision:** Print filename before error in non-verbose mode

**2. Verbose mode with no issues:**
- Currently prints "No definition lists found" etc. from rule implementations
- Should continue this behavior
- Rule's verbose output happens during `rule.check()` call

**3. JSON mode:**
- No changes needed, already skips all printing

## Testing Plan

### Test Case 1: Non-Verbose with Issues
```bash
cargo run --bin qmd-syntax-helper -- check --rule definition-lists 'external-sites/**/*.qmd'
```

**Expected:**
```
external-sites/quarto-web/docs/file1.qmd
  ✗ Definition list found
  ✗ Definition list found

external-sites/quarto-web/docs/file2.qmd
  ✗ Definition list found

=== Summary ===
...
```

### Test Case 2: Non-Verbose with No Issues
```bash
cargo run --bin qmd-syntax-helper -- check --rule definition-lists 'clean-files/*.qmd'
```

**Expected:**
```
=== Summary ===
Total files:         5
Files with issues:   0 ✓
...
```

### Test Case 3: Verbose with Issues
```bash
cargo run --bin qmd-syntax-helper -- check --rule definition-lists 'file-with-issues.qmd' --verbose
```

**Expected:**
```
Checking: file-with-issues.qmd
  Found 2 definition list(s)
  ✗ Definition list found
  ✗ Definition list found

=== Summary ===
...
```

### Test Case 4: Verbose with No Issues
```bash
cargo run --bin qmd-syntax-helper -- check --rule definition-lists 'clean-file.qmd' --verbose
```

**Expected:**
```
Checking: clean-file.qmd
  No definition lists found

=== Summary ===
...
```

## Implementation Steps

1. **Refactor main loop** to buffer results per file
2. **Add conditional printing logic** for verbose/non-verbose
3. **Test non-verbose mode** - should show filenames with issues
4. **Test verbose mode** - should maintain current behavior
5. **Verify JSON mode** - should not be affected

## Additional Improvement (Optional)

Consider showing rule name in non-verbose mode when multiple rules are checked:

```
external-sites/quarto-web/docs/file1.qmd
  ✗ [definition-lists] Definition list found
  ✗ [grid-tables] Grid table found
```

This helps users understand which rule flagged which issue when checking with `--rule all`.

## Summary

The fix involves buffering results per file and only printing the filename in non-verbose mode when that file has issues. This provides context while keeping output concise. Verbose mode continues to print all filenames.
