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

### `shortcode.qmd` / `shortcode.json`
Shortcodes, which reach the JSON as `quarto-shortcode__` Spans with no
attribute source (`a`) and with argument Spans that pampa currently places at
`[0, 0]` (bd-1f3cfv2k).

## Generating JSON

To regenerate the JSON files from the .qmd sources, run from the
**repository root**:

```bash
for f in ts-packages/annotated-qmd/examples/*.qmd; do
  cargo run --quiet --bin pampa -- -t json -i "$f" > "${f%.qmd}.json"
done
```

Two details matter:

- The binary is **`pampa`** (it was once called `quarto-markdown-pandoc`; that
  name no longer exists).
- Run it **from the repository root**, with the repo-relative paths above.
  `pampa` records the input path it was given in `astContext.files[].name`,
  and the CI guard
  `crates/pampa/tests/integration/annotated_qmd_fixture_guard.rs` regenerates
  every fixture the same way and fails on any byte difference. Running from
  this directory records bare file names and fails the guard.

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
