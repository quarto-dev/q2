# QMD Sugaring: Div/Span to CustomNode Normalization

**Parent Epic**: kyoto-6jv
**Beads Issue**: kyoto-50m
**Related**: kyoto-kl1 (Phase 1: HTML Postprocessing Infrastructure)
**Created**: 2026-01-24
**Status**: Planning

---

## Overview

This plan covers the "sugaring" stage of QMD processing - converting syntactic patterns in the Pandoc AST (Divs with special classes like `.panel-tabset`, `.callout-warning`) into typed `CustomNode` representations.

### Conceptual Model

```
QMD Source                  Pandoc AST                    Quarto AST
─────────────               ───────────                   ──────────
::: {.callout-note}   ──►   Div(classes=["callout-note"]) ──►   CustomNode(type="Callout")
## Title                      Header(2, "Title")                  slots: {title, content}
Body                          Paragraph("Body")                   plain_data: {type: "note"}
:::

::: {.panel-tabset}   ──►   Div(classes=["panel-tabset"]) ──►   CustomNode(type="Tabset")
## Tab 1                      Header(2, "Tab 1")                  slots: {tabs: [...]}
Content 1                     Paragraph("Content 1")              plain_data: {level: 2}
## Tab 2                      Header(2, "Tab 2")
Content 2                     Paragraph("Content 2")
:::
```

### Why This Matters

1. **Type Safety**: CustomNodes have typed slots (title, content, tabs) vs generic Div children
2. **Rendering**: HTML writer can render CustomNodes directly without class-checking
3. **Tooling**: LSP, analysis tools can understand document structure
4. **Testing**: Each custom node type has predictable structure

---

## Current State

### Existing Infrastructure

**`quarto-pandoc-types/src/custom.rs`**:
```rust
pub struct CustomNode {
    pub type_name: String,           // "Callout", "Tabset", etc.
    pub slots: LinkedHashMap<String, Slot>,  // Named AST content
    pub plain_data: Value,           // JSON configuration
    pub attr: Attr,                  // Original Div attributes
    pub source_info: SourceInfo,
}

pub enum Slot {
    Block(Box<Block>),
    Inline(Box<Inline>),
    Blocks(Blocks),
    Inlines(Inlines),
}
```

**`quarto-pandoc-types/src/block.rs`**:
```rust
pub enum Block {
    // ... standard Pandoc blocks ...
    Custom(CustomNode),  // ✅ Already present
}
```

### Existing Transform: CalloutTransform

`crates/quarto-core/src/transforms/callout.rs` already implements this pattern:

1. Walk AST looking for Divs
2. Check if Div has `.callout-*` class
3. Extract title (first Header) and content (remaining blocks)
4. Build `CustomNode` with type="Callout", slots, plain_data
5. Replace Div with `Block::Custom(node)`

This is the reference implementation for new custom node types.

---

## Custom Node Types to Implement

### Already Implemented

| Type | Implementation | Notes |
|------|----------------|-------|
| **Callout** | `Block::Custom(CustomNode)` | ✅ Complete in `transforms/callout.rs` |
| **Shortcode** | `Inline::Shortcode(Shortcode)` | First-class Inline variant (not CustomNode). Parsing exists in tree-sitter. May need handler expansion. |

### Priority 1: Core Features

| Type | QMD Pattern | Slots | Plain Data |
|------|-------------|-------|------------|
| **Tabset** | `.panel-tabset` | tabs: Vec<Tab> | level |
| **DecoratedCodeBlock** | CodeBlock + filename attr | code: Block | filename, fold, annotations |
| **FloatRefTarget** | Divs with `#fig-*`, `#tbl-*` IDs | content, caption_long, caption_short | ref_type, identifier |

**Note on FloatRefTarget**: This is foundational for cross-references. In TS Quarto, it's created programmatically by `parse_floatreftargets()`, not by class-matching. It's simpler than PanelLayout but more important.

### Priority 2: Secondary Features

| Type | QMD Pattern | Slots | Plain Data |
|------|-------------|-------|------------|
| **ContentHidden** | `.content-visible`, `.content-hidden` | content: Blocks | condition, when |
| **Theorem** | `.theorem`, `.lemma`, etc. | title: Inlines, content: Blocks | theorem_type, number |
| **Proof** | `.proof` | content: Blocks | - |

### Priority 3: Deferred (Complex Dependencies)

| Type | QMD Pattern | Reason to Defer |
|------|-------------|-----------------|
| **PanelLayout** | `.panel-*` classes | Interacts with FloatRefTarget in non-trivial ways (search TS Quarto for `is_float_reftarget`). Should implement after FloatRefTarget is working. |

---

## Design

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    AstTransformsStage                           │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐   │
│  │ Callout      │  │ Tabset       │  │ DecoratedCodeBlock   │   │
│  │ Transform    │  │ Transform    │  │ Transform            │   │
│  │ (existing)   │  │ (new)        │  │ (new)                │   │
│  └──────────────┘  └──────────────┘  └──────────────────────┘   │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐                             │
│  │ PanelLayout  │  │ ContentHidden│                             │
│  │ Transform    │  │ Transform    │                             │
│  │ (new)        │  │ (new)        │                             │
│  └──────────────┘  └──────────────┘                             │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    Block::Custom(CustomNode)
```

### Transform Pattern

Each transform follows the same pattern as `CalloutTransform`:

```rust
pub struct TabsetTransform;

impl AstTransform for TabsetTransform {
    fn name(&self) -> &str { "tabset" }

    fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        transform_blocks(&mut ast.blocks);
        Ok(())
    }
}

fn transform_blocks(blocks: &mut Vec<Block>) {
    for block in blocks.iter_mut() {
        // 1. Recurse into nested blocks
        // 2. Check if this Div matches our pattern
        // 3. Convert to CustomNode if so
    }
}
```

### Registration

Transforms are registered in `build_transform_pipeline()`:

```rust
pub fn build_transform_pipeline() -> TransformPipeline {
    let mut pipeline = TransformPipeline::new();

    // Existing
    pipeline.push(Box::new(CalloutTransform::new()));
    pipeline.push(Box::new(CalloutResolveTransform::new()));

    // New sugaring transforms
    pipeline.push(Box::new(TabsetTransform::new()));
    pipeline.push(Box::new(DecoratedCodeBlockTransform::new()));
    pipeline.push(Box::new(PanelLayoutTransform::new()));
    pipeline.push(Box::new(ContentHiddenTransform::new()));

    // Existing
    pipeline.push(Box::new(MetadataNormalizeTransform::new()));
    pipeline.push(Box::new(TitleBlockTransform::new()));
    pipeline.push(Box::new(ResourceCollectorTransform::new()));

    pipeline
}
```

---

## Detailed Specifications

### Tabset (`.panel-tabset`)

**Input**:
```markdown
::: {.panel-tabset}
## Tab 1
Content for tab 1

## Tab 2
Content for tab 2
:::
```

**Parsed as**:
```
Div(classes=["panel-tabset"])
  Header(level=2, "Tab 1")
  Paragraph("Content for tab 1")
  Header(level=2, "Tab 2")
  Paragraph("Content for tab 2")
```

**CustomNode**:
```rust
CustomNode {
    type_name: "Tabset",
    slots: {
        "tabs": Slot::Blocks([
            // Each tab is a Div containing header + content
            Div { content: [Header("Tab 1"), Paragraph("Content 1")] },
            Div { content: [Header("Tab 2"), Paragraph("Content 2")] },
        ])
    },
    plain_data: { "level": 2 },
    attr: original_div_attr,
}
```

**Algorithm**:
1. Find first Header to determine tab level
2. Split content at Headers of that level
3. Each section becomes a tab
4. Preserve Header in tab for title extraction during rendering

### DecoratedCodeBlock

**Input**:
```markdown
```{.python filename="example.py"}
print("Hello")
```
```

**Parsed as**:
```
CodeBlock(classes=["python"], attrs={"filename": "example.py"})
```

**CustomNode**:
```rust
CustomNode {
    type_name: "DecoratedCodeBlock",
    slots: {
        "code": Slot::Block(CodeBlock { ... })
    },
    plain_data: {
        "filename": "example.py",
        "language": "python"
    },
    attr: code_block_attr,
}
```

**Note**: This is slightly different - it transforms a CodeBlock, not a Div. The transform checks for decoration attributes (filename, code-fold, etc.) on CodeBlocks.

### ContentHidden

**Input**:
```markdown
::: {.content-visible when-format="html"}
HTML-only content
:::
```

**Parsed as**:
```
Div(classes=["content-visible"], attrs={"when-format": "html"})
  Paragraph("HTML-only content")
```

**CustomNode**:
```rust
CustomNode {
    type_name: "ContentHidden",
    slots: {
        "content": Slot::Blocks([Paragraph(...)])
    },
    plain_data: {
        "visible": true,  // content-visible vs content-hidden
        "when_format": "html",
        "unless_format": null
    },
    attr: original_div_attr,
}
```

---

## Work Items

### Phase A: Infrastructure (if needed)

- [ ] Verify CustomNode serialization works for new slot patterns
- [ ] Add any needed slot types (e.g., Vec<Tab> might need special handling)
- [ ] Verify Shortcode handling is complete (already exists as `Inline::Shortcode`)

### Phase B: Tabset Transform

- [ ] Create `crates/quarto-core/src/transforms/tabset.rs`
- [ ] Implement `TabsetTransform` following CalloutTransform pattern
- [ ] Parse Headers to identify tabs
- [ ] Build CustomNode with tab slots
- [ ] Unit tests with various tabset configurations
- [ ] Register in pipeline

### Phase C: DecoratedCodeBlock Transform

- [ ] Create `crates/quarto-core/src/transforms/decorated_code_block.rs`
- [ ] Detect CodeBlocks with `filename` attribute
- [ ] Wrap in CustomNode preserving CodeBlock in slot
- [ ] Unit tests
- [ ] Register in pipeline

### Phase D: FloatRefTarget Transform

**Note**: This is more important than ContentHidden because it's foundational for cross-references.

- [ ] Study `parse_floatreftargets()` in TS Quarto (`quarto-pre/parsefiguredivs.lua`)
- [ ] Create `crates/quarto-core/src/transforms/float_ref_target.rs`
- [ ] Detect Divs/Figures with `#fig-*`, `#tbl-*`, `#lst-*` identifiers
- [ ] Extract caption (short and long variants)
- [ ] Build CustomNode with content, caption_long, caption_short slots
- [ ] Unit tests
- [ ] Register in pipeline

### Phase E: ContentHidden Transform

- [ ] Create `crates/quarto-core/src/transforms/content_hidden.rs`
- [ ] Detect `.content-visible` and `.content-hidden` classes
- [ ] Parse condition attributes (`when-format`, `unless-format`, etc.)
- [ ] Unit tests
- [ ] Register in pipeline

### Phase F: PanelLayout Transform (DEFERRED)

**Deferred**: PanelLayout has complex interactions with FloatRefTarget. Should only implement after FloatRefTarget is working and tested.

- [ ] Study `is_float_reftarget` usage in TS Quarto to understand interactions
- [ ] Create `crates/quarto-core/src/transforms/panel_layout.rs`
- [ ] Detect `.panel-*` classes (sidebar, fill, etc.)
- [ ] Handle FloatRefTarget children correctly
- [ ] Parse layout structure
- [ ] Unit tests
- [ ] Register in pipeline

---

## Testing Strategy

### Unit Test Structure

Each transform gets tests in its module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn parse_qmd(content: &str) -> Pandoc {
        // Parse QMD content to AST
    }

    #[test]
    fn test_simple_tabset() {
        let mut ast = parse_qmd(r#"
::: {.panel-tabset}
## Tab 1
Content 1
## Tab 2
Content 2
:::
        "#);

        let transform = TabsetTransform::new();
        transform.transform(&mut ast, &mut ctx).unwrap();

        match &ast.blocks[0] {
            Block::Custom(node) => {
                assert_eq!(node.type_name, "Tabset");
                // Verify tab structure
            }
            _ => panic!("Expected Custom block"),
        }
    }

    #[test]
    fn test_nested_tabset_in_callout() { ... }

    #[test]
    fn test_tabset_preserves_attributes() { ... }
}
```

### Test Fixtures

Create test fixtures in `crates/quarto-core/test-fixtures/sugaring/`:
- `tabset-simple.qmd` / `tabset-simple.expected.json`
- `tabset-nested.qmd` / `tabset-nested.expected.json`
- `decorated-code.qmd` / `decorated-code.expected.json`
- etc.

### Integration Tests

Add pipeline tests that verify end-to-end sugaring:

```rust
#[test]
fn test_full_sugaring_pipeline() {
    let ast = parse_document("test.qmd");
    let pipeline = build_transform_pipeline();
    pipeline.execute(&mut ast, &mut ctx).unwrap();

    // Verify all expected Divs became CustomNodes
    assert_no_raw_callout_divs(&ast);
    assert_no_raw_tabset_divs(&ast);
}
```

---

## Success Criteria

1. **Completeness**: All targeted Div patterns converted to CustomNodes
2. **Correctness**: Slot structure matches TS Quarto custom nodes
3. **Idempotence**: Running transform twice produces same result
4. **Preservation**: Non-matching Divs unchanged
5. **Source Info**: CustomNodes preserve source locations from original Divs
6. **Tests**: >90% code coverage on transform logic

---

## Open Questions

1. **Tab representation**: Should each tab be a separate slot, or should tabs be in a single `Slot::Blocks`?
   - Recommendation: Single `Slot::Blocks` where each Block is a Div containing one tab

2. **DecoratedCodeBlock trigger**: Should any CodeBlock with `filename` attr be wrapped, or only those with specific classes?
   - Recommendation: Wrap if has `filename` OR `code-fold` OR `code-annotations`

3. **Ordering**: Should sugaring transforms run before or after metadata normalization?
   - Recommendation: Before, so custom nodes can access normalized metadata

---

## References

### TS Quarto Custom Nodes
- `external-sources/quarto-cli/src/resources/filters/customnodes/`
- `external-sources/quarto-cli/src/resources/filters/ast/parse.lua`

### Existing Rust Implementation
- `crates/quarto-pandoc-types/src/custom.rs` - CustomNode type
- `crates/quarto-core/src/transforms/callout.rs` - Reference implementation
- `crates/quarto-core/src/pipeline.rs` - Transform registration
