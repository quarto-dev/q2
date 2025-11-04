# Iterative Fixing for qmd-syntax-helper

**Date**: 2025-11-03
**Issue**: test_div_whitespace_conversion failing due to parser only reliably detecting one error at a time

## Problem Analysis

### Root Cause

The parser can only reliably report **one error at a time**. After encountering the first parse error, the parser enters error recovery mode and subsequent error reports have incorrect/unreliable locations.

**Example with multiple `:::{` errors:**

```
Initial state: :::{.class}\n...\n:::{#id}\n...\n:::{}

Parser encounters :::{.class}
├─ Reports: "Missing Space After Div Fence" at offset 16 ✓ (accurate)
├─ Gets confused, enters error recovery
└─ Reports: "Parse error" at offset 47 ✗ (wrong location - blank line after closing fence)
    Reports: "Parse error" at offset 77 ✗ (wrong location - blank line after closing fence)

After fixing :::{.class} → ::: {.class}
New state: ::: {.class}\n...\n:::{#id}\n...\n:::{}

Parser re-parses cleanly through first div
├─ Now encounters :::{#id}
├─ Reports: "Missing Space After Div Fence" at correct location ✓
└─ Gets confused again, next error will be wrong

After fixing :::{#id} → ::: {#id}
New state: ::: {.class}\n...\n::: {#id}\n...\n:::{}

Parser re-parses cleanly through both divs
└─ Now encounters :::{}
    Reports: Accurate error for this one ✓
```

### Why Other Approaches Won't Work

**Option 1: Expand Search Range** ❌
- Parser error locations are fundamentally unreliable after first error
- Would require complex, brittle heuristics
- Different error types have different location offset patterns
- Unmaintainable

**Option 2: Per-Rule Iteration** ⚠️
- Each rule implements its own iteration logic
- Pros: Each rule controls its behavior
- Cons: Duplicated code, doesn't handle inter-rule dependencies

## Solution: Global Iteration

**Approach**: Iterate the entire convert process until convergence.

```rust
loop {
    let mut total_fixes = 0;
    for rule in &rules {
        total_fixes += rule.convert(file)?.fixes_applied;
    }
    if total_fixes == 0 { break; }  // Converged
}
```

**Why this works:**
- After each fix, file is reparsed with clean state
- Parser can accurately report next error
- Converges when no more fixes are found
- General solution for ALL parser-based rules

## Current Architecture

### qmd-syntax-helper Structure

1. **Main binary** (`main.rs`):
   - CLI with `check` and `convert` commands
   - Processes multiple files and applies multiple rules
   - **Current flow**: For each file → for each rule → convert once → done

2. **Rule trait** (`rule.rs`):
   - `check()`: Detects issues in a file
   - `convert()`: Fixes issues in a file
   - Returns `ConvertResult` with `fixes_applied` count

3. **Converters** (e.g., `DivWhitespaceConverter`):
   - Relies on parser error reporting
   - **Parser limitation**: After hitting first error, subsequent errors are reported poorly

### Key Files to Modify

- `crates/qmd-syntax-helper/src/main.rs` - Add iteration logic
- `crates/qmd-syntax-helper/tests/div_whitespace_test.rs` - Tests should pass

## Implementation Plan

### Phase 1: Add Iteration to Convert Command

#### 1.1 Modify CLI Args

**Location**: `main.rs:49-69`

Add new arguments:

```rust
Convert {
    // ... existing args ...

    /// Maximum iterations for fixing (default: 10)
    #[arg(long, default_value = "10")]
    max_iterations: usize,

    /// Disable iterative fixing (run each rule once)
    #[arg(long)]
    no_iteration: bool,
}
```

#### 1.2 Refactor Convert Logic

**Location**: `main.rs:179-224`

Current structure:
```rust
for file_path in file_paths {
    for rule in &rules {
        rule.convert(&file_path, ...)?;
    }
}
```

New structure:
```rust
for file_path in file_paths {
    loop {
        let mut fixes_this_iteration = 0;
        for rule in &rules {
            fixes_this_iteration += rule.convert(&file_path, ...)?.fixes_applied;
        }
        if fixes_this_iteration == 0 { break; }  // Converged
        if iteration >= max_iterations { break; }
    }
}
```

#### 1.3 Implementation Details

**Key changes to `Commands::Convert` match arm:**

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

        let mut iteration = 0;
        let mut total_fixes_for_file = 0;

        loop {
            iteration += 1;
            let mut fixes_this_iteration = 0;

            if verbose && !no_iteration && iteration > 1 {
                println!("  Iteration {}:", iteration);
            }

            // Apply fixes sequentially, reparsing between each rule
            for rule in &rules {
                match rule.convert(&file_path, in_place, check_mode, verbose) {
                    Ok(result) => {
                        if result.fixes_applied > 0 {
                            fixes_this_iteration += result.fixes_applied;
                            total_fixes_for_file += result.fixes_applied;

                            if verbose || check_mode {
                                let prefix = if no_iteration { "" } else { "  " };
                                println!(
                                    "{}  {} {} - {}",
                                    prefix,
                                    if check_mode { "Would fix" } else { "Fixed" },
                                    rule.name(),
                                    result.message.clone().unwrap_or_default()
                                );
                            }

                            if !in_place && !check_mode && result.message.is_some() {
                                print!("{}", result.message.unwrap());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  {} Error converting {}: {}", "✗".red(), rule.name(), e);
                        return Err(e);
                    }
                }
            }

            // Check convergence
            if fixes_this_iteration == 0 {
                if verbose && !no_iteration && iteration > 1 {
                    println!("  Converged after {} iteration(s) ({} total fixes)",
                             iteration, total_fixes_for_file);
                }
                break;
            }

            // Check max iterations
            if iteration >= max_iter {
                if !no_iteration {
                    eprintln!("  {} Warning: Reached max iterations ({}), but file may still have issues",
                             "⚠".yellow(), max_iter);
                }
                break;
            }
        }
    }

    Ok(())
}
```

#### 1.4 Verbose Output Example

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

#### 1.5 Safety Guards

1. **Max iterations limit**: Default 10, configurable via `--max-iterations`
2. **Convergence detection**: Stop when `fixes_this_iteration == 0`
3. **Warning on non-convergence**: Alert user if max iterations reached
4. **Optional: Infinite loop detection**: If same fix count for N consecutive iterations, warn and break

### Phase 2: Testing

#### Test Cases

1. **test_div_whitespace_conversion** - Should now pass
   - File with 3 div fence errors
   - Requires 3 iterations to fix all
   - Verifies all fixes are applied

2. **test_iteration_convergence** - New test
   - File needs multiple iterations
   - Verify iteration count is correct
   - Verify total fixes reported

3. **test_already_clean** - New test
   - File with no issues
   - Should complete in 1 iteration
   - No fixes reported

4. **test_max_iterations** - New test
   - Mock a file that would need > max iterations
   - Verify warning is emitted
   - Verify it stops at max

5. **test_no_iteration_flag** - New test
   - File with multiple errors
   - Run with `--no-iteration`
   - Verify only first error fixed
   - Backward compatibility check

6. **test_multiple_rules_iteration** - New test
   - File needs fixes from multiple rules
   - Verify rules interact correctly
   - Check iteration count

#### Test Implementation Location

- `crates/qmd-syntax-helper/tests/div_whitespace_test.rs` - Update existing test
- `crates/qmd-syntax-helper/tests/iteration_test.rs` - New test file for iteration-specific tests

### Phase 3: Documentation

1. **Update CLI help text**
   - Document `--max-iterations` flag
   - Document `--no-iteration` flag
   - Add examples

2. **Add usage examples**
   - Basic usage (default iteration)
   - Limiting iterations
   - Disabling iteration for backward compatibility

## Benefits

1. **Fixes immediate problem**: test_div_whitespace_conversion will pass
2. **General solution**: Works for ANY parser-based rule, not just div-whitespace
3. **Backward compatible**: `--no-iteration` preserves old behavior
4. **Safe**: Guards against infinite loops
5. **User-friendly**: Clear progress reporting in verbose mode
6. **Minimal changes**: Existing rules don't need modification
7. **Handles dependencies**: If rule A's fixes enable rule B to find issues, iteration handles it

## Edge Cases Handled

1. **No fixes needed**: Exits after iteration 1 (fast path)
2. **Infinite loop**: Max iterations prevents runaway
3. **Multiple rules**: Each rule runs, file reparsed naturally between rules
4. **Check mode**: Iterations still happen (to report all potential fixes) but no file writes
5. **Non-in-place mode**: Only final result outputs to stdout (or output per iteration if verbose)

## Design Decisions & Questions

### Decisions Made

1. **Default max iterations: 10**
   - Most files should converge in 2-5 iterations
   - 10 provides safety margin
   - User can override with `--max-iterations`

2. **Iteration enabled by default**
   - Correct behavior for most users
   - `--no-iteration` for edge cases

3. **Verbose shows iteration details**
   - Non-verbose shows final result only
   - Clear feedback for debugging

### Open Questions

1. **Should non-verbose mode show iteration count when > 1?**
   - Pro: User knows multiple passes happened
   - Con: More output clutter
   - Recommendation: Only show if > 3 iterations (unusual case)

2. **Should --check mode iterate?**
   - Current plan: Yes (to find all potential fixes)
   - Alternative: Only report first-pass fixes
   - Recommendation: Iterate (more useful info)

3. **Infinite loop detection threshold?**
   - If same fix count for N consecutive iterations, break with warning
   - Suggested N = 2 or 3
   - May not be necessary if max_iterations is low enough

4. **Non-in-place iteration output?**
   - Current plan: Only output final result
   - Alternative: Output each iteration (confusing)
   - Recommendation: Final result only, verbose shows progress

## Implementation Checklist

- [ ] Add CLI arguments (`max_iterations`, `no_iteration`)
- [ ] Refactor `Commands::Convert` to add iteration loop
- [ ] Add verbose iteration reporting
- [ ] Add convergence detection
- [ ] Add max iterations warning
- [ ] Update existing test (test_div_whitespace_conversion)
- [ ] Write new test cases
- [ ] Update `--help` documentation
- [ ] Manual testing with various files
- [ ] Consider: Add metric tracking (iterations per file)

## Related Issues

- Test failure: `test_div_whitespace_conversion` in `crates/qmd-syntax-helper/tests/div_whitespace_test.rs`
- Root cause: Parser error recovery makes subsequent error locations unreliable

## Notes

- This approach is similar to how linters/formatters like `prettier` work with `--write` (multiple passes until stable)
- The iteration overhead is acceptable because:
  - Most files converge quickly (1-3 iterations)
  - Parser is already reasonably fast
  - Correctness > performance for a syntax helper tool
- Rules don't need to be aware of iteration - they just report `fixes_applied`
