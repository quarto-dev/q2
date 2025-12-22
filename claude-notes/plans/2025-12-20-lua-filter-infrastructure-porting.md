# Lua Filter Infrastructure Porting to Rust

**Date**: 2025-12-20
**Status**: Research Complete - Design Phase
**Issue**: k-thpl
**Parent Epic**: k-xlko (quarto render prototype)

## Executive Summary

This document analyzes Quarto's Lua filter infrastructure and proposes a design for porting it to Rust. The key insight is that in quarto-cli, the custom node system exists because all filters run through Pandoc's Lua environment. In the Rust port, we have the opportunity to implement custom nodes more directly in the AST layer.

## Current Lua Architecture

### Overview

The Lua filter infrastructure consists of ~2,500 lines across 8 core files:

| File | Lines | Purpose |
|------|-------|---------|
| `customnodes.lua` | ~800 | Custom node handler system |
| `emulatedfilter.lua` | ~150 | Filter wrapping and integration |
| `runemulation.lua` | ~300 | Filter orchestration and execution |
| `parse.lua` | ~100 | Div/Span → custom node conversion |
| `render.lua` | ~150 | Custom node → Pandoc AST rendering |
| `scopedwalk.lua` | ~400 | Alternative tree traversal |
| `init.lua` (datadir) | ~1,070 | Bootstrap and core utilities |
| `_utils.lua` (datadir) | ~640 | AST manipulation utilities |

### The Dual Representation Problem

Pandoc's AST only understands its native node types. Quarto needs additional node types (Callout, FloatRefTarget, Theorem, etc.). The solution:

**At Pandoc level**: Custom nodes are stored as `Div` or `Span` with special attributes:
```
Div {
  attributes = {
    __quarto_custom = "true",
    __quarto_custom_type = "Callout",
    __quarto_custom_id = "42",
    __quarto_custom_context = "Block"
  },
  content = [...slots as blocks...]
}
```

**At Quarto level**: Filters see strongly-typed objects:
```lua
{
  type = "note",
  title = Inlines {...},
  content = Blocks {...},
  appearance = "default",
  icon = true,
  collapse = false
}
```

**The Bridge**: Lua metatables create proxy objects that forward field access to the underlying Pandoc AST stored in `custom_node_data[id]`.

### Handler Registration

Handlers are registered with `_quarto.ast.add_handler()`:

```lua
_quarto.ast.add_handler({
  -- What triggers parsing
  class_name = { "callout", "callout-note", "callout-warning", ... },

  -- The custom node type name
  ast_name = "Callout",

  -- Block or Inline
  kind = "Block",

  -- Fields that contain AST content (stored in wrapper's content)
  slots = { "title", "content" },

  -- Convert Div with matching class to custom node
  parse = function(div)
    return {
      type = extract_type(div),
      title = extract_title(div),
      content = div.content,
      ...
    }
  end,

  -- Build custom node from parameters
  constructor = function(tbl)
    return {
      type = tbl.type or "note",
      title = tbl.title,
      content = tbl.content,
      ...
    }
  end
})
```

### Conditional Renderers

Multiple renderers can be registered for different output formats:

```lua
-- Default renderer (must exist)
_quarto.ast.add_renderer("Callout",
  function(_) return true end,  -- condition: always matches
  function(node)
    return pandoc.BlockQuote(node.content)
  end
)

-- HTML-specific renderer (checked first)
_quarto.ast.add_renderer("Callout",
  function(_) return _quarto.format.isHtmlOutput() end,
  function(node)
    return pandoc.RawBlock("html", render_bootstrap_callout(node))
  end
)

-- LaTeX-specific renderer
_quarto.ast.add_renderer("Callout",
  function(_) return _quarto.format.isLatexOutput() end,
  function(node)
    return pandoc.RawBlock("latex", render_tcolorbox(node))
  end
)
```

Renderers are checked in **reverse insertion order** (newest first). The first matching condition wins.

### Filter Execution Flow

```
Document with Divs/Spans
         ↓
[parse.lua] parse_extended_nodes()
    - Find Divs matching handler class names
    - Call handler.parse() → custom node data
    - Create wrapper Div with special attributes
    - Store data in custom_node_data[id]
         ↓
Document with Custom Node Wrappers
         ↓
[runemulation.lua] run_emulated_filter_chain()
    For each filter:
        ↓
    [customnodes.lua] run_emulated_filter()
        - Wrap Div/Span handlers to intercept custom nodes
        - When wrapper encountered:
          - Resolve actual data via custom_node_data[id]
          - Call filter.Callout(custom_data) if defined
          - Or filter.CustomBlock(custom_data) if defined
          - Or filter.Custom(custom_data) if defined
        - Regular Divs handled normally
        ↓
    ensure_vault() - manage temporary storage
         ↓
[render.lua] render_extended_nodes()
    - Find custom node wrappers
    - Look up handler by type name
    - Try conditional renderers (format-specific)
    - Fall back to default renderer
    - Return Pandoc AST
         ↓
Standard Pandoc Document
```

### The Vault System

A hidden `Div` with special UUID stores temporary content during filtering:
- Content added via `_quarto.ast.vault._added`
- Content removed via `_quarto.ast.vault._removed`
- Cleaned between each filter pass
- Removed before final output

### Slot-Based Storage

Slots are fields that contain AST content (Blocks or Inlines). They're stored in the wrapper Div's content array and accessed through the proxy:

```
Wrapper Div content = [slot1_blocks, slot2_inlines, ...]

Proxy access:
  callout.title    → wrapper.content[1]  (Inlines)
  callout.content  → wrapper.content[2]  (Blocks)
```

This allows Pandoc to traverse and transform the content while Quarto maintains the semantic structure.

## Design for Rust (Unified CustomNode)

After discussion, we've settled on a **unified CustomNode representation** that:
1. Works for all custom nodes (both "core" like Callout and user extensions)
2. Enables JSON filter compatibility
3. Supports future Lua filter interop with quarto-cli compatibility
4. Provides reasonable Rust filter ergonomics

### Slot System

Custom nodes have **slots** that contain AST content. Four slot types:

```rust
pub enum Slot {
    Block(Block),           // Single block
    Inline(Inline),         // Single inline
    Blocks(Vec<Block>),     // Multiple blocks
    Inlines(Vec<Inline>),   // Multiple inlines
}
```

**Example: Callout** has slots:
- `title: Inlines` - the callout title
- `content: Blocks` - the callout body

**Example: PanelTabset** has slots:
- `titles: Inlines` - Vec<Inline> where i-th element is a Span for tab i's title
- `contents: Blocks` - Vec<Block> where i-th element is a Div for tab i's content

This **parallel array storage** matches the Lua implementation. Filters must maintain the invariant that parallel arrays have matching lengths.

### CustomNode Structure

```rust
use hashlink::LinkedHashMap;  // preserves insertion order, already used in workspace
use serde_json::Value;

pub struct CustomNode {
    /// The custom node type name (e.g., "Callout", "PanelTabset", "FloatRefTarget")
    pub type_name: String,

    /// Slots containing AST content, ordered by insertion
    /// Order matters for serialization to Pandoc wrapper Div
    pub slots: LinkedHashMap<String, Slot>,

    /// Plain data (non-AST fields) stored as JSON
    /// Enables JSON filter compatibility and Lua serialization
    pub plain_data: Value,

    /// Pandoc attributes on the node
    pub attr: Attr,
}
```

**Why JSON for plain_data?**
- JSON filters can process custom nodes
- Lua filters need JSON serialization for the bridge
- quarto-cli's design stored plain data as JSON-serializable values
- Accepts dynamic typing tradeoff for interoperability

**Why LinkedHashMap for slots?**
- Preserves insertion order (important for wrapper Div serialization)
- Fast key lookup
- Iteration in insertion order
- Already a dependency of `quarto-pandoc-types` via `hashlink` crate

### Rust Filter Ergonomics

Utility methods provide typed access:

```rust
impl CustomNode {
    // === Slot Accessors ===

    pub fn get_block(&self, name: &str) -> Option<&Block> {
        match self.slots.get(name)? {
            Slot::Block(b) => Some(b),
            _ => None,
        }
    }

    pub fn get_inline(&self, name: &str) -> Option<&Inline> {
        match self.slots.get(name)? {
            Slot::Inline(i) => Some(i),
            _ => None,
        }
    }

    pub fn get_blocks(&self, name: &str) -> Option<&Vec<Block>> {
        match self.slots.get(name)? {
            Slot::Blocks(bs) => Some(bs),
            _ => None,
        }
    }

    pub fn get_inlines(&self, name: &str) -> Option<&Vec<Inline>> {
        match self.slots.get(name)? {
            Slot::Inlines(is) => Some(is),
            _ => None,
        }
    }

    // Mutable versions
    pub fn get_block_mut(&mut self, name: &str) -> Option<&mut Block> { ... }
    pub fn get_inline_mut(&mut self, name: &str) -> Option<&mut Inline> { ... }
    pub fn get_blocks_mut(&mut self, name: &str) -> Option<&mut Vec<Block>> { ... }
    pub fn get_inlines_mut(&mut self, name: &str) -> Option<&mut Vec<Inline>> { ... }

    // === Plain Data Accessors ===

    pub fn get_str(&self, name: &str) -> Option<&str> {
        self.plain_data.get(name)?.as_str()
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.plain_data.get(name)?.as_bool()
    }

    pub fn get_i64(&self, name: &str) -> Option<i64> {
        self.plain_data.get(name)?.as_i64()
    }

    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.plain_data.get(name)?.as_f64()
    }

    // Setters
    pub fn set_str(&mut self, name: &str, value: &str) {
        self.plain_data[name] = Value::String(value.to_string());
    }

    pub fn set_bool(&mut self, name: &str, value: bool) {
        self.plain_data[name] = Value::Bool(value);
    }
    // etc.
}
```

### Example: Rust Filter for Callout

```rust
fn filter_custom(node: &mut CustomNode, ctx: &FilterContext) -> FilterResult {
    if node.type_name != "Callout" {
        return FilterResult::Unchanged;
    }

    // Access plain data
    let callout_type = node.get_str("type").unwrap_or("note");
    let icon = node.get_bool("icon").unwrap_or(true);

    // Access and modify slots
    if callout_type == "warning" && icon {
        if let Some(title) = node.get_inlines_mut("title") {
            // Prepend warning emoji
            title.insert(0, Inline::Str("⚠️ ".to_string()));
        }
    }

    FilterResult::Modified
}
```

### Example: Rust Filter for PanelTabset

```rust
fn filter_custom(node: &mut CustomNode, ctx: &FilterContext) -> FilterResult {
    if node.type_name != "PanelTabset" {
        return FilterResult::Unchanged;
    }

    // Access parallel arrays
    let titles = node.get_inlines("titles");
    let contents = node.get_blocks("contents");

    if let (Some(titles), Some(contents)) = (titles, contents) {
        // Iterate over tabs (parallel arrays)
        for (i, (title, content)) in titles.iter().zip(contents.iter()).enumerate() {
            println!("Tab {}: {:?}", i, title);
        }
    }

    FilterResult::Unchanged
}
```

## Handler System Design

### Handler Trait

Handlers define how to parse Div/Span elements into CustomNode instances:

```rust
pub enum NodeKind {
    Block,
    Inline,
}

pub enum SlotType {
    Block,
    Inline,
    Blocks,
    Inlines,
}

pub struct SlotDefinition {
    pub name: &'static str,
    pub slot_type: SlotType,
}

pub trait CustomNodeHandler: Send + Sync {
    /// Class names that trigger parsing (e.g., ["callout", "callout-note"])
    fn class_names(&self) -> &[&str];

    /// The custom node type name (e.g., "Callout")
    fn type_name(&self) -> &str;

    /// Block or Inline
    fn kind(&self) -> NodeKind;

    /// Slot definitions for this node type
    fn slots(&self) -> &[SlotDefinition];

    /// Parse a Div/Span into a CustomNode
    fn parse(&self, node: &Block, ctx: &ParseContext) -> Option<CustomNode>;

    /// Construct a CustomNode from parameters (for programmatic creation)
    fn construct(&self, slots: LinkedHashMap<String, Slot>, plain_data: Value, attr: Attr) -> CustomNode {
        CustomNode {
            type_name: self.type_name().to_string(),
            slots,
            plain_data,
            attr,
        }
    }
}
```

### Example: Callout Handler

```rust
pub struct CalloutHandler;

impl CustomNodeHandler for CalloutHandler {
    fn class_names(&self) -> &[&str] {
        &["callout", "callout-note", "callout-warning", "callout-tip",
          "callout-caution", "callout-important"]
    }

    fn type_name(&self) -> &str { "Callout" }

    fn kind(&self) -> NodeKind { NodeKind::Block }

    fn slots(&self) -> &[SlotDefinition] {
        &[
            SlotDefinition { name: "title", slot_type: SlotType::Inlines },
            SlotDefinition { name: "content", slot_type: SlotType::Blocks },
        ]
    }

    fn parse(&self, node: &Block, ctx: &ParseContext) -> Option<CustomNode> {
        let Block::Div(attr, content) = node else { return None };

        // Extract callout type from class (e.g., "callout-warning" -> "warning")
        let callout_type = extract_callout_type(attr);

        // Extract title and content from div structure
        let (title, body) = extract_callout_parts(content);

        let mut slots = LinkedHashMap::new();
        slots.insert("title".to_string(), Slot::Inlines(title));
        slots.insert("content".to_string(), Slot::Blocks(body));

        let plain_data = json!({
            "type": callout_type,
            "appearance": extract_appearance(attr),
            "icon": extract_icon(attr),
            "collapse": extract_collapse(attr),
        });

        Some(CustomNode {
            type_name: "Callout".to_string(),
            slots,
            plain_data,
            attr: attr.clone(),
        })
    }
}
```

### Renderer Trait

Renderers convert CustomNode back to standard Pandoc AST, with format-conditional dispatch:

```rust
pub trait CustomNodeRenderer: Send + Sync {
    /// The custom node type this renders
    fn type_name(&self) -> &str;

    /// Check if this renderer applies to the current format
    fn condition(&self, format: &Format) -> bool;

    /// Render the custom node to standard Pandoc AST
    fn render(&self, node: &CustomNode, ctx: &RenderContext) -> Result<Vec<Block>>;
}

/// Registry with format-conditional rendering
pub struct RendererRegistry {
    /// Renderers by type name, in priority order (newest first)
    renderers: HashMap<String, Vec<Box<dyn CustomNodeRenderer>>>,
}

impl RendererRegistry {
    pub fn add_renderer(&mut self, renderer: Box<dyn CustomNodeRenderer>) {
        self.renderers
            .entry(renderer.type_name().to_string())
            .or_default()
            .insert(0, renderer);  // Insert at front (newest first)
    }

    pub fn render(&self, node: &CustomNode, format: &Format, ctx: &RenderContext) -> Result<Vec<Block>> {
        let renderers = self.renderers.get(&node.type_name)
            .ok_or_else(|| anyhow!("No renderers for {}", node.type_name))?;

        for renderer in renderers {
            if renderer.condition(format) {
                return renderer.render(node, ctx);
            }
        }

        Err(anyhow!("No matching renderer for {} in format {:?}", node.type_name, format))
    }
}
```

### Example: Callout Renderers

```rust
/// Default renderer (fallback for unsupported formats)
pub struct CalloutDefaultRenderer;

impl CustomNodeRenderer for CalloutDefaultRenderer {
    fn type_name(&self) -> &str { "Callout" }

    fn condition(&self, _format: &Format) -> bool { true }  // Always matches

    fn render(&self, node: &CustomNode, _ctx: &RenderContext) -> Result<Vec<Block>> {
        let content = node.get_blocks("content").unwrap_or(&vec![]);
        Ok(vec![Block::BlockQuote(content.clone())])
    }
}

/// HTML renderer (Bootstrap callouts)
pub struct CalloutHtmlRenderer;

impl CustomNodeRenderer for CalloutHtmlRenderer {
    fn type_name(&self) -> &str { "Callout" }

    fn condition(&self, format: &Format) -> bool {
        format.is_html_output()
    }

    fn render(&self, node: &CustomNode, ctx: &RenderContext) -> Result<Vec<Block>> {
        let callout_type = node.get_str("type").unwrap_or("note");
        let title = node.get_inlines("title").unwrap_or(&vec![]);
        let content = node.get_blocks("content").unwrap_or(&vec![]);
        let icon = node.get_bool("icon").unwrap_or(true);

        // Generate Bootstrap callout HTML
        let html = render_bootstrap_callout(callout_type, title, content, icon);
        Ok(vec![Block::RawBlock(Format::Html, html)])
    }
}
```

### Filter Integration

```rust
/// Result of filtering a node
pub enum FilterResult {
    /// Node unchanged
    Unchanged,
    /// Node was modified in place
    Modified,
    /// Replace node with these blocks
    Replace(Vec<Block>),
    /// Remove the node entirely
    Remove,
}

/// A filter that processes the document
pub trait DocumentFilter: Send + Sync {
    fn name(&self) -> &str;

    /// Filter a custom block node
    fn filter_custom_block(&self, _node: &mut CustomNode, _ctx: &mut FilterContext) -> FilterResult {
        FilterResult::Unchanged
    }

    /// Filter a custom inline node
    fn filter_custom_inline(&self, _node: &mut CustomNode, _ctx: &mut FilterContext) -> FilterResult {
        FilterResult::Unchanged
    }

    /// Filter a regular block (Div, Para, etc.)
    fn filter_block(&self, _block: &mut Block, _ctx: &mut FilterContext) -> FilterResult {
        FilterResult::Unchanged
    }

    /// Filter a regular inline (Span, Str, etc.)
    fn filter_inline(&self, _inline: &mut Inline, _ctx: &mut FilterContext) -> FilterResult {
        FilterResult::Unchanged
    }
}
```

## Core Custom Nodes to Implement

Based on the Lua filter analysis, these are the most important custom nodes:

### Tier 1 - Critical (Implement First)

1. **FloatRefTarget** (~1,080 LOC in Lua)
   - Figures and tables with cross-reference support
   - Subfloat hierarchies
   - Caption locations (top, bottom, margin)
   - Multiple format renderers

2. **Callout** (~427 LOC in Lua)
   - Note, warning, tip, caution, important
   - Collapsible, icon options
   - Bootstrap HTML, LaTeX tcolorbox renderers

3. **Theorem/Proof**
   - Mathematical theorem environments
   - Numbered, cross-referenced

### Tier 2 - Important

4. **PanelLayout**
   - Multi-cell layouts
   - Grid-based arrangement

5. **PanelTabset**
   - Tabbed content panels

6. **DecoratedCodeBlock**
   - Code blocks with annotations, filenames

### Tier 3 - Specialized

7. **ShortCode** - Shortcode expansion
8. **ContentHidden** - Conditional content
9. **LatexEnvironment** / **HtmlTag** - Raw format wrappers

## Pandoc Compatibility

The unified CustomNode design must serialize to/from Pandoc's wrapper Div format for:
1. JSON filters (users running custom JSON filters)
2. Lua filter interop (future Rust-Lua bridge)
3. Pandoc invocation (for non-native output formats)

### Serialization to Pandoc Wrapper Div

```rust
impl CustomNode {
    /// Convert to Pandoc wrapper Div for JSON/Lua interop
    pub fn to_pandoc_wrapper(&self) -> Block {
        // Slots become content blocks in insertion order
        let content = self.slots_to_blocks();

        // Plain data serialized as JSON string in attribute
        let plain_data_json = serde_json::to_string(&self.plain_data)
            .unwrap_or_else(|_| "{}".to_string());

        let mut attr = self.attr.clone();
        attr.classes.push("__quarto_custom".to_string());
        attr.attributes.extend([
            ("__quarto_custom".to_string(), "true".to_string()),
            ("__quarto_custom_type".to_string(), self.type_name.clone()),
            ("__quarto_custom_data".to_string(), plain_data_json),
        ]);

        Block::Div(attr, content)
    }

    /// Convert slots to content blocks for wrapper Div
    fn slots_to_blocks(&self) -> Vec<Block> {
        self.slots.iter().map(|(name, slot)| {
            // Each slot becomes a Div with class indicating slot name and type
            let (slot_type, content) = match slot {
                Slot::Block(b) => ("block", vec![b.clone()]),
                Slot::Inline(i) => ("inline", vec![Block::Plain(vec![i.clone()])]),
                Slot::Blocks(bs) => ("blocks", bs.clone()),
                Slot::Inlines(is) => ("inlines", vec![Block::Plain(is.clone())]),
            };

            Block::Div(
                Attr {
                    classes: vec!["__quarto_custom_slot".to_string()],
                    attributes: vec![
                        ("name".to_string(), name.clone()),
                        ("type".to_string(), slot_type.to_string()),
                    ],
                    ..Default::default()
                },
                content,
            )
        }).collect()
    }
}
```

### Deserialization from Pandoc Wrapper Div

```rust
impl CustomNode {
    /// Parse from Pandoc wrapper Div
    pub fn from_pandoc_wrapper(block: &Block) -> Option<Self> {
        let Block::Div(attr, content) = block else { return None };

        // Check for custom node marker
        if !attr.classes.contains(&"__quarto_custom".to_string()) {
            return None;
        }

        let type_name = attr.get_attribute("__quarto_custom_type")?;

        // Parse plain data from JSON attribute
        let plain_data_str = attr.get_attribute("__quarto_custom_data")
            .unwrap_or("{}");
        let plain_data: Value = serde_json::from_str(plain_data_str).ok()?;

        // Parse slots from content Divs
        let mut slots = LinkedHashMap::new();
        for block in content {
            if let Block::Div(slot_attr, slot_content) = block {
                if slot_attr.classes.contains(&"__quarto_custom_slot".to_string()) {
                    let name = slot_attr.get_attribute("name")?;
                    let slot_type = slot_attr.get_attribute("type")?;

                    let slot = match slot_type.as_str() {
                        "block" => Slot::Block(slot_content.first()?.clone()),
                        "inline" => {
                            if let Block::Plain(inlines) = slot_content.first()? {
                                Slot::Inline(inlines.first()?.clone())
                            } else { continue; }
                        }
                        "blocks" => Slot::Blocks(slot_content.clone()),
                        "inlines" => {
                            if let Block::Plain(inlines) = slot_content.first()? {
                                Slot::Inlines(inlines.clone())
                            } else { continue; }
                        }
                        _ => continue,
                    };

                    slots.insert(name.to_string(), slot);
                }
            }
        }

        // Remove custom node attributes from attr
        let mut clean_attr = attr.clone();
        clean_attr.classes.retain(|c| c != "__quarto_custom");
        clean_attr.attributes.retain(|(k, _)| {
            !k.starts_with("__quarto_custom")
        });

        Some(CustomNode {
            type_name: type_name.to_string(),
            slots,
            plain_data,
            attr: clean_attr,
        })
    }
}
```

### Round-Trip Guarantee

The serialization format is designed to round-trip correctly:

```rust
#[test]
fn test_round_trip() {
    let original = CustomNode {
        type_name: "Callout".to_string(),
        slots: LinkedHashMap::from_iter([
            ("title".to_string(), Slot::Inlines(vec![Inline::Str("Warning".into())])),
            ("content".to_string(), Slot::Blocks(vec![Block::Para(vec![Inline::Str("Be careful!".into())])])),
        ]),
        plain_data: json!({
            "type": "warning",
            "icon": true,
        }),
        attr: Attr::default(),
    };

    let wrapper = original.to_pandoc_wrapper();
    let restored = CustomNode::from_pandoc_wrapper(&wrapper).unwrap();

    assert_eq!(original.type_name, restored.type_name);
    assert_eq!(original.plain_data, restored.plain_data);
    // Slots should match (comparing structure, not object identity)
}
```

## Implementation Phases

### Phase 1: Core Infrastructure

1. Implement `Slot` enum and `CustomNode` struct
2. Implement slot accessor methods (get_block, get_inlines, etc.)
3. Implement plain_data accessor methods
4. Add Pandoc wrapper serialization/deserialization

### Phase 2: Handler/Renderer System

1. Implement `CustomNodeHandler` trait
2. Implement `HandlerRegistry` with class-name lookup
3. Implement `CustomNodeRenderer` trait
4. Implement `RendererRegistry` with format-conditional dispatch

### Phase 3: Core Handlers

1. Implement `CalloutHandler` with HTML/default renderers
2. Implement `FloatRefTargetHandler` with HTML renderer
3. Implement `PanelTabsetHandler` with HTML renderer
4. Implement `TheoremHandler` with HTML renderer

### Phase 4: Filter Integration

1. Implement `DocumentFilter` trait
2. Add custom node dispatch to document walker
3. Integrate with transform pipeline from render prototype

### Phase 5: Format-Specific Renderers

1. Add LaTeX renderers for Callout, FloatRefTarget, etc.
2. Add Typst renderers
3. Add fallback/default renderers for all node types

### Phase 6: Lua Interop (Future)

1. Design Rust-Lua bridge for filter execution
2. Enable calling existing Lua filters from Rust pipeline
3. Enable Lua filters to work with Rust-created CustomNodes

## Open Questions (Resolved)

1. **Block enum integration**: ✅ Decided: Use `Block::Custom(CustomNode)` variant
   - CustomNode is a first-class AST element as a Quarto extension to the Pandoc AST
   - No need for wrapper Div resolution during normal processing
   - Wrapper Div format only used for JSON filter and Lua interop
   - **Implementation note**: Must desugar CustomNode→Div/Span when writing Pandoc JSON, and resugar Div/Span→CustomNode when reading. Needs round-trip tests.

2. **Handler discovery**: ✅ Decided: Static registration to start
   - Register handlers at compile time initially
   - Can add dynamic registration later if needed for user extensions

3. **Lua interop priority**: ✅ Decided: Needed soon
   - Callout and FloatRefTarget require complex rendering logic
   - Porting all that Lua code immediately is expensive
   - Plan: Rust-Lua bridge to call existing Lua filters/renderers

4. **Performance (caching)**: Clarified - start without caching
   - "Parsing CustomNodes" means converting wrapper Div → CustomNode struct
   - This only happens for JSON filter and Lua interop paths
   - Start without caching (parsing is cheap: no I/O, just struct construction)
   - Add caching later if profiling shows it's a bottleneck

## Design Decisions Made

1. **Unified CustomNode struct** - All custom nodes (core and extension) use the same structure
2. **Slot-based storage** - Four slot types: Block, Inline, Blocks, Inlines
3. **JSON plain_data** - Non-slot fields stored as serde_json::Value for serialization
4. **LinkedHashMap for slots** - Preserves insertion order for wrapper Div serialization (uses existing `hashlink` dependency)
5. **Format-conditional renderers** - Multiple renderers per node type, selected by format
6. **Parallel array storage** - PanelTabset uses parallel titles/contents arrays (filter-maintained invariant)

## References

- **Pipeline Analysis**: [lua-filter-pipeline/00-index.md](./lua-filter-pipeline/00-index.md) - Full stage-by-stage analysis with side effects, Pandoc API usage, and WASM compatibility
- [Lua customnodes.lua](../../external-sources/quarto-cli/src/resources/filters/ast/customnodes.lua)
- [Lua emulatedfilter.lua](../../external-sources/quarto-cli/src/resources/filters/ast/emulatedfilter.lua)
- [Lua runemulation.lua](../../external-sources/quarto-cli/src/resources/filters/ast/runemulation.lua)
- [Lua init.lua (datadir)](../../external-sources/quarto-cli/src/resources/pandoc/datadir/init.lua)
- [Callout handler](../../external-sources/quarto-cli/src/resources/filters/customnodes/callout.lua)
- [FloatRefTarget handler](../../external-sources/quarto-cli/src/resources/filters/customnodes/floatreftarget.lua)
