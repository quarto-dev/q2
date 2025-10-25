# annotated-qmd Type Cleanup Analysis

## Current State

### Document Types (3 overlapping definitions)

#### 1. RustQmdJson (types.ts)
```typescript
interface RustQmdJson {
  meta: Record<string, JsonMetaValue>;
  blocks: unknown[];  // ❌ Too loose, no type safety
  astContext: {
    sourceInfoPool: SerializableSourceInfo[];
    files: RustFileInfo[];
    metaTopLevelKeySources?: Record<string, number>;
  };
  'pandoc-api-version': [number, number, number];
}
```
**Purpose**: Public API input type for parse functions
**Usage**: Input to `parseRustQmdDocument()`, `parseRustQmdBlocks()`, etc
**Issues**: `blocks: unknown[]` provides no type safety

#### 2. QmdPandocDocument (pandoc-types.ts)
```typescript
interface QmdPandocDocument extends PandocDocument {
  astContext: { ... };
}

// Where PandocDocument is:
interface PandocDocument {
  meta: Record<string, MetaValue>;      // ❌ Wrong! Rust uses JsonMetaValue
  blocks: Block[];                       // ❌ Wrong! Rust uses Annotated_Block[]
  'pandoc-api-version': [number, number, number];
}
```
**Purpose**: Unclear - attempt at typed Rust output?
**Usage**: Exported publicly, has type guard, has test, but NOT used in implementation
**Issues**:
- Extends `PandocDocument` which has wrong types (`MetaValue` instead of `JsonMetaValue`, `Block[]` instead of `Annotated_Block[]`)
- This is a **type lie** - Rust output doesn't match these types

#### 3. AnnotatedPandocDocument (document-converter.ts)
```typescript
interface AnnotatedPandocDocument {
  "pandoc-api-version": [number, number, number];
  meta: Record<string, JsonMetaValue>;    // ✅ Correct
  blocks: Annotated_Block[];               // ✅ Correct
  // ❌ Missing astContext
}
```
**Purpose**: Internal type for `DocumentConverter.convertDocument()`
**Usage**: Parameter type for one method
**Issues**:
- Defined in implementation file, not types file
- Missing `astContext` field (has to be passed separately)
- Only used in one place

### Metadata Types (2 overlapping definitions)

#### JsonMetaValue (types.ts)
```typescript
interface JsonMetaValue {
  t: string;
  c?: unknown;  // Loose typing
  s: number;    // Source info ID
}
```
**Purpose**: Raw JSON representation from Rust
**Usage**: In `RustQmdJson.meta`

#### Annotated_MetaValue (pandoc-types.ts)
```typescript
type Annotated_MetaValue =
  | Annotated_MetaValue_Map
  | Annotated_MetaValue_List
  | ...  // Discriminated union with proper typing
```
**Purpose**: Strongly typed representation with proper discriminated union
**Usage**: After parsing/validation

## Problems

### 1. QmdPandocDocument is Fundamentally Flawed
- Extends `PandocDocument` which has `blocks: Block[]`
- But Rust actually produces `blocks: Annotated_Block[]`
- This creates a type mismatch that makes the type useless
- The user is correct: **"extending PandocDocument won't work"**

### 2. Confusing Proliferation
- Three document types that represent similar things
- No clear guidance on which to use when
- Two live in types.ts, one in document-converter.ts, one in pandoc-types.ts

### 3. Naming is Unclear
- "RustQmdJson" - suggests raw JSON from Rust (✓)
- "QmdPandocDocument" - suggests Quarto Pandoc document (confusing)
- "AnnotatedPandocDocument" - clearest name (✓)

### 4. Missing Comprehensive Type
- No single type that accurately represents Rust output with proper typing
- `RustQmdJson` is loose (`blocks: unknown[]`)
- `AnnotatedPandocDocument` is missing `astContext`
- `QmdPandocDocument` has wrong base types

## Proposal: Consolidate and Clarify

### Phase 1: Create One Canonical Input Type

**Rename and fix `RustQmdJson` → keep name but improve docs:**

```typescript
/**
 * Complete JSON output from quarto-markdown-pandoc (Rust parser).
 * This is the input format for all parse functions.
 *
 * - Use `parseRustQmdDocument(json)` to convert to AnnotatedParse
 * - Use `parseRustQmdBlocks(json.blocks, json)` for just blocks
 * - Use `parseRustQmdMetadata(json)` for just metadata
 */
export interface RustQmdJson {
  "pandoc-api-version": [number, number, number];

  /** Metadata with source info (JsonMetaValue includes source ID) */
  meta: Record<string, JsonMetaValue>;

  /**
   * Blocks with source info (runtime type is Annotated_Block[] but left
   * as unknown[] to avoid circular dependencies and because JSON parsing
   * doesn't validate types)
   */
  blocks: unknown[];

  /** Source location tracking data */
  astContext: {
    sourceInfoPool: SerializableSourceInfo[];
    files: RustFileInfo[];
    metaTopLevelKeySources?: Record<string, number>;
  };
}
```

### Phase 2: Remove QmdPandocDocument

1. **Remove from pandoc-types.ts**:
   - Delete `QmdPandocDocument` interface
   - Delete `isQmdPandocDocument()` type guard

2. **Remove from index.ts exports**

3. **Update test**:
   - Remove test in `test/pandoc-types.test.ts`
   - Or replace with test showing how to use `RustQmdJson` properly

**Justification**:
- It's broken (wrong base types)
- It's not used in implementation
- It's not documented
- It confuses users

### Phase 3: Simplify AnnotatedPandocDocument

**Option A: Remove it entirely**
- Only used in one place (`DocumentConverter.convertDocument()`)
- Can inline the type or just use looser typing

**Option B: Move to types.ts and make it internal-only**
```typescript
/**
 * Internal type for validated document structure.
 *
 * @internal - This is cast from RustQmdJson.blocks after type assertion.
 * External users should use RustQmdJson.
 */
export interface _InternalAnnotatedDocument {
  "pandoc-api-version": [number, number, number];
  meta: Record<string, JsonMetaValue>;
  blocks: Annotated_Block[];
}
```

### Phase 4: Add Comprehensive Typed Version (Optional)

If users need a fully typed version for advanced use cases:

```typescript
/**
 * Fully typed version of Rust output with proper type discrimination.
 * This is NOT the raw JSON type - use RustQmdJson for that.
 *
 * This type is useful for:
 * - Type guards and narrowing
 * - Walking the AST with type safety
 * - Advanced validation
 */
export interface AnnotatedQmdDocument {
  "pandoc-api-version": [number, number, number];
  meta: Record<string, Annotated_MetaValue>;
  blocks: Annotated_Block[];
  astContext: {
    sourceInfoPool: SerializableSourceInfo[];
    files: RustFileInfo[];
    metaTopLevelKeySources?: Record<string, number>;
  };
}
```

## File Organization

### Current (Scattered)
- **types.ts**: RustQmdJson, JsonMetaValue, AnnotatedParse
- **pandoc-types.ts**: PandocDocument, QmdPandocDocument, Annotated_* types
- **document-converter.ts**: AnnotatedPandocDocument

### Proposed (Organized by Purpose)

#### types.ts - Core I/O Types
- `AnnotatedParse` - output type
- `RustQmdJson` - input type (with astContext)
- `JsonMetaValue` - raw JSON metadata
- Helper types (JSONValue, MetaMapEntry, RustFileInfo)

#### pandoc-types.ts - Pandoc AST Type Definitions
- Base Pandoc types (Inline, Block, MetaValue, PandocDocument)
- Annotated Pandoc types (Annotated_Inline, Annotated_Block, Annotated_MetaValue)
- Supporting types (Attr, Target, AttrSourceInfo, etc.)
- NO document types - those go in types.ts

#### document-converter.ts - Implementation Only
- `DocumentConverter` class
- No exported types (or only internal ones prefixed with _)

## Migration Path

1. ✅ Fix `info_string.rs` bug (already done)
2. Add deprecation notice to `QmdPandocDocument`
3. Update README to only show `RustQmdJson`
4. Next major version: remove `QmdPandocDocument`
5. Consider adding fully typed `AnnotatedQmdDocument` if requested

## Summary

**Keep**:
- `RustQmdJson` (improve docs) - public input API
- `PandocDocument` - reference for standard Pandoc
- `Annotated_*` types - for typed AST manipulation

**Remove**:
- `QmdPandocDocument` - broken, unused, confusing

**Simplify**:
- `AnnotatedPandocDocument` - either remove or make internal-only

**Result**:
- One clear input type: `RustQmdJson`
- One clear output type: `AnnotatedParse`
- Optional typed AST: `Annotated_Block[]`, `Annotated_Inline[]`
