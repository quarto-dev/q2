# k-308 Citation Parsing Analysis - November 2, 2025

## The Problem

Input: `[prefix @c1 suffix; @c2; @c3]`

**Expected** (Pandoc):
```
Cite [
  Citation { id="c1", prefix=["prefix"], suffix=[" ", "suffix"], mode=NormalCitation },
  Citation { id="c2", prefix=[], suffix=[], mode=NormalCitation },
  Citation { id="c3", prefix=[], suffix=[], mode=NormalCitation }
] ["[prefix @c1 suffix; @c2; @c3]"]
```

**Actual** (Our parser):
```
Span [
  Str "prefix", Space, 
  Cite [Citation { id="c1", mode=AuthorInText }] ["@c1"], 
  Space, Str "suffix;", Space,
  Cite [Citation { id="c2", mode=AuthorInText }] ["@c2"],
  Str ";", Space,
  Cite [Citation { id="c3", mode=AuthorInText }] ["@c3"]
]
```

## Root Cause

In `span_link_helpers.rs:114-133`, `process_pandoc_span()` only handles single citations:
```rust
if content_inlines.len() == 1 && <is single Cite> {
    // Convert AuthorInText → NormalCitation
    return unwrapped Cite;
}
```

For multiple citations, it falls through to creating a Span.

## Tree-Sitter Structure

The grammar provides a flat list:
- pandoc_str: "prefix"
- citation: @c1 (mode: AuthorInText)
- pandoc_space
- pandoc_str: "suffix;"
- citation: @c2 (mode: AuthorInText)
- pandoc_str: ";"
- citation: @c3 (mode: AuthorInText)

There's NO structured grouping of prefix/suffix with citations.

## Fix Strategy

Need to add logic in `process_pandoc_span()` to:

1. **Detect multi-citation pattern**: 
   - No target, no attributes
   - Contains multiple Cite objects mixed with text
   
2. **Parse prefix/suffix/separators**:
   - Prefix: all inlines before first citation
   - Between citations: look for semicolon separators
   - Text before semicolon = suffix for previous citation
   - Text after semicolon = prefix for next citation (if any)
   
3. **Merge citations**:
   - Extract Citation from each Cite
   - Set prefix/suffix appropriately
   - Change all modes to NormalCitation
   - Create single Cite with vec of Citations

4. **Handle edge cases**:
   - Single citation with prefix/suffix: `[prefix @c1 suffix]`
   - Citation without brackets: `@c1` (already works - AuthorInText)
   - Suppress author: `[-@c1]`
   - Multiple separators: `@c1; @c2, @c3` (comma vs semicolon?)

## Complexity Concerns

This is a non-trivial parser that needs to:
- Scan through mixed inline content
- Identify citation boundaries
- Parse semicolon separators
- Distribute text correctly as prefix/suffix
- Handle whitespace correctly

## Alternative Approach?

Could we fix this in the tree-sitter grammar instead? Give it structure like:
```
citation_group:
  prefix: ...
  citation: @c1
  suffix: ...
  separator: ;
  citation: @c2
  ...
```

But that would be a much larger change and might conflict with other grammar constraints.

## Recommendation

Implement the fix in `process_pandoc_span()`, but:
1. Write comprehensive tests first
2. Handle one case at a time
3. Start with simple multi-citation: `[@c1; @c2]`
4. Then add prefix: `[prefix @c1; @c2]`
5. Then add suffix: `[@c1 suffix; @c2]`
6. Finally complex: `[prefix @c1 suffix; @c2; @c3]`
