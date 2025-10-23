# Plan: TypeScript Module to Convert quarto-markdown-pandoc JSON to AnnotatedParse

## Problem Statement

We need to create a TypeScript module in `ts-packages/rust-qmd-json/` that can read the JSON output from `quarto-markdown-pandoc` and convert the metadata into an `AnnotatedParse` structure compatible with existing quarto-cli code.

## Key Challenge

**Metadata strings are converted to Markdown**: In the JSON output, YAML string values have been parsed as Markdown and serialized as `MetaInlines` (arrays of Inline nodes like `Str`, `Strong`, `Emph`, etc.). The existing `AnnotatedParse` structure in quarto-cli expects `JSONValue` types, and we need to verify that storing complex JSON arrays in the `result` field is compatible.

## Compatibility Analysis

**✅ VERIFIED SAFE**: Storing JSON arrays in `AnnotatedParse.result` is fully compatible with existing quarto-cli code.

**Key Findings:**
1. `AnnotatedParse.result` is typed as `JSONValue`, which explicitly includes arrays: `JSONValue[]`
2. All existing code uses defensive type guards before accessing result properties:
   - `typeof value.result === "object" && !Array.isArray(value.result)` - guards object access
   - `Array.isArray(value.result)` - explicitly checks for arrays
3. The `kind` field is only used for navigation structure, not result type determination
4. Array validation already works through the `components` array, not direct `result` access

**Conclusion:** The existing codebase is already defensively written to handle arrays in `result`. Our approach is safe.

Full analysis: `claude-notes/plans/2025-10-23-annotated-parse-compatibility.md`

## Current Structures

### 1. quarto-markdown-pandoc Output (Rust → JSON)

**MetaValueWithSourceInfo variants:**
- `MetaString { value, source_info }`
- `MetaBool { value, source_info }`
- `MetaInlines { content: Inlines, source_info }` ← **Challenge!**
- `MetaBlocks { content: Blocks, source_info }` ← **Challenge!**
- `MetaList { items, source_info }`
- `MetaMap { entries: [{ key, key_source, value }], source_info }`

**SourceInfo serialization:**
- Each `SourceInfo` is assigned an ID and stored in a pool
- References use `s: <id>` where `<id>` is an integer
- Pool entry format: `{"r": [start_offset, end_offset], "t": type_code, "d": type_data}`
  - `t: 0` = Original `{file_id}`
  - `t: 1` = Substring `{parent_id}`
  - `t: 2` = Concat `{pieces: [[source_info_id, offset, length], ...]}`

**Example JSON:**
```json
{
  "meta": {
    "title": {
      "c": [
        {"c": "My", "s": 0, "t": "Str"},
        {"s": 1, "t": "Space"},
        {"c": [{"c": "Document", "s": 2, "t": "Str"}], "s": 3, "t": "Strong"}
      ],
      "s": 6,
      "t": "MetaInlines"
    }
  },
  "source_pool": [...],
  "source_context": {
    "files": [
      {"id": 0, "path": "main.qmd", "content": "---\ntitle: My **Document**\n---"}
    ]
  }
}
```

### 2. quarto-cli AnnotatedParse (TypeScript)

```typescript
interface AnnotatedParse {
  start: number;        // byte offset
  end: number;          // byte offset
  result: JSONValue;    // JSONValue includes arrays!
  kind: string;         // type indicator
  source: MappedString; // source text with mapping
  components: AnnotatedParse[];
}

interface MappedString {
  value: string;
  fileName?: string;
  map: (index: number, closest?: boolean) => StringMapResult | undefined;
}
```

## Solution Approach

### Strategy: Direct JSON Value Mapping

Since `AnnotatedParse.result` is typed as `JSONValue` (which includes arrays), and the JSON representation of `MetaInlines`/`MetaBlocks` is already a valid `JSONValue` (an array of inline/block nodes), we can **directly use the JSON structure** without any text reconstruction.

Key insight: The serialized metadata already tracks the source location (start/end offsets) of the entire YAML value. For a string that was parsed as markdown, the source offsets point to the original YAML string location, and the `result` contains the parsed inline/block JSON structure.

### Conversion Rules

**MetaInlines → AnnotatedParse:**
- `result`: The JSON array of inline nodes AS-IS (e.g., `[{t: "Str", c: "My"}, {t: "Space"}, {t: "Strong", c: [...]}]`)
- `kind`: "MetaInlines"
- `source`: MappedString from SourceInfo (points to original YAML string location)
- `components`: `[]` (empty - current implementation cannot track internal locations; future enhancement)
- `start/end`: From SourceInfo offsets

**Note on components:** MetaInlines has a **complex JSONValue in result** but **empty components[]**, indicating the implementation cannot yet navigate into the inline structure. This will be enhanced in the future without breaking changes.

**MetaBlocks → AnnotatedParse:**
- `result`: The JSON array of block nodes AS-IS
- `kind`: "MetaBlocks"
- `source`: MappedString from SourceInfo (points to original YAML string location)
- `components`: `[]` (empty - same reasoning as MetaInlines)
- `start/end`: From SourceInfo offsets

**MetaString → AnnotatedParse:**
- `result`: Plain string value
- `kind`: "MetaString"
- `source`: MappedString from SourceInfo
- `components`: `[]` (leaf)
- `start/end`: From SourceInfo offsets

**MetaBool → AnnotatedParse:**
- `result`: Boolean value
- `kind`: "MetaBool"
- `source`: MappedString from SourceInfo
- `components`: `[]` (leaf)
- `start/end`: From SourceInfo offsets

**MetaList → AnnotatedParse:**
- `result`: Array of child results (e.g., `[childResult1, childResult2]`)
- `kind`: "MetaList"
- `source`: MappedString from SourceInfo
- `components`: AnnotatedParse entries for each item (non-empty)
- `start/end`: From SourceInfo offsets

**MetaMap → AnnotatedParse:**
- `result`: Object with key-value pairs (e.g., `{title: childResult, author: childResult}`)
- `kind`: "MetaMap"
- `source`: MappedString from SourceInfo
- `components`: Interleaved key/value AnnotatedParse entries (matching js-yaml pattern)
- `start/end`: From SourceInfo offsets

## Development Setup

The implementation is developed as a standalone TypeScript package in `ts-packages/rust-qmd-json/`
that will be published to npm as `@quarto/rust-qmd-json`. This approach:

- Allows independent testing and iteration
- Uses published `@quarto/mapped-string` from npm
- Can be consumed by quarto-cli and other projects via npm
- Makes local development and testing straightforward

**Setup verified:**
- ✅ Node.js v23.11.0 and npm 10.9.2 available
- ✅ TypeScript 5.4.2 configured
- ✅ `@quarto/mapped-string` ^0.1.8 installed and working
- ✅ Basic tests passing (MappedString functionality verified)
- ✅ Compatibility analysis complete (arrays in result field are safe)

## Implementation Plan

### Phase 1: Core Infrastructure (SourceInfo Reconstruction)

**File: `ts-packages/rust-qmd-json/src/source-map.ts`**

```typescript
interface SourcePool {
  pool: SerializableSourceInfo[];
}

interface SerializableSourceInfo {
  r: [number, number]; // [start_offset, end_offset]
  t: number;           // type code (0=Original, 1=Substring, 2=Concat)
  d: any;              // type-specific data
}

interface SourceContext {
  files: Array<{id: number, path: string, content: string}>;
}

class SourceInfoReconstructor {
  private resolvedCache = new Map<number, {file_id: number, range: [number, number]}>();

  constructor(pool: SourcePool, sourceContext: SourceContext, errorHandler?: (msg: string, id?: number) => void);

  // Convert SourceInfo ID to MappedString
  toMappedString(id: number): MappedString;

  // Get offsets from SourceInfo
  getOffsets(id: number): [number, number];

  // Recursively resolve SourceInfo chains with caching
  private resolveChain(id: number): {file_id: number, range: [number, number]};
}
```

**Implementation details:**
1. **Original (t=0)**: Direct mapping to file content at offsets
   ```typescript
   // d is file_id, r is [start, end]
   return {file_id: info.d, range: info.r};
   ```

2. **Substring (t=1)**: Chain through parent, adding offsets
   ```typescript
   // d is parent_id, r is [local_start, local_end]
   const parent = this.resolveChain(info.d);
   const [localStart, localEnd] = info.r;
   const [parentStart, _] = parent.range;
   return {
     file_id: parent.file_id,
     range: [parentStart + localStart, parentStart + localEnd]
   };
   ```

3. **Concat (t=2)**: Concatenate multiple MappedStrings
   ```typescript
   // d.pieces is [[source_info_id, offset, length], ...]
   // Build composed MappedString with proper index routing
   ```

4. **Caching**: Use `resolvedCache` to avoid re-resolving chains

5. **Error handling**: Call `errorHandler` for:
   - Invalid SourceInfo ID
   - Circular references (TODO: implement detection)
   - Missing file_id in source_context

### Phase 2: Metadata Conversion

**File: `ts-packages/rust-qmd-json/src/meta-converter.ts`**

```typescript
interface JsonMetaValue {
  t: string;      // "MetaInlines", "MetaString", etc.
  c?: any;        // content
  s: number;      // source_info id
}

class MetadataConverter {
  constructor(sourceReconstructor: SourceInfoReconstructor);

  // Main entry point - convert top-level metadata object
  convertMeta(jsonMeta: Record<string, JsonMetaValue>): AnnotatedParse;

  // Convert individual MetaValue to AnnotatedParse
  private convertMetaValue(meta: JsonMetaValue): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(meta.s);
    const [start, end] = this.sourceReconstructor.getOffsets(meta.s);

    switch (meta.t) {
      case "MetaString":
        return {
          result: meta.c,
          kind: "MetaString",
          source,
          components: [],
          start,
          end
        };

      case "MetaBool":
        return {
          result: meta.c,
          kind: "MetaBool",
          source,
          components: [],
          start,
          end
        };

      case "MetaInlines":
        return {
          result: meta.c,  // Array of inline JSON objects AS-IS
          kind: this.extractKind(meta),  // Handle tagged values
          source,
          components: [],  // Empty - cannot track internal locations yet
          start,
          end
        };

      case "MetaBlocks":
        return {
          result: meta.c,  // Array of block JSON objects AS-IS
          kind: "MetaBlocks",
          source,
          components: [],
          start,
          end
        };

      case "MetaList":
        const items = meta.c.map(item => this.convertMetaValue(item));
        return {
          result: items.map(item => item.result),
          kind: "MetaList",
          source,
          components: items,
          start,
          end
        };

      case "MetaMap":
        return this.convertMetaMap(meta);
    }
  }

  // Special handling for MetaMap with interleaved key/value components
  private convertMetaMap(meta: JsonMetaValue): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(meta.s);
    const [start, end] = this.sourceReconstructor.getOffsets(meta.s);
    const entries = meta.c.entries;
    const components: AnnotatedParse[] = [];
    const result: Record<string, any> = {};

    for (const entry of entries) {
      const keySource = this.sourceReconstructor.toMappedString(entry.key_source);
      const [keyStart, keyEnd] = this.sourceReconstructor.getOffsets(entry.key_source);

      const keyAP: AnnotatedParse = {
        result: entry.key,
        kind: "key",
        source: keySource,
        components: [],
        start: keyStart,
        end: keyEnd
      };

      const valueAP = this.convertMetaValue(entry.value);

      components.push(keyAP, valueAP);
      result[entry.key] = valueAP.result;
    }

    return {
      result,
      kind: "MetaMap",
      source,
      components,
      start,
      end
    };
  }

  // Extract kind with special tag handling
  private extractKind(meta: JsonMetaValue): string {
    // TODO: For now, use simple encoding like "MetaInlines:tagged:expr"
    // Future enhancement: Modify @quarto/mapped-string to add optional tag field
    // to AnnotatedParse interface, then use that instead

    if (meta.t !== "MetaInlines" || !Array.isArray(meta.c) || meta.c.length === 0) {
      return meta.t;
    }

    // Check if wrapped in Span with yaml-tagged-string class
    const first = meta.c[0];
    if (first.t === "Span" && Array.isArray(first.c)) {
      const [attrs, _content] = first.c;
      if (Array.isArray(attrs.c) && attrs.c.includes("yaml-tagged-string")) {
        const tag = attrs.kv?.find(([k, _]: [string, string]) => k === "tag")?.[1];
        if (tag) {
          return `MetaInlines:tagged:${tag}`;
        }
      }
    }

    return "MetaInlines";
  }
}
```

### Phase 3: Integration & Testing

**File: `ts-packages/rust-qmd-json/src/types.ts`**

```typescript
export interface RustQmdJson {
  meta: Record<string, JsonMetaValue>;
  blocks: any[];
  source_pool: SerializableSourceInfo[];
  source_context: {
    files: Array<{id: number, path: string, content: string}>;
  };
}
```

**File: `ts-packages/rust-qmd-json/src/index.ts`**

```typescript
export function parseRustQmdMetadata(
  json: RustQmdJson,
  errorHandler?: (msg: string, sourceId?: number) => void
): AnnotatedParse {
  const sourceReconstructor = new SourceInfoReconstructor(
    {pool: json.source_pool},
    json.source_context,
    errorHandler
  );

  const converter = new MetadataConverter(sourceReconstructor);
  return converter.convertMeta(json.meta);
}
```

**Test file: `ts-packages/rust-qmd-json/test/meta-conversion.test.ts`**

Test cases (implemented alongside code, but passing tests not required before implementation):
1. Simple string metadata: `title: "Hello"`
2. Markdown in metadata: `title: "My **Document**"`
3. Nested structures: arrays and objects
4. Boolean and empty values
5. Complex inlines: links, code, emphasis
6. Metadata with special tags: `!path`, `!expr`
7. SourceInfo chains: Substring of Substring
8. Concat SourceInfo type

## Design Decisions

### D1: MetaInlines vs MetaString Distinction

**Decision:** Use the `kind` field to distinguish:
- `AnnotatedParse.kind = "MetaInlines"` for parsed markdown with complex result
- `AnnotatedParse.kind = "MetaString"` for plain strings
- Existing validation code can check `kind` to determine processing approach

### D2: Components for MetaInlines/MetaBlocks

**Decision:** Initially empty, future enhancement.
- MetaInlines/MetaBlocks have **complex JSONValue in result** but **empty components[]**
- This indicates the current implementation cannot track internal inline/block locations
- Future enhancement can populate components without breaking changes
- Existing code already handles this pattern (validated by compatibility analysis)

### D3: SourceInfo Without File Content

**Decision:** JSON must include `source_context`:
```json
{
  "source_context": {
    "files": [
      {"id": 0, "path": "main.qmd", "content": "..."}
    ]
  }
}
```

The SourceInfoReconstructor uses this to build MappedStrings.

### D4: Special YAML Tags (!expr, !path)

**Observation:** These are wrapped in Span with class "yaml-tagged-string" and tag attribute:
```json
{
  "t": "Span",
  "c": [
    {"t": "", "c": ["yaml-tagged-string"], "kv": [["tag", "expr"]]},
    [{"t": "Str", "c": "x + 1"}]
  ]
}
```

**Decision (temporary):** Use `kind` field encoding: `"MetaInlines:tagged:expr"`.
- Not ideal (violates type safety), but works for now
- TODO comment in code: Future enhancement should modify @quarto/mapped-string to add optional `tag?: string` field to AnnotatedParse
- See `extractKind()` method in meta-converter.ts

### D5: Error Handling

**Decision:** Optional error handler callback.
- Constructor accepts: `errorHandler?: (msg: string, sourceId?: number) => void`
- Called for invalid SourceInfo IDs, missing files, etc.
- Allows caller to decide error handling strategy (throw, log, ignore)
- TODO: Implement circular reference detection in future (call errorHandler when detected)

### D6: SourceInfo Caching

**Decision:** Implement `resolvedCache` in SourceInfoReconstructor.
- Cache resolved chains: `Map<number, {file_id: number, range: [number, number]}>`
- Avoids re-resolving deep chains (Substring of Substring of Original)
- Improves performance for large metadata structures

## File Structure

```
ts-packages/rust-qmd-json/
├── src/
│   ├── index.ts           # Main entry point and exports
│   ├── source-map.ts      # SourceInfo → MappedString conversion
│   ├── meta-converter.ts  # MetaValue → AnnotatedParse
│   └── types.ts           # TypeScript interfaces for JSON format
├── test/
│   ├── basic.test.ts      # Basic functionality tests
│   ├── source-map.test.ts # SourceInfo reconstruction tests
│   └── meta-conversion.test.ts  # Metadata conversion tests
├── package.json           # Package configuration for npm
├── tsconfig.json          # TypeScript configuration
└── README.md              # Usage documentation
```

**Note:** No `inline-extractor.ts` needed! The simplified approach eliminates the need for text reconstruction.

## Benefits of This Approach

1. **Preserves all source mapping**: Every value can be traced back to original file location via SourceInfo
2. **Compatible with existing code**: Verified safe - existing quarto-cli code handles arrays in result
3. **No data loss**: The full JSON structure is preserved in `result` for MetaInlines/MetaBlocks
4. **Simple and clean**: No text reconstruction logic needed - just direct mapping
5. **Extensible**: Can populate `components` for MetaInlines/MetaBlocks in the future without breaking changes
6. **Testable**: Clear separation of concerns allows unit testing each phase
7. **Performance**: Minimal processing with caching - just structural conversion

## Potential Limitations & Mitigations

1. **MetaInlines result is complex**: Code expecting simple strings in `result` will see JSON arrays
   - ✅ Mitigation: Verified safe - existing code uses defensive type guards
   - Use `kind` field to distinguish MetaString (string) vs MetaInlines (array)

2. **Requires source_context in JSON**: Adds to file size
   - Mitigation: Source context can be compressed or optionally included

3. **SourceInfo chains can be deep**: Substring of Substring of Original
   - ✅ Mitigation: SourceInfoReconstructor uses `resolvedCache` (D6)

4. **Components empty for MetaInlines/MetaBlocks**: Navigation into inline structure not yet supported
   - ✅ Mitigation: Future enhancement without breaking changes (D2)

5. **Tag encoding is fragile**: String-based kind encoding for !expr, !path
   - ⚠️ Mitigation: TODO comment for future @quarto/mapped-string enhancement (D4)

## Next Steps

1. ✅ Review plan with user
2. ✅ Set up standalone TypeScript package
3. ✅ Verify @quarto/mapped-string integration
4. ✅ Verify compatibility with quarto-cli (arrays in result field)
5. **→ Implement Phase 1** (SourceInfo reconstruction with caching)
6. **→ Implement Phase 2** (Metadata conversion with tag handling)
7. **→ Implement Phase 3** (Integration, tests, documentation)
8. Prepare for npm publishing as `@quarto/rust-qmd-json`

## Comparison: Original vs Simplified Approach

| Aspect | Original Approach | Simplified Approach |
|--------|------------------|---------------------|
| MetaInlines result | Reconstructed plain text string | JSON array of inline nodes AS-IS |
| Text extraction | Complex recursive inline traversal | Not needed! |
| Code complexity | 3 phases, ~300 lines | 2 phases, ~150 lines |
| Data fidelity | Text only, formatting lost | Full structure preserved |
| Performance | Slower (text reconstruction) | Faster (direct mapping + caching) |
| Future inline navigation | Need to re-parse | Already have structure in result |
| Compatibility risk | Unknown | ✅ Verified safe |

The simplified approach is clearly superior - it's simpler, faster, preserves more information, and has been verified compatible with existing code.
