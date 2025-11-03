# k-308 Implementation Summary - November 2, 2025

## TL;DR

**We don't need to write 150-200 lines of new code!** The multi-citation parsing logic already exists in `make_cite_inline()` (inline.rs:413-520). We just need to call it from `process_pandoc_span()`.

## The Existing Code

**Function**: `make_cite_inline()` in `src/pandoc/inline.rs:413-520`

**Signature**:
```rust
pub fn make_cite_inline(
    attr: Attr,
    target: Target,
    content: Inlines,
    source_info: quarto_source_map::SourceInfo,
    attr_source: AttrSourceInfo,
    target_source: TargetSourceInfo,
) -> Inline
```

**What it does**:
1. Validates that content is citation-worthy (semicolon-separated segments each contain a Cite)
2. If not valid, backtracks and returns a Span via `make_span_inline()`
3. Splits content along semicolons
4. For each segment: extracts prefix (before Cite), suffix (after Cite)
5. Merges all citations with correct prefix/suffix distribution
6. Changes all modes from AuthorInText → NormalCitation

**Currently used by**: `inline_link.rs:88-95` (old grammar processor)

**NOT used by**: `span_link_helpers.rs` (new tree-sitter grammar processor)

## The Gap in New Grammar

`span_link_helpers.rs:process_pandoc_span()` currently only handles:
- Single citation: `[@cite]` → unwrap and change mode (lines 114-133)
- Multi-citation: `[prefix @c1 suffix; @c2]` → falls through to Span (WRONG!)

## The Fix

Replace the single-citation check in `span_link_helpers.rs:114-133` with a citation pattern check that calls `make_cite_inline()`:

```rust
// Around line 111 in span_link_helpers.rs

// Check if this looks like a citation (no target, no attributes)
if target.is_none() && is_empty_attr(&attr) {
    // Check if content contains any citations
    let has_citations = content_inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Cite(_)));

    if has_citations {
        // Use make_cite_inline to handle both single and multi-citation cases
        return PandocNativeIntermediate::IntermediateInline(
            make_cite_inline(
                attr,
                ("".to_string(), "".to_string()),  // empty target
                content_inlines,
                node_source_info_with_context(node, context),
                attr_source,
                TargetSourceInfo::empty(),
            )
        );
    }
}
```

## Why This Works

1. **Reuses battle-tested code** - `make_cite_inline()` already handles all the edge cases
2. **Minimal code changes** - ~15 lines changed in span_link_helpers.rs
3. **Covers all patterns**:
   - `[@c1]` - single citation
   - `[@c1; @c2]` - multiple citations
   - `[prefix @c1 suffix; @c2]` - with prefix/suffix
   - `[-@c1]` - suppress author mode
4. **Self-validating** - `make_cite_inline()` backtracks to Span if content isn't citation-worthy

## Test Coverage

`make_cite_inline()` already has test coverage:
- `inline.rs:578-619` - test_make_cite_inline_with_multiple_citations()

We'll need to add tests for the new code path in `span_link_helpers.rs`.

## Implementation Steps

1. ✅ Analysis complete (this document)
2. ⬜ Write test for failing behavior
3. ⬜ Run test to verify it fails as expected
4. ⬜ Modify `process_pandoc_span()` to call `make_cite_inline()`
5. ⬜ Run test to verify it passes
6. ⬜ Run full test suite
7. ⬜ Close k-308

## Code Location Reference

- **Existing logic**: `crates/quarto-markdown-pandoc/src/pandoc/inline.rs:413-520`
- **Old usage**: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/inline_link.rs:88-95`
- **New usage needed**: `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/span_link_helpers.rs:114-133`
- **Tests to add**: `crates/quarto-markdown-pandoc/tests/test_treesitter_refactoring.rs` or snapshot tests

## Related Issues

- k-274 (closed) - Tree-sitter grammar refactoring (where this logic got left behind)
- unit_test_snapshots_native - blocked by k-308
- test_qmd_roundtrip_consistency - blocked by k-308
