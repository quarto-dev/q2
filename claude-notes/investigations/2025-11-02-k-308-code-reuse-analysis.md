# k-308 Code Reuse Analysis - November 2, 2025

## Discovery

Found existing multi-citation parsing code in `src/pandoc/inline.rs:420-520` - the `make_cite_inline()` function!

This code was used by the **OLD grammar** (`inline_link.rs`) but is NOT being used by the **NEW tree-sitter grammar** (`span_link_helpers.rs`).

## How It Works

The `make_cite_inline()` function has sophisticated logic that:

### 1. Validates Citation Content (lines 425-435)
```rust
let is_semicolon = |inline: &Inline| match &inline {
    Inline::Str(Str { text, .. }) => text == ";",
    _ => false,
};

let is_good_cite = content.split(is_semicolon).all(|slice| {
    slice.iter().any(|inline| match inline {
        Inline::Cite(_) => true,
        _ => false,
    })
});
```

Checks that each semicolon-separated segment contains a Cite. If not, backtrack to Span.

### 2. Splits Content Along Semicolons (lines 453-454)
```rust
let citations: Vec<Citation> = content
    .split(is_semicolon)
    .flat_map(|slice| {
```

Processes each semicolon-separated segment independently.

### 3. Distributes Prefix/Suffix (lines 456-472)
```rust
let mut cite: Option<Cite> = None;
let mut prefix: Inlines = vec![];
let mut suffix: Inlines = vec![];

inlines.into_iter().for_each(|inline| {
    if cite == None {
        if let Inline::Cite(c) = inline {
            cite = Some(c);
        } else {
            prefix.push(inline);  // Before first Cite = prefix
        }
    } else {
        suffix.push(inline);  // After Cite = suffix
    }
});
```

Scans through each segment:
- Everything before first Cite → prefix
- Everything after → suffix

### 4. Handles Single vs Multiple Citations (lines 480-512)
```rust
if c.citations.len() == 1 {
    // Simple case: one citation, apply prefix and suffix directly
    let mut citation = c.citations.pop().unwrap();
    if citation.mode == CitationMode::AuthorInText {
        citation.mode = CitationMode::NormalCitation;
    }
    citation.prefix = prefix;
    citation.suffix = suffix;
    vec![citation]
} else {
    // Complex case: multiple citations already present
    // Apply prefix to the first citation and suffix to the last
    for (i, citation) in c.citations.iter_mut().enumerate() {
        if citation.mode == CitationMode::AuthorInText {
            citation.mode = CitationMode::NormalCitation;
        }
        if i == 0 {
            // Prepend prefix to the first citation's prefix
            let mut new_prefix = prefix.clone();
            new_prefix.extend(citation.prefix.clone());
            citation.prefix = new_prefix;
        }
        if i == num_citations - 1 {
            // Append suffix to the last citation's suffix
            citation.suffix.extend(suffix.clone());
        }
    }
    c.citations
}
```

### 5. Changes Mode to NormalCitation
All citations are converted from AuthorInText to NormalCitation when inside brackets.

## Current Usage

**OLD Grammar (still in use)**:
- `inline_link.rs:88-95` calls `make_cite_inline()`
- Handles: `[prefix @c1 suffix; @c2; @c3]`

**NEW Grammar (missing this logic)**:
- `span_link_helpers.rs:114-133` only handles single citations
- Missing: multi-citation support

## The Fix for k-308

We need to apply the same `make_cite_inline()` logic in `process_pandoc_span()`:

```rust
// In span_link_helpers.rs, around line 114

// Check if this is a multi-citation pattern
if target.is_none() && is_empty_attr(&attr) {
    // Detect if content has citations
    let has_citations = content_inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Cite(_)));

    if has_citations {
        // Call make_cite_inline to handle multi-citation parsing
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

## Why This Was Missed

During the tree-sitter grammar refactoring (k-274), the old `inline_link.rs` code path was replaced by `span_link_helpers.rs`, but only the simple single-citation case was ported over. The complex multi-citation logic in `make_cite_inline()` was left behind, unused.

## Benefits of This Approach

1. **Reuses existing, tested code** - `make_cite_inline()` has been working for the old grammar
2. **Minimal new code** - just need to call the function from the right place
3. **Handles all edge cases** - prefix, suffix, semicolons, mode changes
4. **Already has tests** - see `inline.rs:578-619`

## Implementation Plan

1. **Update `span_link_helpers.rs`** to detect multi-citation patterns
2. **Call `make_cite_inline()`** instead of creating a Span
3. **Write tests** to verify behavior matches Pandoc
4. **Run test suite** to ensure no regressions

This is a much simpler fix than the original plan of writing 150-200 lines of new parsing logic!
