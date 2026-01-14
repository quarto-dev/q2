# Complete AST Generators Plan

**Date:** 2026-01-14
**Status:** Phases 1-3 Complete, Bug kyoto-sz3 Fixed
**Parent:** 2026-01-14-reconciliation-correctness.md
**Epic:** kyoto-tsq

## Purpose

**The purpose of this plan is to find and fix bugs in the reconciliation code.**

We are building comprehensive AST generators so that property-based tests can exercise all reconciliation code paths. When generators find bugs, we **fix them** - we do not work around them.

## Completion Criteria

This plan is **NOT FINISHED** until:

1. **Generators can generate full ASTs** - Every Block and Inline variant can be generated with positive probability
2. **Properties are tested against full ASTs** - Property tests run with `gen_full_pandoc()` or equivalent
3. **Properties pass** - All property tests pass with full AST generation enabled

## Problem Statement

The original generators in `generators.rs` only covered ~10% of the AST:
- 2/18 Block variants (Paragraph, BulletList)
- 2/24+ Inline variants (Str, Space)

This meant property tests could not exercise most reconciliation code paths.

## Progress Summary

**Completed:**
- GenConfig with depth-limited recursion
- All leaf block generators (Paragraph, Plain, CodeBlock, RawBlock, HorizontalRule, Header, LineBlock)
- All leaf inline generators (Str, Space, SoftBreak, LineBreak, Code, Math, RawInline)
- All inline container generators (Emph, Strong, Underline, Strikeout, Superscript, Subscript, SmallCaps, Quoted, Span, Link, Image)
- All block container generators (BlockQuote, BulletList, OrderedList, Div)
- Helper generators (Attr, Target, ListAttributes, QuoteType, MathType)
- 329 tests passing
- Full AST property test (`reconciliation_preserves_structure_full_ast`) passes

**Bugs Found and Fixed:**
1. kyoto-sz3: Container blocks (OrderedList, Div, Figure) were preserving `attr` from `before` instead of using `after`'s attr. Fixed in `apply_block_container_reconciliation`.
2. Header was preserving `attr` and `level` from `before` instead of using `after`'s values. Fixed in `apply_inline_block_reconciliation`.

**Remaining (Phases 4-6):**
- Complex structures: Table, Figure, Note, Cite, DefinitionList
- Special types: NoteDefinitionPara, NoteDefinitionFencedBlock, CaptionBlock, NoteReference, Shortcode, CustomNode
- Unified `gen_any_*` generators

## Design Goals

Create generators that can produce **any valid AST** with positive probability, while:
1. Controlling size via depth/breadth limits
2. Supporting the existing feature-flag system for targeted testing
3. Maintaining good shrinking behavior for proptest

## Design: Size-Limited Recursive Generation

### Core Insight

AST generation is inherently recursive. We need a "fuel" or "size budget" parameter that decreases with depth to ensure termination.

```rust
struct GenConfig {
    /// Maximum recursion depth for containers
    max_depth: usize,
    /// Maximum number of children at each level
    max_children: usize,
    /// Feature flags for selective generation
    block_features: BlockFeatures,
    inline_features: InlineFeatures,
}
```

### Generation Strategy

When `depth > 0`, we can generate container types. When `depth == 0`, we only generate leaf types.

```rust
fn gen_block(config: GenConfig, depth: usize) -> impl Strategy<Value = Block> {
    if depth == 0 {
        // Leaf blocks only
        prop_oneof![
            gen_paragraph(config.inline_features, depth),
            gen_code_block(),
            gen_raw_block(),
            gen_horizontal_rule(),
        ]
    } else {
        // Include containers
        prop_oneof![
            // Leaves (still possible)
            gen_paragraph(config.inline_features, depth),
            gen_code_block(),
            gen_horizontal_rule(),
            // Containers (recurse with depth-1)
            gen_blockquote(config, depth - 1),
            gen_bullet_list(config, depth - 1),
            gen_ordered_list(config, depth - 1),
            gen_div(config, depth - 1),
        ]
    }
}
```

### Weighted Selection

Use `prop_oneof!` with weights to control distribution:

```rust
prop_oneof![
    10 => gen_paragraph(...),  // Common
    5 => gen_code_block(),     // Less common
    3 => gen_bullet_list(...), // Container
    1 => gen_table(...),       // Rare/complex
]
```

## Implementation Plan

### Phase 1: Leaf Types (No Recursion) ✅ COMPLETE

These are straightforward - generate content without recursion.

**Blocks:**
- [x] `gen_horizontal_rule()` - Empty content
- [x] `gen_code_block()` - Attr + text string
- [x] `gen_raw_block()` - Format + text
- [x] `gen_paragraph()` - Inlines
- [x] `gen_plain()` - Inlines
- [x] `gen_header()` - Level + Attr + inlines
- [x] `gen_line_block()` - Vec of inline lines

**Inlines:**
- [x] `gen_soft_break()` - Empty
- [x] `gen_line_break()` - Empty
- [x] `gen_code_inline()` - Attr + text
- [x] `gen_math()` - MathType + text
- [x] `gen_raw_inline()` - Format + text
- [x] `gen_str()` - Text
- [x] `gen_space()` - Empty

### Phase 2: Simple Inline Containers ✅ COMPLETE

These wrap `Inlines` but don't nest blocks.

- [x] `gen_emph(config)` - Wraps inlines
- [x] `gen_strong(config)` - Wraps inlines
- [x] `gen_underline(config)` - Wraps inlines
- [x] `gen_strikeout(config)` - Wraps inlines
- [x] `gen_superscript(config)` - Wraps inlines
- [x] `gen_subscript(config)` - Wraps inlines
- [x] `gen_smallcaps(config)` - Wraps inlines
- [x] `gen_span(config)` - Attr + inlines
- [x] `gen_link(config)` - Attr + inlines + target
- [x] `gen_image(config)` - Attr + inlines + target
- [x] `gen_quoted(config)` - QuoteType + inlines

### Phase 3: Block Containers ✅ COMPLETE

These contain nested `Blocks`.

- [x] `gen_blockquote(config)` - Wraps blocks
- [x] `gen_div(config)` - Attr + blocks
- [x] `gen_bullet_list(config)` - List items
- [x] `gen_ordered_list(config)` - ListAttrs + items
- [ ] `gen_definition_list(config)` - Terms + definitions (deferred)

### Phase 4: Complex Structures

- [ ] `gen_note(config)` - Inline containing blocks
- [ ] `gen_figure(config)` - Attr + Caption + blocks
- [ ] `gen_table(config)` - Complex table structure
- [ ] `gen_cite(config)` - Citations + inlines

### Phase 5: Special Types

- [ ] `gen_note_definition_para()` - ID + inlines
- [ ] `gen_note_definition_fenced()` - ID + blocks
- [ ] `gen_caption_block()` - Inlines
- [ ] `gen_note_reference()` - ID string
- [ ] `gen_shortcode()` - Shortcode structure
- [ ] `gen_custom_node()` - CustomNode with slots

### Phase 6: Unified Generator

- [ ] `gen_any_block(config, depth)` - Chooses from all block types
- [ ] `gen_any_inline(config, depth)` - Chooses from all inline types
- [ ] `gen_any_pandoc(config)` - Complete document with any content

## Helper Generators Needed

```rust
/// Generate valid Attr (id, classes, key-value pairs)
fn gen_attr() -> impl Strategy<Value = Attr>

/// Generate valid Target (url, title)
fn gen_target() -> impl Strategy<Value = Target>

/// Generate valid ListAttributes (start, style, delimiter)
fn gen_list_attributes() -> impl Strategy<Value = ListAttributes>

/// Generate valid Citation
fn gen_citation() -> impl Strategy<Value = Citation>
```

## Feature Flags Update

The existing feature system should be extended:

```rust
#[derive(Clone, Default)]
pub struct BlockFeatures {
    // Leaves
    pub paragraph: bool,
    pub plain: bool,
    pub code_block: bool,
    pub raw_block: bool,
    pub horizontal_rule: bool,
    pub header: bool,

    // Containers
    pub blockquote: bool,
    pub bullet_list: bool,
    pub ordered_list: bool,
    pub definition_list: bool,
    pub div: bool,
    pub figure: bool,
    pub line_block: bool,

    // Special
    pub table: bool,
    pub note_definition: bool,
    pub caption_block: bool,
    pub custom: bool,
}

#[derive(Clone, Default)]
pub struct InlineFeatures {
    // Leaves
    pub str_: bool,
    pub space: bool,
    pub soft_break: bool,
    pub line_break: bool,
    pub code: bool,
    pub math: bool,
    pub raw_inline: bool,

    // Containers
    pub emph: bool,
    pub strong: bool,
    pub underline: bool,
    pub strikeout: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub smallcaps: bool,
    pub quoted: bool,
    pub span: bool,
    pub link: bool,
    pub image: bool,

    // Special
    pub cite: bool,
    pub note: bool,
    pub note_reference: bool,
    pub shortcode: bool,
    pub attr: bool,
    pub insert: bool,
    pub delete: bool,
    pub highlight: bool,
    pub edit_comment: bool,
    pub custom: bool,
}
```

## Testing Strategy

### Progressive Complexity Tests

Run property tests at increasing complexity:

```rust
#[test]
fn reconciliation_leaf_blocks_only() {
    // depth=0, only leaf blocks/inlines
}

#[test]
fn reconciliation_shallow_containers() {
    // depth=1, containers with leaf content
}

#[test]
fn reconciliation_nested_containers() {
    // depth=2-3, nested structures
}

#[test]
fn reconciliation_full_ast() {
    // depth=3-4, all features enabled
}
```

### Targeted Tests

Keep feature-specific tests for debugging:

```rust
#[test]
fn reconciliation_lists_only() {
    // Just list operations
}

#[test]
fn reconciliation_inline_formatting() {
    // Emph, Strong, etc.
}
```

## Implementation Order

1. **Refactor GenConfig** - Add depth parameter, update feature flags
2. **Phase 1** - Leaf types (quick wins, no recursion)
3. **Phase 2** - Inline containers (test inline reconciliation)
4. **Phase 3** - Block containers (test nested block reconciliation)
5. **Phase 4-5** - Complex/special types
6. **Phase 6** - Unified `gen_any_*` generators

## Success Criteria

- [ ] Every Block variant has a generator
- [ ] Every Inline variant has a generator
- [ ] Property test with `gen_any_pandoc(depth=3)` passes
- [ ] Reconciliation handles all AST combinations correctly
- [ ] Shrinking works well (finds minimal failing cases)

## Notes

- Keep existing tests passing as we add generators
- Use `#[ignore]` for slow comprehensive tests, run in CI
- Consider adding `Arbitrary` impl for Pandoc types eventually
