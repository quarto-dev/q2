# Quoted Nodes Implementation Plan

**Date**: 2025-10-31
**Context**: Implement `pandoc_single_quote` and `pandoc_double_quote` node handlers

## Current Status

**✅ Verified**: Superscript and Subscript already work!
- `^superscript^` → `Superscript [Str "superscript"]` ✅
- `~subscript~` → `Subscript [Str "subscript"]` ✅

**❌ Broken**: Quoted text is currently lost in output
- Input: `'single quoted' text`
- Our output: `Str "text"` (missing quoted portion!)
- Expected: `Quoted SingleQuote [Str "single", Space, Str "quoted"], Space, Str "text"`

## Problem Analysis

### Tree-Sitter Grammar Structure

**Single Quote**: `pandoc_single_quote`
```
pandoc_single_quote
  single_quote      (opening delimiter)
  content           (optional _inlines)
    pandoc_str
    pandoc_space
    ...
  single_quote      (closing delimiter)
```

**Double Quote**: `pandoc_double_quote`
```
pandoc_double_quote
  double_quote      (opening delimiter)
  content           (optional _inlines)
    pandoc_str
    pandoc_space
    ...
  double_quote      (closing delimiter)
```

### Key Observations

1. **Similar to Emph/Strong pattern**: Opening delimiter, content, closing delimiter
2. **Content is `_inlines`**: Already processed by our content handler → `IntermediateInlines`
3. **Quotes can nest**: Single inside double, double inside single
4. **Content can be empty**: `''` and `""` are valid (though rare)
5. **Delimiters are aliased**:
   - `_single_quote_span_open/close` → `single_quote`
   - `_double_quote_span_open/close` → `double_quote`

## Pandoc Expected Output

```rust
// Single quotes
'text' → Quoted SingleQuote [ Str "text" ]

// Double quotes
"text" → Quoted DoubleQuote [ Str "text" ]

// Nested (double inside single)
'outer "inner" text' → Quoted SingleQuote [ Str "outer", Space, Quoted DoubleQuote [Str "inner"], Space, Str "text" ]

// Nested (single inside double)
"outer 'inner' text" → Quoted DoubleQuote [ Str "outer", Space, Quoted SingleQuote [Str "inner"], Space, Str "text" ]

// With formatting
"**bold** text" → Quoted DoubleQuote [ Strong [Str "bold"], Space, Str "text" ]
```

## Pandoc Data Structures

```rust
pub struct Quoted {
    pub quote_type: QuoteType,
    pub content: Inlines,
    pub source_info: SourceInfo,
}

pub enum QuoteType {
    SingleQuote,
    DoubleQuote,
}
```

No attributes needed - quotes don't support them!

## Implementation Plan

### Phase 1: Add Delimiter Handlers (5 min)
```rust
"single_quote" | "double_quote" => {
    // Delimiter nodes - marker only
    PandocNativeIntermediate::IntermediateUnknown(node_location(node))
}
```

### Phase 2: Add `pandoc_single_quote` Handler (15 min)
```rust
"pandoc_single_quote" => {
    let mut content_inlines: Vec<Inline> = Vec::new();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            "single_quote" => {} // Skip delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateInline(Inline::Quoted(Quoted {
        quote_type: QuoteType::SingleQuote,
        content: content_inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}
```

### Phase 3: Add `pandoc_double_quote` Handler (15 min)
```rust
"pandoc_double_quote" => {
    let mut content_inlines: Vec<Inline> = Vec::new();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            "double_quote" => {} // Skip delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateInline(Inline::Quoted(Quoted {
        quote_type: QuoteType::DoubleQuote,
        content: content_inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}
```

### Phase 4: Add Imports (2 min)
Add to inline imports:
```rust
use crate::pandoc::inline::{..., Quoted, QuoteType, ...};
```

### Phase 5: Write Comprehensive Tests (30 min)

Test cases needed:

```rust
// Basic single quote
"'text'" → Quoted SingleQuote [Str "text"]

// Basic double quote
"\"text\"" → Quoted DoubleQuote [Str "text"]

// Single quote in context
"before 'quoted' after" → [Str "before", Space, Quoted SingleQuote [...], Space, Str "after"]

// Double quote in context
"before \"quoted\" after" → [Str "before", Space, Quoted DoubleQuote [...], Space, Str "after"]

// Nested: single inside double
"\"outer 'inner' text\"" → Quoted DoubleQuote [Str "outer", Space, Quoted SingleQuote [...], Space, Str "text"]

// Nested: double inside single
"'outer \"inner\" text'" → Quoted SingleQuote [Str "outer", Space, Quoted DoubleQuote [...], Space, Str "text"]

// With formatting
"\"**bold** text\"" → Quoted DoubleQuote [Strong [...], Space, Str "text"]

// Multiple words
"'multiple word quote'" → Quoted SingleQuote [Str "multiple", Space, Str "word", Space, Str "quote"]

// Empty quotes (edge case)
"''" → Quoted SingleQuote []
"\"\"" → Quoted DoubleQuote []
```

### Phase 6: Verify All Tests Pass (5 min)

## Files to Modify

1. **`src/pandoc/treesitter.rs`**:
   - Add imports: `Quoted, QuoteType`
   - Add delimiter handlers: `single_quote`, `double_quote`
   - Add `pandoc_single_quote` handler
   - Add `pandoc_double_quote` handler

2. **`tests/test_treesitter_refactoring.rs`**:
   - Add 9 comprehensive tests for quoted text

## Success Criteria

- ✅ Single quotes parse correctly
- ✅ Double quotes parse correctly
- ✅ Nested quotes work (both directions)
- ✅ Quotes with formatting work
- ✅ All existing tests still pass
- ✅ Zero MISSING NODE warnings
- ✅ Output matches Pandoc exactly

## Estimate

- Phase 1 (delimiters): 5 minutes
- Phase 2 (single_quote handler): 15 minutes
- Phase 3 (double_quote handler): 15 minutes
- Phase 4 (imports): 2 minutes
- Phase 5 (tests): 30 minutes
- Phase 6 (verification): 5 minutes
- **Total**: ~1 hour

## Notes

1. **Pattern similar to Emph/Strong**: Same delimiter + content structure
2. **Content handler already works**: Returns `IntermediateInlines` for non-empty children
3. **No attributes**: Quotes don't support attributes (unlike spans/links)
4. **Nesting works automatically**: Content contains processed inlines, including nested Quoted
5. **Delimiters are simple markers**: Just skip them like we do for emph/strong

## Testing Notes

**⚠️ IMPORTANT**: Use files for testing, NOT echo!
- Shell echo has issues with quote characters
- Always create test files and use `cargo run -- -i file.md`
- Use tree-sitter parse from grammar directory for structure inspection

## Comparison with Similar Nodes

| Feature | Emph | Strong | Quoted |
|---------|------|--------|--------|
| Delimiters | Yes (*/_) | Yes (**/__) | Yes ('/") |
| Content | Inlines | Inlines | Inlines |
| Attributes | No | No | No |
| Nesting | Yes | Yes | Yes |
| Quote Type | N/A | N/A | Single/Double |

The implementation is almost identical to Emph/Strong, just need to:
1. Handle two node types instead of one
2. Set appropriate `quote_type` field
3. Skip appropriate delimiter names

## References

- Grammar: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
- Inline types: `crates/quarto-markdown-pandoc/src/pandoc/inline.rs`
- Similar patterns: See `pandoc_emph` and `pandoc_strong` handlers in `treesitter.rs`
