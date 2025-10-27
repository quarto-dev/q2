# K-237: Citation Component Ordering Issue

## Problem

Tree validation failing with:
```
Components should be in source order: Space start 811 >= citation-id start 813
```

**Document**: academic-paper.qmd  
**Location**: Around position 811-813, which is in a multi-citation: `[@doe2020; @smith2020]`

## Analysis

### Current State
When converting `Cite` inline elements, components are ordered as:
1. Content (the rendered text like "@doe2020")
2. Citation metadata (citation-id, prefix, suffix for each citation)

### The Issue
In multi-citation contexts like `[@doe2020; @smith2020]`, there are:
- Multiple citation-id components (one per citation)
- Space components between citations (the `;` separator and spaces)

**Current component ordering**: `[Str("@doe2020"), Space, Str("@smith2020"), citation-id-1, citation-id-2]`  
**Expected ordering**: `[Str("@doe2020"), citation-id-1, Space, Str("@smith2020"), citation-id-2]`

The citation metadata should be interleaved with the content, not all placed at the end.

### Root Cause

In `inline-converter.ts` lines 240-251:
```typescript
case 'Cite':
  return {
    components: [
      ...inline.c[1].map(child => this.convertInline(child)),  // All content
      ...inline.c[0].flatMap(citation => this.convertCitation(citation))  // All citations
    ],
  };
```

This flattens all content first, then all citation metadata. But in source order, they should be interleaved.

### Citation Structure

From the Rust parser, a `Cite` has:
- `citations: Vec<Citation>` - array of citation objects, each with:
  - `citationId` + `citationIdS` (source location)
  - `citationPrefix` (inlines before the ID)
  - `citationSuffix` (inlines after the ID)
- `content: Inlines` - the rendered text (already includes separators)

The `content` field contains ALL the rendered text including:
- The citation IDs as strings
- Spaces and separators between citations

But the `citations` array contains the metadata about each citation, including source locations for the IDs.

### Solution Approach

The TypeScript converter needs to:
1. Parse through the content to find where each citation appears
2. Insert the citation-id component at the correct position in source order
3. Maintain the interleaving of content and citation metadata

This is complex because:
- The content is already flattened (Str, Space, Str, Space, ...)
- The citation metadata is separate (array of citation objects)
- We need to match them up by source position

### Example

Source: `[@doe2020; @smith2020]`

**Current output**:
```
Cite [
  Str "@doe2020" [805-813]
  Space [813-814]
  Str "@smith2020" [814-824]
  citation-id "doe2020" [806-813]
  citation-id "smith2020" [815-824]
]
```

**Desired output**:
```
Cite [
  Str "@doe2020" [805-813]
  citation-id "doe2020" [806-813]
  Space [813-814]
  Str "@smith2020" [814-824]  
  citation-id "smith2020" [815-824]
]
```

## Related Work

- Fixed similar issues for Image/Link (k-234) - components were [attr, content, target] but should be [content, target, attr]
- Fixed similar issues for Table (k-236) - components were [attr, caption, ...] but should be [..., caption, attr]
- The pattern is: components should be ordered by their source position

## Next Steps

1. Modify `convertCitation` to return components in source order
2. Consider if we need to sort all components by start position after conversion
3. Add regression test for multi-citation ordering
4. Verify fix doesn't break single-citation cases

## Files

- `ts-packages/annotated-qmd/src/inline-converter.ts` - Cite conversion (lines 240-251)
- `ts-packages/annotated-qmd/test/tree-validation.test.ts` - Failing test
- `ts-packages/annotated-qmd/examples/academic-paper.qmd` - Test document with multi-citations
