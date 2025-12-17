# Property Testing Framework for CommonMark Subset Validation

**Issue**: k-g9uc (child of k-333)
**Date**: 2025-12-16
**Status**: Implemented ✅

## Implementation Summary

Property testing framework is complete. All 12 roundtrip tests pass.

### Key Bug Fixed (k-igyi)

The investigation discovered that `comrak-to-pandoc` was losing Space inlines in several scenarios:

1. **Trailing/leading spaces in Text nodes**: `tokenize_text("aA ")` was returning `[Str("aA")]` instead of `[Str("aA"), Space]`

2. **Pure whitespace Text nodes**: For input like `` `code1` `code2` ``, comrak produces `[Code, Text(" "), Code]` but `tokenize_text(" ")` was returning `[]` instead of `[Space]`

### Normalization Additions

To handle known pampa vs comrak differences:

1. **Empty Span unwrapping**: pampa wraps some content in `Span { attr: empty, content: [...] }` that comrak doesn't produce
2. **CodeBlock trailing newline**: comrak includes trailing `\n` in code block text, pampa doesn't
3. **Header leading/trailing spaces**: pampa includes space after ATX markers as content
4. **Heading ID stripping**: pampa auto-generates IDs, comrak doesn't
5. **Link `uri` class stripping**: pampa adds `uri` class to autolinks

### Known Parser Differences (Cannot Normalize)

These are excluded from generators because they produce fundamentally different ASTs:

1. **Nested Emph/Strong**: `*some **strong** text*` - pampa produces multiple Emph spans, comrak produces nested Emph(Strong)
2. **HR inside BlockQuote**: Different block structure
3. **Lists inside BlockQuote**: Different block structure
4. **Autolinks in complex nesting**: Link inside Emph inside Link, etc.
5. **LineBreak**: Generates markdown pampa can't parse

## Overview

This document describes a property testing approach to validate that the CommonMark-compatible subset of qmd produces identical ASTs when parsed by both quarto-markdown-pandoc (pampa) and comrak.

## Core Property

The fundamental property we test is:

```
x <- arbitraryPandocAST    // Generate random valid AST
text <- qmd_writer(x)      // Serialize to markdown
ast1 <- normalize(qmd_reader(text))        // Parse with pampa
ast2 <- normalize(comrak_to_pandoc(comrak(text)))  // Parse with comrak
assert(ast1 == ast2)
```

## Rust Property Testing Framework

**Recommended**: [proptest](https://crates.io/crates/proptest)

Reasons:
- Sophisticated shrinking to find minimal failing cases
- Strategies API for constraining generation
- `proptest-derive` for automatic Arbitrary derivation
- Widely used in production Rust projects

Alternative: quickcheck (simpler but less powerful shrinking)

## Comrak Node Types Analysis

### CommonMark Core (Generate These)

Based on analysis of `external-sources/comrak/src/nodes.rs`:

**Blocks:**
| NodeValue | Pandoc Equivalent | Notes |
|-----------|-------------------|-------|
| `Document` | `Pandoc` | Root container |
| `Paragraph` | `Para` / `Plain` | Standard paragraphs |
| `Heading(NodeHeading)` | `Header` | ATX only (exclude setext) |
| `BlockQuote` | `BlockQuote` | Standard blockquotes |
| `List` + `Item` | `BulletList` / `OrderedList` | Ordered and unordered |
| `CodeBlock` | `CodeBlock` | Fenced only (exclude indented) |
| `ThematicBreak` | `HorizontalRule` | Horizontal rules |

**Inlines:**
| NodeValue | Pandoc Equivalent | Notes |
|-----------|-------------------|-------|
| `Text` | `Str` | Text content |
| `Emph` | `Emph` | `*text*` or `_text_` |
| `Strong` | `Strong` | `**text**` |
| `Code` | `Code` | Inline code spans |
| `Link` | `Link` | Inline links and autolinks |
| `Image` | `Image` | Inline images |
| `SoftBreak` | `SoftBreak` | Newline within paragraph |
| `LineBreak` | `LineBreak` | Hard break (`\` at EOL) |

**Note on Autolinks:** Autolinks (`<https://example.com>`) are included. pampa adds a `uri` class to autolinks which comrak doesn't; this is handled in normalization by stripping the class.

### Excluded from Generator

**Comrak parses but we exclude:**
- `HtmlBlock` / `HtmlInline` - Not in CommonMark subset
- `FrontMatter` - Extension (needs explicit option)
- Setext headings (`NodeHeading.setext = true`)
- Indented code blocks (`NodeCodeBlock.fenced = false`)

**GFM/Extension types (disabled by default in comrak):**
- `Strikethrough`, `Table`, `TaskItem`
- `FootnoteDefinition`, `FootnoteReference`
- `Superscript`, `Subscript`, `Underline`, `Math`
- `DescriptionList`, `MultilineBlockQuote`, `WikiLink`
- `Alert`, `Highlight`, `SpoileredText`, `Subtext`, `ShortCode`

**Quarto extensions (never generate):**
- Footnote references/definitions
- Shortcodes
- Editorial marks
- Divs, spans with attributes
- Callouts

## Generator Design: Feature Sets

The generator uses a **feature set** approach: a generator takes a set of enabled features, and when it uses a feature (like Emph), it recurses with that feature removed from the set. This elegantly:

1. Prevents Emph-inside-Emph (Emph is removed when recursing into Emph content)
2. Naturally limits nesting depth (features get consumed as you nest deeper)
3. Enables progressive complexity testing (start with small feature sets, expand)

### Feature Set Types

```rust
/// Features available for inline generation
#[derive(Clone, Default)]
struct InlineFeatures {
    emph: bool,
    strong: bool,
    code: bool,
    link: bool,
    image: bool,
    autolink: bool,
    linebreak: bool,
}

/// Features available for block generation
#[derive(Clone, Default)]
struct BlockFeatures {
    header: bool,
    code_block: bool,
    blockquote: bool,
    bullet_list: bool,
    ordered_list: bool,
    horizontal_rule: bool,
}

impl InlineFeatures {
    /// No features - just Str, Space, SoftBreak
    fn plain_text() -> Self {
        Self::default()
    }

    /// All inline features enabled
    fn full() -> Self {
        Self {
            emph: true,
            strong: true,
            code: true,
            link: true,
            image: true,
            autolink: true,
            linebreak: true,
        }
    }

    /// Remove emph feature for recursion
    fn without_emph(&self) -> Self {
        Self { emph: false, ..self.clone() }
    }

    /// Remove link feature for recursion
    fn without_link(&self) -> Self {
        Self { link: false, ..self.clone() }
    }

    // ... similar methods for other features
}
```

### Progressive Complexity Levels

Testing proceeds through progressively more complex generators. Each level includes all features from previous levels.

**Inline Progression:**

| Level | Name | Features Added | Example Output |
|-------|------|----------------|----------------|
| L0 | `PLAIN_TEXT` | Str, Space, SoftBreak | `hello world` |
| L1 | `WITH_EMPH` | + Emph | `hello *world*` |
| L2 | `WITH_STRONG` | + Strong | `hello **world**` |
| L3 | `WITH_CODE` | + Code | `hello `code`` |
| L4 | `WITH_LINK` | + Link | `[text](url)` |
| L5 | `WITH_IMAGE` | + Image | `![alt](url)` |
| L6 | `WITH_AUTOLINK` | + Autolink | `<https://example.com>` |
| L7 | `FULL_INLINES` | + LineBreak | `text\` (hard break) |

**Block Progression:**

| Level | Name | Features Added | Notes |
|-------|------|----------------|-------|
| B0 | `PARA_ONLY` | Paragraph | Single paragraphs |
| B1 | `WITH_HEADER` | + Header | ATX headings 1-6 |
| B2 | `WITH_CODE_BLOCK` | + CodeBlock | Fenced code blocks |
| B3 | `WITH_HR` | + HorizontalRule | Thematic breaks |
| B4 | `WITH_BLOCKQUOTE` | + BlockQuote | Recursive block container |
| B5 | `WITH_BULLET_LIST` | + BulletList | Recursive, tight lists first |
| B6 | `FULL_BLOCKS` | + OrderedList | Full block support |

### Feature Removal on Recursion

When generating a container node, the feature used is removed before recursing:

```rust
fn gen_inlines(features: InlineFeatures) -> impl Strategy<Value = Vec<Inline>> {
    // Build list of possible choices based on enabled features
    let mut choices: Vec<BoxedStrategy<Inline>> = vec![
        // Always available: Str, Space, SoftBreak
        safe_text().prop_map(Inline::Str).boxed(),
        Just(Inline::Space).boxed(),
        Just(Inline::SoftBreak).boxed(),
    ];

    // Leaf features (no recursion needed)
    if features.code {
        choices.push(
            safe_text().prop_map(|s| Inline::Code(Code { text: s, .. })).boxed()
        );
    }

    if features.linebreak {
        choices.push(Just(Inline::LineBreak).boxed());
    }

    // Container features (recurse with feature removed)
    if features.emph {
        let inner = features.without_emph();
        choices.push(
            gen_inlines(inner)
                .prop_map(|content| Inline::Emph(Emph { content, .. }))
                .boxed()
        );
    }

    if features.strong {
        let inner = features.without_strong();
        choices.push(
            gen_inlines(inner)
                .prop_map(|content| Inline::Strong(Strong { content, .. }))
                .boxed()
        );
    }

    if features.link {
        let inner = features.without_link();
        choices.push(
            (gen_inlines(inner), gen_url(), gen_title())
                .prop_map(|(content, url, title)| Inline::Link(Link {
                    content,
                    target: (url, title),
                    ..
                }))
                .boxed()
        );
    }

    if features.image {
        let inner = features.without_image();
        choices.push(
            (gen_inlines(inner), gen_url(), gen_title())
                .prop_map(|(alt, url, title)| Inline::Image(Image {
                    content: alt,
                    target: (url, title),
                    ..
                }))
                .boxed()
        );
    }

    // Select from available choices and build a sequence
    prop::sample::select(choices)
        .prop_flat_map(|strategy| strategy)
        .prop_map(|inline| vec![inline])  // simplified; real impl builds sequences
}
```

### Safe Text Generation

Text content avoids punctuation that could create markdown syntax:

```rust
fn safe_text() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ]{1,20}".prop_filter("valid spacing", |s| {
        !s.starts_with(' ') && !s.ends_with(' ') && !s.contains("  ")
    })
}

fn gen_url() -> impl Strategy<Value = String> {
    "[a-z]{3,10}".prop_map(|s| format!("https://{}.example.com", s))
}

fn gen_title() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),  // No title
        safe_text(),          // Optional title
    ]
}
```

### Block Generator with Features

```rust
fn gen_blocks(
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
) -> impl Strategy<Value = Vec<Block>> {
    let mut choices: Vec<BoxedStrategy<Block>> = vec![
        // Paragraph is always available
        gen_inlines(inline_features.clone())
            .prop_map(|content| Block::Para(Para { content, .. }))
            .boxed(),
    ];

    if block_features.header {
        choices.push(
            (1..=6usize, gen_inlines(inline_features.clone()))
                .prop_map(|(level, content)| Block::Header(Header { level, content, .. }))
                .boxed()
        );
    }

    if block_features.code_block {
        choices.push(
            (safe_text(), option_lang())
                .prop_map(|(text, lang)| Block::CodeBlock(CodeBlock { text, .. }))
                .boxed()
        );
    }

    if block_features.horizontal_rule {
        choices.push(Just(Block::HorizontalRule).boxed());
    }

    if block_features.blockquote {
        // Recurse with same features (blockquote can contain blockquote)
        // but use lazy() to avoid infinite recursion at strategy construction
        choices.push(
            prop::lazy(|| gen_blocks(block_features.clone(), inline_features.clone()))
                .prop_map(|content| Block::BlockQuote(BlockQuote { content, .. }))
                .boxed()
        );
    }

    // ... lists similarly

    prop::collection::vec(prop::sample::select(choices).prop_flat_map(|s| s), 1..5)
}
```

### List Considerations

- Start with tight lists only (items use `Plain` not `Para`)
- Tight lists are simpler to verify and less prone to whitespace edge cases
- Add loose lists (items use `Para`) in later testing phases

## Normalization Function

The `normalize()` function handles known differences between pampa and comrak output.

### Transformations Required

1. **Strip heading IDs**: pampa auto-generates IDs from heading text; comrak doesn't generate IDs

2. **Figure → Paragraph(Image)**: pampa wraps standalone images in `Figure` blocks; comrak keeps them as `Paragraph` containing `Image`. The normalization filter detects `Figure` blocks containing a single image and replaces them with `Paragraph(Image)`.

3. **Strip autolink `uri` class**: pampa adds `("", ["uri"], [])` attributes to autolinks; comrak produces empty attributes. Strip the `uri` class from Link attrs.

4. **Strip extra code block attributes**: Keep only the language class, remove any additional attributes.

5. **Source info**: Already handled separately by `ast_eq_ignore_source`.

### Implementation Sketch

```rust
fn normalize(ast: Pandoc) -> Pandoc {
    let blocks = ast.blocks.into_iter().map(normalize_block).collect();
    Pandoc { blocks, ..ast }
}

fn normalize_block(block: Block) -> Block {
    match block {
        // Strip heading IDs
        Block::Header(mut h) => {
            h.attr.0 = String::new();
            h.content = h.content.into_iter().map(normalize_inline).collect();
            Block::Header(h)
        }

        // Figure → Paragraph(Image) for standalone images
        Block::Figure(fig) => {
            // If figure contains single Para with single Image, unwrap to Para(Image)
            if let [Block::Plain(plain)] = &fig.content[..] {
                if plain.content.len() == 1 {
                    if let Inline::Image(_) = &plain.content[0] {
                        return Block::Para(Para {
                            content: plain.content.iter().map(normalize_inline).collect(),
                            ..Default::default()
                        });
                    }
                }
            }
            // Otherwise keep as-is (shouldn't happen in subset)
            Block::Figure(fig)
        }

        // Recurse into other blocks
        Block::Para(mut p) => {
            p.content = p.content.into_iter().map(normalize_inline).collect();
            Block::Para(p)
        }
        // ... other block types
        _ => block,
    }
}

fn normalize_inline(inline: Inline) -> Inline {
    match inline {
        // Strip uri class from autolinks
        Inline::Link(mut link) => {
            link.attr.1.retain(|c| c != "uri");
            link.content = link.content.into_iter().map(normalize_inline).collect();
            Inline::Link(link)
        }

        // Recurse into other inlines
        Inline::Emph(mut e) => {
            e.content = e.content.into_iter().map(normalize_inline).collect();
            Inline::Emph(e)
        }
        // ... other inline types
        _ => inline,
    }
}
```

## Implementation Plan

### Phase 1: Infrastructure
1. Add `proptest` to `comrak-to-pandoc/Cargo.toml` dev-dependencies
2. Create `src/generators.rs` module for feature sets and generators
3. Create `src/normalize.rs` module for AST normalization
4. Create `tests/proptest_roundtrip.rs` for property tests

### Phase 2: Plain Text (L0, B0)
1. Implement `safe_text()` strategy
2. Implement `InlineFeatures` and `BlockFeatures` structs with preset constructors
3. Implement `gen_inlines(InlineFeatures::plain_text())` - just Str, Space, SoftBreak
4. Implement `gen_blocks(BlockFeatures::para_only(), ...)` - just Paragraph
5. Write first property test: plain text roundtrips correctly
6. **Checkpoint**: Verify L0/B0 passes before proceeding

### Phase 3: Style Inlines (L1-L3)
1. Add Emph support with `without_emph()` recursion
2. Write property test for L1; verify passes
3. Add Strong support with `without_strong()` recursion
4. Write property test for L2; verify passes
5. Add Code support (leaf, no recursion)
6. Write property test for L3; verify passes
7. **Checkpoint**: All style inlines working

### Phase 4: Links and Images (L4-L6)
1. Implement `gen_url()` and `gen_title()` helpers
2. Add Link support with `without_link()` recursion
3. Write property test for L4; verify passes
4. Add Image support with `without_image()` recursion
5. Update normalization for Figure → Paragraph(Image)
6. Write property test for L5; verify passes
7. Add Autolink support
8. Update normalization to strip `uri` class
9. Write property test for L6; verify passes
10. **Checkpoint**: All inline features working

### Phase 5: Simple Blocks (B1-B3)
1. Add Header support (levels 1-6, ATX only)
2. Update normalization to strip heading IDs
3. Write property test for B1; verify passes
4. Add CodeBlock support (fenced only)
5. Write property test for B2; verify passes
6. Add HorizontalRule support
7. Write property test for B3; verify passes
8. **Checkpoint**: Non-recursive blocks working

### Phase 6: Recursive Blocks (B4-B6)
1. Add BlockQuote support with `prop::lazy()` for recursion
2. Write property test for B4; verify passes
3. Add BulletList support (tight lists only initially)
4. Write property test for B5; verify passes
5. Add OrderedList support
6. Write property test for B6; verify passes
7. **Checkpoint**: Full block support working

### Phase 7: Full Integration
1. Write combined property test with all features
2. Add LineBreak support (L7)
3. Run extended test campaigns (more iterations)
4. Document any discovered edge cases
5. Consider adding loose list support if tight lists are stable

## Expected Discoveries

Property testing will likely reveal edge cases missed by hand-written tests:
- Whitespace handling differences
- Empty content handling
- Attribute ordering differences
- Unicode edge cases

## Files to Create/Modify

- `crates/comrak-to-pandoc/Cargo.toml` - add proptest dependency
- `crates/comrak-to-pandoc/src/generators.rs` - NEW: feature sets and generators
- `crates/comrak-to-pandoc/src/normalize.rs` - NEW: AST normalization
- `crates/comrak-to-pandoc/src/lib.rs` - add module declarations
- `crates/comrak-to-pandoc/tests/proptest_roundtrip.rs` - NEW: property tests with progressive levels

## References

- proptest documentation: https://docs.rs/proptest
- comrak source: `external-sources/comrak/src/nodes.rs`
- Parent plan: `claude-notes/plans/2025-11-06-commonmark-compatible-subset.md`
