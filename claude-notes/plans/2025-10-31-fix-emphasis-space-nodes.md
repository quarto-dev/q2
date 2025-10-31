# Plan: Fix Missing Space Nodes Around Emphasis

**Date**: 2025-10-31
**Status**: Planning
**Related**: pandoc_emph implementation, k-274

## Problem

Delimiters like `" *"` and `"* "` capture adjacent spaces, causing missing Space nodes.

**Example**: `"x *y* z"` produces `[Str "x", Emph [Str "y"], Str "z"]`
**Expected**: `[Str "x", Space, Emph [Str "y"], Space, Str "z"]`

## Solution Design

### Key Insight
Return `IntermediateInlines(vec![Space?, Emph, Space?])` instead of `IntermediateInline(Emph)`.

### Implementation

Replace the macro call in `pandoc_emph` handler with custom code that:

1. **Scan delimiters** for captured spaces:
```rust
let mut has_leading_space = false;
let mut has_trailing_space = false;
let mut first_delimiter = true;

for (node_name, child) in &children {
    if node_name == "emphasis_delimiter" {
        if let PandocNativeIntermediate::IntermediateUnknown(range) = child {
            let text = std::str::from_utf8(&input_bytes[range.start.byte..range.end.byte]).unwrap();
            if first_delimiter {
                has_leading_space = text.starts_with(char::is_whitespace);
                first_delimiter = false;
            } else {
                has_trailing_space = text.ends_with(char::is_whitespace);
            }
        }
    }
}
```

2. **Build Emph** using existing helper
3. **Inject Spaces** as needed:
```rust
let mut result = Vec::new();
if has_leading_space { result.push(Inline::Space(...)); }
result.push(emph);
if has_trailing_space { result.push(Inline::Space(...)); }
return PandocNativeIntermediate::IntermediateInlines(result);
```

## Test Cases

- `"x *y* z"` → 2 Spaces (before and after)
- `"x*y*z"` → 0 Spaces
- `"x *y*z"` → 1 Space (before only)
- `"x*y* z"` → 1 Space (after only)

## Next Steps

1. Implement custom handler
2. Test with verbose output
3. Update existing tests
4. Verify matches Pandoc output

**Estimated time**: 60-80 minutes
