---
title: 'Add Quoted Node Support to Slides Renderer'
date: 2026-03-12
description: 'Teaches the hub-client slides renderer to handle `Quoted` inline nodes from the Pandoc AST, rendering them with curly quote marks instead of rendering nothing.'
---

## Overview

The `ReactAstSlideRenderer` in hub-client doesn't handle `Quoted` inline nodes from the Pandoc AST. When a document contains quoted text (e.g., `"hello"` or `'hello'`), the renderer falls through to the `default` case and renders nothing meaningful.

## JSON Shape

Pampa serializes Quoted nodes as:
```json
{
  "t": "Quoted",
  "c": [
    { "t": "SingleQuote" },   // or { "t": "DoubleQuote" }
    [ ...inlines... ]
  ]
}
```

## Plan

- [x] Add `QuotedInline` type definition (following existing pattern)
- [x] Add `QuotedInline` to the `Inline` union type
- [x] Add `case 'Quoted'` to `renderInline()` switch statement
- [ ] Verify manually with a q2-slides document containing quoted text

## Implementation Details

### Type definition (around line 66)

```typescript
type QuotedInline = { t: 'Quoted'; c: [{ t: string }, Inline[]] };
```

This follows the same pattern as `MathInline` which also has a discriminant object + content: `{ t: 'Math'; c: [{ t: string }, string] }`.

### Render case (in `renderInline()`, after the `Strong` case around line 761)

```typescript
case 'Quoted': {
  const quotedInline = inline as QuotedInline;
  const [quoteType, inlines] = quotedInline.c;
  const quote = quoteType.t === 'SingleQuote' ? '\u2018' : '\u201c';
  const endQuote = quoteType.t === 'SingleQuote' ? '\u2019' : '\u201d';
  return (
    <span key={key}>
      {quote}
      {renderInlines(inlines, currentFilePath, onNavigateToDocument)}
      {endQuote}
    </span>
  );
}
```

Uses Unicode curly quotes (`\u2018`/`\u2019` for single, `\u201c`/`\u201d` for double), matching Pandoc's standard rendering behavior. Wraps in a `<span>` to provide a React key and keep the quote marks grouped with their content.

### Why this approach

- Follows the exact same pattern as other inline wrapper nodes (Emph, Strong, Span)
- Uses the `[discriminant, content]` destructuring pattern already used by MathInline
- Curly quotes match Pandoc's HTML writer behavior
- No new dependencies or helpers needed
