# K-233: Figure Block Source Range Bug in quarto-markdown-pandoc

**Date**: 2025-10-26
**Status**: Diagnosed - Ready for Rust fix
**Priority**: Critical (P0)
**Discovered By**: Tree validation tests in k-228 (Phase 4)

## Problem Summary

The Rust `quarto-markdown-pandoc` binary outputs Figure blocks with invalid source ranges `[0, 0]` in the `sourceInfoPool`. This causes all nested blocks within Figures (caption Plain blocks and content blocks) to also have invalid source ranges.

## Minimal Test Case

**File**: `ts-packages/annotated-qmd/examples/minimal-figure.qmd`

```markdown
---
title: "Minimal Figure Test"
---

# Simple Figure

![Figure caption text](image.png)

# Detailed Figure with Attributes

![Detailed caption with **bold**](image.png){#fig-test}
```

## Diagnosis

### Step 1: Tree validation detected invalid ranges

Tree validation tests found:
```
Plain with start=0, end=0:
  Path: Document[13] > Figure[0]
  Components: 3
  Text: "Quarto Logo"
```

### Step 2: Examined JSON output structure

Generated JSON shows Figure structure:
```json
{
  "t": "Figure",
  "s": 26,  // Source ID
  "c": [
    [...],  // attr
    [null, [{"t": "Plain", "s": 17, ...}]],  // caption
    [{"t": "Plain", "s": 25, ...}]  // content blocks
  ]
}
```

Both Plain blocks HAVE source IDs (17 and 25), as does the Figure itself (26).

### Step 3: Checked sourceInfoPool entries

```javascript
Source ID 17: { d: 0, r: [0, 0], t: 0 }  // Caption Plain
Source ID 25: { d: 0, r: [0, 0], t: 0 }  // Content Plain
Source ID 26: { d: 0, r: [0, 0], t: 0 }  // Figure itself
```

**All have `r: [0, 0]` - invalid ranges!**

### Step 4: Verified in TypeScript conversion

```javascript
Figure at: Document[2]
  Figure source: start=0, end=0        // ❌ Invalid
  Components: 2
  [0] Plain: start=0, end=0            // ❌ Invalid
  [1] Plain: start=0, end=0            // ❌ Invalid
```

## Root Cause

The Rust parser is not tracking source locations when constructing Figure blocks. Likely causes:

1. **Pandoc AST transformation**: Figures are synthetic structures created by Pandoc from images with captions. The source tracking may not be propagating through this transformation.

2. **Missing instrumentation**: The code that builds Figure blocks may not be calling the source tracking functions.

3. **Tree-sitter gaps**: The tree-sitter parser may not have Figure-specific rules, causing it to fall back to default [0, 0] ranges.

## Impact

- **Current**: 5 failing tree validation tests (all in k-228)
- **User impact**: Figure blocks cannot be properly linted or have accurate error messages pointing to source locations
- **Scope**: Affects ALL documents with figures (very common in Quarto)

## Location in Rust Code

The bug is in: `crates/quarto-markdown-pandoc/src/`

Look for:
- Figure block construction
- Source tracking for Pandoc AST nodes
- Tree-sitter instrumentation for images/figures

## Fix Strategy

### Option A: Track source from image syntax

Figures are created from `![caption](url)` syntax. The fix should:
1. Track the source range of the entire image markdown
2. Assign this range to the Figure block
3. Derive caption Plain block range from caption text
4. Derive content Plain block range from the image element

### Option B: Synthesize ranges from components

If direct tracking is hard:
1. Use the Image element's source range (which IS tracked)
2. Expand to include the caption text
3. Assign to the Figure and nested Plain blocks

## Testing

After fix, verify:
```bash
cargo run --bin quarto-markdown-pandoc -- -t json -i ts-packages/annotated-qmd/examples/minimal-figure.qmd > output.json

# Check source IDs 17, 25, 26 have valid ranges != [0, 0]
jq '.astContext.sourceInfoPool[17,25,26]' output.json
```

Expected:
```json
{ "d": 0, "r": [63, 85], "t": 0 }  // Caption
{ "d": 0, "r": [63, 89], "t": 0 }  // Content
{ "d": 0, "r": [63, 89], "t": 0 }  // Figure
```

## Related Issues

- **k-228**: Phase 4: Components Tree Validation (discovered this bug)
- **k-232**: Invalid source ranges for Plain/Figure blocks (TypeScript side manifestation)

## Success Criteria

✅ Figure blocks have valid source ranges
✅ Caption Plain blocks have valid source ranges
✅ Content Plain blocks have valid source ranges
✅ All 5 tree validation tests pass
✅ Minimal test case shows correct ranges in JSON output

## Notes

This is a **Rust parser bug**, not a TypeScript bug. The TypeScript annotated-qmd package is correctly reading and converting the JSON - it's the JSON itself that has invalid data.

Priority is Critical (P0) because:
1. Figures are very common in Quarto documents
2. Breaks source mapping for a major content type
3. Blocking completion of k-228 (tree validation)
