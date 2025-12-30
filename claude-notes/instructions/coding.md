- Try hard to avoid "TODO" comments in the code base. If are running low on context and you do have to add it, make sure there's a beads task (even if low-priority) to track the TODO, and add the issue id to the TODO line.

## hub-client (TypeScript/React)

When making changes to `hub-client/`:

1. **After making TypeScript changes**, run preflight checks:
   ```bash
   cd hub-client && npm run preflight
   ```
   This builds WASM and type-checks with Vite-compatible settings.

2. **Type imports**: Use `import type` for type-only imports (interfaces, type aliases). Vite's esbuild transformer requires this due to `verbatimModuleSyntax: true`.
   ```typescript
   // Correct
   import { useCallback } from 'react';
   import type { RefObject } from 'react';

   // Wrong - will fail at runtime in Vite
   import { useCallback, RefObject } from 'react';
   ```

3. **Don't use plain `tsc --noEmit`** - it uses different settings and misses errors. Always use `npm run typecheck` or `npm run preflight`.

## WASM (wasm-quarto-hub-client)

When making changes to the WASM module (`crates/wasm-quarto-hub-client/`):

1. **CRITICAL: Test WASM changes with Node.js BEFORE claiming they work**. The fastest way to verify WASM behavior is with a Node.js test script, NOT by opening the browser. See `crates/wasm-quarto-hub-client/README.md` for details.

2. **Create or update `hub-client/test-wasm.mjs`** to test new WASM functionality:
   ```javascript
   import { readFile } from 'fs/promises';
   import { dirname, join } from 'path';
   import { fileURLToPath } from 'url';

   const __dirname = dirname(fileURLToPath(import.meta.url));
   const wasmPath = join(__dirname, 'node_modules/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm');

   // Load and test the WASM module
   const wasm = await import('wasm-quarto-hub-client');
   const wasmBytes = await readFile(wasmPath);
   await wasm.default(wasmBytes);

   // Test your functionality here
   const result = wasm.render_qmd_content_with_options(content, '', '{"source_location": true}');
   console.log(JSON.parse(result));
   ```

3. **Build WASM before testing**:
   ```bash
   cd hub-client && npm run build:wasm
   ```

4. **The WASM crate is excluded from the workspace** - it has its own `Cargo.toml` dependencies. If you add a new dependency to the WASM code, you must add it to `crates/wasm-quarto-hub-client/Cargo.toml`, not just the workspace root.
