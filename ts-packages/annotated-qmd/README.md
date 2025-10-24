# @quarto/annotated-qmd

Convert quarto-markdown-pandoc JSON output to AnnotatedParse structures with full source mapping.

## Overview

This package converts the JSON output from the Rust-based `quarto-markdown-pandoc` parser
into `AnnotatedParse` structures that are compatible with quarto-cli's YAML validation
infrastructure. It preserves complete source location information through the conversion.

## Installation

```bash
npm install @quarto/annotated-qmd
```

## Quick Start

```typescript
import { parseRustQmdMetadata } from '@quarto/annotated-qmd';
import type { RustQmdJson } from '@quarto/annotated-qmd';

// JSON from quarto-markdown-pandoc
const json: RustQmdJson = {
  meta: {
    title: { t: 'MetaString', c: 'My Document', s: 0 }
  },
  blocks: [],
  source_pool: [
    { r: [11, 22], t: 0, d: 0 }
  ],
  source_context: {
    files: [
      { id: 0, path: 'doc.qmd', content: '---\ntitle: My Document\n---' }
    ]
  }
};

const annotatedParse = parseRustQmdMetadata(json);

console.log(annotatedParse.result);  // { title: 'My Document' }
console.log(annotatedParse.kind);    // 'mapping'
console.log(annotatedParse.components.length);  // 2 (key + value)
```

## API

### `parseRustQmdMetadata(json, errorHandler?)`

Main entry point for converting quarto-markdown-pandoc JSON to AnnotatedParse.

**Parameters:**
- `json: RustQmdJson` - The JSON output from quarto-markdown-pandoc
- `errorHandler?: (msg: string, id?: number) => void` - Optional error handler for SourceInfo reconstruction errors

**Returns:** `AnnotatedParse`

**Example with error handling:**

```typescript
import { parseRustQmdMetadata } from '@quarto/annotated-qmd';

const errorHandler = (msg: string, id?: number) => {
  console.error(`SourceInfo error: ${msg}`, id);
};

const result = parseRustQmdMetadata(json, errorHandler);
```

### Types

The package exports all necessary TypeScript types:

```typescript
import type {
  AnnotatedParse,
  JSONValue,
  JsonMetaValue,
  MetaMapEntry,
  RustQmdJson,
  SerializableSourceInfo,
  SourceContext,
  SourceInfoErrorHandler
} from '@quarto/annotated-qmd';
```

### Advanced Usage

For more control, you can use the underlying classes directly:

```typescript
import { SourceInfoReconstructor, MetadataConverter } from '@quarto/annotated-qmd';

const reconstructor = new SourceInfoReconstructor(
  json.source_pool,
  json.source_context
);

const converter = new MetadataConverter(reconstructor);
const result = converter.convertMeta(json.meta);
```

## Development

```bash
# Install dependencies
npm install

# Build
npm run build

# Test
npm test

# Clean
npm run clean
```
