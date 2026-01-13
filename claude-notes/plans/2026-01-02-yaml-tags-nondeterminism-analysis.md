# Analysis: Nondeterministic sourceInfoPool IDs in JSON Writer

**Beads Issue**: k-gv05
**Date**: 2026-01-02
**Status**: Fixed (2026-01-03)

## Problem Summary

The `yaml-tags.qmd` snapshot test fails intermittently:
- Passes on macOS CI, fails on Linux CI (same commit)
- Local testing shows nondeterministic output across runs on the same machine
- The `path` metadata value's Str node gets different `s` values (pool IDs): sometimes `2`, sometimes `4`

## Root Cause Analysis

### The Bug: Stale Pointer Cache Hits Due to Memory Reuse

The `SourceInfoSerializer` in `crates/pampa/src/writers/json.rs` uses a pointer-based cache (`id_map: HashMap<*const SourceInfo, usize>`) for performance. The bug occurs when:

1. A temporary SourceInfo `A` is created at address `0x1000`
2. `intern(&A)` is called, adding `0x1000 -> id_A` to `id_map`
3. `A` is dropped (temporary goes out of scope)
4. A **new** SourceInfo `B` is created at address `0x1000` (memory reuse)
5. `intern(&B)` is called
6. Pointer lookup: `id_map.get(&0x1000)` returns `id_A`
7. **Early return with wrong ID** - content check is never performed!

### Why Pointers Cause Problems (Even Though Never Serialized)

The user correctly noted that pointers aren't serialized to disk. The issue is subtler:

The pointer cache **short-circuits** the content comparison:

```rust
fn intern(&mut self, source_info: &SourceInfo) -> usize {
    let ptr = source_info as *const SourceInfo;

    // If pointer matches, return immediately WITHOUT checking content
    if let Some(&id) = self.id_map.get(&ptr) {
        return id;  // <-- BUG: Returns stale ID for reused address
    }

    // Content check only runs if pointer lookup fails
    for (existing, id) in &self.content_map { ... }
    ...
}
```

When a memory address is reused, the stale cache entry causes us to return the wrong ID **without ever checking if the content matches**.

### Specific Code Path

In `write_config_value`, processing YAML tagged values:

```rust
ConfigValueKind::Expr(s) => {
    let inlines = vec![...Str { source_info: value.source_info.clone() }...];
    json!({
        "c": write_inlines(&inlines, ctx),  // Interns cloned SourceInfo
        "s": ctx.serializer.to_json_ref(&value.source_info)
    })
}  // <-- inlines dropped here, cloned SourceInfo's memory freed

ConfigValueKind::Path(s) => {
    let inlines = vec![...Str { source_info: value.source_info.clone() }...];
    // This clone might get allocated at the same address as the
    // now-freed Expr clone!
    json!({
        "c": write_inlines(&inlines, ctx),  // May return wrong ID!
        ...
    })
}
```

### Why It's Nondeterministic

Memory reuse patterns depend on:
- Allocator internal state
- ASLR (Address Space Layout Randomization)
- Stack layout (affected by compiler optimizations)
- Order of allocations/deallocations

These vary between runs, so the bug manifests nondeterministically.

## Evidence

### Reproduction

Multiple consecutive runs of the same binary produce different output:

```
=== Run 1 === 4
=== Run 2 === 4
=== Run 3 === 2  <-- Different!
```

### Pool Entry Analysis

- Entry 2: `{"d":1,"r":[15,20],"t":1}` - offsets for "x + 1" (expr value)
- Entry 4: `{"d":1,"r":[33,47],"t":1}` - offsets for "/usr/local/bin" (path value)

When path gets `s:2`, it's pointing to expr's source location - clearly wrong.

## Why Content-Based Deduplication Alone Isn't the Answer

The user asks: "Doesn't content-based deduplication also cause unintended sharing?"

**Answer**: No, because SourceInfo contains offsets that uniquely identify locations:

```rust
SourceInfo::Substring {
    parent: Arc<SourceInfo>,
    start_offset: usize,  // Different for each YAML value
    end_offset: usize,
}
```

The expr value has offsets [15,20], path has [33,47]. These are **not equal**, so content comparison correctly distinguishes them.

The problem is that the pointer cache **bypasses** content comparison entirely.

However, the current O(n) fallback for content comparison is problematic for large documents. We need O(1) or O(log n) content-based lookup.

## Detailed Code Trace

The YAML metadata is processed sequentially in `write_config_value_as_meta` (line 1437):
```rust
.map(|entry| (entry.key.clone(), write_config_value(&entry.value, ctx)))
```

For the test file:
```yaml
compute: !expr x + 1
path: !path /usr/local/bin
date: !date 2024-01-15
```

**Step 1: Processing `compute` (Expr) - Lines 1391-1412**

```rust
let span = crate::pandoc::Span {
    content: vec![crate::pandoc::Inline::Str(crate::pandoc::Str {
        text: s.clone(),
        source_info: value.source_info.clone(),  // CLONE_A at line 1402
    })],
    ...
};
let inlines = vec![crate::pandoc::Inline::Span(span)];
json!({
    "c": write_inlines(&inlines, ctx),  // Interns CLONE_A
    ...
})
```

- CLONE_A (with offsets [15,20]) is created and stored in the `inlines` Vec
- `write_inlines` interns CLONE_A; `id_map` gets `addr_clone_a -> 2`
- `json!` returns a Value
- **`inlines` is dropped**, freeing CLONE_A's memory

**Step 2: Processing `path` (Path) - Lines 1356-1366**

```rust
let inlines = vec![crate::pandoc::Inline::Str(crate::pandoc::Str {
    text: s.clone(),
    source_info: value.source_info.clone(),  // CLONE_B at line 1359
})];
json!({
    "c": write_inlines(&inlines, ctx),  // Interns CLONE_B
    ...
})
```

- CLONE_B (with offsets [33,47]) is created
- **If the allocator reuses the freed memory**, CLONE_B is at address `addr_clone_a`
- `write_inlines` calls `intern(&clone_b_source_info)`
- In `intern`: `ptr = addr_clone_a` (due to memory reuse)
- `id_map.get(&ptr)` finds the entry for CLONE_A → returns `2`
- **Wrong ID returned without checking content!**

**All three problematic clone sites:**
- **Line 1359**: `source_info: value.source_info.clone()` for Path
- **Line 1379**: `source_info: value.source_info.clone()` for Glob
- **Line 1402**: `source_info: value.source_info.clone()` for Expr

## Reframing the Bug

The bug can be stated as: **we should only add SourceInfo pointers to `id_map` if they belong to the Pandoc AST being serialized, not to temporary objects created during serialization.**

The problematic clones are created in `write_config_value` when converting ConfigValue variants (Expr, Path, Glob) to MetaInlines format. These temporaries are dropped during serialization, their memory is reused, and stale cache hits occur.

## Proposed Solutions

### Option 1: Pre-compute ConfigValue Replacements (Recommended)

Instead of creating temporary Inline structures during serialization, pre-compute them at the start of `write_pandoc()` and store them in a HashMap that lives for the entire serialization.

**Implementation:**

1. At the start of `write_pandoc()`, walk the **entire Pandoc structure**:
   - `pandoc.meta` (top-level document metadata)
   - All blocks recursively, checking for `Block::BlockMetadata` which contains `ConfigValue`
2. For each ConfigValue containing Expr/Path/Glob, create a replacement ConfigValue with `ConfigValueKind::PandocInlines(...)` containing the pre-built Inline structures
3. Store in `HashMap<*const ConfigValue, ConfigValue>` (keys = pointers to originals, values = replacements)
4. Add this HashMap to `JsonWriterContext`
5. In `write_config_value`, check if the current ConfigValue has a replacement; if so, use it

**Important finding: ConfigValue exists in two places:**

- `pandoc.meta: ConfigValue` - top-level document metadata
- `Block::BlockMetadata(MetaBlock)` where `MetaBlock.meta: ConfigValue` - embedded metadata blocks

Both must be walked during pre-computation.

```rust
fn precompute_config_value_replacements(
    config_value: &ConfigValue,
    replacements: &mut HashMap<*const ConfigValue, ConfigValue>,
) {
    match &config_value.value {
        ConfigValueKind::Expr(s) => {
            let inlines = create_expr_inlines(s, &config_value.source_info);
            let replacement = ConfigValue {
                value: ConfigValueKind::PandocInlines(inlines),
                source_info: config_value.source_info.clone(),
            };
            replacements.insert(config_value as *const ConfigValue, replacement);
        }
        ConfigValueKind::Path(s) => {
            let inlines = create_path_inlines(s, &config_value.source_info);
            let replacement = ConfigValue {
                value: ConfigValueKind::PandocInlines(inlines),
                source_info: config_value.source_info.clone(),
            };
            replacements.insert(config_value as *const ConfigValue, replacement);
        }
        ConfigValueKind::Glob(s) => {
            let inlines = create_glob_inlines(s, &config_value.source_info);
            let replacement = ConfigValue {
                value: ConfigValueKind::PandocInlines(inlines),
                source_info: config_value.source_info.clone(),
            };
            replacements.insert(config_value as *const ConfigValue, replacement);
        }
        ConfigValueKind::Map(entries) => {
            for entry in entries {
                precompute_config_value_replacements(&entry.value, replacements);
            }
        }
        ConfigValueKind::Array(items) => {
            for item in items {
                precompute_config_value_replacements(item, replacements);
            }
        }
        _ => {}
    }
}
```

**Why this works:**

1. Original ConfigValues are part of `&pandoc.meta`, borrowed for the entire `write_pandoc()` call - their addresses are stable
2. Replacement ConfigValues live in the HashMap, which lives for entire serialization
3. SourceInfo in replacements (both the ConfigValue's source_info and the Str/Span's source_info) have stable addresses - no memory reuse during serialization
4. The existing `ConfigValueKind::PandocInlines` match arm handles the converted values correctly

**Edge case - SourceInfo::default():**

The Span wrapper uses `SourceInfo::default()`. This is not a problem because all defaults have identical content (file_id=0, offsets=0,0), so even a stale cache hit returns the correct ID.

**Pros:**
- Doesn't copy the entire Pandoc structure
- Only creates replacements for the three problematic variants
- Keeps all SourceInfo pointers valid during serialization
- Minimal overhead (one HashMap lookup per ConfigValue)
- Conceptually clean: separates "AST transformation" from "serialization"

**Cons:**
- Adds complexity to write_pandoc()
- Extra memory for the HashMap (but much smaller than copying entire Pandoc)

### Option 2: Implement Hash for SourceInfo

Add a custom Hash implementation for SourceInfo that hashes content recursively, then use `HashMap<SourceInfo, usize>` for O(1) content-based lookup.

**Pros**: Clean, O(1) lookup, deterministic
**Cons**: Hash computation for deeply nested Substring chains could be expensive; requires changes to quarto-source-map crate

### Option 3: Full Pandoc Copy

Create a local copy of `pandoc: Pandoc` that pre-converts all ConfigValue Expr/Path/Glob entries to PandocInlines at the start of write_pandoc().

**Pros**: Simple to understand
**Cons**: Creates a full copy of the Pandoc structure, expensive

## Recommendation

**Option 1 (Pre-compute ConfigValue Replacements)** is recommended because it:
- Solves the root cause (temporary SourceInfo objects being cached)
- Has minimal memory overhead
- Doesn't require changes to other crates
- Maintains good performance

## Fix Implementation (2026-01-03)

The fix was implemented using **Option 1 (Pre-compute ConfigValue Replacements)** with a slight modification:
instead of storing pre-computed Inlines, we pre-**serialize** them to JSON during the precomputation phase.

### Implementation Details

1. Added `precompute_all_json()` function that walks the entire Pandoc structure (meta + blocks)
2. For each Path/Glob/Expr ConfigValue, it:
   - Builds temporary Inlines using `build_path_inlines()`, `build_glob_inlines()`, `build_expr_inlines()`
   - Immediately serializes them using `write_inlines()`, which interns their SourceInfos into the pool
   - Stores the resulting JSON in `ctx.precomputed_json` HashMap (keyed by ConfigValue pointer)
3. In `write_config_value()`, Path/Glob/Expr cases now retrieve the pre-serialized JSON directly

### Why Pre-serialization Works

By serializing during the precomputation phase:
- All SourceInfos from Path/Glob/Expr variants are interned **first**, before any temporary allocations
- The temporary Inlines created during precomputation are dropped immediately after serialization
- During the main serialization pass, we only retrieve pre-serialized JSON (no new SourceInfo creation)
- The pointer cache never sees temporary SourceInfos that could have their memory reused

### Files Changed

- `crates/pampa/src/writers/json.rs` - Added precomputation functions and modified write_config_value
- `crates/pampa/snapshots/json/yaml-tags.snap` - Updated with deterministic pool IDs
- `.github/workflows/test-suite.yml` - Removed diagnostic CI step
- Removed `crates/pampa/tests/diagnostic_yaml_tags.rs` (diagnostic test no longer needed)

### Verification

- Multiple consecutive runs produce identical output (`s:4` for path consistently)
- All 1411 tests pass

## Files Involved

- `crates/pampa/src/writers/json.rs` - SourceInfoSerializer implementation
- `crates/quarto-source-map/src/source_info.rs` - SourceInfo type definition
- `crates/pampa/snapshots/json/yaml-tags.snap` - Affected snapshot

---

## CORRECTION (2026-01-13): Original Fix Was Incomplete

**The fix described above did NOT fully solve the problem.** The bug reappeared in CI on 2026-01-13.

### What Was Wrong With the Original Analysis

The "Why Pre-serialization Works" section claimed:
> "The temporary Inlines created during precomputation are dropped immediately after serialization"
> "The pointer cache never sees temporary SourceInfos that could have their memory reused"

This reasoning was **flawed**. The fix prevented memory reuse between the precomputation phase and main serialization, but it did NOT prevent memory reuse **within the precomputation phase itself**.

### The Actual Bug

When processing entries sequentially in `precompute_config_value_json`:
1. Process `compute` (Expr): create clone at addr A, intern, drop clone (A freed)
2. Process `path` (Path): create clone - allocator might reuse addr A!
3. If addr A is reused, `id_map.get(&A)` returns the wrong ID

The temporaries have **non-overlapping lifetimes** because they're processed sequentially, allowing the allocator to reuse addresses.

### The Correct Fix

Keep ALL temporary Inlines alive until precomputation completes:

```rust
fn precompute_all_json(pandoc: &Pandoc, ctx: &mut JsonWriterContext) {
    let mut inlines_keeper: Vec<Inlines> = Vec::new();  // Keep all alive!
    precompute_config_value_json(&pandoc.meta, ctx, &mut inlines_keeper);
    for block in &pandoc.blocks {
        precompute_block_json(block, ctx, &mut inlines_keeper);
    }
    // inlines_keeper dropped here, AFTER all precomputation
}
```

### See Also

- `claude-notes/plans/2026-01-13-precomputation-memory-reuse-bug.md` - Full analysis of the corrected fix
