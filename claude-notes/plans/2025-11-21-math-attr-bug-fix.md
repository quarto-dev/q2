# Bug: Math+Attr Feature Completely Broken

## Problem

The Math+Attr desugaring feature described in `docs/syntax/desugaring/math-attributes.qmd` is completely broken. Attributes following math expressions are being silently dropped.

**Example**:
```qmd
$E = mc^2$ {#eq-einstein}
```

**Expected output**: Span wrapping Math with attribute
**Actual output**: Just Math, attribute disappears entirely

## Root Cause Analysis

### Evidence

1. **Tree-sitter parsing works correctly**:
   ```
   attribute_specifier: {Node attribute_specifier (0, 31) - (0, 45)}
     commonmark_specifier: {Node commonmark_specifier (0, 32) - (0, 44)}
       attribute_id: {Node attribute_id (0, 32) - (0, 44)}
   ```
   The attribute `{#eq-einstein}` is correctly parsed.

2. **IntermediateAttr is created correctly**:
   `treesitter.rs` lines 982-1001 handle `attribute_specifier` nodes and create `IntermediateAttr`.

3. **But the Attr disappears in output**:
   ```json
   {"t": "Math", "s": 6},
   {"t": "Space", "s": 7},
   {"t": "Space", "s": 8},  // ← No Attr here!
   {"t": "Str", "c": "shows"}
   ```

### The Bug

**Location**: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/paragraph.rs` lines 19-28

```rust
for (node, child) in children {
    if node == "block_continuation" {
        continue; // skip block continuation nodes
    }
    if let PandocNativeIntermediate::IntermediateInline(inline) = child {
        inlines.push(inline);
    } else if let PandocNativeIntermediate::IntermediateInlines(inner_inlines) = child {
        inlines.extend(inner_inlines);
    }
    // ← IntermediateAttr is silently DROPPED here!
}
```

**The paragraph processor only handles**:
- `IntermediateInline` → push to inlines
- `IntermediateInlines` → extend inlines
- Everything else (including `IntermediateAttr`) → **SILENTLY IGNORED**

When an `attribute_specifier` node is processed, it returns `IntermediateAttr(Attr, AttrSourceInfo)`, but the paragraph processor doesn't have a case for it, so it's dropped!

### Why This Wasn't Caught

- No tests for Math+Attr feature
- Silent dropping (no error or warning)
- Documentation exists but not tested against implementation

## The Fix

### Option 1: Convert IntermediateAttr to Inline::Attr in paragraph processor (RECOMMENDED)

**Change**: `paragraph.rs` lines 23-28

```rust
if let PandocNativeIntermediate::IntermediateInline(inline) = child {
    inlines.push(inline);
} else if let PandocNativeIntermediate::IntermediateInlines(inner_inlines) = child {
    inlines.extend(inner_inlines);
} else if let PandocNativeIntermediate::IntermediateAttr(attr, attr_source) = child {
    // Convert IntermediateAttr to Inline::Attr
    inlines.push(Inline::Attr(attr, attr_source));
}
```

**Pros**:
- Minimal change
- Matches the existing pattern
- Attrs become available in paragraph inlines

**Cons**:
- None

### Option 2: Convert IntermediateAttr to IntermediateInline earlier (NOT RECOMMENDED)

Change `treesitter.rs` to wrap IntermediateAttr in IntermediateInline immediately.

**Pros**:
- Would work

**Cons**:
- More invasive
- Loses clarity about what's an attr vs inline
- Doesn't match the pattern for other intermediate types

## Implementation Plan

### Step 1: Add IntermediateAttr handling to paragraph.rs

**File**: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/paragraph.rs`

**Change** (lines 19-28):

```rust
let mut inlines: Vec<Inline> = Vec::new();
for (node, child) in children {
    if node == "block_continuation" {
        continue; // skip block continuation nodes
    }
    if let PandocNativeIntermediate::IntermediateInline(inline) = child {
        inlines.push(inline);
    } else if let PandocNativeIntermediate::IntermediateInlines(inner_inlines) = child {
        inlines.extend(inner_inlines);
    } else if let PandocNativeIntermediate::IntermediateAttr(attr, attr_source) = child {
        // Attributes can appear in paragraphs (e.g., after math expressions)
        // Convert to Inline::Attr so postprocessing can handle them
        inlines.push(Inline::Attr(attr, attr_source));
    }
}
```

### Step 2: Write comprehensive tests

**Test file**: `crates/quarto-markdown-pandoc/tests/test_math_attr_feature.rs`

**Test cases**:

1. **Basic inline math with ID**
   ```qmd
   $E = mc^2$ {#eq-einstein}
   ```
   Expected: Span with id="eq-einstein", class="quarto-math-with-attribute", containing Math

2. **Inline math with class**
   ```qmd
   $x$ {.equation}
   ```
   Expected: Span with classes=["quarto-math-with-attribute", "equation"]

3. **Inline math with multiple attributes**
   ```qmd
   $x$ {#eq1 .equation key="value"}
   ```
   Expected: Span with all attributes preserved

4. **Display math with attributes**
   ```qmd
   $$
   \int_0^\infty e^{-x^2} dx
   $$ {#eq-gaussian}
   ```
   Expected: Span wrapping display Math

5. **Math with Space before attribute**
   ```qmd
   $x$ {.eq}
   ```
   (explicit space) Expected: Space is consumed, Span is created

6. **Math without attribute (regression)**
   ```qmd
   $x$
   ```
   Expected: Just Math, no Span

7. **Multiple math expressions with attributes**
   ```qmd
   $x$ {#eq1} and $y$ {#eq2}
   ```
   Expected: Two Spans

8. **Attribute alone (should not crash)**
   ```qmd
   Some text {.class} more text
   ```
   Expected: Attr should appear in inlines (might be consumed by adjacent element or left as-is)

### Step 3: Verify postprocess code still works

The postprocess code at `postprocess.rs:644-681` expects to find `Inline::Attr` in the inlines. After our fix, it should now find them!

Verify:
- Math + Attr pattern is detected
- Span is created with correct classes
- Math is wrapped properly

### Step 4: Test the documentation example

**File**: `docs/syntax/desugaring/math-attributes.qmd`

Run the example and verify it produces the expected JSON output shown in the documentation.

```bash
echo 'The famous equation $E = mc^2$ {#eq-einstein} shows the relationship.' | \
  cargo run --quiet --bin quarto-markdown-pandoc -- -t json | \
  jq '.blocks[0].c' | \
  grep -q '"t": "Span"'
```

Should find a Span in the output.

### Step 5: Check for other places that might drop IntermediateAttr

Search for other processors that might have the same bug:

```bash
rg "IntermediateInline|IntermediateInlines" crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/ -A 3
```

Look for pattern matching on intermediate types that might be dropping IntermediateAttr.

**Known safe locations**:
- Block processors (don't expect inlines)
- List processors (handle block content)

**Need to check**:
- Note content processing
- List item processing
- Quote processing
- Any other inline container

## Testing Strategy

### Unit Tests
Test the paragraph processor specifically:
- Paragraph with Math + Attr converts to Para with Math, Inline::Attr
- After postprocessing, becomes Para with Span

### Integration Tests
Test end-to-end QMD → JSON conversion:
- All test cases listed above
- Verify JSON structure matches expected output

### Regression Tests
- Math without attributes still works
- Attributes in other contexts still work (headings, etc.)
- All existing tests still pass

### Documentation Test
The example in `docs/syntax/desugaring/math-attributes.qmd` should work.

## Success Criteria

- ✅ `$E = mc^2$ {#eq-einstein}` produces a Span wrapping Math
- ✅ The Span has `id="eq-einstein"` and class `"quarto-math-with-attribute"`
- ✅ All 8 test cases pass
- ✅ Documentation example works
- ✅ All existing tests pass (no regressions)
- ✅ No other processors have the same IntermediateAttr dropping bug

## Estimated Effort

- Fix in paragraph.rs: 10 minutes
- Write tests: 2-3 hours
- Check other processors: 30 minutes
- Test and debug: 1 hour
- **Total**: 4-4.5 hours

## Notes

This is a **critical bug** - a documented feature is completely non-functional. The fix is simple (3 lines), but we need comprehensive tests to:
1. Verify the fix works
2. Prevent regression
3. Cover all the cases mentioned in documentation

After fixing this bug, we can then proceed with k-372 (improving source tracking for the Span wrapper).
