# Fix Trailing LineBreak at End of Block (k-0dqw)

**Issue**: k-0dqw
**Created**: 2025-12-17
**Status**: COMPLETED (2025-12-17)

## Problem Statement

In CommonMark 0.31.2, a hard line break (`\` at end of line) does NOT work at the end of a block element. The CommonMark spec (lines 9362-9391) states:

> Hard line breaks are for separating inline content within a block.
> Neither syntax for hard line breaks works at the end of a paragraph or
> other block element.

### Spec Examples (lines 9366-9391)

**Example 670** - Backslash at end of paragraph:
```markdown
foo\
```
Produces:
```html
<p>foo\</p>
```

**Example 671** - Two spaces at end of paragraph (stripped):
```markdown
foo
```
Produces:
```html
<p>foo</p>
```

**Example 672** - Backslash at end of header:
```markdown
### foo\
```
Produces:
```html
<h3>foo\</h3>
```

### Current pampa Behavior

pampa currently produces a `LineBreak` inline when parsing `foo\` at the end of a paragraph or header. This differs from CommonMark (comrak), which produces `Str("\\")`.

This discrepancy was discovered during property testing in the `comrak-to-pandoc` crate.

## Proposed Solution

### Option 1: Postprocess with Block-Specific Handlers (Recommended)

Modify the `with_paragraph` and `with_header` handlers in `postprocess.rs` to detect and convert trailing `LineBreak` inlines to `Str("\\")`.

**Pros**:
- Block context is directly available
- Can handle different block types differently if needed
- Clean separation of concerns

**Cons**:
- Must add logic to multiple handlers (paragraph, header, any other block with inlines)

### Option 2: Track Block Context in with_inlines

Pass block context through the filter context to `with_inlines`, allowing it to know when it's processing the final inlines of a block.

**Pros**:
- Single location for the fix
- Centralized inline processing

**Cons**:
- More complex - requires modifying filter infrastructure
- Filter context doesn't currently track this information

### Option 3: Normalize in `with_inlines` Unconditionally

Since `with_inlines` doesn't know block context, we could check if the inlines end with LineBreak and convert it, assuming that valid LineBreaks mid-content will have been consumed by the caller.

**Cons**:
- Risky - might incorrectly convert LineBreaks that should stay
- The `with_inlines` handler is called for ALL inline sequences, not just top-level block content

### Recommendation: Option 1

Option 1 is safest because we have direct access to the block context. The affected blocks are:
- `Paragraph` - most common case
- `Header` - also affected per spec
- `Plain` - used in tight lists, same behavior expected

## Implementation Plan

### Phase 1: Create Failing Unit Test

**Location**: `crates/pampa/tests/` (new test file or add to existing)

**Test Cases**:

1. **Backslash at end of paragraph**:
   ```markdown
   foo\
   ```
   Expected: `Paragraph([Str("foo"), Str("\\")])` (or `Str("foo\\")` if merged)
   NOT: `Paragraph([Str("foo"), LineBreak])`

2. **Backslash in middle of paragraph** (should remain LineBreak):
   ```markdown
   foo\
   bar
   ```
   Expected: `Paragraph([Str("foo"), LineBreak, Str("bar")])`

3. **Backslash at end of header**:
   ```markdown
   # foo\
   ```
   Expected: `Header(1, [Str("foo"), Str("\\")])`
   NOT: `Header(1, [Str("foo"), LineBreak])`

4. **Backslash in tight list item**:
   ```markdown
   - foo\
   ```
   Expected: `BulletList([[Plain([Str("foo"), Str("\\")])]])`

### Phase 2: Implement Fix

**File**: `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`

**Approach**:

1. Create a helper function:
   ```rust
   /// Convert trailing LineBreak to Str("\\") for CommonMark compatibility.
   ///
   /// Per CommonMark spec, hard line breaks don't work at the end of a block.
   /// A backslash at end of paragraph/header produces literal "\", not LineBreak.
   fn convert_trailing_linebreak_to_str(inlines: &mut Inlines) {
       if let Some(Inline::LineBreak(lb)) = inlines.last() {
           let source_info = lb.source_info.clone();
           inlines.pop();
           inlines.push(Inline::Str(Str {
               text: "\\".to_string(),
               source_info,
           }));
       }
   }
   ```

2. Modify `with_paragraph` handler to call this helper:
   ```rust
   .with_paragraph(|mut para, _ctx| {
       // ... existing figure transformation logic ...

       // Convert trailing LineBreak to literal backslash (CommonMark spec)
       convert_trailing_linebreak_to_str(&mut para.content);

       // ... rest of handler ...
   })
   ```

3. Modify `with_header` handler similarly.

4. Consider if `Plain` blocks need the same treatment (likely yes, for tight lists).

### Phase 3: Source Location Handling

The key challenge mentioned is recovering source location information. However, looking at the implementation:

1. When tree-sitter parses `\` at EOL, it creates a `LineBreak` node with source info pointing to the `\` character
2. When we convert to `Str("\\")`, we can preserve the same `source_info` from the `LineBreak`
3. The source info should correctly point to the backslash in the original source

**Source Info Transfer**:
```rust
// LineBreak.source_info points to the "\" in source
// When converting to Str("\\"), use the same source_info
let source_info = linebreak.source_info.clone();  // Preserves location
```

This should work correctly because:
- The `LineBreak` inline was created with source info pointing to the backslash
- We transfer that exact source info to the new `Str` inline
- No location information is lost

### Phase 4: Verify and Test

1. Run the new unit test and verify it passes
2. Run existing pampa test suite to ensure no regressions
3. Run comrak-to-pandoc property tests to verify the discrepancy is resolved

## Implementation Summary

**Completed**: 2025-12-17

### Changes Made

1. **Added helper function** `convert_trailing_linebreak_to_str()` in `postprocess.rs` (lines 520-541)
   - Checks if the last inline is a LineBreak
   - If so, converts it to `Str("\\")` preserving source info
   - Returns bool indicating whether conversion occurred

2. **Modified `with_header` handler** to call the helper at the start, before ID generation
   - Converts trailing LineBreak before any other processing
   - Returns FilterResult if conversion made (to signal change)

3. **Modified `with_paragraph` handler** to call the helper
   - Converts trailing LineBreak before checking for single-image figure conversion
   - Returns FilterResult if conversion made and not converted to figure

4. **Added `with_plain` handler** for Plain blocks (used in tight lists)
   - Converts trailing LineBreak for list items

5. **Updated test** `test_hard_break_at_end` in `test_hard_soft_break.rs`
   - Changed from expecting LineBreak to expecting literal backslash

6. **Created new test file** `test_trailing_linebreak_commonmark.rs`
   - 6 comprehensive tests for CommonMark-correct behavior

### Test Results

- All 892 pampa tests pass
- All 95 comrak-to-pandoc tests pass (14 skipped for known differences)
- All 6 new CommonMark trailing linebreak tests pass

## File Changes

| File | Change |
|------|--------|
| `crates/pampa/tests/test_trailing_linebreak_commonmark.rs` (new) | Unit tests for CommonMark behavior |
| `crates/pampa/tests/test_hard_soft_break.rs` | Updated test to expect literal backslash |
| `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs` | Added helper and modified 3 handlers |

## Edge Cases to Consider

1. **Multiple trailing LineBreaks**: `foo\<newline>\<newline>` - unlikely to parse this way, but handle gracefully

2. **LineBreak followed by SoftBreak**: The existing code already handles this pattern (lines 1108-1132 in postprocess.rs). Ensure the new logic doesn't interfere.

3. **Two-space hard break at end**: Per spec, this should be stripped entirely (not converted to `\`). However, if pampa produces a `LineBreak` for this, we'd convert it to `Str("\\")` which is technically incorrect.
   - **Note**: The spec says two trailing spaces produce `<p>foo</p>` (no trailing anything), while backslash produces `<p>foo\</p>` (literal backslash).
   - pampa may or may not distinguish these cases. Investigation needed.

4. **LineBreak in nested structures**: e.g., inside emphasis at end of paragraph `*foo\*` - should the `\` become literal? Yes, per spec.

## Test File Template

```rust
// tests/linebreak_at_end_of_block.rs

fn main() {}

#[cfg(test)]
mod tests {
    use pampa::readers::qmd::read;
    use quarto_pandoc_types::{Block, Inline, Pandoc};

    fn parse(input: &str) -> Pandoc {
        let mut output = Vec::new();
        let (pandoc, _ctx, _errors) = read(
            input.as_bytes(),
            false,
            "test.md",
            &mut output,
            true,
            None,
        ).expect("parse failed");
        pandoc
    }

    fn get_paragraph_inlines(doc: &Pandoc) -> Option<&Vec<Inline>> {
        match doc.blocks.first()? {
            Block::Paragraph(p) => Some(&p.content),
            _ => None,
        }
    }

    fn get_header_inlines(doc: &Pandoc) -> Option<&Vec<Inline>> {
        match doc.blocks.first()? {
            Block::Header(h) => Some(&h.content),
            _ => None,
        }
    }

    #[test]
    fn backslash_at_end_of_paragraph_is_literal() {
        let doc = parse("foo\\\n");
        let inlines = get_paragraph_inlines(&doc).expect("expected paragraph");

        // Should NOT end with LineBreak
        assert!(!matches!(inlines.last(), Some(Inline::LineBreak(_))));

        // Should end with Str containing backslash
        match inlines.last() {
            Some(Inline::Str(s)) => assert!(s.text.ends_with('\\')),
            _ => panic!("Expected trailing Str with backslash"),
        }
    }

    #[test]
    fn backslash_in_middle_is_linebreak() {
        let doc = parse("foo\\\nbar\n");
        let inlines = get_paragraph_inlines(&doc).expect("expected paragraph");

        // Should have LineBreak in the middle
        let has_linebreak = inlines.iter().any(|i| matches!(i, Inline::LineBreak(_)));
        assert!(has_linebreak, "Expected LineBreak for backslash in middle");
    }

    #[test]
    fn backslash_at_end_of_header_is_literal() {
        let doc = parse("# foo\\\n");
        let inlines = get_header_inlines(&doc).expect("expected header");

        // Should NOT end with LineBreak
        assert!(!matches!(inlines.last(), Some(Inline::LineBreak(_))));
    }
}
```

## Success Criteria

1. All new unit tests pass
2. All existing pampa tests continue to pass
3. comrak-to-pandoc property tests no longer need to disable `linebreak` feature for end-of-block cases
4. Source location information is preserved correctly

## Related Documentation

- CommonMark Spec: `external-sources/commonmark-spec/spec.txt` lines 9244-9391
- CommonMark Spec Index: `claude-notes/commonmark-reference/spec-index.md`
- Property Testing Documentation: `claude-notes/plans/2025-11-06-commonmark-compatible-subset.md`
