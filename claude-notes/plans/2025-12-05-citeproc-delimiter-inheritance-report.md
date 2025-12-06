# Citeproc Delimiter Inheritance Bug Report

## Summary

This document analyzes a bug in `quarto-citeproc` where CSL group delimiters were not being inherited through `<choose>` elements, causing bibliography entries to have missing spacing/punctuation. The bug has been fixed, and this report includes recommendations for refactoring to prevent similar issues.

## The Bug

### Symptoms

When using the Chicago author-date style (or any style with a similar structure), bibliography entries were missing delimiters:

**Expected:**
```
Jones, Alice. 2019. An Important Book. Academic Press.
```

**Actual (buggy):**
```
Jones, Alice2019An Important BookAcademic Press.
```

### Root Cause

The bug was in `evaluate_choose` in `quarto-citeproc/src/eval.rs`. When evaluating a `<choose>` element's branches, the function always passed an empty delimiter to `evaluate_elements`:

```rust
fn evaluate_choose(ctx: &mut EvalContext, choose_el: &ChooseElement) -> Result<Output> {
    for branch in &choose_el.branches {
        if branch.conditions.is_empty() {
            return evaluate_elements(ctx, &branch.elements, "");  // BUG: empty delimiter
        }
        if matches {
            return evaluate_elements(ctx, &branch.elements, "");  // BUG: empty delimiter
        }
    }
    Ok(Output::Null)
}
```

This meant that when a CSL style had a structure like:

```xml
<group delimiter=". ">
  <choose>
    <else>
      <text macro="author"/>
      <text macro="date"/>
      <text macro="title"/>
    </else>
  </choose>
</group>
```

The group's `delimiter=". "` was never applied to the choose branch's children.

### The Fix

The fix adds an `inherited_delimiter` parameter that flows through the evaluation chain:

1. `evaluate_elements` passes the delimiter to `evaluate_element`
2. `evaluate_element` passes it to `evaluate_choose` for choose elements
3. `evaluate_choose` uses the inherited delimiter when evaluating branches

```rust
fn evaluate_choose(
    ctx: &mut EvalContext,
    choose_el: &ChooseElement,
    inherited_delimiter: &str,  // NEW: inherited from parent
) -> Result<Output> {
    for branch in &choose_el.branches {
        if branch.conditions.is_empty() {
            return evaluate_elements(ctx, &branch.elements, inherited_delimiter);
        }
        if matches {
            return evaluate_elements(ctx, &branch.elements, inherited_delimiter);
        }
    }
    Ok(Output::Null)
}
```

## Code Duplication Analysis

### The Problem

The `quarto-citeproc` crate has two parallel code paths for converting the Output AST to final output:

1. **`render()` → String**: Used by CSL conformance tests and `generate_bibliography()`
2. **`to_blocks()`/`to_inlines()` → Pandoc AST**: Used by the citeproc filter

Both paths implement similar logic for:
- Applying delimiters between elements
- Smart punctuation handling (not duplicating periods, etc.)
- Applying prefix/suffix
- Handling text case transformations
- Handling display attributes

### Evidence of Duplication

**In `output.rs`, the `render()` method (lines ~358-400):**
```rust
let inner: String = if let Some(ref delim) = formatting.delimiter {
    let rendered: Vec<String> = children
        .iter()
        .map(|c| c.render())
        .filter(|s| !s.is_empty())
        .collect();
    join_with_smart_delim(rendered, delim)
} else {
    let rendered: Vec<String> = children.iter().map(|c| c.render()).collect();
    fix_punct(rendered).join("")
};
```

**In `output.rs`, the `to_inlines_inner()` function (lines ~1427-1450):**
```rust
let with_delimiters: Vec<Vec<Inline>> = if let Some(ref delim) = formatting.delimiter {
    let mut result = Vec::new();
    for (i, child_inlines) in child_results.into_iter().enumerate() {
        if i > 0 && !delim.is_empty() {
            let first_char = get_leading_char(&child_inlines);
            if !matches!(first_char, Some(',') | Some(';') | Some('.')) {
                result.push(vec![Inline::Str(Str { text: delim.clone(), ... })]);
            }
        }
        result.push(child_inlines);
    }
    result
} else {
    child_results
};
```

Both implement:
- Delimiter application between children
- Smart punctuation collision avoidance
- Filtering empty children

### Why This Bug Slipped Through

The CSL conformance tests use `render()` → `to_blocks()` → `render_blocks_to_csl_html()`. This path goes through `to_blocks()` for structural transformation but uses a custom HTML renderer for final output.

The bug was in `eval.rs`, which affects both paths equally. However:

1. The CSL conformance tests were passing because the test styles in the CSL test suite don't all use the `<group delimiter><choose>` pattern as extensively as Chicago author-date.
2. When a test did use this pattern, the test expectation was based on our buggy output (if we generated the expected output ourselves).

## Recommendations

### Immediate: Consider Unifying Rendering Logic

The current duplication means:
- Bug fixes must be applied in multiple places
- Behavior can diverge between `render()` and `to_inlines()`
- Testing is more complex

**Option A: Single Abstract Visitor**

Create an abstract visitor pattern that can produce either String or Pandoc Inlines:

```rust
trait OutputVisitor {
    type Result;
    fn visit_literal(&mut self, s: &str) -> Self::Result;
    fn visit_formatted(&mut self, formatting: &Formatting, children: Vec<Self::Result>) -> Self::Result;
    // etc.
}

struct StringVisitor;
impl OutputVisitor for StringVisitor {
    type Result = String;
    // ...
}

struct InlineVisitor;
impl OutputVisitor for InlineVisitor {
    type Result = Vec<Inline>;
    // ...
}
```

This ensures identical traversal and delimiter handling.

**Option B: Intermediate Representation**

Convert Output AST to an intermediate representation (like a flat list of tokens with formatting metadata), then render that to either String or Inlines.

**Option C: Generate String, Parse to Inlines**

For bibliography entries, generate the formatted string using `render()`, then parse it back into Pandoc Inlines. This is less efficient but guarantees consistency.

### Testing Recommendations

1. **Add delimiter inheritance tests**: The tests added in this fix (`test_choose_inherits_group_delimiter`) should be part of the standard test suite.

2. **Equivalence testing**: Add tests that verify `render()` and `to_inlines()` produce equivalent semantic output:
   ```rust
   fn test_render_and_to_inlines_equivalence() {
       let output = /* generate Output */;
       let rendered = output.render();
       let inlines = output.to_inlines();
       let inline_text = inlines_to_plain_text(&inlines);
       assert_eq!(rendered, inline_text);
   }
   ```

3. **End-to-end style testing**: Add tests that verify real CSL styles (Chicago, APA, IEEE) produce correct output through both the `generate_bibliography()` and citeproc filter paths.

### Long-term: Architectural Consideration

The separation between `eval.rs` (CSL evaluation) and `output.rs` (Output AST rendering) is good. However, the current design has two issues:

1. **Delimiter handling is split**: Delimiters are stored in `Formatting` during evaluation but applied during rendering. This means eval must correctly propagate delimiters, and render must correctly apply them.

2. **Choose is special**: The `<choose>` element is "transparent" for delimiter inheritance, but this isn't obvious from the code structure.

Consider documenting these design decisions clearly and adding invariant checks:

```rust
/// Choose elements are transparent for delimiter inheritance.
/// This means the delimiter from a parent group flows through choose
/// to the branch elements.
fn evaluate_choose(
    ctx: &mut EvalContext,
    choose_el: &ChooseElement,
    inherited_delimiter: &str,
) -> Result<Output> {
    // ...
}
```

## Files Changed

1. **`quarto-citeproc/src/eval.rs`**:
   - Added `inherited_delimiter` parameter to `evaluate_element`
   - Added `inherited_delimiter` parameter to `evaluate_choose`
   - Updated `evaluate_elements` to pass delimiter to `evaluate_element`
   - Added tests: `test_choose_inherits_group_delimiter`, `test_choose_without_parent_delimiter`

2. **`quarto-markdown-pandoc/tests/test_citeproc_integration.rs`** (new file):
   - Added integration tests for bibliography delimiter handling
   - Tests end-to-end pipeline through the command-line binary

## Test Coverage

- **quarto-citeproc**: 820 tests pass (including 2 new tests for this fix)
- **quarto-markdown-pandoc**: 862 tests pass (including 2 new integration tests)
- No regressions detected
