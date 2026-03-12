- **CRITICAL - TEST FIRST**: When fixing bugs using tests, you MUST run the failing test BEFORE implementing any fix. This is non-negotiable. Verify the test fails in the expected way, then implement the fix, then verify the test passes.
- Always strive for minimal test documents as small as possible. Create many small test documents instead of a few large test documents.
- You are encouraged to spend time and tokens on thinking about good tests.
- If writing tests is taking a lot of time, decompose the writing of tests into subtasks. Good tests are important!
- Precise tests are good tests. **bad**: testing for the presence of a field in an object. **good** testing if the value of the field is correct.
- When choosing hex colors for CSS test assertions (`ensureCssRegexMatches`), use **non-condensable** 6-digit hex values. CSS minifiers shorten `#RRGGBB` to `#RGB` when each pair is a repeated digit (e.g., `#cc5500` → `#c50`). Break at least one pair to prevent this: `#cc5501` instead of `#cc5500`.
- Do not write tests that expect known-bad inputs. Instead, add a failing test, and create a beads task to handle the problem.

## End-to-End Testing for WASM Features

**CRITICAL**: When implementing features that involve the WASM module (`wasm-quarto-hub-client`), you MUST write and run end-to-end tests BEFORE claiming the feature works.

### Why This Matters

The WASM module is a separate compilation target with its own:
- `Cargo.toml` (excluded from workspace)
- Runtime environment (browser or Node.js)
- Dependencies (must be added separately)

Changes that compile in the Rust workspace may NOT work in WASM. Always verify with actual WASM execution.

### How to Test WASM Features

1. **Build the WASM module**:
   ```bash
   cd hub-client && npm run build:wasm
   ```

2. **Create a Node.js test script** (`hub-client/test-wasm.mjs`):
   ```javascript
   import { readFile } from 'fs/promises';
   import { dirname, join } from 'path';
   import { fileURLToPath } from 'url';

   const __dirname = dirname(fileURLToPath(import.meta.url));

   // Import from the built pkg directory
   const wasm = await import('./node_modules/wasm-quarto-hub-client/wasm_quarto_hub_client.js');
   const wasmPath = join(__dirname, 'node_modules/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm');
   const wasmBytes = await readFile(wasmPath);
   await wasm.default(wasmBytes);

   // Test your feature
   const content = '# Hello\n\nWorld';
   const result = JSON.parse(wasm.render_qmd_content(content, ''));
   console.log('Success:', result.success);
   console.log('HTML:', result.html);

   // Verify expected output
   if (!result.html.includes('data-loc')) {
     console.error('FAIL: Expected data-loc attributes in HTML');
     process.exit(1);
   }
   ```

3. **Run the test**:
   ```bash
   cd hub-client && node test-wasm.mjs
   ```

### What to Verify

For any WASM feature, the test should verify:
1. The WASM function is callable (no missing exports)
2. The function returns expected data structure
3. The actual content/behavior is correct (not just "no errors")

### DO NOT

- Claim a WASM feature is complete based only on `cargo check` or `npm run build`
- Assume TypeScript type declarations match actual WASM exports
- Test only in the browser when a Node.js test would be faster and more reliable

## Smoke-All Tests

Smoke-all test fixtures live in `crates/quarto/tests/smoke-all/`. Each `.qmd` file embeds assertions in `_quarto.tests` frontmatter. There are **three independent runners** that exercise the same fixtures through different pipelines:

### 1. Rust (native renderer)
```bash
cargo nextest run -p quarto --test smoke_all
```
Fastest (~1s). Renders via `quarto-core` directly. Runs all assertion types including `ensureHtmlElements` (CSS selectors via `scraper`), `ensureCssRegexMatches`, `ensureFileRegexMatches`, etc.

### 2. WASM Vitest (jsdom)
```bash
cd hub-client && npm run test:wasm
```
~3s. Renders via WASM module in Node.js with jsdom for HTML assertions. Runs the full smoke-all suite plus other WASM tests.

### 3. Playwright E2E (browser)
```bash
cd hub-client && npx playwright test e2e/smoke-all.spec.ts
```
~12s. Full pipeline: Automerge sync → hub server → browser → WASM render → preview iframe. Tests the complete hub-client integration.

### Writing Fixtures

Each fixture is a `.qmd` file with test assertions in frontmatter. The project must have a `_quarto.yml`. Assertions use a two-array format `[mustMatch[], mustNotMatch[]]`:

```yaml
_quarto:
  tests:
    html:
      ensureCssRegexMatches:
        - ["#170229", "my-custom-rule"]   # patterns that must appear in CSS
        - ["unwanted-pattern"]             # patterns that must NOT appear
      ensureHtmlElements:
        - ["nav#TOC", "div.callout"]       # CSS selectors that must match
        - ["div.should-not-exist"]         # selectors that must NOT match
      ensureFileRegexMatches:
        - ["pattern in HTML output"]
      noErrors: true
```

### Running a Subset

To debug a specific fixture, check the rendered output directly:
```bash
cargo run -- render crates/quarto/tests/smoke-all/path/to/doc.qmd -v
```