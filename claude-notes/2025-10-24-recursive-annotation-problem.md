# Problem Report: Recursive Type Annotation Issue

**Date**: 2025-10-24
**Issue**: Intersection-based annotation doesn't transform nested type references
**Status**: Identified, solution needed

## Problem Summary

The current approach of using TypeScript intersection to create annotated types (`Annotated_Inline_Str = Inline_Str & { s: number }`) works for leaf nodes but **fails for recursive nodes** that contain child AST nodes.

## Concrete Examples

### Issue 1: Annotated_Inline_Span

**Current (Incorrect):**
```typescript
// Base type
export type Inline_Span = { t: "Span"; c: [Attr, Inline[]] };

// Annotated type via intersection
export type Annotated_Inline_Span = Inline_Span & { s: number };
// Expands to: { t: "Span"; c: [Attr, Inline[]]; s: number }
//                                      ^^^^^^^^
//                                      WRONG! Should be Annotated_Inline[]
```

**Expected:**
```typescript
export type Annotated_Inline_Span = {
  t: "Span";
  c: [Attr, Annotated_Inline[]];  // <-- Should use Annotated_Inline
  s: number;
};
```

### Issue 2: Annotated_Block_Div

**Current (Incorrect):**
```typescript
// Base type
export type Block_Div = { t: "Div"; c: [Attr, Block[]] };

// Annotated type via intersection
export type Annotated_Block_Div = Block_Div & { s: number };
// Expands to: { t: "Div"; c: [Attr, Block[]]; s: number }
//                                    ^^^^^^^^
//                                    WRONG! Should be Annotated_Block[]
```

**Expected:**
```typescript
export type Annotated_Block_Div = {
  t: "Div";
  c: [Attr, Annotated_Block[]];  // <-- Should use Annotated_Block
  s: number;
};
```

### Issue 3: Annotated_Inline_Emph (and all formatting inlines)

**Current (Incorrect):**
```typescript
export type Inline_Emph = { t: "Emph"; c: Inline[] };
export type Annotated_Inline_Emph = Inline_Emph & { s: number };
// Expands to: { t: "Emph"; c: Inline[]; s: number }
//                              ^^^^^^^^
//                              WRONG!
```

**Expected:**
```typescript
export type Annotated_Inline_Emph = {
  t: "Emph";
  c: Annotated_Inline[];  // <-- Should use Annotated_Inline
  s: number;
};
```

### Issue 4: Citation nested in Annotated_Inline_Cite

**Current (Incorrect):**
```typescript
export interface Citation {
  citationId: string;
  citationPrefix: Inline[];      // <-- Base Inline
  citationSuffix: Inline[];      // <-- Base Inline
  citationMode: CitationMode;
  citationNoteNum: number;
  citationHash: number;
}

export type Inline_Cite = { t: "Cite"; c: [Citation[], Inline[]] };
export type Annotated_Inline_Cite = Inline_Cite & { s: number };
// The Citation[] still contains base Inline[] types!
```

**Expected:**
```typescript
export interface Annotated_Citation {
  citationId: string;
  citationPrefix: Annotated_Inline[];   // <-- Annotated
  citationSuffix: Annotated_Inline[];   // <-- Annotated
  citationMode: CitationMode;
  citationNoteNum: number;
  citationHash: number;
}

export type Annotated_Inline_Cite = {
  t: "Cite";
  c: [Annotated_Citation[], Annotated_Inline[]];
  s: number;
};
```

## Affected Node Types

### Inline Nodes with Recursive References

1. **Inline_Emph**: `c: Inline[]` → should be `Annotated_Inline[]`
2. **Inline_Strong**: `c: Inline[]` → should be `Annotated_Inline[]`
3. **Inline_Strikeout**: `c: Inline[]` → should be `Annotated_Inline[]`
4. **Inline_Superscript**: `c: Inline[]` → should be `Annotated_Inline[]`
5. **Inline_Subscript**: `c: Inline[]` → should be `Annotated_Inline[]`
6. **Inline_SmallCaps**: `c: Inline[]` → should be `Annotated_Inline[]`
7. **Inline_Underline**: `c: Inline[]` → should be `Annotated_Inline[]`
8. **Inline_Quoted**: `c: [QuoteType, Inline[]]` → should be `[QuoteType, Annotated_Inline[]]`
9. **Inline_Link**: `c: [Attr, Inline[], Target]` → should be `[Attr, Annotated_Inline[], Target]`
10. **Inline_Image**: `c: [Attr, Inline[], Target]` → should be `[Attr, Annotated_Inline[], Target]`
11. **Inline_Span**: `c: [Attr, Inline[]]` → should be `[Attr, Annotated_Inline[]]`
12. **Inline_Cite**: `c: [Citation[], Inline[]]` → needs `Annotated_Citation[]` and `Annotated_Inline[]`
13. **Inline_Note**: `c: Block[]` → should be `Annotated_Block[]` (cross-type reference!)

### Block Nodes with Recursive References

1. **Block_Plain**: `c: Inline[]` → should be `Annotated_Inline[]`
2. **Block_Para**: `c: Inline[]` → should be `Annotated_Inline[]`
3. **Block_Header**: `c: [number, Attr, Inline[]]` → should be `[number, Attr, Annotated_Inline[]]`
4. **Block_BlockQuote**: `c: Block[]` → should be `Annotated_Block[]`
5. **Block_BulletList**: `c: Block[][]` → should be `Annotated_Block[][]`
6. **Block_OrderedList**: `c: [ListAttributes, Block[][]]` → should be `[ListAttributes, Annotated_Block[][]]`
7. **Block_DefinitionList**: `c: [Inline[], Block[][]][]` → should be `[Annotated_Inline[], Annotated_Block[][]][]`
8. **Block_Div**: `c: [Attr, Block[]]` → should be `[Attr, Annotated_Block[]]`
9. **Block_Figure**: `c: [Attr, Caption, Block[]]` → needs `Annotated_Caption` and `Annotated_Block[]`

### Complex Nested Structures

**Caption**:
```typescript
// Current
export interface Caption {
  shortCaption: Inline[] | null;
  longCaption: Block[];
}

// Should be for annotated version
export interface Annotated_Caption {
  shortCaption: Annotated_Inline[] | null;
  longCaption: Annotated_Block[];
}
```

**Cell** (in tables):
```typescript
// Current
export interface Cell {
  attr: Attr;
  alignment: Alignment;
  rowSpan: number;
  colSpan: number;
  content: Block[];  // <-- Should be Annotated_Block[]
}

// Need Annotated_Cell
export interface Annotated_Cell {
  attr: Attr;
  alignment: Alignment;
  rowSpan: number;
  colSpan: number;
  content: Annotated_Block[];
}
```

**Row, TableHead, TableBody, TableFoot** all transitively need annotation.

## Why Intersection Doesn't Work

TypeScript intersection (`A & B`) creates a type that has all properties of both A and B. However:

1. **It doesn't transform nested properties**: When we write `Inline_Emph & { s: number }`, TypeScript keeps the `c: Inline[]` field as-is and adds `s: number`. It does **not** recursively transform `Inline[]` to `Annotated_Inline[]`.

2. **Structural compatibility issue**: An annotated document contains annotated children. A node with `c: Inline[]` can contain non-annotated inlines, which would be inconsistent with the annotated document structure.

3. **Type safety is compromised**: The current types allow mixing annotated and non-annotated nodes, which defeats the purpose of having separate types.

## Real-World Impact

Consider parsing a QMD document with quarto-markdown-pandoc:

```typescript
// Actual JSON from quarto-markdown-pandoc
{
  "t": "Para",
  "c": [
    { "t": "Emph", "c": [{ "t": "Str", "c": "text", "s": 0 }], "s": 1 }
  ],
  "s": 2
}
```

With current types:
```typescript
const para: Annotated_Block_Para = /* the above JSON */;
// Type says: c: Inline[]
// Reality: c contains Annotated_Inline (has 's' field)
// Type system doesn't enforce the 's' field on children!
```

This means:
- Consumers can't rely on children having `s` fields
- We lose type safety for recursive traversal
- Converting annotated AST to AnnotatedParse will have type issues

## Potential Solutions (To Be Explored)

### Option 1: Manual Type Definitions (Verbose)

Manually define each `Annotated_*` type with correct nested types:

```typescript
export type Annotated_Inline_Emph = {
  t: "Emph";
  c: Annotated_Inline[];
  s: number;
};

export type Annotated_Block_Para = {
  t: "Para";
  c: Annotated_Inline[];
  s: number;
};
```

**Pros**: Full control, clear types
**Cons**: Extremely verbose, error-prone, not DRY

### Option 2: Mapped Types with Conditional Transformation

Use TypeScript's advanced type system to recursively transform types:

```typescript
type AnnotateNode<T> = /* recursive transformation magic */;

export type Annotated_Inline_Emph = AnnotateNode<Inline_Emph>;
```

**Pros**: DRY, mechanical
**Cons**: Complex type-level programming, needs design

### Option 3: Separate Base and Annotated Trees (Current in Meta)

Define two completely separate type hierarchies:

```typescript
// Base tree
type Inline = Inline_Str | Inline_Emph | ...;

// Annotated tree (separate definitions)
type Annotated_Inline = Annotated_Inline_Str | Annotated_Inline_Emph | ...;
```

Where each `Annotated_*` type is manually defined with proper nested references.

**Pros**: Clear separation, type safe
**Cons**: Code duplication, maintenance burden

### Option 4: Template/Code Generation

Generate annotated types from base types using a build step:

```typescript
// Generated from Inline_Emph
export type Annotated_Inline_Emph = {
  t: "Emph";
  c: Annotated_Inline[];  // <-- transformed
  s: number;
};
```

**Pros**: DRY source, correct output
**Cons**: Build complexity, generated code

## Scope of the Problem

This affects:
- **13 of 20 Inline types** (65%)
- **9 of 14 Block types** (64%)
- **Multiple supporting types**: Citation, Caption, Cell, Row, TableHead, TableBody, TableFoot
- **Meta types**: MetaValue_Inlines, MetaValue_Blocks

Only leaf types (no nested AST nodes) work correctly:
- Inline_Str, Inline_Space, Inline_SoftBreak, Inline_LineBreak
- Inline_Code, Inline_Math, Inline_RawInline
- Block_CodeBlock, Block_RawBlock, Block_HorizontalRule, Block_Null
- MetaValue_String, MetaValue_Bool

## Next Steps

1. **Design a solution** using a small TypeScript prototype
2. **Validate** the solution handles all cases:
   - Inline → Annotated_Inline transformation
   - Block → Annotated_Block transformation
   - Cross-references (Inline_Note contains Blocks)
   - Complex nested structures (Citation, Caption, Cell, etc.)
3. **Implement** the chosen solution in pandoc-types.ts
4. **Test** that annotated types correctly enforce nested annotation

## Questions to Answer in Prototype

1. Can we use conditional types to recursively transform array elements?
2. Can we transform tuple elements (e.g., `[Attr, Inline[]]` → `[Attr, Annotated_Inline[]]`)?
3. How do we handle cross-type references (Inline_Note containing Block[])?
4. How do we handle complex nested objects (Citation, Caption)?
5. Can we make the transformation mechanical and maintainable?
6. What are the TypeScript compiler performance implications?

---

**Conclusion**: The intersection-based approach was a good first step to make the structure clear, but it's insufficient for recursive AST nodes. We need a solution that properly transforms nested type references throughout the tree.
