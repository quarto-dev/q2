# Lua Filter Traversal Order Fix

**Issue**: k-477 (Investigate Lua filter traversal order)
**Parent**: k-409 (Lua filter support for quarto-markdown-pandoc)
**Date**: 2025-12-02

## Executive Summary

Our Lua filter traversal does not match Pandoc's behavior. Pandoc's typewise traversal performs **four separate passes** over the document, while ours does a **single bottom-up pass**. This must be fixed for compatibility.

## Pandoc's Traversal System

### Source Files Analyzed

1. `pandoc-lua-engine/src/Text/Pandoc/Lua/Module/Pandoc.hs` - `walkElement` function
2. `pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Pandoc.hs` - `applyFully` function
3. `pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Shared.hs` - `walkBlocksAndInlines` function
4. `pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Filter.hs` - `Filter` type and `getFunctionFor`
5. `pandoc-lua-marshal/src/Text/Pandoc/Lua/Walk.hs` - walk functions
6. `pandoc-lua-marshal/src/Text/Pandoc/Lua/Topdown.hs` - topdown traversal

### Filter Type

```haskell
data Filter = Filter
  { filterWalkingOrder :: WalkingOrder
  , filterMap :: Map Name FilterFunction
  }

data WalkingOrder
  = WalkForEachType  -- typewise (default)
  | WalkTopdown      -- topdown
```

The `traverse` field in the filter table determines the walking order:
- `"typewise"` or absent → `WalkForEachType`
- `"topdown"` → `WalkTopdown`

### Typewise Traversal (WalkForEachType)

From `walkBlocksAndInlines` in Shared.hs:

```haskell
walkBlocksAndInlines filter' =
  case filterWalkingOrder filter' of
    WalkForEachType -> walkInlineSplicing filter'
                   >=> walkInlinesStraight filter'
                   >=> walkBlockSplicing filter'
                   >=> walkBlocksStraight filter'
```

This is **four separate passes** over the entire document:

1. **`walkInlineSplicing`**: Visit ALL inline elements (Str, Emph, Strong, etc.)
   - Uses `getFunctionFor` which tries constructor name first, then type name ("Inline")
   - Splicing support: filter can return a list to replace a single element

2. **`walkInlinesStraight`**: Visit ALL inline lists (calls "Inlines" filter)
   - Only calls the "Inlines" function, not element-specific ones
   - No splicing: must return a list

3. **`walkBlockSplicing`**: Visit ALL block elements (Para, Header, Div, etc.)
   - Same pattern as inline splicing

4. **`walkBlocksStraight`**: Visit ALL block lists (calls "Blocks" filter)
   - Same pattern as inline straight

Each pass does a **bottom-up traversal** of the tree (children before parents), but crucially, each pass visits the ENTIRE document before the next pass begins.

### `getFunctionFor` - Generic Fallback Logic

```haskell
getFunctionFor filter' x =
  let constrName = fromString . showConstr . toConstr $ x
      typeName = fromString . tyconUQname . dataTypeName . dataTypeOf $ x
  in constrName `lookup` filter' <|>
     typeName   `lookup` filter'
```

For an inline like `Str "hello"`:
1. First tries `"Str"` (constructor name)
2. If not found, tries `"Inline"` (type name)

This is the **fallback logic** that makes `Inline` and `Block` act as generic handlers.

### `applyFully` - Full Document Filtering

```haskell
applyFully filter' doc = case filterWalkingOrder filter' of
  WalkForEachType -> walkBlocksAndInlines filter' doc
                 >>= applyMetaFunction filter'
                 >>= applyPandocFunction filter'
  WalkTopdown     -> applyPandocFunction filter' doc
                 >>= applyMetaFunction filter'
                 >>= walkBlocksAndInlines filter'
```

For typewise: Inlines → Blocks → Meta → Pandoc
For topdown: Pandoc → Meta → Inlines/Blocks (depth-first)

### Topdown Traversal

From `Topdown.hs` and `Shared.hs`:

```haskell
applyFilterTopdown :: Filter -> Topdown -> LuaE e Topdown
```

- Processes root first, descends depth-first to children
- Can be cut short by returning `false` as second value
- Uses `TraversalControl` (Continue/Stop)

## Our Current Implementation

### Problem 1: Single-pass Bottom-up Traversal

In `filter.rs`, our `apply_filter_to_block` does:

```rust
fn apply_filter_to_block(lua, filter_table, block) -> Result<Vec<Block>> {
    // 1. First, recursively process children
    let block_with_filtered_children = filter_block_children(lua, filter_table, block)?;

    // 2. Then apply type-specific or generic filter
    // ...
}
```

This processes each block's inline children completely before processing the block itself.

### Problem 2: Blocks/Inlines Filters Called First

In `apply_filter_to_blocks`:

```rust
fn apply_filter_to_blocks(lua, filter_table, blocks) -> Result<Vec<Block>> {
    // Check for Blocks filter FIRST
    if let Ok(blocks_fn) = filter_table.get::<Function>("Blocks") {
        // ...
    }

    // Then process each block
    for block in blocks {
        result.extend(apply_filter_to_block(lua, filter_table, block)?);
    }
}
```

This calls the Blocks filter BEFORE individual block filters, which is wrong.

### Problem 3: Missing `traverse` Field Support

We don't read the `traverse` field from the filter table to determine typewise vs topdown.

## Required Changes

### Phase 1: Separate Traversal Passes

Rewrite `apply_lua_filter` to use four separate passes for typewise:

```rust
pub fn apply_lua_filter(...) -> FilterResult<(Pandoc, ASTContext)> {
    // ...load filter...

    let walking_order = get_walking_order(&filter_table)?;

    let filtered_blocks = match walking_order {
        WalkingOrder::Typewise => {
            // Pass 1: Walk all inlines (splicing)
            let blocks = walk_inline_splicing(&lua, &filter_table, &pandoc.blocks)?;
            // Pass 2: Walk all inline lists (Inlines filter)
            let blocks = walk_inlines_straight(&lua, &filter_table, &blocks)?;
            // Pass 3: Walk all blocks (splicing)
            let blocks = walk_block_splicing(&lua, &filter_table, &blocks)?;
            // Pass 4: Walk all block lists (Blocks filter)
            walk_blocks_straight(&lua, &filter_table, &blocks)?
        }
        WalkingOrder::Topdown => {
            walk_topdown(&lua, &filter_table, &pandoc.blocks)?
        }
    };

    // ...
}
```

### Phase 2: Walk Functions

Implement four separate walk functions:

#### `walk_inline_splicing`
- Traverses the entire document tree
- For each inline element, applies its filter (type-specific or `Inline` fallback)
- Returns modified blocks with all inlines filtered
- Does NOT call `Inlines` or block filters

```rust
fn walk_inline_splicing(lua: &Lua, filter: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    // For each block, recursively walk its structure
    // Only apply inline element filters, not Inlines/Block/Blocks
    blocks.iter().map(|block| walk_block_for_inlines(lua, filter, block)).collect()
}

fn walk_block_for_inlines(lua: &Lua, filter: &Table, block: &Block) -> Result<Block> {
    match block {
        Block::Paragraph(p) => {
            // Walk inline content recursively
            let content = walk_inlines_for_elements(lua, filter, &p.content)?;
            Ok(Block::Paragraph(Paragraph { content, ..p.clone() }))
        }
        // ... other block types with inline content
        Block::BlockQuote(b) => {
            // Recurse into nested blocks
            let content = walk_inline_splicing(lua, filter, &b.content)?;
            Ok(Block::BlockQuote(BlockQuote { content, ..b.clone() }))
        }
        // ...
    }
}

fn walk_inlines_for_elements(lua: &Lua, filter: &Table, inlines: &[Inline]) -> Result<Vec<Inline>> {
    let mut result = Vec::new();
    for inline in inlines {
        // First walk children
        let walked = walk_inline_children(lua, filter, inline)?;
        // Then apply filter to this element
        result.extend(apply_inline_filter(lua, filter, &walked)?);
    }
    Ok(result)
}
```

#### `walk_inlines_straight`
- Traverses the entire document tree
- For each list of inlines, applies the `Inlines` filter if present
- Does NOT apply individual inline or block filters

```rust
fn walk_inlines_straight(lua: &Lua, filter: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    let inlines_fn = filter.get::<Function>("Inlines").ok();
    if inlines_fn.is_none() {
        return Ok(blocks.to_vec()); // No Inlines filter, pass through
    }

    // Walk the tree, applying Inlines filter to each inline list
    blocks.iter().map(|block| walk_block_for_inline_lists(lua, &inlines_fn.unwrap(), block)).collect()
}
```

#### `walk_block_splicing`
- Traverses the entire document tree
- For each block element, applies its filter (type-specific or `Block` fallback)
- Does NOT call `Blocks` or inline filters

#### `walk_blocks_straight`
- Traverses the entire document tree
- For each list of blocks, applies the `Blocks` filter if present
- Does NOT apply individual block filters

### Phase 3: Topdown Traversal

Implement `walk_topdown` for `traverse = "topdown"`:

```rust
fn walk_topdown(lua: &Lua, filter: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    // Apply Blocks filter first (if present)
    let (blocks, should_continue) = apply_blocks_filter_topdown(lua, filter, blocks)?;
    if !should_continue {
        return Ok(blocks);
    }

    // Then recurse into each block
    let mut result = Vec::new();
    for block in blocks {
        let (filtered, should_continue) = apply_block_filter_topdown(lua, filter, &block)?;
        if should_continue {
            // Continue walking children
            for b in filtered {
                result.push(walk_block_children_topdown(lua, filter, &b)?);
            }
        } else {
            result.extend(filtered);
        }
    }
    Ok(result)
}
```

Key difference: topdown applies filter BEFORE recursing into children, and respects `false` return to stop descent.

### Phase 4: types.rs Walk Method Updates

The `walk()` method on elements (`LuaBlock`, `LuaInline`) should also use the correct traversal based on the `traverse` field:

```rust
// In types.rs walk implementation
pub fn walk_blocks_with_filter(lua: &Lua, blocks: &[Block], filter: &Table) -> Result<Vec<Block>> {
    match get_walking_order(filter)? {
        WalkingOrder::Typewise => {
            // Four-pass traversal
            let blocks = walk_inline_splicing(lua, filter, blocks)?;
            let blocks = walk_inlines_straight(lua, filter, &blocks)?;
            let blocks = walk_block_splicing(lua, filter, &blocks)?;
            walk_blocks_straight(lua, filter, &blocks)
        }
        WalkingOrder::Topdown => {
            walk_topdown(lua, filter, blocks)
        }
    }
}
```

## Implementation Steps

1. **Add `WalkingOrder` enum and `get_walking_order` function**
   - Read `traverse` field from filter table
   - Default to `Typewise`

2. **Refactor `apply_lua_filter` to use walking order**
   - Branch on typewise vs topdown

3. **Implement `walk_inline_splicing`**
   - Traverse entire tree
   - Only apply inline element filters

4. **Implement `walk_inlines_straight`**
   - Traverse entire tree
   - Only apply `Inlines` list filter

5. **Implement `walk_block_splicing`**
   - Traverse entire tree
   - Only apply block element filters

6. **Implement `walk_blocks_straight`**
   - Traverse entire tree
   - Only apply `Blocks` list filter

7. **Implement `walk_topdown`**
   - Top-down depth-first traversal
   - Respect `false` return to stop descent

8. **Update `types.rs` walk methods**
   - Use the same traversal logic

9. **Add tests verifying traversal order**
   - Test that inlines are ALL processed before blocks
   - Test that `Inlines` filter is called after inline elements
   - Test that topdown respects order and stop signal

## Test Cases

### Test 1: Typewise Order Verification

```lua
local order = {}

function Str(elem)
    table.insert(order, "Str:" .. elem.text)
    return elem
end

function Inlines(inlines)
    table.insert(order, "Inlines")
    return inlines
end

function Para(elem)
    table.insert(order, "Para")
    return elem
end

function Blocks(blocks)
    table.insert(order, "Blocks")
    return blocks
end
```

With document `Para([Str("a")]), Para([Str("b")])`:
- Expected order: `["Str:a", "Str:b", "Inlines", "Inlines", "Para", "Para", "Blocks"]`
- Current (wrong): Something like `["Inlines", "Str:a", "Para", "Inlines", "Str:b", "Para", "Blocks"]`

### Test 2: Generic Fallback Priority

```lua
function Str(elem)
    return pandoc.Str(elem.text .. "-specific")
end

function Inline(elem)
    if elem.tag == "Space" then
        return pandoc.Str("-inline-")
    end
    return elem
end
```

- `Str` should use specific handler
- `Space` should use generic `Inline` handler (no `Space` function defined)

### Test 3: Topdown Stop Signal

```lua
return {
    traverse = "topdown",
    Div = function(elem)
        -- Stop walking into this div's children
        return elem, false
    end,
    Str = function(elem)
        return pandoc.Str(elem.text:upper())
    end
}
```

- Str elements outside Div should be uppercased
- Str elements inside Div should NOT be uppercased (descent stopped)

## Files to Modify

1. `crates/quarto-markdown-pandoc/src/lua/filter.rs` - Main refactor
2. `crates/quarto-markdown-pandoc/src/lua/types.rs` - Update walk methods
3. `crates/quarto-markdown-pandoc/tests/test_lua_filter_traversal.rs` - New test file

## Risks and Considerations

1. **Performance**: Four passes instead of one may be slower. Consider lazy evaluation or early exit when no relevant filters are present.

2. **Backward compatibility**: Existing filters may rely on our current (incorrect) behavior. This is acceptable since we're aiming for Pandoc compatibility.

3. **Edge cases**: Filters that modify structure (add/remove blocks) during traversal need careful handling.

## Implementation Status: COMPLETE (2025-12-02)

### What Was Implemented

1. **Added `WalkingOrder` enum and `get_walking_order` function** - Reads `traverse` field from filter table
2. **Implemented four-pass typewise traversal**:
   - `walk_inline_splicing` - Pass 1: all inline element filters
   - `walk_inlines_straight` - Pass 2: all Inlines list filters
   - `walk_block_splicing` - Pass 3: all block element filters
   - `walk_blocks_straight` - Pass 4: all Blocks list filters
   - `apply_typewise_filter` - Orchestrates the four passes
3. **Refactored `apply_lua_filter`** to use the new traversal system
4. **Removed old single-pass code** - Cleaned up unused functions
5. **Added comprehensive tests**:
   - `test_typewise_traversal_order` - Verifies correct four-pass order
   - `test_generic_inline_fallback` - Tests Inline generic handler
   - `test_generic_block_fallback` - Tests Block generic handler
   - `test_type_specific_overrides_generic` - Tests fallback priority

### Follow-up Issues

The following items were deferred to separate issues:

1. **k-478: Implement topdown traversal with stop signal**
   - Plan: `claude-notes/plans/2025-12-02-topdown-traversal.md`
   - When `traverse = "topdown"`, process parents before children
   - Support stop signal (returning `false` as second value)

2. **k-479: Update elem:walk{} to use correct four-pass traversal** (blocked by k-478)
   - Plan: `claude-notes/plans/2025-12-02-elem-walk-fix.md`
   - The `walk_inlines_with_filter` and `walk_blocks_with_filter` functions in types.rs
   - Used for `elem:walk{}` Lua method
   - Should use same four-pass traversal as document-level filtering
   - Should also support topdown mode with stop signal

### Test Results

All 488 tests pass after implementation.

## References

- Pandoc Lua filters documentation: `external-sources/pandoc/doc/lua-filters.md`
- Pandoc source: `external-sources/pandoc-lua-marshal/src/Text/Pandoc/Lua/`
