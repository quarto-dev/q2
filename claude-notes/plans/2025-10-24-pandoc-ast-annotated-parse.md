# Plan: Pandoc AST Support for @quarto/annotated-qmd

**Date**: 2025-10-24
**Status**: Planning
**Owner**: Claude Code

## Overview

Extend `@quarto/annotated-qmd` to support the full Pandoc JSON AST (blocks and inlines), not just YAML metadata. This will enable source-aware processing of the entire document structure, similar to how we currently handle metadata.

**Primary Use Case**: Linting support in quarto-cli. The goal is to enable syntax-directed linting rules that can emit document warnings based on the AST structure, with accurate source locations. This is analogous to how YAML validation currently works using AnnotatedParse over YAML objects in `_quarto.yml`.

## Current State

- ✅ `@quarto/annotated-qmd` converts YAML metadata (Meta objects) from quarto-markdown-pandoc to AnnotatedParse
- ✅ SourceInfoReconstructor handles the source info pool (Original, Substring, Concat)
- ✅ MetadataConverter handles all Meta types: MetaString, MetaBool, MetaInlines, MetaBlocks, MetaList, MetaMap
- ✅ Integration with `@quarto/mapped-string` for source mapping
- ❌ No support for Blocks (Para, Header, BulletList, etc.)
- ❌ No support for Inlines (Str, Space, Emph, Strong, etc.) as independent entities
- ❌ No TypeScript type declarations for Pandoc AST nodes

## Architecture Analysis

### Pandoc JSON Schema Structure

From observing `pandoc -t json` and `quarto-markdown-pandoc -t json`:

**Standard Pandoc format**:
```typescript
{
  "pandoc-api-version": [1, 23, 1],
  "meta": { ... },
  "blocks": [
    { "t": "Para", "c": [...inlines...] },
    { "t": "Header", "c": [level, attrs, [...inlines...]] }
  ]
}
```

**quarto-markdown-pandoc extension**:
- Adds `"s": <number>` field to every AST node (points to sourceInfoPool index)
- Adds `astContext` object at top level:
  ```typescript
  {
    "sourceInfoPool": [...],
    "files": [...],
    "metaTopLevelKeySources": {...}
  }
  ```

### Key AST Node Categories

1. **Blocks** - Top-level document structure
   - Simple: `Para`, `Plain`, `HorizontalRule`, `Null`
   - With content: `Header`, `BlockQuote`, `CodeBlock`, `RawBlock`
   - Lists: `BulletList`, `OrderedList`, `DefinitionList`
   - Structural: `Div`, `Table`, `Figure`

2. **Inlines** - Content within blocks
   - Simple: `Str`, `Space`, `SoftBreak`, `LineBreak`
   - Formatting: `Emph`, `Strong`, `Strikeout`, `Superscript`, `Subscript`, `SmallCaps`, `Underline`
   - Links/Images: `Link`, `Image`
   - Code: `Code`, `RawInline`
   - Math: `Math` (InlineMath, DisplayMath)
   - Structured: `Span`, `Quoted`, `Cite`
   - Special: `Note` (footnote content)

3. **Attributes** - Common pattern `[id, [classes], [[key, value]]]`
   - Used by: Header, CodeBlock, Code, Div, Span, Link, Image, Table, etc.

4. **Meta Values** (already implemented)
   - MetaString, MetaBool, MetaInlines, MetaBlocks, MetaList, MetaMap

### Design Patterns from Existing Code

From `meta-converter.ts` and `annotated-yaml.ts`:

1. **Recursive conversion** - Each node type has a converter that recursively converts children
2. **Component tracking** - AnnotatedParse.components stores child nodes (interleaved for maps)
3. **Source preservation** - Each AnnotatedParse maintains link to source via MappedString
4. **Type-specific handling** - Switch on `.t` field to dispatch to type-specific handlers
5. **Error resilience** - Graceful degradation when source info is invalid

## Implementation Plan

### Phase 1: TypeScript Type Declarations

**Goal**: Create accurate TypeScript type declarations for Pandoc JSON schema

**Approach**: Incremental discovery
- Start with core types observed from `pandoc -t json` output
- Extend types as we encounter new structures
- Use discriminated unions for node types (based on `t` field)
- Add quarto-markdown-pandoc extensions (`s` field)

**Tasks**:
1. Create `ts-packages/annotated-qmd/src/pandoc-types.ts`
2. Define base types:
   - `PandocNode` with `t` and optional `s` fields
   - `Attr` type for attributes
   - `Target` type for links
3. Define Block types (discriminated union):
   - Start with: Para, Plain, Header, CodeBlock, RawBlock, BlockQuote
   - Add: BulletList, OrderedList, DefinitionList
   - Add: Div, HorizontalRule, Null, Table, Figure
4. Define Inline types (discriminated union):
   - Start with: Str, Space, Emph, Strong, Code, Math, Span
   - Add: Link, Image, Quoted, Cite, Note
   - Add: SoftBreak, LineBreak, RawInline, etc.
5. Define Meta types (align with existing JsonMetaValue)
6. Define top-level Document type
7. Write validation tests for type correctness

**Deliverable**: Complete TypeScript type definitions in `pandoc-types.ts`

### Phase 2: Inline Converter

**Goal**: Convert Pandoc Inline nodes to AnnotatedParse

**Why inlines first?** Inlines are simpler (no recursive block nesting), and they're needed by blocks (Para contains inlines).

**Tasks**:
1. Create `ts-packages/annotated-qmd/src/inline-converter.ts`
2. Implement `InlineConverter` class (mirror pattern from MetadataConverter)
3. Implement converters for simple inlines:
   - `Str` - text content
   - `Space`, `SoftBreak`, `LineBreak` - no content
4. Implement converters for formatting inlines:
   - `Emph`, `Strong`, `Strikeout`, etc. - recursive on child inlines
5. Implement converters for special inlines:
   - `Code` - text + attributes
   - `Math` - math type + content
   - `Span` - attributes + child inlines
   - `Link`, `Image` - attributes + target + child inlines
6. Implement converters for complex inlines:
   - `Quoted` - quote type + child inlines
   - `Cite` - citations + child inlines
   - `Note` - child blocks (defer to BlockConverter)
7. Write unit tests for each inline type
8. Test with real quarto-markdown-pandoc output

**Deliverable**: `InlineConverter` class with full inline support

### Phase 3: Block Converter (Core Blocks)

**Goal**: Convert core Pandoc Block nodes to AnnotatedParse

**Tasks**:
1. Create `ts-packages/annotated-qmd/src/block-converter.ts`
2. Implement `BlockConverter` class with dependency on `InlineConverter`
3. Implement converters for simple blocks:
   - `Para`, `Plain` - list of inlines (use InlineConverter)
   - `HorizontalRule`, `Null` - no content
4. Implement converters for content blocks:
   - `Header` - level + attributes + inlines
   - `CodeBlock` - attributes + text
   - `RawBlock` - format + text
   - `BlockQuote` - child blocks (recursive)
5. Implement converters for simple list blocks:
   - `BulletList` - list of block lists
   - `OrderedList` - list attributes + list of block lists
6. Implement converters for structural blocks:
   - `Div` - attributes + child blocks
   - `Figure` - attributes + caption + child blocks
7. Write unit tests for each block type
8. Test with real quarto-markdown-pandoc output

**Deliverable**: `BlockConverter` class with core block support (excluding DefinitionList and Table)

### Phase 3b: Definition List Support

**Goal**: Handle DefinitionList with proper source mapping through desugaring

**Background**: In quarto-markdown-pandoc, DefinitionList blocks are created through desugaring of `div.definition-list` structures (see `src/pandoc/treesitter_utils/postprocess.rs`). The desugaring preserves source info from the original div. The structure is:
```rust
DefinitionList {
  content: Vec<(Inlines, Vec<Vec<Block>>)>,  // [(term, [definitions])]
  source_info: SourceInfo  // from original div
}
```

**Tasks**:
1. Add DefinitionList type to `pandoc-types.ts`:
   ```typescript
   { t: "DefinitionList"; c: [Inline[], Block[][]][]; s: number }
   ```
2. Implement converter in `BlockConverter`:
   - Convert each (term, definitions) pair
   - Term is converted using InlineConverter
   - Each definition list is converted recursively with BlockConverter
   - Components should interleave terms and definition lists
3. Create test fixtures:
   - Simple definition list
   - Nested definition list
   - Definition list with complex terms (formatted text)
   - Definition list with complex definitions (multiple paragraphs)
4. Verify source mapping through desugaring:
   - Create div.definition-list in QMD
   - Process with quarto-markdown-pandoc
   - Verify AnnotatedParse has correct source locations
   - Verify source traces back to original div syntax
5. Write unit tests for all definition list scenarios
6. Integration test: Load fixture, convert, verify source locations

**Special Considerations**:
- Source info comes from the original div, not individual list items
- Need to handle nested definitions (each definition can contain multiple blocks)
- Component structure should allow navigation to individual terms and definitions

**Deliverable**: Full DefinitionList support with accurate source mapping

### Phase 3c: Table Support

**Goal**: Handle Table blocks with complete structure

**Background**: Table is the most complex block type in Pandoc. Structure includes:
- Attributes
- Caption (with short caption and long caption blocks)
- Column specifications (alignments, widths)
- Table head (rows of cells)
- Table bodies (multiple bodies, each with row head + body rows)
- Table foot (rows of cells)

**Tasks**:
1. Add Table types to `pandoc-types.ts`:
   ```typescript
   // Table structure types
   type TableCell = { ... };
   type Row = { ... };
   type TableHead = { ... };
   type TableBody = { ... };
   type TableFoot = { ... };
   type Caption = { ... };
   type ColSpec = { ... };

   // Main table type
   { t: "Table";
     c: [Attr, Caption, ColSpec[], TableHead, TableBody[], TableFoot];
     s: number
   }
   ```
2. Study actual table output:
   - Create various table fixtures in QMD
   - Generate JSON with quarto-markdown-pandoc
   - Document observed structure
3. Implement converter in `BlockConverter`:
   - Convert table attributes
   - Convert caption (short + long)
   - Convert column specs
   - Convert head/body/foot rows
   - Each cell contains blocks (recursive conversion)
4. Design component structure:
   - How to represent table structure in components?
   - Need to preserve cell-by-cell navigation
   - Consider: flat list vs nested structure
5. Create comprehensive test fixtures:
   - Simple table (headers + data rows)
   - Table with caption
   - Table with complex cells (multiple paragraphs)
   - Table with formatting in cells
   - Table with different column alignments
   - Multi-body table
   - Table with foot
6. Write unit tests for all table scenarios
7. Integration test: Complex table end-to-end

**Special Considerations**:
- Tables are the most complex structure - may require multiple iterations
- Component structure needs careful design for navigation
- Source info may be at different granularities (table, row, cell)
- Performance: large tables may have many nodes

**Deliverable**: Full Table support with comprehensive testing

### Phase 4: Document Converter

**Goal**: Convert entire Pandoc document to AnnotatedParse

**Prerequisites**: Phases 1, 2, 3, 3b, and 3c must be complete

**Tasks**:
1. Create `ts-packages/annotated-qmd/src/document-converter.ts`
2. Implement `DocumentConverter` class that orchestrates all converters
3. Support converting:
   - Full document (meta + blocks) → AnnotatedParse with both
   - Just blocks → AnnotatedParse of blocks
   - Individual block → AnnotatedParse
   - Individual inline → AnnotatedParse
4. Update `index.ts` to export new converters and functions
5. Add convenience functions:
   - `parseRustQmdDocument(json)` - convert full document
   - `parseRustQmdBlocks(json)` - convert just blocks
   - `parseRustQmdBlock(block, context)` - convert single block
   - `parseRustQmdInline(inline, context)` - convert single inline
6. Write integration tests
7. Update README with new API and examples

**Deliverable**: Complete document conversion API

### Phase 5: Testing & Validation

**Goal**: Comprehensive testing of all converters

**Tasks**:
1. Create test fixtures:
   - Generate various QMD files
   - Process with `quarto-markdown-pandoc -t json`
   - Save JSON outputs as fixtures
2. Write end-to-end tests:
   - Complex documents with nested structures
   - Documents with all block types
   - Documents with all inline types
   - Edge cases (empty content, deeply nested, etc.)
3. Validate AnnotatedParse output:
   - Verify `result` matches JSON structure
   - Verify `source` MappedStrings are correct
   - Verify `components` tree is correct
   - Verify `start`/`end` offsets are accurate
4. Performance testing:
   - Large documents
   - Deeply nested structures
5. Write documentation:
   - Usage examples
   - API reference
   - Migration guide

**Deliverable**: Comprehensive test suite + documentation

### Phase 6: Integration with quarto-cli

**Goal**: Enable quarto-cli to use annotated Pandoc AST

This phase is dependent on quarto-cli architecture and may be deferred.

**Potential tasks**:
- Identify integration points in quarto-cli
- Update quarto-cli to use new converters
- Test with quarto-cli validation infrastructure

## Implementation Details

### Type System Design

Use discriminated unions for type safety:

```typescript
// Inline union type
type Inline =
  | { t: "Str"; c: string; s: number }
  | { t: "Space"; s: number }
  | { t: "Emph"; c: Inline[]; s: number }
  | { t: "Strong"; c: Inline[]; s: number }
  | { t: "Code"; c: [Attr, string]; s: number }
  | { t: "Math"; c: [MathType, string]; s: number }
  | { t: "Span"; c: [Attr, Inline[]]; s: number }
  | { t: "Link"; c: [Attr, Inline[], Target]; s: number }
  // ... more inline types

// Block union type
type Block =
  | { t: "Para"; c: Inline[]; s: number }
  | { t: "Plain"; c: Inline[]; s: number }
  | { t: "Header"; c: [number, Attr, Inline[]]; s: number }
  | { t: "CodeBlock"; c: [Attr, string]; s: number }
  | { t: "BulletList"; c: Block[][]; s: number }
  // ... more block types

// Supporting types
type Attr = [string, string[], [string, string][]];
type Target = [string, string];
type MathType = { t: "InlineMath" } | { t: "DisplayMath" };
```

### Converter Pattern

Follow the established pattern from `MetadataConverter`:

```typescript
class InlineConverter {
  constructor(
    private sourceReconstructor: SourceInfoReconstructor
  ) {}

  convertInline(inline: Inline): AnnotatedParse {
    const source = this.sourceReconstructor.toMappedString(inline.s);
    const [start, end] = this.sourceReconstructor.getOffsets(inline.s);

    switch (inline.t) {
      case "Str":
        return {
          result: inline.c,
          kind: "Str",
          source,
          components: [],
          start,
          end
        };

      case "Emph":
        return {
          result: inline.c, // Keep as Pandoc JSON
          kind: "Emph",
          source,
          components: inline.c.map(child => this.convertInline(child)),
          start,
          end
        };

      // ... more cases
    }
  }
}
```

### Result Format Decision

**Question**: What should `AnnotatedParse.result` contain?

**Option 1**: Keep Pandoc JSON structure
```typescript
result: { t: "Emph", c: [...] }  // Full Pandoc node
```

**Option 2**: Extract semantic value
```typescript
result: "text content"  // For Str
result: [...inline objects...]  // For Emph
```

**Recommendation**: Option 1 (keep Pandoc JSON) because:
- Preserves all Pandoc information
- Allows consumers to use standard Pandoc processing
- Consistent with MetaInlines/MetaBlocks (which keep AST as-is)
- AnnotatedParse provides source mapping layer on top

### Handling Attributes

Attributes `[id, classes, kvPairs]` appear frequently. Consider helper:

```typescript
private convertAttr(attr: Attr, attrSourceId: number): AnnotatedParse {
  // Convert attribute components to AnnotatedParse
  // May need separate source IDs for id, classes, kvPairs
  // For now, may use single source for entire attr
}
```

### Error Handling

Follow existing pattern:
- Use SourceInfoErrorHandler for source reconstruction errors
- Gracefully degrade when source info is missing
- Return valid AnnotatedParse even with errors (use empty MappedString)

## Open Questions

1. **Attribute source mapping**: Do we need separate source IDs for attribute components (id, classes, kv pairs), or is one source ID per attribute sufficient?
   - **Initial answer**: Use single source ID (what Rust currently provides)
   - **Future**: May need to enhance Rust to provide finer-grained source info

2. **Result format**: Should `AnnotatedParse.result` contain Pandoc JSON or extracted values?
   - **Answer**: Keep Pandoc JSON structure to preserve all information

3. **Components for complex structures**: How to represent components for structures like Table?
   - **Answer**: Design during Phase 3c - may need nested structure to preserve table semantics

4. **Performance**: Will deeply nested documents cause performance issues?
   - **Answer**: Test with real documents in Phase 5, optimize if needed

5. **Integration**: How will quarto-cli use this?
   - **Primary use case**: Linting support with syntax-directed rules
   - **Answer**: Document API in Phase 5, actual integration deferred to Phase 6

6. **Definition list source granularity**: The entire DefinitionList has one source_info from the desugared div. Do we need finer-grained source info for individual terms/definitions?
   - **Initial answer**: Use what Rust provides (div-level source info)
   - **Future consideration**: May want to track individual term/definition locations

## Success Criteria

- ✅ Complete TypeScript type definitions for Pandoc AST
- ✅ InlineConverter handles all inline types with source mapping
- ✅ BlockConverter handles all core block types with source mapping
- ✅ DefinitionList support with source tracking through desugaring
- ✅ Table support with complete structure and source mapping
- ✅ DocumentConverter provides full document conversion API
- ✅ All converters maintain accurate source mapping
- ✅ Source locations enable precise linting error messages
- ✅ Comprehensive test coverage (>90%)
- ✅ Integration tests with real quarto-markdown-pandoc output
- ✅ Documentation with linting use case examples
- ✅ Backward compatible with existing metadata conversion

## Dependencies

- `@quarto/mapped-string` - source mapping (already integrated)
- `quarto-markdown-pandoc` binary - for generating test fixtures
- TypeScript compiler - for type checking

## Timeline Estimate

- Phase 1 (Types): 1-2 days
- Phase 2 (Inlines): 2-3 days
- Phase 3 (Core Blocks): 2-3 days
- Phase 3b (Definition Lists): 1-2 days
- Phase 3c (Tables): 2-3 days (most complex structure)
- Phase 4 (Document): 1-2 days
- Phase 5 (Testing): 2-3 days
- **Total**: 11-18 days of focused work

**Approach**: Incremental full implementation (no minimal POC needed)

## Notes

- This is a complex project requiring careful attention to the Pandoc AST structure
- We'll discover new node types incrementally through testing
- The Pandoc AST is not officially documented in JSON form, so we rely on observation
- Priority is correctness and completeness over performance
- The existing MetadataConverter provides an excellent pattern to follow
- Definition lists require special attention due to desugaring from div.definition-list
- Tables are the most complex structure and will require the most time (Phase 3c)
- The linting use case drives requirements for accurate source locations
- We're building a full incremental implementation, not a minimal POC

## References

- External annotated-json package: `/Users/cscheid/repos/github/cscheid/kyoto/external-sources/quarto/packages/annotated-json/`
- Pandoc documentation: https://pandoc.org/
- Pandoc Lua filters documentation (shows AST structure): https://pandoc.org/lua-filters.html
- quarto-markdown-pandoc source: `/Users/cscheid/repos/github/cscheid/kyoto/crates/quarto-markdown-pandoc/`

---

*Note: This plan will be refined as we progress through implementation and discover new requirements.*
