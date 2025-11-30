# Flip-Flop Formatting / NoDecor Investigation

**Date**: 2025-11-30
**Status**: Partially Fixed
**Issue**: k-432

## Summary

The flip-flop formatting tests involve the `nodecor` (no decoration) class which should prevent formatting (italic, bold, small-caps) from being applied to specific spans of text within a formatted context.

## Root Cause Found and Fixed

### The Bug

The issue was in `extract_display_regions` (output.rs:1082-1084):

```rust
Output::Tagged { child, .. } => {
    // Tags are transparent - look inside
    extract_display_regions(child)
}
```

When extracting display regions for block-level rendering, the function looked **through** Tagged nodes and extracted from their children. Later, when accumulating children without display attributes, it pushed the region's content (the child of the Tagged, not the Tagged itself), **losing the Tag wrapper**.

### Data Flow Example

For the title `"Lessard <span class="nodecor">v.</span> Schmidt"` with italic formatting:

1. Parsing creates: `Output::Tagged(NoDecoration, Literal("v."))`
2. `extract_display_regions` on Tagged returns regions from the child
3. When rebuilding, the child `Literal("v.")` is used, not `Tagged(NoDecoration, Literal("v."))`
4. The NoDecoration tag is lost!

### The Fix (output.rs:1082-1104)

```rust
Output::Tagged { child, .. } => {
    // Tags like NoDecoration and NoCase must be preserved as-is.
    // Check if the child has any display regions that need extraction.
    let child_regions = extract_display_regions(child);
    if child_regions.iter().all(|r| r.display.is_none()) {
        // No display in child - preserve the whole Tagged node
        vec![DisplayRegion {
            display: None,
            content: output.clone(),
        }]
    } else {
        // There's display inside - return child regions
        child_regions
    }
}
```

### Why Haskell Works Differently

The Haskell implementation doesn't have this "extract display regions" step. It applies formatting through function composition and preserves all structure. The `CslNoDecoration` wrapper is never stripped.

## Current Status After Fix

### What's Now Working

The nodecor rendering is now working through the full pipeline:
- `<span style="font-style:normal;">v.</span>` now appears in output
- All existing 741 tests still pass

### What's Still Blocking flipflop Tests

**1. Suffix Placement (affixesInside)**: Several tests including `flipflop_ItalicsWithOk`
- Expected: `Schmidt</i>. 1972` (suffix after closing `</i>`)
- Actual: `Schmidt. </i>1972` (suffix before closing `</i>`)
- This is the same as the deferred `simplespace_case1` issue
- Requires implementing `formatAffixesInside` flag like Pandoc citeproc

**2. HTML Entity Parsing in Text Values**: `flipflop_BoldfaceNodeLevelMarkup`
- CSL has: `<text value="&#60;b&#62;friend&#60;/b&#62;"/>`
- These HTML entities (`&#60;` = `<`) should be unescaped and interpreted as markup
- We're outputting the escaped entities instead

**3. Quote/Apostrophe Handling**: `flipflop_ApostropheInsideTag`
- Expected: `l'''`
- Actual: `l'`
- Missing smart quote handling inside tags

## Test Results Summary

```
flipflop tests: 11 passing, 8 failing

Passing (11):
- flipflop_CompleteCiteInPrefix
- flipflop_LeadingMarkupWithApostrophe
- flipflop_LeadingSingleQuote
- flipflop_ItalicsSimple
- flipflop_ItalicsFlipped
- flipflop_Apostrophes
- flipflop_LongComplexPrefix
- flipflop_SmallCaps
- flipflop_SingleQuotesOnItalics
- (and 2 more)

Failing (8) - blocked by other issues:
- flipflop_ItalicsWithOk (suffix placement)
- flipflop_ItalicsWithOkAndTextcase (suffix placement + textcase)
- flipflop_BoldfaceNodeLevelMarkup (HTML entity parsing)
- flipflop_ApostropheInsideTag (quote handling)
- flipflop_QuotesNodeLevelMarkup (quote handling)
- flipflop_QuotesInFieldNotOnNode (quote handling)
- flipflop_OrphanQuote (quote handling)
- flipflop_SingleBeforeColon (unknown)
```

## Next Steps

1. **Cannot enable flipflop tests yet**: The remaining failures are blocked by other architectural issues (affixesInside, HTML entity parsing, quote handling)

2. **Consider deferring remaining flipflop tests** with documented reasons:
   - `flipflop_ItalicsWithOk` - blocked by affixesInside (same as simplespace_case1)
   - `flipflop_BoldfaceNodeLevelMarkup` - blocked by HTML entity parsing in text values
   - `flipflop_ApostropheInsideTag` - blocked by quote handling

3. **The nodecor fix is complete**: The core rendering logic works; it's other issues blocking these tests

## Related Files

- `crates/quarto-citeproc/src/output.rs` - Main output processing (fix at line 1082-1104)
- `crates/quarto-citeproc/src/eval.rs` - CSL evaluation
- `crates/quarto-citeproc/tests/deferred_tests.txt` - Deferred test documentation
