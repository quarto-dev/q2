# Inline Note Reference Whitespace Handling Plan

**Date**: 2025-10-31
**Issue**: Whitespace around `inline_note_reference` nodes is not being preserved correctly
**Context**: User request to distinguish between 'Hi [^ref]' and 'Hi[^ref]'

## Problem Statement

The current implementation of `inline_note_reference` handling uses `.trim()` on the node text, which removes leading whitespace that should be preserved. This makes it impossible to distinguish between:

- `Hi [^ref]` (space before note reference)
- `Hi[^ref]` (no space before note reference)

## Current Behavior Analysis

### Tree-sitter Output Patterns

Through testing, I've identified how tree-sitter handles whitespace around `inline_note_reference`:

1. **"Hi [^ref] bye"**:
   ```
   pandoc_str "Hi" (0,0)-(0,2)
   inline_note_reference " [^ref]" (0,2)-(0,9)  ← 7 chars, includes leading space
   pandoc_space " " (0,9)-(0,10)
   pandoc_str "bye" (0,10)-(0,13)
   ```

2. **"Hi[^ref]bye"**:
   ```
   pandoc_str "Hi" (0,0)-(0,2)
   inline_note_reference "[^ref]" (0,2)-(0,8)  ← 6 chars, no leading space
   pandoc_str "bye" (0,8)-(0,11)
   ```

3. **"[^ref]bye"**:
   ```
   inline_note_reference "[^ref]" (0,0)-(0,6)  ← 6 chars, no leading space
   pandoc_str "bye" (0,6)-(0,9)
   ```

4. **"Hi[^ref] bye"**:
   ```
   pandoc_str "Hi" (0,0)-(0,2)
   inline_note_reference "[^ref]" (0,2)-(0,8)  ← 6 chars, no leading space
   pandoc_space " " (0,8)-(0,9)
   pandoc_str "bye" (0,9)-(0,12)
   ```

### Key Observations

1. **Leading whitespace**: Tree-sitter INCLUDES leading whitespace in the `inline_note_reference` node text
2. **Trailing whitespace**: Tree-sitter creates a SEPARATE `pandoc_space` node for trailing whitespace
3. **Current bug**: The implementation uses `.trim()` which strips the leading space, losing the distinction

### Current Output

Both "Hi [^ref]" and "Hi[^ref]" currently produce:
```
[ Para [Str "Hi", Span (...) []] ]
```

Missing the Space node in the first case.

## Solution Design

### Approach

Similar to `process_inline_with_delimiter_spaces` (used for emphasis, strong, etc.), we need to:

1. Detect if the node text starts with whitespace
2. Extract the note ID from the trimmed text
3. Return `IntermediateInlines` with a leading `Space` node if whitespace was detected
4. Otherwise return `IntermediateInline` with just the `NoteReference`

### Implementation

The `inline_note_reference` handler should be modified to:

```rust
"inline_note_reference" => {
    // Extract the note reference text (e.g., " [^id]" or "[^id]")
    let text = node.utf8_text(input_bytes).unwrap();

    // Check for leading whitespace
    let has_leading_space = text.starts_with(char::is_whitespace);

    // Trim to extract the actual reference
    let trimmed = text.trim();

    // Verify format and extract ID
    if trimmed.starts_with("[^") && trimmed.ends_with("]") {
        let id = trimmed[2..trimmed.len() - 1].to_string();
        let note_ref = Inline::NoteReference(NoteReference {
            id,
            source_info: node_source_info_with_context(node, context),
        });

        // Build result with leading Space if needed
        if has_leading_space {
            PandocNativeIntermediate::IntermediateInlines(vec![
                Inline::Space(Space {
                    source_info: node_source_info_with_context(node, context),
                }),
                note_ref,
            ])
        } else {
            PandocNativeIntermediate::IntermediateInline(note_ref)
        }
    } else {
        // Error handling...
        eprintln!("Warning: unexpected inline_note_reference format: '{}'", trimmed);
        PandocNativeIntermediate::IntermediateUnknown(node_location(node))
    }
}
```

### Alternative Considered: Using Children

Initially, I considered checking if the node has delimiter children (like emphasis does), but `inline_note_reference` is a single token node from tree-sitter with no children. It's similar to how the old grammar had atomic tokens for certain constructs.

### Comparison with Other Inline Elements

- **Emphasis/Strong/etc.**: Use delimiter children to detect captured spaces
- **Citations**: Keep the full text (including leading space) in their content Str
- **inline_note_reference**: Need to inject Space nodes since the Span has empty content

## Expected Behavior After Fix

### Test Cases

1. **"Hi [^ref]"** should produce:
   ```
   [ Para [Str "Hi", Space, Span (...) []] ]
   ```

2. **"Hi[^ref]"** should produce:
   ```
   [ Para [Str "Hi", Span (...) []] ]
   ```

3. **"[^ref] bye"** should produce:
   ```
   [ Para [Span (...) [], Space, Str "bye"] ]
   ```
   (The trailing space comes from the separate `pandoc_space` node)

4. **"[^ref]bye"** should produce:
   ```
   [ Para [Span (...) [], Str "bye"] ]
   ```

## Testing Strategy

1. **Update existing tests** in `test_treesitter_refactoring.rs` to verify spacing behavior
2. **Add new tests** specifically for whitespace edge cases:
   - Leading space only
   - Trailing space only (should already work via pandoc_space node)
   - Both leading and trailing
   - No spaces at all
   - Multiple note references with various spacing

3. **Test format**:
   ```rust
   #[test]
   fn test_inline_note_reference_with_leading_space() {
       let input = "Hi [^ref]";
       let result = parse_qmd_to_json(input);

       // Should have a Space node between "Hi" and the Span
       assert!(result.contains("\"t\":\"Space\""));
       // Verify order: Str, Space, Span
   }

   #[test]
   fn test_inline_note_reference_no_leading_space() {
       let input = "Hi[^ref]";
       let result = parse_qmd_to_json(input);

       // Should NOT have a Space node
       assert!(!result.contains("\"t\":\"Space\""));
       // Verify order: Str, Span
   }
   ```

## Implementation Steps

1. Modify the `inline_note_reference` handler in `treesitter.rs`
2. Update tests to check for correct spacing behavior
3. Run full test suite to ensure no regressions
4. Test manually with various edge cases

## Potential Issues

### Source Info for Injected Space

The injected Space node needs proper source info. We use `node_source_info_with_context(node, context)` which gives the full node's range (including the leading space). This is semantically correct - the space is part of the inline_note_reference token.

### Interaction with Postprocessing

The `postprocess` step (merge_strs, etc.) should handle the IntermediateInlines wrapper correctly. This pattern is already used by `process_inline_with_delimiter_spaces`, so it should work.

### Edge Case: Multiple Consecutive Spaces

If the input is "Hi  [^ref]" (two spaces), tree-sitter might capture it as:
- pandoc_space " "
- inline_note_reference " [^ref]"

Or it might capture both spaces in the inline_note_reference. Need to test this.

## References

- `process_inline_with_delimiter_spaces` in `treesitter_utils/text_helpers.rs` (lines 226-284)
- Current `inline_note_reference` handler in `treesitter.rs` (lines 767-790)
- Tree-sitter grammar for inline_note_reference in `tree-sitter-markdown/grammar.js`

## Success Criteria

1. "Hi [^ref]" produces different output than "Hi[^ref]"
2. All existing tests continue to pass
3. New whitespace-specific tests pass
4. Manual testing confirms correct behavior with various spacing patterns
