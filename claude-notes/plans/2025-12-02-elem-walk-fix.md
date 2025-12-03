# Fix elem:walk{} Traversal Order

**Issue**: k-479 (Update elem:walk{} to use correct four-pass traversal)
**Parent**: k-477 (discovered-from)
**Date**: 2025-12-02

## Executive Summary

The `walk()` method on Pandoc elements (`elem:walk{}`) should use the same traversal semantics as document-level filtering. Currently, `types.rs` implements a single-pass traversal that interleaves inline and block processing, which doesn't match Pandoc's behavior.

## Current Implementation Analysis

### Location

`crates/quarto-markdown-pandoc/src/lua/types.rs`:
- `walk_inlines_with_filter` (line 1767)
- `walk_blocks_with_filter` (line 1853)

### Current Behavior

```rust
pub fn walk_inlines_with_filter(lua: &Lua, inlines: &[Inline], filter: &Table) -> Result<Vec<Inline>> {
    let topdown = is_topdown_traversal(filter);
    let mut result = Vec::new();

    for inline in inlines {
        if topdown {
            // Apply filter first, then walk children
        } else {
            // Walk children first, then apply filter
            let walked = walk_inline_with_filter(lua, inline, filter)?;
            // Apply type-specific or generic Inline filter
            let filtered = apply_filter_to_inline(...);
            result.extend(filtered);
        }
    }

    // PROBLEM: Inlines filter applied at END of THIS list
    if let Ok(filter_fn) = filter.get::<Function>("Inlines") {
        // Apply to result
    }

    Ok(result)
}
```

### Problems

1. **Interleaved passes**: For a structure like `Para([Str("a")]), Para([Str("b")])`, the current code processes:
   - First Para's inlines (Str:a, then Inlines for first list)
   - Second Para's inlines (Str:b, then Inlines for second list)
   - Para filters
   - Blocks filter

   But Pandoc's four-pass does:
   - ALL Str elements (Str:a, Str:b)
   - ALL Inlines lists
   - ALL Para elements
   - Blocks filter

2. **Blocks/Inlines called at wrong time**: Currently called after each list is processed, not as a separate pass.

3. **No support for topdown stop signal**: The topdown branch doesn't handle the `false` return value to stop descent.

## Pandoc's elem:walk() Semantics

From Pandoc documentation:

> `walk(self, lua_filter)`
>
> Applies a Lua filter to the element subtree. Returns a (deep) copy of the modified element.

The `walk()` method should behave identically to applying a filter to a document containing just that element. This means:

1. For typewise (default): four separate passes over the subtree
2. For topdown: depth-first, parent-before-children, with stop signal support

## Implementation Plan

### Phase 1: Refactor to Use Shared Traversal Functions

The four-pass functions from `filter.rs` should be reusable. Either:

**Option A**: Move traversal functions to a shared location and use from both places
**Option B**: Have `types.rs` call into `filter.rs` functions

Recommendation: **Option A** - create traversal module that both use.

```rust
// New file: crates/quarto-markdown-pandoc/src/lua/traversal.rs

pub mod typewise {
    pub fn walk_inline_splicing(...) -> Result<Vec<Block>>
    pub fn walk_inlines_straight(...) -> Result<Vec<Block>>
    pub fn walk_block_splicing(...) -> Result<Vec<Block>>
    pub fn walk_blocks_straight(...) -> Result<Vec<Block>>
    pub fn apply_filter(...) -> Result<Vec<Block>>
}

pub mod topdown {
    pub fn walk(...) -> Result<Vec<Block>>
}

pub fn get_walking_order(filter: &Table) -> Result<WalkingOrder>
```

### Phase 2: Update types.rs to Use New Module

```rust
// In types.rs

pub fn walk_blocks_with_filter(lua: &Lua, blocks: &[Block], filter: &Table) -> Result<Vec<Block>> {
    match traversal::get_walking_order(filter)? {
        WalkingOrder::Typewise => traversal::typewise::apply_filter(lua, filter, blocks),
        WalkingOrder::Topdown => traversal::topdown::walk(lua, filter, blocks),
    }
}

pub fn walk_inlines_with_filter(lua: &Lua, inlines: &[Inline], filter: &Table) -> Result<Vec<Inline>> {
    // For inlines, we need to wrap in a dummy block, walk, then extract
    // OR: implement inline-specific versions of the walk functions
    match traversal::get_walking_order(filter)? {
        WalkingOrder::Typewise => traversal::typewise::walk_inlines(lua, filter, inlines),
        WalkingOrder::Topdown => traversal::topdown::walk_inlines(lua, filter, inlines),
    }
}
```

### Phase 3: Add Inline-Specific Four-Pass Functions

For `inlines:walk{}`, we need versions that:
1. Only walk inline elements (no block processing)
2. Apply Inlines filter correctly

```rust
pub fn walk_inlines_typewise(lua: &Lua, filter: &Table, inlines: &[Inline]) -> Result<Vec<Inline>> {
    // Pass 1: Walk all inline elements
    let inlines = walk_inline_elements(lua, filter, inlines)?;
    // Pass 2: Apply Inlines filter
    apply_inlines_filter(lua, filter, &inlines)
}
```

### Phase 4: Topdown Support with Stop Signal

Implement topdown traversal with stop signal for element walk:

```rust
pub fn walk_inlines_topdown(lua: &Lua, filter: &Table, inlines: &[Inline]) -> Result<Vec<Inline>> {
    // Apply Inlines filter first
    let (inlines, ctrl) = apply_inlines_filter_topdown(lua, filter, inlines)?;
    if ctrl == TraversalControl::Stop {
        return Ok(inlines);
    }

    // Then walk each inline
    let mut result = Vec::new();
    for inline in &inlines {
        let (filtered, ctrl) = apply_inline_filter_topdown(lua, filter, inline)?;
        for i in filtered {
            if ctrl == TraversalControl::Stop {
                result.push(i);
            } else {
                result.push(walk_inline_children_topdown(lua, filter, &i)?);
            }
        }
    }
    Ok(result)
}
```

## Test Cases

### Test 1: elem:walk{} Traversal Order

```lua
local order = {}

function test_filter()
    local para = pandoc.Para({
        pandoc.Str("a"),
        pandoc.Space(),
        pandoc.Emph({pandoc.Str("b")})
    })

    para:walk {
        Str = function(elem)
            table.insert(order, "Str:" .. elem.text)
            return elem
        end,
        Inlines = function(inlines)
            table.insert(order, "Inlines")
            return inlines
        end
    }

    -- Expected: ["Str:a", "Str:b", "Inlines", "Inlines"] (nested Emph has its own inline list)
    -- NOT: ["Str:a", "Inlines", "Str:b", "Inlines"]
end
```

### Test 2: blocks:walk{} Four-Pass Order

```lua
local blocks = pandoc.Blocks{
    pandoc.Para{pandoc.Str("a")},
    pandoc.Para{pandoc.Str("b")}
}

local order = {}
blocks:walk {
    Str = function(elem)
        table.insert(order, "Str:" .. elem.text)
        return elem
    end,
    Inlines = function(inlines)
        table.insert(order, "Inlines")
        return inlines
    end,
    Para = function(elem)
        table.insert(order, "Para")
        return elem
    end,
    Blocks = function(blocks)
        table.insert(order, "Blocks")
        return blocks
    end
}

-- Expected: ["Str:a", "Str:b", "Inlines", "Inlines", "Para", "Para", "Blocks"]
```

### Test 3: Topdown with Stop Signal on Element

```lua
local div = pandoc.Div{
    pandoc.Para{pandoc.Str("inside")}
}

local walked = div:walk {
    traverse = "topdown",
    Div = function(elem)
        return elem, false  -- Stop descent
    end,
    Str = function(elem)
        return pandoc.Str(elem.text:upper())
    end
}

-- Expected: Str inside should NOT be uppercased (descent stopped at Div)
```

### Test 4: Consistency with Document-Level Filter

```lua
-- These should produce identical results:

-- Method 1: Document-level filter
local doc1 = pandoc.Pandoc{pandoc.Para{pandoc.Str("hello")}}
local result1 = doc1:walk { Str = function(e) return pandoc.Str(e.text:upper()) end }

-- Method 2: Element-level walk
local para = pandoc.Para{pandoc.Str("hello")}
local result2 = para:walk { Str = function(e) return pandoc.Str(e.text:upper()) end }

-- result1.blocks[1] should equal result2
```

## Files to Modify

1. **New file**: `crates/quarto-markdown-pandoc/src/lua/traversal.rs`
   - Shared traversal functions for typewise and topdown
   - Used by both `filter.rs` and `types.rs`

2. **Modify**: `crates/quarto-markdown-pandoc/src/lua/filter.rs`
   - Move traversal functions to new module
   - Import and use from traversal module

3. **Modify**: `crates/quarto-markdown-pandoc/src/lua/types.rs`
   - Replace `walk_inlines_with_filter` and `walk_blocks_with_filter`
   - Use new traversal module

4. **Modify**: `crates/quarto-markdown-pandoc/src/lua/mod.rs`
   - Add `mod traversal;`

## Implementation Order

1. Create `traversal.rs` with the four-pass typewise functions (extracted from filter.rs)
2. Update `filter.rs` to use the new module
3. Verify all existing tests pass
4. Add topdown traversal to the module (depends on k-478)
5. Update `types.rs` to use the new module
6. Add new tests for elem:walk{}
7. Verify Pandoc compatibility

## Dependencies

- **Depends on k-478**: Topdown traversal implementation should be done first or in parallel, as elem:walk{} needs to support both traversal modes.

## Notes

The current implementation in types.rs does attempt topdown traversal, but it:
1. Doesn't support the stop signal (second return value)
2. Still processes Inlines/Blocks at the wrong time

Both issues need to be fixed together.
