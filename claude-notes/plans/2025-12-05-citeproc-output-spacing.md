# Fix Bibliography Spacing in quarto-citeproc to_blocks() Conversion

**Beads Issue:** k-vku8
**Created:** 2025-12-05
**Status:** Planning

## Problem Statement

When using the `-F citeproc` filter in quarto-markdown-pandoc, bibliography entries have missing spacing and punctuation between elements. For example:

**Expected (Pandoc citeproc):**
```
Jones, Alice. 2019. An Important Book. Academic Press.
```

**Actual (our output):**
```
Jones, Alice2019An Important Book. Academic Press.
```

The issue manifests as:
1. Missing ". " between author name and year ("Alice2019" vs "Alice. 2019.")
2. Missing ". " between year and title
3. Missing space before quoted titles

## Root Cause Analysis

### How Output is Generated

1. **CSL Evaluation (eval.rs):** Processes CSL layout rules and generates an `Output` AST
2. **Output AST:** Tree structure with `Formatted`, `Literal`, `Tagged`, `Linked`, `InNote` nodes
3. **Rendering:** Two paths exist:
   - `render()` → String (used by CSL conformance tests, works correctly)
   - `to_blocks()` → Pandoc Blocks (used by citeproc filter, has spacing issues)

### The Delimiter Mechanism

CSL uses `<group delimiter=". ">` to insert punctuation between elements. In Chicago author-date:

```xml
<macro name="bibliography-author-date">
  <group delimiter=". ">
    <text macro="author-bib"/>
    <text macro="date"/>
    <text macro="title-and-source-bib"/>
  </group>
</macro>
```

The delimiter is stored in `Formatting.delimiter` and should be applied between non-empty children.

### Where the Bug Is

In `output.rs`, the `to_inlines_inner()` function handles `Output::Formatted` nodes:

```rust
// Step 2: Add delimiters between elements (as separate siblings)
let with_delimiters: Vec<Vec<Inline>> = if let Some(ref delim) = formatting.delimiter {
    // ...applies delimiter...
}
```

However, the delimiter stored in the `Formatting` struct may not be reaching this code path, OR the Output AST structure differs from what `render()` processes.

### Evidence

Comparing JSON output:

**Pandoc produces:**
```json
["Jones,", Space, "Alice.", Space, "2019.", Space, ...]
```

**Our output produces:**
```json
["Jones", ", ", "Alice", "2019", ...]
```

Key differences:
- Pandoc has periods IN the strings ("Alice." vs "Alice")
- Pandoc uses explicit `Space` inlines
- Our output is missing the delimiter suffixes

## Testing Strategy

### Principle: Test at Appropriate Boundaries

1. **quarto-citeproc tests:** Verify the `to_blocks()` output matches expected Pandoc structure
2. **quarto-markdown-pandoc tests:** Verify end-to-end HTML output is correct

### 1. Unit Tests in quarto-citeproc

Location: `crates/quarto-citeproc/src/output.rs` (add test module) or `crates/quarto-citeproc/tests/`

**Test approach A: Compare render() vs to_blocks() text content**

```rust
#[test]
fn test_bibliography_entry_spacing() {
    // Create a processor with chicago-author-date style
    // Add a reference
    // Generate bibliography with both methods
    // Assert that to_blocks() rendered as text matches render()
}
```

**Test approach B: Verify specific Pandoc inline structure**

```rust
#[test]
fn test_to_blocks_produces_correct_inlines() {
    // Create Output AST with known delimiter
    // Call to_blocks()
    // Assert specific Inline sequence exists (Str with period, Space, etc.)
}
```

### 2. Integration Tests in quarto-markdown-pandoc

Location: `crates/quarto-markdown-pandoc/tests/`

**Test approach: Verify HTML output**

```rust
#[test]
fn test_citeproc_bibliography_spacing() {
    let input = r#"---
title: Test
references:
- id: smith2020
  type: article-journal
  author:
    - family: Smith
      given: John
  title: A Paper
  issued:
    date-parts: [[2020]]
---

A citation [@smith2020].
"#;

    // Run through parser and citeproc filter
    // Render to HTML
    // Assert output contains "Smith, John. 2020." (with proper spacing)
}
```

### 3. Regression Prevention

- Run existing CSL conformance suite before/after changes
- Add new test cases for bibliography output (currently tests focus on citations)
- Consider adding "bibliography mode" tests to the conformance suite

## Implementation Plan

### Phase 1: Diagnosis (Investigation)

1. Add debug logging to trace Output AST structure for a bibliography entry
2. Compare the AST structure when calling `render()` vs `to_blocks()`
3. Identify exactly where the delimiter information is lost

### Phase 2: Fix Development

Based on investigation, likely fixes:

**Option A: Fix delimiter propagation in to_inlines_inner()**
- Ensure `Formatting.delimiter` is properly read and applied
- Verify children are correctly identified as non-empty

**Option B: Fix Output AST construction in eval.rs**
- Ensure group delimiters are stored in Formatting when evaluating bibliography
- Compare with how citations are evaluated

**Option C: Harmonize render() and to_blocks() logic**
- The `render()` path uses `join_with_smart_delim()` which handles delimiters correctly
- Port that logic to `to_inlines_inner()`

### Phase 3: Testing

1. Write failing unit test in quarto-citeproc
2. Implement fix
3. Verify unit test passes
4. Write failing integration test in quarto-markdown-pandoc
5. Verify integration test passes
6. Run full CSL conformance suite to check for regressions

### Phase 4: Cleanup

1. Remove any debug logging
2. Document the fix
3. Consider if similar issues exist for citations (not just bibliography)

## Files to Modify

### Primary (in quarto-citeproc)
- `src/output.rs`: Fix `to_inlines_inner()` delimiter handling
- `src/eval.rs`: Possibly fix how Formatting is constructed for bibliography

### Tests
- `crates/quarto-citeproc/tests/output_tests.rs` (new): Unit tests for to_blocks()
- `crates/quarto-markdown-pandoc/tests/citeproc_tests.rs` (new): Integration tests

## Success Criteria

1. Bibliography entries have correct spacing between all elements
2. CSL conformance suite pass rate does not decrease
3. Unit tests verify to_blocks() produces correct Pandoc structure
4. Integration tests verify HTML output is correctly formatted

## Comparison: render() vs to_blocks()

| Aspect | render() | to_blocks() |
|--------|----------|-------------|
| Output type | String | Vec<Block> |
| Delimiter handling | `join_with_smart_delim()` | Manual in `to_inlines_inner()` |
| Used by | CSL conformance tests | Citeproc filter |
| Current status | Works correctly | Missing delimiters |

The fix should align `to_blocks()` behavior with `render()` without breaking either path.

## Notes

- The `render()` method is extensively tested by the CSL conformance suite
- The `to_blocks()` method is newer and was added for Pandoc AST integration
- Both should produce semantically equivalent output
- Chicago author-date style is a good test case as it has clear delimiter requirements
