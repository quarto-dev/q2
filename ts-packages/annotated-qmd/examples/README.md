# Examples

This directory contains example Quarto Markdown files and their corresponding JSON output from `quarto-markdown-pandoc`.

## Files

### `simple.qmd` / `simple.json`
A basic document demonstrating:
- YAML metadata (title, author)
- Headers
- Inline formatting (bold, italic)
- Code blocks
- Bullet lists

### `table.qmd` / `table.json`
Demonstrates table support with:
- Pipe tables
- Table caption
- Table ID attribute

### `links.qmd` / `links.json`
Demonstrates inline elements:
- Links
- Inline code
- Block quotes with nested links

## Generating JSON

To regenerate the JSON files from the .qmd sources:

```bash
# From the repository root
cargo run --bin quarto-markdown-pandoc -- -t json -i ts-packages/annotated-qmd/examples/simple.qmd > ts-packages/annotated-qmd/examples/simple.json
cargo run --bin quarto-markdown-pandoc -- -t json -i ts-packages/annotated-qmd/examples/table.qmd > ts-packages/annotated-qmd/examples/table.json
cargo run --bin quarto-markdown-pandoc -- -t json -i ts-packages/annotated-qmd/examples/links.qmd > ts-packages/annotated-qmd/examples/links.json
```

## Using in Code

```typescript
import { parseRustQmdDocument } from '@quarto/annotated-qmd';
import * as fs from 'fs';

// Load one of the example JSON files
const json = JSON.parse(fs.readFileSync('examples/simple.json', 'utf-8'));

// Convert to AnnotatedParse
const doc = parseRustQmdDocument(json);

// Explore the structure
console.log('Document has', doc.components.length, 'top-level components');
doc.components.forEach((comp, i) => {
  console.log(`Component ${i}: kind=${comp.kind}, source="${comp.source}"`);
});
```
