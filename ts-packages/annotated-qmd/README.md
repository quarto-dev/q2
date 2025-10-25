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
  blocks: [
    { t: 'Para', c: [{ t: 'Str', c: 'Hello', s: 1 }], s: 2 }
  ],
  astContext: {
    sourceInfoPool: [
      { r: [11, 22], t: 0, d: 0 },
      { r: [30, 35], t: 0, d: 0 },
      { r: [30, 35], t: 0, d: 0 }
    ],
    files: [
      { name: 'doc.qmd', content: '---\ntitle: My Document\n---\n\nHello' }
    ]
  },
  'pandoc-api-version': [1, 23, 1]
};

// Convert entire document
const doc = parseRustQmdDocument(json);
console.log(doc.components.length);  // metadata + blocks

// Convert just blocks
const blocks = parseRustQmdBlocks(json.blocks, json);
console.log(blocks[0].kind);  // 'Para'

// Convert single block
const block = parseRustQmdBlock(json.blocks[0], json);
console.log(block.source);  // MappedString with source location
```

## API

### Document Conversion

#### `parseRustQmdDocument(json, errorHandler?)`

Convert a complete Pandoc document (metadata + blocks) to AnnotatedParse.

**Parameters:**
- `json: RustQmdJson` - The JSON output from quarto-markdown-pandoc
- `errorHandler?: (msg: string, id?: number) => void` - Optional error handler

**Returns:** `AnnotatedParse` with kind `'Document'`

**Example:**

```typescript
import { parseRustQmdDocument } from '@quarto/annotated-qmd';

const doc = parseRustQmdDocument(json);
// doc.components contains metadata and all blocks
```

### Block Conversion

#### `parseRustQmdBlocks(blocks, json, errorHandler?)`

Convert an array of blocks to AnnotatedParse structures.

**Parameters:**
- `blocks: Annotated_Block[]` - Array of blocks from the JSON
- `json: RustQmdJson` - Full JSON for source context
- `errorHandler?: SourceInfoErrorHandler` - Optional error handler

**Returns:** `AnnotatedParse[]`

#### `parseRustQmdBlock(block, json, errorHandler?)`

Convert a single block to AnnotatedParse.

**Parameters:**
- `block: Annotated_Block` - Single block from the JSON
- `json: RustQmdJson` - Full JSON for source context
- `errorHandler?: SourceInfoErrorHandler` - Optional error handler

**Returns:** `AnnotatedParse`

### Inline Conversion

#### `parseRustQmdInline(inline, json, errorHandler?)`

Convert a single inline element to AnnotatedParse.

**Parameters:**
- `inline: Annotated_Inline` - Single inline from the JSON
- `json: RustQmdJson` - Full JSON for source context
- `errorHandler?: SourceInfoErrorHandler` - Optional error handler

**Returns:** `AnnotatedParse`

### Metadata Conversion

#### `parseRustQmdMetadata(json, errorHandler?)`

Convert only the document metadata to AnnotatedParse.

**Parameters:**
- `json: RustQmdJson` - The JSON output from quarto-markdown-pandoc
- `errorHandler?: SourceInfoErrorHandler` - Optional error handler

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

## Examples

The `examples/` directory contains sample .qmd files and their corresponding JSON output from `quarto-markdown-pandoc`:

- **simple.qmd** - Basic document with metadata, headers, formatting, code blocks, and lists
- **table.qmd** - Table with caption and attributes
- **links.qmd** - Links, inline code, and blockquotes

Each example includes both the source .qmd file and the generated .json file. See `examples/README.md` for usage examples.

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

The package consists of several converter classes that work together:

- **SourceInfoReconstructor** - Reconstructs source locations from the sourceInfoPool
- **MetadataConverter** - Converts YAML metadata to AnnotatedParse
- **InlineConverter** - Converts inline elements (Str, Emph, Link, etc.)
- **BlockConverter** - Converts block elements (Para, Header, Table, etc.)
- **DocumentConverter** - Orchestrates all converters for complete documents

All converters preserve source location information through `MappedString` objects that track the original source text and its location.
