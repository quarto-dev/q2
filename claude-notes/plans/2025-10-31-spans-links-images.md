# Spans, Links, and Images Implementation Plan

**Date**: 2025-10-31 (REVISED after testing)
**Context**: Implement `pandoc_span` and `pandoc_image` handlers to support spans, links, and images

## CRITICAL CORRECTION

**Initial mistake**: Thought `![alt](img)` parsed as `!` + `pandoc_span`. This was due to bash echo issues with `!`.

**Actual behavior**: Tree-sitter correctly produces `pandoc_image` as a SEPARATE node type.

**Key findings from file-based testing**:
- `pandoc_image` is real and works correctly
- `[text]` (no target, no attrs) → Pandoc outputs literal `[text]`, BUT we want `Span` (QMD difference!)
- `[text]{}` (empty attrs) → `Span` with empty attributes (same as Pandoc)
- Standalone images become `Figure` blocks (deferred to later work)

## ⚠️ Design Decision: QMD vs Pandoc Difference

**Pandoc behavior**: `[text]` → literal `Str "[text]"`
**QMD behavior**: `[text]` → `Span ( "" , [] , [] ) [ Str "text" ]`

**Rationale**: QMD treats bare brackets as a span with empty attributes. This is an intentional divergence from Pandoc to provide consistent bracket semantics in QMD.

## Problem Statement

The tree-sitter grammar produces THREE node types for bracket constructs:
1. **Span**: `[text]{.class}` → `pandoc_span` (no target) → Pandoc `Span`
2. **Link**: `[text](url)` → `pandoc_span` (with target) → Pandoc `Link`
3. **Image**: `![text](url)` → `pandoc_image` → Pandoc `Image` (or `Figure` block if standalone)

**Key Insight**:
- `pandoc_span` is used for BOTH spans and links - presence of `target` child distinguishes them
- `pandoc_image` is a SEPARATE node type (not a variant of pandoc_span)
- Images have similar structure to links but use different node name

## Current Grammar Behavior

From file-based testing (bash echo has issues with `!`):

### Link: `[link text](url)`
```
pandoc_span
  [              (delimiter)
  content        (inline content)
    pandoc_str
    pandoc_space
    pandoc_str
  target         ← KEY: Has target = LINK
    ](           (delimiter)
    url
    )            (delimiter)
  attribute_specifier  (optional)
```

### Span: `[span text]{.class}`
```
pandoc_span
  [              (delimiter)
  content        (inline content)
    pandoc_str
    pandoc_space
    pandoc_str
  ]              (delimiter)
  attribute_specifier    ← KEY: No target, has attributes
```

### Image: `![alt text](image.png)`
```
pandoc_image     ← SEPARATE NODE TYPE!
  ![             (implicit - starts with ![)
  content        (inline content)
    pandoc_str
    pandoc_space
    pandoc_str
  target         (same structure as link)
    ](
    url
    )
  attribute_specifier  (optional)
```

**Key finding**: `pandoc_image` IS properly recognized as its own node type!

## Pandoc Expected Output

```bash
# Link
[link](url) → Link ( "" , [] , [] ) [ Str "link" ] ( "url" , "" )

# Link with title
[link](url "title") → Link ( "" , [] , [] ) [ Str "link" ] ( "url" , "title" )

# Link with attributes
[link](url){.class} → Link ( "" , [ "class" ] , [] ) [ Str "link" ] ( "url" , "" )

# Span
[text]{.class} → Span ( "" , [ "class" ] , [] ) [ Str "text" ]

# Image (inline)
Text ![alt](img) more → Para [ Str "Text" , Space , Image (...) [...] ("img", "") , Space , Str "more" ]

# Image (standalone) → becomes Figure BLOCK
![alt](img) → Figure ( "" , [] , [] ) (Caption ...) [ Plain [ Image (...) [...] ("img", "") ] ]
```

**Important**: Standalone images (paragraph with ONLY an image) become `Figure` blocks, not `Para` with `Image`. This conversion happens at block level (paragraph processing).

## Node Structure Details

### `target` node structure:
```
target
  ](           delimiter
  url          text content
  title        optional quoted string
  )            delimiter
```

### `url` node:
- Simple text node
- Content: everything between `](` and `)` or space before title

### `title` node (optional):
- Quoted string after URL
- Format: `"title text"` or `'title text'`

## Implementation Plan

### Phase 1: Add Leaf Node Handlers

1. **`[` delimiter** → `IntermediateUnknown` (marker only)
2. **`]` delimiter** → `IntermediateUnknown` (marker only)
3. **`](` delimiter** → `IntermediateUnknown` (marker only)
4. **`)` delimiter** → `IntermediateUnknown` (marker only)
5. **`url` node** → `IntermediateBaseText` (extract text directly)
6. **`title` node** → `IntermediateBaseText` (use `extract_quoted_text` helper)

### Phase 2: Add `target` Handler

Collect URL and optional title:

```rust
"target" => {
    let mut url = String::new();
    let mut title = String::new();

    for (node_name, child) in children {
        match node_name.as_str() {
            "url" => {
                if let IntermediateBaseText(text, _) = child {
                    url = text;
                }
            }
            "title" => {
                if let IntermediateBaseText(text, _) = child {
                    title = text;
                }
            }
            "]("|")" => {} // Skip delimiters
            _ => {}
        }
    }

    PandocNativeIntermediate::IntermediateTarget(url, title, node_location(node))
}
```

Need to add `IntermediateTarget` variant to `PandocNativeIntermediate`.

### Phase 3: Add `content` Handler

The `content` node is an alias for `_inlines` containing inline content:

```rust
"content" => {
    // Process children as inlines
    let inlines: Vec<Inline> = children.into_iter()
        .filter_map(|(_, child)| match child {
            PandocNativeIntermediate::IntermediateInline(inline) => Some(inline),
            PandocNativeIntermediate::IntermediateInlines(inlines) => Some(inlines).into_iter().flatten(),
            _ => None
        })
        .flatten()
        .collect();

    PandocNativeIntermediate::IntermediateInlines(inlines)
}
```

**Wait**: `content` node is already handled as returning `IntermediateUnknown` with range! Need to check if parent can extract from range or if it needs the processed inlines.

**Decision**: Parent (`pandoc_span`) will need the processed inlines, so `content` should return `IntermediateInlines`.

### Phase 4: Add `pandoc_span` Handler

This is the main handler that needs to distinguish between TWO cases:
- Link (has target)
- Span (no target) - ALWAYS a Span, even without attributes

**QMD Design**: Unlike Pandoc, `[text]` is a Span with empty attributes, not literal brackets.

```rust
"pandoc_span" => {
    let mut content_inlines: Vec<Inline> = Vec::new();
    let mut target: Option<(String, String)> = None;  // (url, title)
    let mut attr = ("".to_string(), vec![], HashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            "target" => {
                if let IntermediateTarget(url, title, _) = child {
                    target = Some((url, title));
                }
            }
            "attribute_specifier" => {
                if let IntermediateAttr(attrs, attrs_src) = child {
                    attr = attrs;
                    attr_source = attrs_src;
                }
            }
            "["|"]" => {} // Skip delimiters
            _ => {}
        }
    }

    // Decide what to create based on presence of target
    if let Some((url, title)) = target {
        // This is a LINK
        PandocNativeIntermediate::IntermediateInline(Inline::Link(Link {
            attr,
            text: content_inlines,
            target: (url, title),
            source_info: node_source_info_with_context(node, context),
            attr_source,
        }))
    } else {
        // No target → SPAN (even if attributes are empty)
        // This is a QMD design choice: [text] becomes Span, not literal
        PandocNativeIntermediate::IntermediateInline(Inline::Span(Span {
            attr,
            text: content_inlines,
            source_info: node_source_info_with_context(node, context),
            attr_source,
        }))
    }
}
```

### Phase 5: Add `pandoc_image` Handler

Images are a separate node type with the same structure as links:

```rust
"pandoc_image" => {
    let mut content_inlines: Vec<Inline> = Vec::new();
    let mut target: Option<(String, String)> = None;  // (url, title)
    let mut attr = ("".to_string(), vec![], HashMap::new());
    let mut attr_source = AttrSourceInfo::empty();

    for (node_name, child) in children {
        match node_name.as_str() {
            "content" => {
                if let IntermediateInlines(inlines) = child {
                    content_inlines = inlines;
                }
            }
            "target" => {
                if let IntermediateTarget(url, title, _) = child {
                    target = Some((url, title));
                }
            }
            "attribute_specifier" => {
                if let IntermediateAttr(attrs, attrs_src) = child {
                    attr = attrs;
                    attr_source = attrs_src;
                }
            }
            "![" => {} // Skip delimiter (implicit in node type)
            _ => {}
        }
    }

    // Create Image inline
    let (url, title) = target.unwrap_or_else(|| ("".to_string(), "".to_string()));

    PandocNativeIntermediate::IntermediateInline(Inline::Image(Image {
        attr,
        alt: content_inlines,  // Image uses 'alt' not 'text'
        target: (url, title),
        source_info: node_source_info_with_context(node, context),
        attr_source,
    }))
}
```

**Note**: Converting standalone images to `Figure` blocks happens at paragraph level, deferred to future work.

### Phase 6: Testing

Test cases needed:

**Links**:
```rust
// Basic link
"[link](url)" → Link with empty attr, url="url", title=""

// Link with title
"[link](url \"title\")" → Link with title="title"

// Link in context
"text [link](url) more" → [Str "text", Space, Link, Space, Str "more"]

// Link with attributes
"[link](url){#id .class}" → Link with attr=(id="id", classes=["class"])

// Nested formatting
"[**bold** text](url)" → Link with text=[Strong [...], ...]
```

**Spans**:
```rust
// Basic span
"[text]{.class}" → Span with classes=["class"]

// Span with full attributes
"[text]{#id .c1 .c2 key=\"value\"}" → Span with all attributes

// Span with empty attributes
"[text]{}" → Span ( "" , [] , [] ) [ Str "text" ]

// Span without attributes (QMD difference!)
"[text]" → Span ( "" , [] , [] ) [ Str "text" ]
// Note: Pandoc outputs Str "[text]", but QMD treats as Span
```

**Images**:
```rust
// Basic image (inline)
"text ![alt](img.png) more" → [Str "text", Space, Image(...), Space, Str "more"]

// Image with title
"![alt](img \"title\")" → Image with title="title"

// Image with attributes
"![alt](img){.class}" → Image with attr.classes=["class"]

// Standalone image → deferred to Figure block conversion
"![alt](img)" → For now: Para [Image(...)], later: Figure block
```

**Edge cases**:
```rust
// Empty link text
"[](url)" → Link with empty text

// Empty span
"[]{.class}" → Span with empty text

// Link with special chars in URL
"[link](http://example.com?a=1&b=2)" → handle & and other chars

// Title with escaped quotes
"[link](url \"a \\\"quoted\\\" word\")" → handle escapes in title
```

## Files to Modify

1. **`src/pandoc/treesitter_utils/pandocnativeintermediate.rs`**:
   - Add `IntermediateTarget(String, String, Range)` variant

2. **`src/pandoc/treesitter.rs`**:
   - Add handlers for: `[`, `]`, `](`, `)`, `url`, `title`
   - Add handler for `target`
   - Modify `content` handler (currently returns IntermediateUnknown)
   - Add handler for `pandoc_span`

3. **`tests/test_treesitter_refactoring.rs`**:
   - Add comprehensive test suite

## Success Criteria

- ✅ Links parse correctly with URL and title
- ✅ Spans parse correctly with attributes
- ✅ Images parse as `!` + Link (matching Pandoc)
- ✅ All existing tests still pass
- ✅ No "[TOP-LEVEL MISSING NODE]" warnings for these nodes
- ✅ Output matches Pandoc native format exactly

## Estimate

- Phase 1 (leaf nodes): 20 minutes
- Phase 2 (target handler): 20 minutes
- Phase 3 (content handler): 15 minutes
- Phase 4 (pandoc_span handler): 30 minutes
- Phase 5 (pandoc_image handler): 20 minutes
- Phase 6 (testing): 60 minutes
- **Total**: ~2.5-3 hours

## Notes

1. **`pandoc_image` is a SEPARATE node type** from `pandoc_span` - not just a variant!
2. Pandoc DOES have `Image` inline type (confirmed)
3. Standalone images become `Figure` blocks in Pandoc, but inline images stay as `Image` inlines
4. `content` node contains the processed inline elements (link text / span content / image alt text)
5. The `target` node cleanly separates links/images from spans
6. All three can have `attribute_specifier` (attributes support is already implemented)
7. Figure block conversion is deferred - for now, standalone images will just be `Para [Image]`
8. **QMD Design Difference**: `[text]` → `Span`, not literal `[text]` (differs from Pandoc intentionally)

## Open Questions (RESOLVED)

1. **Q**: What should `[text]` (brackets with no target or attributes) produce?
   **A**: DESIGN DECISION - QMD outputs `Span ( "" , [] , [] ) [ Str "text" ]`.
   - Pandoc outputs literal `Str "[text]"`
   - QMD intentionally differs: bare brackets create a Span with empty attributes
   - Tree-sitter correctly parses as `pandoc_span`

2. **Q**: Can we have `[text]{}` (empty attribute block)?
   **A**: TESTED - Yes, produces `Span ( "" , [] , [] ) [ Str "text" ]`. Same as `[text]` in QMD.

3. **Q**: When do we implement Figure block conversion for standalone images?
   **A**: Deferred to later work. Requires paragraph-level logic to detect "paragraph with only Image" and convert to Figure.

## References

- Grammar: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
- Test corpus: `crates/tree-sitter-qmd/tree-sitter-markdown/test/corpus/image.txt`
- Existing helpers: `text_helpers.rs` (`extract_quoted_text` for titles)
