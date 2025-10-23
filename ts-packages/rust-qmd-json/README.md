# @quarto/rust-qmd-json

Convert quarto-markdown-pandoc JSON output to AnnotatedParse structures with full source mapping.

## Overview

This package converts the JSON output from the Rust-based `quarto-markdown-pandoc` parser
into `AnnotatedParse` structures that are compatible with quarto-cli's YAML validation
infrastructure. It preserves complete source location information through the conversion.

## Installation

```bash
npm install @quarto/rust-qmd-json
```

## Quick Start

```typescript
import { parseRustQmdMetadata } from '@quarto/rust-qmd-json';
import type { RustQmdJson } from '@quarto/rust-qmd-json';

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
import { parseRustQmdMetadata } from '@quarto/rust-qmd-json';

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
} from '@quarto/rust-qmd-json';
```

### Advanced Usage

For more control, you can use the underlying classes directly:

```typescript
import { SourceInfoReconstructor, MetadataConverter } from '@quarto/rust-qmd-json';

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

## Architecture

The conversion happens in two phases:

1. **SourceInfo Reconstruction**: Convert the pooled SourceInfo format from JSON into
   MappedString objects that track source locations through transformation chains.

2. **Metadata Conversion**: Recursively convert MetaValue variants into AnnotatedParse
   structures with proper source tracking. MetaInlines/MetaBlocks are treated as leaf
   nodes with the JSON array structure preserved in the result.

## Design Decisions

- **Direct JSON Value Mapping**: MetaInlines and MetaBlocks are preserved as JSON arrays
  in the `result` field, avoiding any text reconstruction
- **Source Tracking**: Every value can be traced back to original file location via SourceInfo
- **Compatible Types**: Produces AnnotatedParse structures compatible with existing validation code

See repository's `claude-notes/plans/2025-10-23-json-to-annotated-parse-conversion.md` for
detailed implementation plan.
