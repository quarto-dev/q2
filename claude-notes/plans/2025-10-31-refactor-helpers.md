# Refactor Tree-Sitter Handlers to Helper Files

**Date**: 2025-10-31
**Context**: The match statement in `treesitter.rs` has grown significantly with recent additions. We need to extract complex handlers into helper files following the established pattern.

## Current Situation

During the tree-sitter refactoring work, I added the following handlers directly in the main match statement:

### Additions in `treesitter.rs`

1. **Delimiter handlers** (trivial, can stay):
   - `[`, `]`, `](`, `)` - Link/span/image delimiters (~1 line each)
   - `single_quote`, `double_quote` - Quote delimiters (~1 line each)

2. **Simple extractors** (trivial, can stay):
   - `url` - Extracts URL text (~3 lines)
   - `title` - Extracts title with quote stripping (~3 lines, uses `extract_quoted_text` helper)

3. **Mid-level collectors** (should extract):
   - `target` - Collects URL and title from children (~23 lines)
   - Modified `content` - Context-aware content handler (~18 lines)

4. **Complex constructors** (should extract):
   - `pandoc_span` - Creates Link or Span based on target presence (~54 lines)
   - `pandoc_image` - Creates Image inline (~42 lines)
   - `pandoc_single_quote` - Creates Quoted with SingleQuote (~21 lines)
   - `pandoc_double_quote` - Creates Quoted with DoubleQuote (~21 lines)

**Total lines added**: ~190 lines in match statement

## Existing Helper File Pattern

Looking at existing helpers, the pattern is:

```rust
// In treesitter_utils/<feature>.rs
pub fn process_<feature>(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
    // ... other params as needed
) -> PandocNativeIntermediate {
    // Extract data from children
    // Build appropriate Inline/Block
    // Return PandocNativeIntermediate result
}
```

Examples:
- `process_inline_with_delimiter_spaces` - Generic inline formatter with space handling
- Old helpers exist but use different grammar: `process_inline_link`, `process_image`, `process_quoted_span`

## Refactoring Plan

### Option A: Create New Helper Files (Recommended)

**Advantages**:
- Clean separation matching new grammar
- Doesn't interfere with old helpers (if still in use)
- Clear naming convention

**Files to create**:

1. **`span_link_helpers.rs`** - Unified helpers for spans and links
   - `process_target()` - Extract target from children
   - `process_content_node()` - Context-aware content handler
   - `process_pandoc_span()` - Main handler for pandoc_span (branches to Link/Span)
   - `process_pandoc_image()` - Main handler for pandoc_image

2. **`quote_helpers.rs`** - Helpers for quoted text
   - `process_quoted()` - Unified handler for both single and double quotes
     - Takes `QuoteType` parameter
     - Extracts content from children
     - Returns Quoted inline

### Option B: Inline Consolidation

**Advantages**:
- Keeps related code together in text_helpers.rs
- Simpler import structure

**Changes**:
- Add all helpers to `text_helpers.rs`

### Option C: Update Existing Helpers

**Advantages**:
- Reuses existing file structure

**Disadvantages**:
- May be confusing if old grammar is still partially in use
- Files may contain both old and new implementations

## Recommended Approach: Option A

Create two new helper files following the new grammar structure.

### Implementation Steps

#### Step 1: Create `span_link_helpers.rs`

```rust
/*
 * span_link_helpers.rs
 *
 * Functions for processing span, link, and image nodes in the new tree-sitter grammar.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::attr::{Attr, AttrSourceInfo, TargetSourceInfo};
use crate::pandoc::inline::{Image, Inline, Link, Span};
use crate::pandoc::location::node_source_info_with_context;
use super::pandocnativeintermediate::PandocNativeIntermediate;

/// Extract target (URL and title) from children
pub fn process_target(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    let mut url = String::new();
    let mut title = String::new();
    let mut range = /* ... */;

    for (node_name, child) in children {
        match node_name.as_str() {
            "url" => {
                if let PandocNativeIntermediate::IntermediateBaseText(text, r) = child {
                    url = text;
                    range = r;
                }
            }
            "title" => {
                if let PandocNativeIntermediate::IntermediateBaseText(text, _) = child {
                    title = text;
                }
            }
            "](" | ")" => {} // Ignore delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateTarget(url, title, range)
}

/// Process content node (context-aware for code_span vs links/spans/images)
pub fn process_content_node(
    children: Vec<(String, PandocNativeIntermediate)>,
) -> PandocNativeIntermediate {
    if children.is_empty() {
        // No children = code_span content, return range
        PandocNativeIntermediate::IntermediateUnknown(/* range */)
    } else {
        // Has children = link/span/image content, return processed inlines
        let inlines: Vec<Inline> = children
            .into_iter()
            .flat_map(|(_, child)| match child {
                PandocNativeIntermediate::IntermediateInline(inline) => vec![inline],
                PandocNativeIntermediate::IntermediateInlines(inlines) => inlines,
                _ => vec![],
            })
            .collect();
        PandocNativeIntermediate::IntermediateInlines(inlines)
    }
}

/// Process pandoc_span node (creates Link or Span based on target presence)
pub fn process_pandoc_span(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut content_inlines: Vec<Inline> = Vec::new();
    let mut target: Option<(String, String)> = None;
    let mut attr = /* empty attr */;
    let mut attr_source = AttrSourceInfo::empty();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            "target" => {
                if let PandocNativeIntermediate::IntermediateTarget(url, title, _) = child {
                    target = Some((url, title));
                }
            }
            "attribute_specifier" => {
                if let PandocNativeIntermediate::IntermediateAttr(attrs, attrs_src) = child {
                    attr = attrs;
                    attr_source = attrs_src;
                }
            }
            "[" | "]" => {} // Skip delimiters
            _ => {}
        }
    }

    // Branch based on target presence
    if let Some((url, title)) = target {
        // Has target = LINK
        PandocNativeIntermediate::IntermediateInline(Inline::Link(Link {
            attr,
            content: content_inlines,
            target: (url, title),
            source_info: node_source_info_with_context(node, context),
            attr_source,
            target_source: TargetSourceInfo::empty(),
        }))
    } else {
        // No target = SPAN
        PandocNativeIntermediate::IntermediateInline(Inline::Span(Span {
            attr,
            content: content_inlines,
            source_info: node_source_info_with_context(node, context),
            attr_source,
        }))
    }
}

/// Process pandoc_image node
pub fn process_pandoc_image(
    node: &tree_sitter::Node,
    children: Vec<(String, PandocNativeIntermediate)>,
    context: &ASTContext,
) -> PandocNativeIntermediate {
    let mut alt_inlines: Vec<Inline> = Vec::new();
    let mut target: Option<(String, String)> = None;
    let mut attr = /* empty attr */;
    let mut attr_source = AttrSourceInfo::empty();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let PandocNativeIntermediate::IntermediateInlines(inlines) = child {
                    alt_inlines = inlines;
                }
            }
            "target" => {
                if let PandocNativeIntermediate::IntermediateTarget(url, title, _) = child {
                    target = Some((url, title));
                }
            }
            "attribute_specifier" => {
                if let PandocNativeIntermediate::IntermediateAttr(attrs, attrs_src) = child {
                    attr = attrs;
                    attr_source = attrs_src;
                }
            }
            _ => {} // Ignore other nodes
        }
    }

    let (url, title) = target.unwrap_or_else(|| ("".to_string(), "".to_string()));

    PandocNativeIntermediate::IntermediateInline(Inline::Image(Image {
        attr,
        content: alt_inlines,
        target: (url, title),
        source_info: node_source_info_with_context(node, context),
        attr_source,
        target_source: TargetSourceInfo::empty(),
    }))
}
```

#### Step 2: Create `quote_helpers.rs`

```rust
/*
 * quote_helpers.rs
 *
 * Functions for processing quoted text nodes in the new tree-sitter grammar.
 *
 * Copyright (c) 2025 Posit, PBC
 */

use crate::pandoc::ast_context::ASTContext;
use crate::pandoc::inline::{Inline, QuoteType, Quoted};
use crate::pandoc::location::node_source_info_with_context;
use super::pandocnativeintermediate::PandocNativeIntermediate;

/// Process quoted text (single or double quotes)
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

#### Step 3: Update `mod.rs` to export new helpers

```rust
pub mod span_link_helpers;
pub mod quote_helpers;
```

#### Step 4: Update `treesitter.rs` imports

```rust
use crate::pandoc::treesitter_utils::span_link_helpers::{
    process_content_node, process_pandoc_image, process_pandoc_span, process_target,
};
use crate::pandoc::treesitter_utils::quote_helpers::process_quoted;
```

#### Step 5: Replace match arms with helper calls

```rust
// In the main match statement:

"target" => process_target(children),

"content" => process_content_node(children),

"pandoc_span" => process_pandoc_span(node, children, context),

"pandoc_image" => process_pandoc_image(node, children, context),

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

#### Step 6: Run tests to verify refactoring

```bash
cargo check
cargo test --test test_treesitter_refactoring
```

## Expected Outcome

- **Reduced match statement size**: ~190 lines → ~10 lines (helper calls)
- **Better organization**: Complex logic separated into focused modules
- **Easier maintenance**: Changes to handlers isolated in helper files
- **Consistent pattern**: Follows established helper file pattern
- **No behavior change**: All 74 tests should still pass

## Files to Modify

1. `src/pandoc/treesitter_utils/span_link_helpers.rs` - **CREATE**
2. `src/pandoc/treesitter_utils/quote_helpers.rs` - **CREATE**
3. `src/pandoc/treesitter_utils/mod.rs` - Add exports
4. `src/pandoc/treesitter.rs` - Replace inline code with helper calls

## Success Criteria

- ✅ All handlers extracted to appropriate helper files
- ✅ Match statement in `treesitter.rs` uses only helper calls
- ✅ All 74 tests pass
- ✅ No compilation warnings (beyond existing ones)
- ✅ Code compiles successfully with `cargo check`

## Notes

- Keep trivial one-liners (delimiters, simple extractors) in main match statement
- Focus on extracting complex logic (20+ lines)
- The `content` handler is special - it's context-aware and used by multiple node types
- The `target` handler is reusable for future target-based constructs
- Quote handler is unified with a `quote_type` parameter rather than two separate functions
