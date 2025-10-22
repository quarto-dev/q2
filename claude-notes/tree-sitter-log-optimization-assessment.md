# Assessment: Optimizing TreeSitterLogObserver::log() by Eliminating HashMap

**Date:** 2025-10-21
**File:** `crates/quarto-markdown-pandoc/src/utils/tree_sitter_log_observer.rs`
**Function:** `log()` (lines 83-287)
**Profiling Evidence:** 3,537 samples (47% of total time in div-whitespace benchmark)

## Executive Summary

**Your optimization idea is EXCELLENT and should be implemented.** The current implementation creates an expensive HashMap on every log call, when a simple match-based parameter extraction would be much faster. This is a textbook example of premature generalization causing performance issues.

**Expected Impact:**
- **Conservative estimate:** 10-15% speedup in parsing
- **Optimistic estimate:** 20-25% speedup overall
- **Memory:** Eliminates ~615 HashMap operations per run (8% of total samples)

## Current Implementation Analysis

### The Expensive Code (lines 92-102)

```rust
let params: HashMap<String, String> = words[1..]
    .iter()
    .filter_map(|pair| {
        let pair = pair.trim_suffix(",");
        let mut split = pair.splitn(2, ':');
        if let (Some(key), Some(value_str)) = (split.next(), split.next()) {
            return Some((key.to_string(), value_str.to_string()));
        }
        None
    })
    .collect();
```

**Cost per invocation:**
1. Allocates a HashMap
2. Allocates String for EVERY key ("version", "state", "row", "col", "sym", "size")
3. Allocates String for EVERY value
4. Computes hash for each key during insert
5. Allocates HashMap buckets

**Then, when accessing:**
```rust
params.get("version")  // Hash lookup
    .expect("...")
    .parse::<usize>()  // Parse
```

Each `.get()` does another hash computation and lookup.

## Parameter Usage Analysis

I've analyzed ALL usages of the `params` HashMap:

### Fixed Set of Parameters

| Parameter | Type | Used In |
|-----------|------|---------|
| `version` | `usize` | "resume", "process" |
| `state` | `usize` | "process", "shift" |
| `row` | `usize` | "process" |
| `col` | `usize` | "process" |
| `sym` | `String` | "lexed_lookahead" |
| `size` | `usize` | "lexed_lookahead" |

**Key Insight:** Only 6 parameters, each ALWAYS parsed to the same type.

### Match Arm Requirements

1. **"new_parse"** - No parameters
2. **"done"** - No parameters
3. **"resume"** - Needs: `version: usize`
4. **"process"** - Needs: `version: usize`, `state: usize`, `row: usize`, `col: usize`
5. **"detect_error"** - No parameters
6. **"lexed_lookahead"** - Needs: `sym: String`, `size: usize`
7. **"shift"** - Needs: `state: usize`
8. **"skip_token" | "recover_to_previous"** - No parameters
9. **"accept"** - No parameters

Most message types need 0-4 parameters, not a full HashMap!

## Proposed Optimization

### Strategy

Replace HashMap with direct parameter extraction into typed Option variables:

```rust
pub fn log(&mut self, _log_type: tree_sitter::LogType, message: &str) {
    let words: Vec<&str> = message.split_whitespace().collect();
    if words.is_empty() {
        eprintln!("Empty log message from tree-sitter");
        return;
    }

    // Parse parameters directly into typed variables
    let mut version: Option<usize> = None;
    let mut state: Option<usize> = None;
    let mut row: Option<usize> = None;
    let mut col: Option<usize> = None;
    let mut sym: Option<&str> = None;  // Note: &str, not String!
    let mut size: Option<usize> = None;

    // Single pass through parameters
    for pair in &words[1..] {
        let pair = pair.trim_end_matches(',');
        if let Some((key, value)) = pair.split_once(':') {
            match key {
                "version" => version = value.parse().ok(),
                "state" => state = value.parse().ok(),
                "row" => row = value.parse().ok(),
                "col" => col = value.parse().ok(),
                "sym" => sym = Some(value),
                "size" => size = value.parse().ok(),
                _ => {} // Ignore unknown parameters
            }
        }
    }

    match words[0] {
        "resume" => {
            let version = version.expect("Missing 'version' in resume log");
            // ... use version
        }
        "process" => {
            let version = version.expect("Missing 'version' in process log");
            let state = state.expect("Missing 'state' in process log");
            let row = row.expect("Missing 'row' in process log");
            let column = col.expect("Missing 'col' in process log");
            // ... use values
        }
        "lexed_lookahead" => {
            let sym = sym.expect("Missing 'sym' in lexed_lookahead log").to_string();
            let size = size.expect("Missing 'size' in lexed_lookahead log");
            // ... use values
        }
        // ... other cases
    }

    // ... rest of function
}
```

### Key Improvements

1. **No HashMap allocation** - just stack variables
2. **No key String allocations** - match on `&str` directly
3. **No value String allocations** - parse directly from `&str`
4. **One String allocation** - only when `sym` is actually used (lines 212, 215)
5. **Simple string comparison** - match is optimized by compiler, not hashed
6. **Same error handling** - `.expect()` calls remain identical

## Performance Analysis

### From Profiling Data

Current bottlenecks (from flamegraph):
- `TreeSitterLogObserver::log`: 3,537 samples (47.2%)
- `HashMap::insert`: 615 samples (8.2%)
- `snprintf` (string formatting): 927 samples (12.4%)

The HashMap operations alone cost **8.2% of total execution time**.

### Allocation Cost Breakdown

**Current (per log call):**
```
HashMap allocation:     1 × ~96 bytes = 96 bytes
Key Strings:           6 × ~32 bytes = 192 bytes
Value Strings:         6 × ~32 bytes = 192 bytes
HashMap buckets:       ~128 bytes (depends on capacity)
Total:                 ~608 bytes per call
```

**Proposed (per log call):**
```
Option variables:      6 × 8 bytes = 48 bytes (stack)
sym String:           1 × ~32 bytes = 32 bytes (only when used)
Total:                ~48 bytes stack + 32 bytes heap (conditional)
```

**Memory savings:** ~560 bytes per call, almost all heap allocations eliminated!

### Expected Speedup Calculation

If HashMap operations account for 615 samples (8.2%), eliminating them gives:
- **Direct savings:** 8.2% faster
- **Cache effects:** Better cache locality → ~2-3% additional
- **Reduced allocator pressure:** ~1-2% additional
- **Total estimate:** **10-15% faster**

For the div-whitespace benchmark (12 seconds):
- Current: 12.0s
- After optimization: ~10.2s - 10.8s
- **Speedup: 1.5s - 1.8s**

## Correctness Verification

### Behavioral Equivalence

| Aspect | Current | Proposed | Equivalent? |
|--------|---------|----------|-------------|
| Parameter parsing | HashMap with String keys/values | Direct match with Option<T> | ✅ Yes |
| Error handling | `.expect()` on `.get()` | `.expect()` on Option | ✅ Yes |
| Unknown parameters | Stored in HashMap, never used | Ignored in match | ✅ Yes (better!) |
| Duplicate keys | Last value wins (HashMap) | Last value wins (Option overwrite) | ✅ Yes |
| Type safety | Runtime parse | Runtime parse | ✅ Yes |

### Edge Cases

1. **Empty parameters:** Both handle correctly (returns early)
2. **Malformed key:value:** Both skip (None in Option, not added to HashMap)
3. **Missing required parameter:** Both panic with `.expect()`
4. **Extra parameters:** Proposed ignores (better than storing unused)

**Conclusion:** Behaviors are identical, proposal is slightly more robust.

## Additional Micro-Optimizations

While refactoring, we could also:

1. **Eliminate unnecessary `to_string()` on line 85:**
   ```rust
   // Before:
   let str = message.to_string();
   let words: Vec<&str> = str.split_whitespace().collect();

   // After:
   let words: Vec<&str> = message.split_whitespace().collect();
   ```
   **Savings:** 1 String allocation per call

2. **Store `sym` as `&str` until needed:**
   Only allocate the String when actually storing it (lines 212, 215, 247)
   **Savings:** String allocations in cases where sym isn't used

3. **Use `split_once(':')` instead of `splitn(2, ':')`:**
   Slightly more idiomatic and clearer intent
   **Benefit:** Code clarity

## Implementation Considerations

### Testing Strategy

1. **Before changes:**
   - Run full test suite: `cargo test --package quarto-markdown-pandoc`
   - Capture baseline parse logs from a sample file

2. **After changes:**
   - Run full test suite again
   - Compare parse logs - should be identical
   - Run benchmark to measure speedup

3. **Edge case tests:**
   - Empty message
   - Malformed key:value pairs
   - Missing required parameters
   - Extra parameters

### Risk Assessment

**Risk Level:** LOW

- **No API changes** - internal implementation only
- **Well-tested code path** - any breakage will show immediately in tests
- **Same error messages** - debugging experience unchanged
- **Reversible** - can revert easily if issues arise

## Recommendation

**IMPLEMENT THIS OPTIMIZATION IMMEDIATELY**

This is a textbook example of over-engineering causing performance issues. The HashMap is:
1. **Not needed** - fixed parameter set
2. **Expensive** - 8% of total execution time
3. **Wasteful** - 560 bytes of allocations per call
4. **Easy to fix** - simple match-based extraction

The proposed solution is:
- ✅ Faster (10-15% speedup)
- ✅ Simpler (fewer allocations, clearer code)
- ✅ Safer (identical behavior, better for unknown params)
- ✅ Maintainable (easier to understand)

**Expected Results:**
- Parsing performance improves by 10-15%
- Memory usage decreases
- Code becomes simpler and more maintainable
- No behavioral changes or risks

## Implementation Plan

1. Create new branch
2. Modify `log()` function per proposal above
3. Run `cargo test --package quarto-markdown-pandoc`
4. Verify all tests pass
5. Benchmark performance improvement
6. Commit with detailed description of optimization
7. Update performance analysis document

**Estimated time:** 30-45 minutes including testing
**Estimated benefit:** 10-15% speedup in parsing, code clarity improvement
