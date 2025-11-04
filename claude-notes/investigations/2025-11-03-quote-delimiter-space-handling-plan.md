# Quote Delimiter Space Handling - Investigation and Plan

## Date: 2025-11-03

## Issue
Test `tests/writers/json/quoted.md` fails because the Space between two quoted elements is missing.

Input: `'single quote' "double quote"`

**Expected output:**
```
Para [Quoted SingleQuote [...], Space, Quoted DoubleQuote [...]]
```

**Our output:**
```
Para [Quoted SingleQuote [...], Quoted DoubleQuote [...]]
```

Missing: The `Space` inline element between the two Quoted elements.

## Root Cause Analysis

### The Problem is NOT About Trimming

Initially suspected our recent `trim_inlines` fix for pipe tables was causing this. However, investigation shows this is a completely different issue.

### The Real Problem: Quote Delimiters Capture Adjacent Spaces

The tree-sitter grammar, like with emphasis/strong/code-span delimiters, can capture adjacent spaces as part of quote delimiters.

#### Evidence from Parse Tree

File bytes:
```
Byte 13: '  (closing single quote)
Byte 14:    (SPACE)
Byte 15: "  (opening double quote)
```

Parse tree:
```
pandoc_single_quote: (0, 0) - (0, 14)
  single_quote: (0, 0) - (0, 1)         # Just '
  content: (0, 1) - (0, 13)              # "single quote"
  single_quote: (0, 13) - (0, 14)       # Just '

pandoc_double_quote: (0, 14) - (0, 29)
  double_quote: (0, 14) - (0, 16)       # INCLUDES SPACE! " (2 bytes)
  content: (0, 16) - (0, 28)              # "double quote"
  double_quote: (0, 28) - (0, 29)       # Just "
```

Lexer output confirms:
```
lexed_lookahead sym:double_quote, size:2  ← TWO bytes for opening delimiter!
```

**The opening `double_quote` delimiter is capturing both the space (byte 14) AND the quote character (byte 15).**

This is exactly analogous to:
- Emphasis: ` *text*` → delimiter captures leading space
- Strong: `**text** ` → delimiter captures trailing space
- Code span: `` `code` `` with spaces in delimiters

### Current Implementation

`process_quoted` in `quote_helpers.rs`:
```rust
pub fn process_quoted(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    quote_type: QuoteType,
    delimiter_name: &str,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content_inlines: Vec<Inline> = Vec::new();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            _ if node_name == delimiter_name => {} // Skip delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateInline(Inline::Quoted(Quoted {
        quote_type,
        content: content_inlines,
        source_info: node_source_info_with_context(node, context),
    }))
}
```

**Problem:** Returns `IntermediateInline` (single inline), doesn't check for/emit delimiter spaces.

### How Other Inline Elements Handle This

#### Emphasis/Strong (`process_inline_with_delimiter_spaces`)

Located in `text_helpers.rs:226-329`:
```rust
pub fn process_inline_with_delimiter_spaces<F, G>(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    delimiter_name: &str,
    input_bytes: &[u8],
    context: &ASTContext,
    native_inline: F,
    create_inline: G,
) -> PandocNativeIntermediate
where
    F: FnMut((String, PandocNativeIntermediate)) -> Inline,
    G: FnOnce(Vec<Inline>, quarto_source_map::SourceInfo) -> Inline,
{
    // 1. Scan delimiters for leading/trailing spaces
    // 2. Calculate adjusted source range (excluding delimiter spaces)
    // 3. Create the inline element with adjusted range
    // 4. Return IntermediateInlines with Space nodes as needed
    // ...
}
```

Key features:
- Checks delimiter bytes for leading/trailing spaces
- Emits Space inline elements when needed
- Returns `IntermediateInlines` (Vec) not single Inline
- Calculates correct source ranges

#### Code Spans (`process_pandoc_code_span`)

Located in `code_span_helpers.rs:16-119`:
```rust
pub fn process_pandoc_code_span(...) -> PandocNativeIntermediate {
    // Check delimiters for spaces
    let mut has_leading_space = false;
    let mut has_trailing_space = false;
    // ...

    // Build result with injected Space nodes
    let mut result = Vec::new();
    if has_leading_space {
        result.push(Inline::Space(...));
    }
    result.push(code);
    if has_trailing_space {
        result.push(Inline::Space(...));
    }

    PandocNativeIntermediate::IntermediateInlines(result)
}
```

## Solution: Apply Same Pattern to Quotes

### Implementation Plan

Modify `process_quoted` in `crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/quote_helpers.rs` to:

1. **Add input_bytes parameter** (needed to check delimiter content)
2. **Check delimiters for spaces** (similar to code_span_helpers.rs approach)
3. **Emit Space nodes** when delimiters contain spaces
4. **Return IntermediateInlines** instead of IntermediateInline
5. **Update callers** in treesitter.rs to pass input_bytes

### Detailed Changes

#### Step 1: Modify `process_quoted` signature and implementation

```rust
use super::pandocnativeintermediate::PandocNativeIntermediate;
use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, QuoteType, Quoted, Space};  // Add Space
use crate::pandoc::location::node_source_info_with_context;

pub fn process_quoted(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    quote_type: QuoteType,
    delimiter_name: &str,
    input_bytes: &[u8],  // NEW PARAMETER
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content_inlines: Vec<Inline> = Vec::new();
    let mut has_leading_space = false;
    let mut has_trailing_space = false;
    let mut first_delimiter = true;

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            _ if node_name == delimiter_name => {
                // Check if delimiter includes spaces
                if let PandocNativeIntermediate::IntermediateUnknown(range) = child {
                    let text = std::str::from_utf8(&input_bytes[range.start.offset..range.end.offset])
                        .unwrap();
                    if first_delimiter {
                        // Opening delimiter - check for leading space
                        has_leading_space = text.starts_with(char::is_whitespace);
                        first_delimiter = false;
                    } else {
                        // Closing delimiter - check for trailing space
                        has_trailing_space = text.ends_with(char::is_whitespace);
                    }
                }
            }
            _ => {}
        }
    }

    let quoted = Inline::Quoted(Quoted {
        quote_type,
        content: content_inlines,
        source_info: node_source_info_with_context(node, context),
    });

    // Build result with injected Space nodes as needed
    let mut result = Vec::new();

    if has_leading_space {
        result.push(Inline::Space(Space {
            source_info: node_source_info_with_context(node, context),
        }));
    }

    result.push(quoted);

    if has_trailing_space {
        result.push(Inline::Space(Space {
            source_info: node_source_info_with_context(node, context),
        }));
    }

    PandocNativeIntermediate::IntermediateInlines(result)
}
```

#### Step 2: Update callers in treesitter.rs

File: `crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs`

Lines 824-837:

**Before:**
```rust
"pandoc_single_quote" => process_quoted(
    node,
    children,
    QuoteType::SingleQuote,
    "single_quote",
    context,
),
"pandoc_double_quote" => process_quoted(
    node,
    children,
    QuoteType::DoubleQuote,
    "double_quote",
    context,
),
```

**After:**
```rust
"pandoc_single_quote" => process_quoted(
    node,
    children,
    QuoteType::SingleQuote,
    "single_quote",
    input_bytes,  // NEW
    context,
),
"pandoc_double_quote" => process_quoted(
    node,
    children,
    QuoteType::DoubleQuote,
    "double_quote",
    input_bytes,  // NEW
    context,
),
```

## Expected Impact

### Tests That Will Be Fixed
- `tests/writers/json/quoted.md` in `test_json_writer`
- Possibly other tests involving quotes with adjacent spaces

### No Regressions Expected

This change:
- Only affects quote processing
- Follows the same pattern as emphasis/strong/code-span (already working)
- Returns IntermediateInlines which paragraph processor already handles correctly
- Adds Space nodes only when delimiters actually contain spaces

### Edge Cases Handled

1. **No spaces in delimiters**: `'quote'` → Just returns Quoted (no extra Spaces)
2. **Leading space**: ` 'quote'` → Returns [Space, Quoted]
3. **Trailing space**: `'quote' ` → Returns [Quoted, Space]
4. **Both spaces**: ` 'quote' ` → Returns [Space, Quoted, Space]
5. **Between quotes**: `'a' 'b'` → Returns [Quoted, Space, Quoted] ✅ (our failing case)

## Test Case Analysis

Input: `'single quote' "double quote"`

**Current behavior:**
- Grammar captures ` "` (space-quote) as opening delimiter for double quote
- `process_quoted` ignores the space, returns single Quoted inline
- Paragraph gets: [Quoted, Quoted] ❌

**After fix:**
- `process_quoted` detects leading space in opening delimiter
- Emits Space before the Quoted
- Paragraph gets: [Quoted, Space, Quoted] ✅

## Files to Modify

1. **crates/quarto-markdown-pandoc/src/pandoc/treesitter_utils/quote_helpers.rs**
   - Add `input_bytes` parameter
   - Add `Space` import
   - Check delimiters for spaces
   - Build result with Space nodes
   - Return IntermediateInlines

2. **crates/quarto-markdown-pandoc/src/pandoc/treesitter.rs**
   - Update two callers (lines 824-837) to pass `input_bytes`

## Notes

- This is NOT related to our pipe table trim_inlines fix
- This is the same pattern as emphasis/strong/code-span delimiter space handling
- The grammar is working correctly - it's intentionally capturing adjacent spaces
- The Rust code just needs to handle it like other inline elements do
