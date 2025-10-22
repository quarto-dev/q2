# Fix _scope: lexical Regression

**Date**: 2025-10-21
**Issue**: Regression in handling `_scope: lexical` block metadata

## Problem

After commit c7c540a (k-90/k-95), the handling of `_scope: lexical` broke because:

1. **Before c7c540a**: YAML string values like "lexical" were represented as `MetaString { value: "lexical" }`
2. **After c7c540a**: YAML string values are parsed as markdown and become `MetaInlines { content: [Str { text: "lexical" }] }`

The check in `src/readers/qmd.rs:178` is still looking for `MetaString`:

```rust
MetaValueWithSourceInfo::MetaString { value, .. } if value == "lexical"
```

This never matches, so `is_lexical` is always `false`, causing all block metadata to be merged into document-level metadata instead of being preserved as `BlockMetadata`.

## Expected Behavior (from a1c871c)

For this input:
```markdown
::: hello

---
_scope: lexical
nested: meta
---

:::
```

The output should have a `BlockMetadata` element inside the Div:
```json
{
  "c": [
    [...],
    [
      {
        "c": { ... },
        "t": "BlockMetadata"
      }
    ]
  ],
  "t": "Div"
}
```

## Current Behavior (kyoto branch)

The BlockMetadata is lost - the inner array is empty:
```json
{
  "c": [
    [...],
    []
  ],
  "t": "Div"
}
```

## Solution

We need to update the check for `_scope: lexical` to handle both cases:
1. `MetaString { value: "lexical" }` - for backward compatibility or special cases
2. `MetaInlines { content: [Str { text: "lexical" }] }` - the new behavior after k-90/k-95

### Implementation Plan

1. **Create a helper function** `is_meta_value_string(meta: &MetaValueWithSourceInfo, expected: &str) -> bool` that checks if a MetaValue represents a string with a specific value, handling both:
   - `MetaString { value, .. }` where `value == expected`
   - `MetaInlines { content, .. }` where content is a single `Str` with text == expected

2. **Update the lexical check** in `src/readers/qmd.rs:172-183` to use this helper function

3. **Add tests** to ensure this works correctly:
   - Test that `_scope: lexical` preserves BlockMetadata
   - Test that block metadata without `_scope: lexical` is merged into document metadata
   - Test that we can capture source location information for inner metadata (the original requirement)

4. **Verify the snapshot test** `tests/snapshots/json/003.qmd` passes with the correct output

## Files to Modify

- `src/readers/qmd.rs` - Update the `is_lexical` check
- Possibly add unit tests for the helper function

## Test Case

The existing test file `tests/snapshots/json/003.qmd` should be used to verify the fix:
- It has `_scope: lexical`
- The snapshot should show BlockMetadata preserved inside the Div
