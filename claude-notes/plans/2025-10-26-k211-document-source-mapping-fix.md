# k-211: Fix Document-Level Source Mapping

**Issue:** `parseRustQmdDocument()` returns AnnotatedParse with empty source (start=0, end=0, source.value='')

**Root Cause:** Architectural misunderstanding of how SourceInfo maps to AnnotatedParse fields

## Architecture Overview

### The Problem

Current implementation has two critical bugs:

1. **`handleOriginal()` creates disconnected MappedStrings** (src/source-map.ts:154-155)
   ```typescript
   const content = file.content.substring(start, end);
   return asMappedString(content, file.path);  // WRONG - loses connection to top-level!
   ```

2. **Converters mix local strings with top-level offsets**
   ```typescript
   const source = this.sourceReconstructor.toMappedString(inline.s);  // local substring
   const [start, end] = this.sourceReconstructor.getOffsets(inline.s);  // top-level offsets
   // Now source.value.substring(start, end) is WRONG - incompatible coordinate systems!
   ```

### The Correct Design

**Three-Layer Architecture:**

#### Layer 1: Top-Level MappedStrings
- One per file
- Full file content
- Created upfront in SourceInfoReconstructor constructor
- NEVER modified

```typescript
topLevelMappedStrings[0] = asMappedString(files[0].content, files[0].path)
```

#### Layer 2: Local MappedStrings
- Tree structure via Substring/Concat operations
- Each SourceInfo ID → local MappedString
- Built via `toMappedString(id)` (cached)

```typescript
// Original: substring of top-level
local[5] = mappedSubstring(topLevel[fileId], start, end)

// Substring: substring of parent local
local[10] = mappedSubstring(local[parentId], localStart, localEnd)

// Concat: concat of piece locals
local[15] = mappedConcat([local[a], local[b], local[c]])
```

#### Layer 3: AnnotatedParse Fields
- `source`: ALWAYS references top-level MappedString
- `start`/`end`: Offsets in top-level coordinates
- Invariant: `source.value.substring(start, end)` extracts correct text

```typescript
{
  source: topLevelMappedStrings[fileId],  // top-level
  start: 42,                              // offset in top-level
  end: 67,                                // offset in top-level
}
```

### Rust SourceInfo Mapping

```rust
pub enum SourceInfo {
    Original { file_id: FileId, start_offset: usize, end_offset: usize },
    Substring { parent: Rc<SourceInfo>, start_offset: usize, end_offset: usize },
    Concat { pieces: Vec<SourcePiece> },
}
```

- **Original**: Leaf node, points to file content
- **Substring**: Relative offsets from parent (recursive)
- **Concat**: Multiple pieces joined together

The `resolveChain()` method already computes top-level offsets correctly by following the chain to Original nodes.

## Implementation Plan

### Step 1: Add top-level MappedStrings to SourceInfoReconstructor

**File:** src/source-map.ts

**Changes:**
1. Add field: `private topLevelMappedStrings: Map<number, MappedString> = new Map();`
2. In constructor, validate files have content and create top-level MappedStrings:
   ```typescript
   for (const file of sourceContext.files) {
     if (!file.content) {
       throw new Error(
         `File ${file.id} (${file.path}) missing content. ` +
         `astContext.files[].content must be populated.`
       );
     }
     this.topLevelMappedStrings.set(
       file.id,
       asMappedString(file.content, file.path)
     );
   }
   ```

**Test:** Verify constructor throws if content missing

---

### Step 2: Fix handleOriginal() to use mappedSubstring

**File:** src/source-map.ts:136-156

**Change:**
```typescript
private handleOriginal(id: number, info: SerializableSourceInfo): MappedString {
  if (typeof info.d !== 'number') {
    this.errorHandler(`Original data must be file_id number`, id);
    return asMappedString('');
  }

  const fileId = info.d;
  const [start, end] = info.r;

  const topLevel = this.topLevelMappedStrings.get(fileId);
  if (!topLevel) {
    this.errorHandler(`File ID ${fileId} not found`, id);
    return asMappedString('');
  }

  // Use mappedSubstring to maintain connection to top-level
  return mappedSubstring(topLevel, start, end);
}
```

**Test:** Run existing tests - local MappedStrings should still work

---

### Step 3: Add public API for AnnotatedParse fields

**File:** src/source-map.ts

**Add methods:**

```typescript
/**
 * Get top-level MappedString for a file
 */
getTopLevelMappedString(fileId: number): MappedString {
  const result = this.topLevelMappedStrings.get(fileId);
  if (!result) {
    throw new Error(`No top-level MappedString for file ${fileId}`);
  }
  return result;
}

/**
 * Get file ID and offsets in top-level coordinates
 */
getSourceLocation(id: number): { fileId: number, start: number, end: number } {
  const resolved = this.resolveChain(id);
  return {
    fileId: resolved.file_id,
    start: resolved.range[0],
    end: resolved.range[1]
  };
}

/**
 * Get all three AnnotatedParse source fields (source, start, end)
 *
 * This is the primary API for converters to use.
 */
getAnnotatedParseSourceFields(id: number): {
  source: MappedString;
  start: number;
  end: number;
} {
  const { fileId, start, end } = this.getSourceLocation(id);
  return {
    source: this.getTopLevelMappedString(fileId),
    start,
    end
  };
}
```

**Note:** Need to make `resolveChain()` public or keep as private (used internally by `getSourceLocation()`)

**Test:** Unit test these methods with various SourceInfo types

---

### Step 4: Update InlineConverter

**File:** src/inline-converter.ts

**Pattern to replace:**

OLD:
```typescript
const source = this.sourceReconstructor.toMappedString(inline.s);
const [start, end] = this.sourceReconstructor.getOffsets(inline.s);
```

NEW:
```typescript
const { source, start, end } =
  this.sourceReconstructor.getAnnotatedParseSourceFields(inline.s);
```

**Locations:** Every case in `convertInline()` switch statement (~15 cases)

**Special cases:**
- `convertAttr()` - attr components (id, classes, kvs)
- `convertTarget()` - target components (url, title)
- `convertCitation()` - citation components

**Test:** Run test/inline-types.test.ts - all should pass

---

### Step 5: Update BlockConverter

**File:** src/block-converter.ts

**Same pattern:** Replace source/start/end extraction in every case

**Locations:** Every case in `convertBlock()` switch statement (~15 cases)

**Special cases:**
- `convertCaption()` - caption structure
- Table handling (multiple AnnotatedParse nodes for rows/cells)

**Test:** Run test/block-types.test.ts - all should pass

---

### Step 6: Update MetadataConverter

**File:** src/meta-converter.ts

**Same pattern:** Replace source/start/end extraction

**Locations:**
- `convertMetaValue()` switch cases
- `convertMeta()` for top-level keys

**Test:** Run test/meta-conversion.test.ts - all should pass

---

### Step 7: Fix DocumentConverter

**File:** src/document-converter.ts:53-79

**Changes:**

```typescript
convertDocument(doc: RustQmdJson): AnnotatedParse {
  const components: AnnotatedParse[] = [];

  // Convert metadata (if present)
  if (doc.meta && Object.keys(doc.meta).length > 0) {
    components.push(this.metadataConverter.convertMeta(doc.meta));
  }

  // Convert all blocks
  if (doc.blocks && doc.blocks.length > 0) {
    components.push(...doc.blocks.map(block => this.blockConverter.convertBlock(block)));
  }

  // Document spans entire file (file ID 0 is main document)
  const source = this.sourceReconstructor.getTopLevelMappedString(0);
  const start = 0;
  const end = source.value.length;

  return {
    result: doc as unknown as import('./types.js').JSONValue,
    kind: 'Document',
    source,
    components,
    start,
    end
  };
}
```

**Test:** Run test/document-level-source.test.ts - should pass!

---

### Step 8: Fix test assertions

**File:** test/document-level-source.test.ts:44-47

**Current (WRONG):**
```typescript
// Document start SHOULD BE 0, not "not equal to 0"!
assert.notStrictEqual(result.start, 0, 'Document start should not be 0');
assert.notStrictEqual(result.end, 0, 'Document end should not be 0');
```

**Fixed:**
```typescript
assert.strictEqual(result.start, 0, 'Document starts at offset 0');
assert.strictEqual(result.end, qmdContent.length, 'Document ends at content length');
assert.notStrictEqual(result.source.value, '', 'Document source should not be empty');
assert.strictEqual(result.source.value, qmdContent, 'Document source is full content');
```

---

### Step 9: Update TODO comment and close task

**File:** src/document-converter.ts:66-68

**Remove:**
```typescript
// Try to get overall document source if we have file context
// For now, use empty MappedString as we don't track document-level source
```

**Replace with:**
```typescript
// Document spans entire file (file ID 0 is main document)
```

**Close:** br close k-211 with comprehensive summary

---

## Testing Strategy

### Incremental Testing
1. After Step 1-2: Run all existing tests - should pass (local MappedStrings still work)
2. After Step 4: Run inline-types.test.ts
3. After Step 5: Run block-types.test.ts
4. After Step 6: Run meta-conversion.test.ts
5. After Step 7-8: Run document-level-source.test.ts (THE BIG WIN!)

### Full Test Suite
After all steps: `npm test` - all 64 tests should pass (63 existing + 1 new document test)

---

## Key Insights

1. **AnnotatedParse.source is ALWAYS top-level** - Never a local substring
2. **Local MappedStrings still useful** - For debugging, text extraction during conversion
3. **Coordinate systems must match** - source and start/end must use same coordinates
4. **Constructor validation critical** - Must ensure file content populated upfront
5. **Mechanical refactor** - Most changes are simple method call replacements

---

## Edge Cases

### Multi-file documents
- For now, Document node uses file ID 0 (main document)
- Individual blocks/inlines correctly track their source file via SourceInfo

### Empty files
- Constructor should handle gracefully (empty string is valid content)
- Document start=0, end=0 is correct for empty file

### Concat spanning multiple files
- `resolveChain()` uses first piece's file (see k-213)
- Local MappedString via `toMappedString()` still works correctly
- Top-level reference via `getSourceLocation()` may be imprecise but is best-effort

---

## Related Issues

- **k-213**: Improve Concat SourceInfo error reporting (low priority)
- **k-214**: Add circular reference detection (low priority)
- **k-193**: Helper APIs for list navigation (separate concern)

---

## Success Criteria

1. ✅ test/document-level-source.test.ts passes
2. ✅ All existing 63 tests continue to pass
3. ✅ `result.source.value.substring(result.start, result.end)` extracts correct text for all nodes
4. ✅ Constructor throws clear error if file content missing
5. ✅ TODO comment removed, code is production-ready
