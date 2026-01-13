# Analysis: Memory Reuse Bug Within Precomputation Phase

**Date**: 2026-01-13
**Related Issue**: Continuation of k-gv05 (originally marked as fixed 2026-01-03)
**Status**: Fixed

## Problem Summary

The `yaml-tags.qmd` snapshot test continues to fail intermittently, despite the fix applied in commit 227acbe (2026-01-03). The symptom is identical to the original bug:
- The `path` metadata value's Str node gets different `s` values (pool IDs): sometimes `2`, sometimes `4`
- `s:4` is correct (points to path's source range [33,47])
- `s:2` is incorrect (points to compute's source range [15,20])

## Root Cause: Incomplete Fix

The original fix (commit 227acbe) introduced precomputation to serialize Path/Glob/Expr ConfigValues at the start of `write_pandoc()`. The reasoning was:

> "This ensures all SourceInfos from these variants are interned before any temporary Inlines could be created and cause memory reuse issues."

**This reasoning is flawed.** The fix prevents memory reuse between the precomputation phase and the main serialization phase, but it does NOT prevent memory reuse **within the precomputation phase itself**.

## Detailed Analysis

### The Flawed Precomputation Logic

Looking at `precompute_config_value_json`:

```rust
fn precompute_config_value_json(config_value: &ConfigValue, ctx: &mut JsonWriterContext) {
    match &config_value.value {
        ConfigValueKind::Path(s) => {
            let inlines = build_path_inlines(s, &config_value.source_info);  // Clone A created
            let json = write_inlines(&inlines, ctx);  // Clone A interned → id=X
            ctx.precomputed_json.insert(config_value as *const ConfigValue, json);
        }  // <-- inlines dropped here, Clone A's memory freed!

        ConfigValueKind::Expr(s) => {
            let inlines = build_expr_inlines(s, &config_value.source_info);  // Clone B created
            // If allocator reuses Clone A's address for Clone B...
            let json = write_inlines(&inlines, ctx);  // id_map[addr] returns X (WRONG!)
            ctx.precomputed_json.insert(config_value as *const ConfigValue, json);
        }
        // ...
    }
}
```

### The Memory Reuse Scenario

For the test file:
```yaml
compute: !expr x + 1
path: !path /usr/local/bin
date: !date 2024-01-15
```

Processing order (document order): compute, path, date

1. **Process `compute` (Expr)**:
   - `build_expr_inlines` creates a Span containing a Str with cloned SourceInfo at address A
   - `write_inlines` interns the clone → `id_map[A] = 2`
   - End of match arm: `inlines` dropped, memory at A is freed

2. **Process `path` (Path)**:
   - `build_path_inlines` creates a Str with cloned SourceInfo
   - **If the allocator reuses address A** for this clone...
   - `write_inlines` calls `intern()` on the clone
   - Pointer check: `id_map.get(&A)` returns `Some(2)` ← **WRONG ID!**
   - Returns 2 instead of creating a new entry with ID 4

3. **Process `date` (PandocInlines)**:
   - Not handled by precomputation (already has PandocInlines)

### Why the Bug is Non-Deterministic

Memory reuse depends on:
- Allocator internal state
- ASLR (Address Space Layout Randomization)
- Stack layout
- Previous allocation/deallocation patterns

These vary between runs, so sometimes the allocator reuses addresses (bug manifests) and sometimes it doesn't (test passes).

### Why the Original Fix Didn't Work

The original fix assumed the problem was memory reuse between:
- Temporaries created during precomputation → dropped
- Temporaries created during main serialization → might reuse addresses

But the actual problem is memory reuse **within precomputation**:
- Temporary for entry N → dropped
- Temporary for entry N+1 → might reuse address from entry N

The temporaries have **non-overlapping lifetimes** because they're processed sequentially.

## The Correct Fix

Keep all temporary `Inlines` alive until precomputation is complete. This ensures no clone's memory can be reused by another clone during the critical section.

```rust
fn precompute_all_json(pandoc: &Pandoc, ctx: &mut JsonWriterContext) {
    // Keep all temporary Inlines alive until precomputation is complete.
    // This prevents memory reuse where a dropped clone's address could be
    // reused by a subsequent clone, causing stale pointer cache hits.
    let mut inlines_keeper: Vec<Inlines> = Vec::new();

    precompute_config_value_json(&pandoc.meta, ctx, &mut inlines_keeper);

    for block in &pandoc.blocks {
        precompute_block_json(block, ctx, &mut inlines_keeper);
    }

    // inlines_keeper is dropped here, AFTER all precomputation is done.
    // At this point, all SourceInfos have been interned and their IDs
    // are safely stored in precomputed_json.
}

fn precompute_config_value_json(
    config_value: &ConfigValue,
    ctx: &mut JsonWriterContext,
    keeper: &mut Vec<Inlines>,
) {
    match &config_value.value {
        ConfigValueKind::Path(s) => {
            let inlines = build_path_inlines(s, &config_value.source_info);
            let json = write_inlines(&inlines, ctx);
            ctx.precomputed_json.insert(config_value as *const ConfigValue, json);
            keeper.push(inlines);  // Keep alive until precomputation completes
        }
        // ... similar for Glob, Expr
    }
}
```

## Alternative Solutions Considered

### Option 1: Content-Based Hashing (Not Chosen)

Implement `Hash` for `SourceInfo` and use `HashMap<SourceInfo, usize>` for O(1) content-based lookup instead of pointer-based caching.

**Pros**: Eliminates pointer-based bugs entirely
**Cons**: Requires changes to `quarto-source-map` crate; hash computation for deep Substring chains could be expensive

### Option 2: Disable Pointer Caching for Temporaries (Not Chosen)

Add a flag to `intern()` to skip pointer caching for known temporaries.

**Pros**: Targeted fix
**Cons**: Caller must know which SourceInfos are temporary; error-prone

### Option 3: Keep Temporaries Alive (Chosen)

Simple, localized fix that doesn't require changes to other crates or the interning logic.

**Pros**: Minimal code change; easy to understand; doesn't affect performance significantly
**Cons**: Slight memory overhead during precomputation (negligible for typical documents)

## Verification

After applying the fix:
1. Run `cargo nextest run -p pampa unit_test_snapshots_json` multiple times
2. All runs should produce identical output with `s:4` for path's inner Str
3. Update the snapshot to reflect the correct output

## Lessons Learned

1. **Memory reuse bugs are subtle**: The original fix addressed one scenario but missed another. When fixing pointer-based cache bugs, consider ALL points where temporaries are created and dropped.

2. **Sequential processing with non-overlapping lifetimes is dangerous**: When processing items in a loop where each iteration creates and drops temporaries, memory reuse is likely.

3. **The fix should match the actual bug location**: The original fix prevented reuse between phases, but the bug was within a single phase.

## Files Changed

- `crates/pampa/src/writers/json.rs` - Modified precomputation to keep temporaries alive
- `crates/pampa/snapshots/json/yaml-tags.snap` - Updated with correct `s:4` value
