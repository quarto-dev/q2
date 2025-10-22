# Plan: Fix quarto-error-reporting Test Failures

## Problem Statement

Three tests are failing in `crates/quarto-error-reporting/src/diagnostic.rs` after commit `c3011c6` (k-103):

1. **`test_to_text_simple_error`** (line 654): Expected no trailing newline, got trailing newline
2. **`test_to_text_with_code`** (line 660): Expected error code in output, got simple error without code
3. **`test_location_in_to_text_without_context`** (line 772): Expected location display ("at 11:6"), not present

## Root Cause Analysis

### Failure 1 & 2: Trailing Newline and Missing Code

**Expected** (tests):
```
"Error: Something went wrong"
"Error [Q-1-1]: Something went wrong"
```

**Actual** (current implementation):
```
"Error: Something went wrong\n"
"Error: Something went wrong\n"
```

**Root cause**: Commit `c3011c6` refactored `to_text()` to support ariadne rendering. The changes:
- Line 335: Always adds `\n` after title: `write!(result, "{}: {}\n", kind_str, self.title)`
- Line 335: Doesn't include error code in simple tidyverse format (code is only shown in ariadne format)
- Line 378: Returns `result` which now has trailing newlines from all the `write!` calls

**Old behavior** (before c3011c6):
```rust
// Line 300 (old): No newline after title
write!(result, "{}: {}", kind_str, self.title).unwrap();

// Line 303 (old): Code was included
if let Some(code) = &self.code {
    write!(result, "{} [{}]: {}", kind_str, code, self.title).unwrap();
}
```

**New behavior** (after c3011c6):
```rust
// Line 335 (new): Always newline
write!(result, "{}: {}\n", kind_str, self.title).unwrap();

// No code display in tidyverse branch - only in ariadne
```

### Failure 3: Missing Location Display

**Expected**:
```
text.contains("at 11:6")
```

**Actual**: Location info not displayed

**Root cause**: The test creates a diagnostic with location but **no source context** (`msg.to_text(None)`).

The new implementation (line 303-320) only renders location info in two ways:
1. **With ariadne** (requires `ctx.is_some()` - not available in this test)
2. **No fallback** for "location without context"

**Old behavior** (before c3011c6):
```rust
// Lines 305-321 (old): Showed location even without context
if let Some(loc) = &self.location {
    if let Some(ctx) = ctx {
        // Map to original with context
    } else {
        // NO CONTEXT: Show immediate location
        write!(
            result,
            " at {}:{}",
            loc.range.start.row + 1,
            loc.range.start.column + 1
        ).unwrap();
    }
}
```

**New behavior** (after c3011c6):
- Only shows location if `has_ariadne == true` (requires source context)
- No fallback for displaying location without context

## Design Question

The refactoring introduced a change in behavior: location info is only shown when we can render full ariadne reports. This seems intentional but breaks backward compatibility.

**Options:**

### Option A: Restore Full Backward Compatibility
- Add error code to title line in tidyverse format
- Strip trailing newlines from final result
- Add fallback location display (without context)

**Pros:**
- Tests pass without modification
- Backward compatible
- Simpler error messages still show all info

**Cons:**
- Duplicates info: code shows in both title and ariadne header
- More complex logic

### Option B: Update Tests to Match New Behavior
- Accept trailing newlines (they're harmless for display)
- Accept no code in simple format (ariadne shows it anyway)
- Only test location display with context

**Pros:**
- Cleaner separation: ariadne shows rich info, simple format shows basics
- Matches new design intent
- Less duplication

**Cons:**
- Changes test expectations
- Less info in simple (non-ariadne) format

### Option C: Hybrid Approach (Recommended)
- **Keep trailing newlines** (they make multi-line output cleaner)
- **Add code to simple format** (important for searchability)
- **Add location fallback** (useful when context unavailable)
- **Update test expectations** to accept trailing newlines

**Pros:**
- Best of both: clean output + backward compatibility
- Code always visible (searchable, documentable)
- Location info available even without full context
- Trailing newlines make sense for output (eprintln!, etc.)

**Cons:**
- Requires both code and test changes

## Recommended Solution: Option C

### Changes Needed

#### Change 1: Add Error Code to Simple Format

**File**: `crates/quarto-error-reporting/src/diagnostic.rs`
**Line**: 335

**Current**:
```rust
write!(result, "{}: {}\n", kind_str, self.title).unwrap();
```

**Proposed**:
```rust
if let Some(code) = &self.code {
    write!(result, "{} [{}]: {}\n", kind_str, code, self.title).unwrap();
} else {
    write!(result, "{}: {}\n", kind_str, self.title).unwrap();
}
```

**Rationale**: Error codes are important for searchability and documentation. They should appear in both simple and ariadne formats.

#### Change 2: Add Location Fallback (Without Context)

**File**: `crates/quarto-error-reporting/src/diagnostic.rs`
**Lines**: After title (after line 335)

**Proposed**:
```rust
// Show location info if available and no ariadne rendering
if !has_ariadne && self.location.is_some() {
    let loc = self.location.as_ref().unwrap();

    // Try to map with context if available
    if let Some(ctx) = ctx {
        if let Some(mapped) = loc.map_offset(loc.range.start.offset, ctx) {
            if let Some(file) = ctx.get_file(mapped.file_id) {
                write!(
                    result,
                    "  at {}:{}:{}\n",
                    file.path,
                    mapped.location.row + 1,
                    mapped.location.column + 1
                ).unwrap();
            }
        }
    } else {
        // No context: show immediate location (1-indexed for display)
        write!(
            result,
            "  at {}:{}\n",
            loc.range.start.row + 1,
            loc.range.start.column + 1
        ).unwrap();
    }
}
```

**Rationale**: Location info is useful even without full source context. Shows where the error occurred.

#### Change 3: Update Test Expectations

**File**: `crates/quarto-error-reporting/src/diagnostic.rs`

**Test 1** (line 654):
```rust
// OLD:
assert_eq!(msg.to_text(None), "Error: Something went wrong");

// NEW:
assert_eq!(msg.to_text(None), "Error: Something went wrong\n");
```

**Test 2** (line 660):
```rust
// OLD:
assert_eq!(msg.to_text(None), "Error [Q-1-1]: Something went wrong");

// NEW: (no change needed - code fix will make this pass)
assert_eq!(msg.to_text(None), "Error [Q-1-1]: Something went wrong\n");
```

**Test 3** (line 772):
```rust
// OLD:
assert!(text.contains("at 11:6"));

// NEW: (no change needed - location fallback will make this pass)
// Just verify it works with the new location display format
assert!(text.contains("at 11:6"));
```

## Implementation Steps

### Step 1: Add error code to simple format
- Update line 335 to check for error code and include it
- Test: `cargo test -p quarto-error-reporting test_to_text_with_code`

### Step 2: Add location fallback
- Add location display after title (before problem statement)
- Test: `cargo test -p quarto-error-reporting test_location_in_to_text_without_context`

### Step 3: Update test expectations
- Update `test_to_text_simple_error` to expect trailing newline
- Update `test_to_text_with_code` to expect trailing newline
- Test: `cargo test -p quarto-error-reporting`

### Step 4: Verify all tests pass
- Run full test suite
- Ensure no regressions

## Alternative: Quick Fix (Update Tests Only)

If we decide the new behavior is correct and tests are wrong:

1. Accept that simple format doesn't show code (only ariadne does)
2. Accept that location requires context
3. Update tests:
   - Remove code expectation from test 2
   - Remove location expectation from test 3
   - Accept trailing newlines

**Trade-off**: Less information in simple format, but cleaner separation of concerns.

## Recommendation

Implement **Option C** (Hybrid Approach):
- Add error code to simple format (important for CLI users without ariadne)
- Add location fallback (useful debugging info)
- Accept trailing newlines (makes sense for text output)

This provides maximum information in all contexts while maintaining clean, structured output.
