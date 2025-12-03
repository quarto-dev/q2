# Topdown Traversal for Lua Filters

**Issue**: k-478 (Implement topdown traversal with stop signal for Lua filters)
**Parent**: k-477 (discovered-from)
**Date**: 2025-12-02

## Executive Summary

Implement the `traverse = "topdown"` mode for Lua filters. This processes parent elements before their children (root-to-leaf, depth-first), and supports a stop signal that prevents descent into children.

## Pandoc's Topdown Traversal

### Source Files

1. `pandoc-lua-marshal/src/Text/Pandoc/Lua/Topdown.hs` - Topdown type and traversal infrastructure
2. `pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Shared.hs` - `applyFilterTopdown` function
3. `pandoc-lua-marshal/src/Text/Pandoc/Lua/Walk.hs` - `TraversalControl` type

### Key Data Types

```haskell
-- Control signal for stopping descent
data TraversalControl = Continue | Stop

-- Wrapper for topdown traversal
data Topdown = Topdown
  { topdownControl :: TraversalControl
  , topdownNode :: TraversalNode
  }

-- Node types in traversal
data TraversalNode
  = TBlock Block
  | TBlocks [Block]
  | TInline Inline
  | TInlines [Inline]
```

### Stop Signal Semantics

From `Walk.hs`:

```haskell
-- | Retrieves a Traversal control value: @nil@ or a truthy value
-- translate to 'Continue', @false@ is treated to mean 'Stop'.
peekTraversalControl idx = (Continue <$ peekNil idx)
  <|> (liftLua (toboolean top) >>= \case
          True -> pure Continue
          False -> pure Stop)
```

Lua filter return values:
- `nil` → Continue (unchanged)
- `element` → Continue (replace element)
- `element, true` → Continue (replace element)
- `element, false` → **Stop** (replace element, don't descend into children)
- `{elements}` → Continue (splice)
- `{elements}, false` → Stop (splice, don't descend into children)

### Traversal Algorithm

From `Topdown.hs`:

```haskell
walkTopdownM mkListNode mkElemNode nodeToList f =
  f . Topdown Continue . mkListNode >=> \case
    Topdown Stop     node -> return $ nodeToList node  -- STOP: return without descending
    Topdown Continue node -> mconcat <$>
      traverse (f . Topdown Continue . mkElemNode >=> \case
                   Topdown Stop     node' -> return $ nodeToList node'  -- STOP
                   Topdown Continue node' -> traverse (walkM f) $       -- CONTINUE: recurse
                                             nodeToList node')
               (nodeToList node)
```

The algorithm:
1. Apply filter to the list first (TBlocks/TInlines)
2. If Stop, return the result without descending
3. If Continue, for each element in the list:
   a. Apply filter to the element (TBlock/TInline)
   b. If Stop, return the element without descending into children
   c. If Continue, recursively walk children

### applyFilterTopdown Implementation

From `Shared.hs`:

```haskell
applyFilterTopdown filter' topdown@(Topdown _ node) =
  case node of
    TBlock x ->
      case filter' `getFunctionFor` x of
        Nothing -> pure topdown  -- No filter, keep going
        Just fn -> do
          (blocks, ctrl) <- applySplicingFunction fn pushBlock peekBlocksFuzzy x
          pure $ Topdown ctrl $ TBlocks blocks

    TBlocks xs ->
      case "Blocks" `lookup` filter' of
        Nothing -> pure topdown
        Just fn -> do
          (blocks, ctrl) <- applyStraightFunction fn pushBlocks peekBlocksFuzzy xs
          pure $ Topdown ctrl $ TBlocks blocks

    -- Similar for TInline and TInlines
```

## Current Implementation Gap

Our current implementation in `filter.rs` has a placeholder:

```rust
WalkingOrder::Topdown => {
    // TODO: Implement topdown traversal
    // For now, fall back to typewise
    apply_typewise_filter(&lua, &filter_table, &pandoc.blocks)?
}
```

## Implementation Plan

### Phase 1: Add TraversalControl

```rust
/// Control signal from filter return value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TraversalControl {
    Continue,
    Stop,
}

/// Parse traversal control from Lua return value
/// Returns (elements, control) where control is:
/// - Continue if second return value is nil or truthy
/// - Stop if second return value is false
fn parse_traversal_control(lua: &Lua, ...) -> Result<(Vec<T>, TraversalControl)> {
    // Check if there's a second return value
    // If nil or missing -> Continue
    // If false -> Stop
    // If true -> Continue
}
```

### Phase 2: Topdown Walk Functions

```rust
/// Topdown traversal: process parents before children, support stop signal
fn walk_topdown(lua: &Lua, filter: &Table, blocks: &[Block]) -> Result<Vec<Block>> {
    // 1. Apply Blocks filter first (if present)
    let (blocks, ctrl) = apply_blocks_filter_topdown(lua, filter, blocks)?;
    if ctrl == TraversalControl::Stop {
        return Ok(blocks);
    }

    // 2. For each block, apply filter then recurse
    let mut result = Vec::new();
    for block in &blocks {
        let (filtered, ctrl) = apply_block_filter_topdown(lua, filter, block)?;
        for b in filtered {
            if ctrl == TraversalControl::Stop {
                // Don't descend into children
                result.push(b);
            } else {
                // Recurse into children
                result.push(walk_block_children_topdown(lua, filter, &b)?);
            }
        }
    }
    Ok(result)
}

fn walk_block_children_topdown(lua: &Lua, filter: &Table, block: &Block) -> Result<Block> {
    match block {
        Block::Paragraph(p) => {
            let content = walk_inlines_topdown(lua, filter, &p.content)?;
            Ok(Block::Paragraph(Paragraph { content, ..p.clone() }))
        }
        Block::BlockQuote(b) => {
            let content = walk_topdown(lua, filter, &b.content)?;
            Ok(Block::BlockQuote(BlockQuote { content, ..b.clone() }))
        }
        // ... other block types
    }
}

fn walk_inlines_topdown(lua: &Lua, filter: &Table, inlines: &[Inline]) -> Result<Vec<Inline>> {
    // Similar pattern:
    // 1. Apply Inlines filter first
    // 2. For each inline, apply filter then recurse if Continue
}
```

### Phase 3: Filter Return Value Parsing

Modify the filter call to capture the second return value:

```rust
fn call_filter_with_control<T>(
    lua: &Lua,
    filter_fn: &Function,
    element: T,
) -> Result<(Vec<T>, TraversalControl)> {
    // Push element
    // Call function with 1 arg, 2 results
    // First result: element(s)
    // Second result: nil/true/false for control
    let ret: MultiValue = filter_fn.call(element_ud)?;
    let mut iter = ret.into_iter();

    let elements = match iter.next() {
        Some(Value::Nil) => vec![original],
        Some(Value::UserData(ud)) => vec![extract_element(ud)?],
        Some(Value::Table(t)) => extract_element_list(t)?,
        _ => vec![original],
    };

    let control = match iter.next() {
        Some(Value::Boolean(false)) => TraversalControl::Stop,
        _ => TraversalControl::Continue,
    };

    Ok((elements, control))
}
```

## Test Cases

### Test 1: Basic Topdown Order

```lua
return {
    traverse = "topdown",

    Para = function(elem)
        print("Para")
        return elem
    end,

    Str = function(elem)
        print("Str:" .. elem.text)
        return elem
    end
}
```

With document `Para([Str("a"), Str("b")])`:
- Expected order: `["Para", "Str:a", "Str:b"]` (parent before children)
- Typewise would be: `["Str:a", "Str:b", "Para"]` (children before parent)

### Test 2: Stop Signal Prevents Descent

```lua
return {
    traverse = "topdown",

    Div = function(elem)
        -- Stop descent into this div
        return elem, false
    end,

    Str = function(elem)
        return pandoc.Str(elem.text:upper())
    end
}
```

With document:
```
Div([Para([Str("inside")])])
Para([Str("outside")])
```

Expected:
- `"outside"` → `"OUTSIDE"` (Str filter applied)
- `"inside"` → `"inside"` (Str filter NOT applied, descent stopped at Div)

### Test 3: Blocks/Inlines Filters in Topdown

```lua
return {
    traverse = "topdown",

    Blocks = function(blocks)
        print("Blocks")
        return blocks
    end,

    Para = function(elem)
        print("Para")
        return elem
    end
}
```

With document `[Para, Para]`:
- Expected order: `["Blocks", "Para", "Para"]`
- Blocks is processed BEFORE individual blocks in topdown mode

### Test 4: Splice with Stop

```lua
return {
    traverse = "topdown",

    Str = function(elem)
        if elem.text == "expand" then
            return {pandoc.Str("a"), pandoc.Str("b")}, false
        end
        return elem
    end,

    Emph = function(elem)
        -- This should NOT be called for content from spliced "expand"
        return elem
    end
}
```

## Files to Modify

1. `crates/quarto-markdown-pandoc/src/lua/filter.rs`:
   - Add `TraversalControl` enum
   - Implement `walk_topdown`, `walk_inlines_topdown`
   - Modify filter calls to capture second return value
   - Update `apply_lua_filter` to use `walk_topdown` for topdown mode

## Edge Cases

1. **Empty return with stop**: `return {}, false` should delete element AND stop descent
2. **Nested stop signals**: If parent returns `Continue` but child returns `Stop`, only child's subtree stops
3. **Filter returns wrong type**: Should treat as unchanged and Continue
4. **Multiple elements from splice**: Stop signal applies to ALL spliced elements

## Performance Considerations

Topdown traversal is a single pass (unlike typewise's four passes), so it may be more efficient for certain filter patterns. However, it visits each node only once, so filters that need to see all inlines before any blocks cannot work correctly in topdown mode.
