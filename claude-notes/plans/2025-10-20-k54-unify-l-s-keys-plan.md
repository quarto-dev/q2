# k-54: Unify 'l' and 's' Source Tracking Keys - Investigation & Plan

**Date**: 2025-10-20
**Issue**: k-54 - Unify 'l' and 's' source tracking keys in JSON format

## Investigation Summary

### Current State

**Two SourceInfo Types Exist:**
1. **`pandoc::location::SourceInfo`** (old) - Simple: `{filename_index, range}`
2. **`quarto_source_map::SourceInfo`** (new) - Sophisticated: `{range, mapping: Original|Substring|Concat|Transformed}`

**All AST Types Now Use New SourceInfo:**
- ✅ All Inline types: `source_info: quarto_source_map::SourceInfo`
- ✅ All Block types: `source_info: quarto_source_map::SourceInfo`
- ✅ All MetaValue types: `source_info: quarto_source_map::SourceInfo`

**JSON Serialization Uses Two Different Formats:**

1. **`.l` format** (flattened, Pandoc-compatible):
   ```json
   {
     "t": "Str",
     "c": "hello",
     "l": {
       "start": {"offset": 0, "row": 0, "column": 0},
       "end": {"offset": 5, "row": 0, "column": 5},
       "filenameIndex": 0
     }
   }
   ```
   - Used by: **All Inlines, All Blocks**
   - Function: `write_location()` - flattens SourceInfo to range + filenameIndex
   - Purpose: Backward compatibility with Pandoc JSON format

2. **`.s` format** (pooled reference):
   ```json
   {
     "t": "MetaInlines",
     "c": [...],
     "s": {"$ref": 2}
   }
   ```
   - Used by: **MetaValues** (the container), **metadata keys**
   - Function: `to_json_ref()` - creates pool reference
   - Purpose: Avoid parent chain duplication in metadata

### Why the Difference?

**Historical Context:**
1. Migration happened incrementally (Phase 1-4)
2. Blocks/Inlines were migrated first (Phase 3, k-63)
3. Metadata was migrated later (Phase 4, k-45-k-50)
4. During metadata migration, pool serialization was implemented (k-57)
5. Pool optimization wasn't backported to Blocks/Inlines

**Technical Reason:**
- **Blocks/Inlines**: Use `write_location()` for Pandoc compatibility
- **Metadata**: Use `to_json_ref()` to avoid YAML parent chain explosion

### Problem

**Inconsistency:**
- Same SourceInfo type, different JSON representations
- `.l` vs `.s` is confusing for consumers
- Harder to document and maintain

**Missing Benefit:**
- Blocks/Inlines don't benefit from pool deduplication
- For documents with repeated structure, could be more efficient

## Unification Options

### Option 1: Use `.s` (pooled) for Everything ✅ RECOMMENDED

**Approach:**
- Replace all `write_location()` calls with `serializer.to_json_ref()`
- All SourceInfo (Blocks, Inlines, Meta) goes into pool
- All references use `{"$ref": id}`

**Pros:**
- ✅ Consistent format across all AST types
- ✅ Maximum deduplication (smaller JSON)
- ✅ Single code path for SourceInfo serialization
- ✅ Better for WASM/TypeScript integration
- ✅ Easier to document

**Cons:**
- ❌ Breaks Pandoc JSON format compatibility (`.l` → `.s`)
- ❌ Consumers must resolve references via pool
- ❌ Slightly more complex to read (indirect references)

**Impact:**
- **Breaking change** for JSON format
- Requires updating any consumers (TypeScript, filters, etc.)
- JSON becomes reference-based instead of self-contained

### Option 2: Use `.l` (flattened) for Everything

**Approach:**
- Replace metadata `to_json_ref()` calls with `write_location()`
- Flatten all SourceInfo to simple range + filenameIndex
- Remove pool for SourceInfo (keep for other purposes if needed)

**Pros:**
- ✅ Maintains Pandoc JSON format compatibility
- ✅ Self-contained (no need to resolve references)
- ✅ Simpler for consumers to read

**Cons:**
- ❌ Loses all mapping information (Substring, Concat, Transformed)
- ❌ Cannot distinguish original vs derived text
- ❌ Larger JSON (no deduplication)
- ❌ Defeats the purpose of quarto-source-map migration

**Impact:**
- **Wastes the entire source-map migration effort**
- Cannot track YAML parent chains properly
- Cannot resolve metadata errors to correct locations

### Option 3: Hybrid - Keep Both, Document Clearly

**Approach:**
- Keep current state: `.l` for Blocks/Inlines, `.s` for Metadata
- Document the distinction clearly
- Add utility functions for consumers

**Pros:**
- ✅ No breaking changes
- ✅ Preserves Pandoc compatibility where it exists
- ✅ Keeps metadata benefits

**Cons:**
- ❌ Inconsistent format
- ❌ Confusing for consumers
- ❌ Two code paths to maintain
- ❌ Doesn't solve the stated problem

**Impact:**
- Status quo (leaves issue open)

## Recommendation: Option 1 (Use `.s` for Everything)

**Rationale:**
1. **Consistency**: Single format for all SourceInfo
2. **Efficiency**: Pool deduplication benefits all AST types
3. **Correctness**: Preserves full source mapping information
4. **Future-proof**: Better foundation for WASM/TypeScript integration
5. **The migration is already done**: All types use `quarto_source_map::SourceInfo`

**Migration Path:**
1. Update `write_inline()` to use `serializer.to_json_ref()` instead of `write_location()`
2. Update `write_block()` to use `serializer.to_json_ref()` instead of `write_location()`
3. Keep `write_location()` only for backward compatibility exports if needed
4. Update documentation to reflect unified `.s` format
5. Update TypeScript/WASM bridge to handle pool-based format

**Backward Compatibility Strategy:**
- Add optional flag `--legacy-json` to output old `.l` format if needed
- Or: provide conversion utility to flatten pool references

## Implementation Plan

### Phase 1: Update JSON Writer (Blocks & Inlines)

**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs`

1. Update `write_inline()`:
   - Replace `"l": write_location(&inline.source_info)`
   - With `"s": serializer.to_json_ref(&inline.source_info)`
   - For all Inline variants

2. Update `write_block()`:
   - Replace `"l": write_location(&block.source_info)`
   - With `"s": serializer.to_json_ref(&block.source_info)`
   - For all Block variants

3. Keep `write_location()` as private helper if needed for other purposes

### Phase 2: Update Tests

**Files**: All test files in `crates/quarto-markdown-pandoc/tests/`

1. Update snapshot tests to expect `.s` instead of `.l`
2. Update JSON reader tests if they check for `.l`
3. Verify roundtrip tests still pass

### Phase 3: Update Documentation

**File**: TBD (k-35 will document final schema)

1. Document unified `.s` format
2. Document pool structure in `astContext.sourceInfoPool`
3. Provide examples of resolving references

### Phase 4: Update JSON Reader (if exists)

**File**: `crates/quarto-markdown-pandoc/src/readers/json.rs`

1. Update to read `.s` references
2. Resolve references from pool
3. Maintain backward compatibility with `.l` if feasible

## Open Questions

1. **Do we have external consumers of the JSON format?**
   - TypeScript code in quarto-cli?
   - Lua filters?
   - Other tools?

2. **Should we maintain backward compatibility with Pandoc's JSON format?**
   - If yes: Need conversion layer or flag
   - If no: Can break cleanly

3. **Should `.l` still exist for any purpose?**
   - Possibly for compatibility exports
   - Or for simpler debugging output

## Decision Needed

Before implementing, we need to decide:
- ✅ Accept breaking change to JSON format?
- ✅ Worth the consistency and efficiency gains?
- ⚠️ Impact on downstream consumers?

**My recommendation: Proceed with Option 1**, but first verify impact on downstream consumers (quarto-cli, filters, etc.).
