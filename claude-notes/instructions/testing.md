- **CRITICAL - TEST FIRST**: When fixing bugs using tests, you MUST run the failing test BEFORE implementing any fix. This is non-negotiable. Verify the test fails in the expected way, then implement the fix, then verify the test passes.
- Always strive for minimal test documents as small as possible. Create many small test documents instead of a few large test documents.
- You are encouraged to spend time and tokens on thinking about good tests.
- If writing tests is taking a lot of time, decompose the writing of tests into subtasks. Good tests are important!
- Precise tests are good tests. **bad**: testing for the presence of a field in an object. **good** testing if the value of the field is correct.
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