# Examples

This directory contains example Quarto Markdown files and their corresponding JSON output from the `pampa` binary.

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

To regenerate the JSON files from the .qmd sources, run from **this directory**
(`ts-packages/annotated-qmd/examples`):

```bash
for q in *.qmd; do
  cargo run --quiet --bin pampa -- -t json -i "$q" > "${q%.qmd}.json"
done
```

Two details matter:

- The binary is **`pampa`** (it was once called `quarto-markdown-pandoc`; that
  name no longer exists).
- Run it **from this directory**, not from the repository root. `pampa` records
  the input path it was given in `astContext.files[].name`, so invoking it with
  a longer path bakes that path into every example.

Some of these fixtures deliberately contain malformed constructs and will emit
warnings on stderr; the JSON on stdout is still what the tests consume.

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
