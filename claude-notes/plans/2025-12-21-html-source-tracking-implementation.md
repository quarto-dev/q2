# HTML Source Tracking Implementation

**Issue:** k-q4rm (child of k-02o9)
**Created:** 2025-12-21
**Status:** Design Review

## Overview

Implement source location tracking in HTML output using a pointer-based AST node identification strategy. The key insight is that for an immutable AST reference, all node addresses are stable, allowing us to use pointers as HashMap keys to associate source info with nodes.

**Critical design constraint:** All source tracking work happens entirely within the HTML writer's entry point function. This guarantees pointer stability - we hold `&Pandoc` for the entire operation, so all node addresses remain valid.

## Architecture

### Core Strategy

```
html_writer::write_with_config(&pandoc, &context, &mut writer, &config)
│
│  All of the following happens while holding &Pandoc:
│
├─ 1. If source tracking enabled:
│     │
│     ├─ Generate JSON Value from &pandoc (in memory, not serialized)
│     │     └─ Uses existing json::write_pandoc() infrastructure
│     │
│     ├─ Parallel walk: &pandoc + JSON Value
│     │     └─ Build HashMap<*const (), SourceNodeInfo>
│     │
│     └─ Extract pool and file metadata for embedding
│
├─ 2. Create HtmlWriterContext with HashMap
│
└─ 3. Write HTML, looking up each node in HashMap
      └─ Emit data-loc and data-sid attributes when found
```

### Why This Design Guarantees Pointer Stability

```rust
pub fn write_with_config<W: Write>(
    pandoc: &Pandoc,           // <-- Reference held for entire function
    context: &ASTContext,
    writer: &mut W,
    config: &HtmlConfig,
) -> Result<()> {
    // All of these operations happen while &Pandoc is borrowed:

    // 1. Generate JSON (uses &pandoc)
    let json_value = generate_json(pandoc, context)?;

    // 2. Build source map (uses &pandoc - same reference!)
    let source_map = build_source_map(pandoc, &json_value);
    //                                 ^^^^^^
    //                                 Same reference - pointers are stable

    // 3. Write HTML (uses &pandoc - still the same reference!)
    let ctx = HtmlWriterContext::new(source_map, config);
    write_blocks_with_context(&pandoc.blocks, writer, &ctx)?;
    //                         ^^^^^^
    //                         Same reference - HashMap lookups work

    Ok(())
}
```

The HashMap stores `*const Block` / `*const Inline` pointers. These remain valid because:
1. We never release the `&Pandoc` borrow
2. The AST is immutable (no modifications that could move data)
3. Everything happens in one function call scope

### Data Structures

```rust
/// Information extracted from JSON for each AST node
#[derive(Debug, Clone)]
pub struct SourceNodeInfo {
    /// Pool ID (the "s" field from JSON)
    pub pool_id: usize,
    /// Resolved location (the "l" field from JSON)
    pub location: Option<ResolvedLocation>,
}

/// Resolved source location
#[derive(Debug, Clone)]
pub struct ResolvedLocation {
    pub file_id: usize,
    pub start_line: usize,   // 1-based
    pub start_col: usize,    // 1-based
    pub end_line: usize,     // 1-based
    pub end_col: usize,      // 1-based
}

impl ResolvedLocation {
    /// Format as data-loc attribute value: "file:line:col-line:col"
    pub fn to_data_loc(&self) -> String {
        format!("{}:{}:{}-{}:{}",
            self.file_id,
            self.start_line, self.start_col,
            self.end_line, self.end_col)
    }
}

/// Configuration for HTML output
#[derive(Debug, Clone, Default)]
pub struct HtmlConfig {
    /// Include source location tracking (data-loc, data-sid attributes)
    pub include_source_locations: bool,
}

/// Context threaded through HTML writer functions.
///
/// This struct is generic over the writer type and implements `Write` itself,
/// so `write!` and `writeln!` macros can be used directly on the context.
/// This simplifies call sites - functions only need one `ctx` parameter.
pub struct HtmlWriterContext<'ast, W: Write> {
    /// The underlying writer
    writer: W,
    /// Map from AST node pointers to source info
    source_map: HashMap<*const (), SourceNodeInfo>,
    /// Source info pool (same format as JSON writer's astContext.sourceInfoPool)
    source_pool: Option<Vec<serde_json::Value>>,
    /// File metadata (same format as JSON writer's astContext.files)
    files: Option<Vec<serde_json::Value>>,
    /// Configuration
    config: HtmlConfig,
    /// Lifetime marker
    _phantom: std::marker::PhantomData<&'ast ()>,
}

/// Implement Write for HtmlWriterContext so write!/writeln! macros work directly
impl<'ast, W: Write> Write for HtmlWriterContext<'ast, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

impl<'ast, W: Write> HtmlWriterContext<'ast, W> {
    /// Look up source info for a block
    pub fn get_block_info(&self, block: &Block) -> Option<&SourceNodeInfo> {
        let key = block as *const Block as *const ();
        self.source_map.get(&key)
    }

    /// Look up source info for an inline
    pub fn get_inline_info(&self, inline: &Inline) -> Option<&SourceNodeInfo> {
        let key = inline as *const Inline as *const ();
        self.source_map.get(&key)
    }

    /// Check if source locations are enabled
    pub fn include_source_locations(&self) -> bool {
        self.config.include_source_locations
    }
}
```

### The Parallel Walk

The JSON and AST structures are parallel - both are produced from the same `&Pandoc`. The walk extracts "s" (pool ID) and "l" (resolved location) from each JSON node and stores them keyed by the corresponding AST node's pointer.

```rust
fn build_source_map<'ast>(
    pandoc: &'ast Pandoc,
    json: &Value,
) -> HashMap<*const (), SourceNodeInfo> {
    let mut map = HashMap::new();

    // Walk blocks
    if let Some(blocks_json) = json.get("blocks").and_then(|v| v.as_array()) {
        walk_blocks(&pandoc.blocks, blocks_json, &mut map);
    }

    map
}

fn walk_block(
    block: &Block,
    json: &Value,
    map: &mut HashMap<*const (), SourceNodeInfo>
) {
    // Extract source info from JSON
    if let Some(info) = extract_source_node_info(json) {
        let key = block as *const Block as *const ();
        map.insert(key, info);
    }

    // Recurse into children based on block type
    match block {
        Block::Paragraph(para) => {
            if let Some(inlines) = json.get("c").and_then(|v| v.as_array()) {
                walk_inlines(&para.content, inlines, map);
            }
        }
        Block::Div(div) => {
            // JSON: {"t": "Div", "c": [attr, blocks], ...}
            if let Some(content) = json.get("c")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.get(1))
                .and_then(|v| v.as_array())
            {
                walk_blocks(&div.content, content, map);
            }
        }
        // ... other block types follow same pattern
    }
}

fn extract_source_node_info(json: &Value) -> Option<SourceNodeInfo> {
    let pool_id = json.get("s")?.as_u64()? as usize;

    let location = json.get("l").and_then(|l| {
        Some(ResolvedLocation {
            file_id: l.get("f")?.as_u64()? as usize,
            start_line: l.get("b")?.get("l")?.as_u64()? as usize,
            start_col: l.get("b")?.get("c")?.as_u64()? as usize,
            end_line: l.get("e")?.get("l")?.as_u64()? as usize,
            end_col: l.get("e")?.get("c")?.as_u64()? as usize,
        })
    });

    Some(SourceNodeInfo { pool_id, location })
}
```

### HTML Output

When source tracking is enabled, elements get `data-loc` and `data-sid` attributes:

```html
<p data-loc="0:5:1-5:42" data-sid="17">Paragraph text</p>
<h2 data-loc="0:10:1-10:20" data-sid="23">Section Title</h2>
```

The source pool is embedded in the document:

```html
<script type="application/json" id="quarto-source-map">
{
  "files": [
    {"name": "document.qmd", "line_breaks": [...], "total_length": 1234}
  ],
  "sourceInfoPool": [
    {"r": [0, 100], "t": 0, "d": 0},
    ...
  ]
}
</script>
```

This uses the same format as the JSON writer's `astContext`, so existing JavaScript code for processing it can be reused.

## Implementation Plan

### Phase A: HtmlWriterContext and Refactor

**Goal:** Thread a context object through all HTML writer functions without changing behavior. The context contains the writer and implements `Write`, so functions only need one parameter.

1. **Create `HtmlConfig` struct**
   - `include_source_locations: bool`

2. **Create `HtmlWriterContext<'ast, W: Write>` struct**
   - Contains the writer (`W`)
   - Placeholder for source_map (empty HashMap)
   - Implements `Write` trait by delegating to inner writer

3. **Refactor internal functions to use context only**
   - `write_block(block, buf)` → `write_block(block, ctx)`
   - `write_inline(inline, buf)` → `write_inline(inline, ctx)`
   - `write_blocks(blocks, buf)` → `write_blocks(blocks, ctx)`
   - `write_inlines(inlines, buf)` → `write_inlines(inlines, ctx)`
   - All functions use `write!(ctx, ...)` and `writeln!(ctx, ...)` directly

4. **Add new entry point**
   - `write_with_config(pandoc, context, writer, config)` - new configurable entry
   - Creates `HtmlWriterContext` wrapping the writer
   - Update existing `write()` to call `write_with_config` with default config

5. **Verify**: All existing tests pass, no behavior change.

**Example of refactored function:**
```rust
// Before
fn write_block<T: Write>(block: &Block, buf: &mut T) -> io::Result<()> {
    match block {
        Block::Paragraph(para) => {
            write!(buf, "<p>")?;
            write_inlines(&para.content, buf)?;
            writeln!(buf, "</p>")?;
        }
        // ...
    }
    Ok(())
}

// After
fn write_block<W: Write>(block: &Block, ctx: &mut HtmlWriterContext<'_, W>) -> io::Result<()> {
    match block {
        Block::Paragraph(para) => {
            write!(ctx, "<p>")?;
            write_inlines(&para.content, ctx)?;
            writeln!(ctx, "</p>")?;
        }
        // ...
    }
    Ok(())
}
```

### Phase B: JSON Generation and Parallel Walk

**Goal:** Build the HashMap by generating JSON and walking both structures.

1. **Add internal JSON generation**
   - Call `json::write_pandoc()` to get `Value` (not serialized string)
   - Need to use `JsonConfig { include_inline_locations: true }`
   - Extract pool and files from the result

2. **Create `SourceNodeInfo` and `ResolvedLocation` structs**

3. **Implement `extract_source_node_info(json: &Value)`**
   - Extract "s" field → pool_id
   - Extract "l" field → ResolvedLocation

4. **Implement parallel walk functions**
   - `build_source_map(pandoc, json)` - entry point
   - `walk_blocks(blocks, json_array, map)`
   - `walk_block(block, json, map)` - match on block type, recurse
   - `walk_inlines(inlines, json_array, map)`
   - `walk_inline(inline, json, map)` - match on inline type, recurse

5. **Handle all AST node types**
   - Paragraph, Div, Header, CodeBlock, BlockQuote, etc.
   - Str, Emph, Strong, Link, Image, Span, etc.
   - Lists (ordered, bullet, definition)
   - Tables
   - Custom nodes (if any remain after transforms)

6. **Wire up in `write_with_config`**
   - If `config.include_source_locations`:
     - Generate JSON
     - Build source map
     - Store in context

### Phase C: Emit Source Attributes

**Goal:** Use the HashMap to emit `data-loc` and `data-sid` on HTML elements.

1. **Add helper method to HtmlWriterContext**
   ```rust
   impl<'ast, W: Write> HtmlWriterContext<'ast, W> {
       /// Write source attributes for a block if source tracking is enabled
       fn write_block_source_attrs(&mut self, block: &Block) -> io::Result<()> {
           if !self.config.include_source_locations {
               return Ok(());
           }
           if let Some(info) = self.get_block_info(block) {
               write!(self, " data-sid=\"{}\"", info.pool_id)?;
               if let Some(loc) = &info.location {
                   write!(self, " data-loc=\"{}\"", loc.to_data_loc())?;
               }
           }
           Ok(())
       }

       /// Write source attributes for an inline if source tracking is enabled
       fn write_inline_source_attrs(&mut self, inline: &Inline) -> io::Result<()> {
           // Similar pattern
       }
   }
   ```

2. **Update `write_block` variants**
   - Call `ctx.write_block_source_attrs(block)` when emitting opening tag
   ```rust
   Block::Paragraph(para) => {
       write!(ctx, "<p")?;
       ctx.write_block_source_attrs(block)?;  // Emits data-loc, data-sid if enabled
       write!(ctx, ">")?;
       write_inlines(&para.content, ctx)?;
       writeln!(ctx, "</p>")?;
   }
   ```

3. **Update `write_inline` variants**
   - Same pattern for inline elements

4. **Emit source pool in output**
   - At end of document (or in designated location)
   - `<script type="application/json" id="quarto-source-map">...</script>`
   - Contains `files` and `sourceInfoPool` in same format as JSON writer

### Phase D: Testing

1. **Unit tests for parallel walk**
   - Simple document (paragraphs, headers)
   - Nested structures (lists, blockquotes)
   - All inline types
   - Tables

2. **Unit tests for source attribute emission**
   - Verify `data-loc` format
   - Verify `data-sid` values
   - Verify pool embedding

3. **Integration test**
   - Full document → HTML with source tracking
   - Verify locations match expected source positions

## File Changes

All changes are within pampa:

```
crates/pampa/src/writers/
├── html.rs           # Refactor: add context parameter, new entry point
├── html_source.rs    # New: SourceNodeInfo, ResolvedLocation, parallel walk,
│                     #      build_source_map, extract functions
└── mod.rs            # Export html_source if needed
```

No changes to:
- Render pipeline
- quarto-core
- Other pampa modules (json.rs stays unchanged, we just call it)

## Edge Cases

1. **Nodes without source info**: If `extract_source_node_info` returns `None`, we simply don't store anything. The HTML writer checks `ctx.get_block_info()` and skips attributes if `None`.

2. **Synthetic nodes**: Some nodes may have default/empty SourceInfo. The JSON will still have "s" and possibly "l" fields. We store whatever is there.

3. **Mismatched walk**: If AST and JSON somehow diverge (shouldn't happen since both come from same `&Pandoc`), the walk may skip some nodes. This is defensive - we just won't have source info for those nodes.

## JavaScript Consumption

The embedded source map uses the same format as the JSON writer, so this code works:

```javascript
// Get source map from document
const sourceMapScript = document.getElementById('quarto-source-map');
const sourceMap = JSON.parse(sourceMapScript.textContent);

// Find element's source location
function getSourceLocation(element) {
    const dataLoc = element.getAttribute('data-loc');
    if (!dataLoc) return null;

    const match = dataLoc.match(/(\d+):(\d+):(\d+)-(\d+):(\d+)/);
    if (!match) return null;

    const fileId = parseInt(match[1]);
    return {
        file: sourceMap.files[fileId]?.name,
        start: { line: parseInt(match[2]), col: parseInt(match[3]) },
        end: { line: parseInt(match[4]), col: parseInt(match[5]) }
    };
}

// Get full source info from pool (for advanced use)
function getFullSourceInfo(element) {
    const poolId = element.getAttribute('data-sid');
    if (!poolId) return null;
    return sourceMap.sourceInfoPool[parseInt(poolId)];
}
```

## Summary of Key Decisions

1. **Store both pool ID and resolved location** - Pool ID (`data-sid`) enables full SourceInfo lookup; resolved location (`data-loc`) enables direct editor jumping.

2. **Use same format as JSON writer** - The embedded pool uses identical format to `astContext`, allowing JavaScript code reuse.

3. **All work inside HTML writer** - No render pipeline changes. Source tracking is purely an internal implementation detail of `write_with_config()`. This guarantees pointer stability.

4. **Emit on every element** - All AST nodes that produce HTML elements get source attributes. Performance implications can be addressed later if needed.
