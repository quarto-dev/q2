import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      '@quarto/quarto-automerge-schema': path.resolve(
        __dirname,
        '../quarto-automerge-schema/src/index.ts',
      ),
      '@quarto/quarto-sync-client': path.resolve(
        __dirname,
        '../quarto-sync-client/src/index.ts',
      ),
      '@quarto/preview-runtime': path.resolve(
        __dirname,
        '../preview-runtime/src',
      ),
      // Integration tests pull in preview-runtime through
      // iframePostProcessor / assetWalker, which transitively imports
      // the wasm-quarto-hub-client glue. The actual WASM module isn't
      // needed for these tests (the dispatch functions are mocked or
      // never called), but the import must still resolve. Point at the
      // hub-client symlink so the JS shim loads.
      'wasm-quarto-hub-client': path.resolve(
        __dirname,
        '../../hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client.js',
      ),
      // wasmRenderer.ts dynamically imports the sass bridge via the
      // Vite-root path `/src/wasm-js-bridge/sass.js`. In test runs the
      // bridge isn't invoked (no `initWasm()`), but Vite still resolves
      // the import statement at transform time, so the path must
      // resolve to *something*. As of A.0 the bridge lives in
      // `@quarto/wasm-js-bridge`; alias the sub-tree (NOT plain
      // `/src`, which would also intercept test files' own absolute
      // paths).
      '/src/wasm-js-bridge': path.resolve(
        __dirname,
        '../wasm-js-bridge/src',
      ),
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    include: [
      'src/**/*.integration.test.ts',
      'src/**/*.integration.test.tsx',
    ],
    setupFiles: ['./src/test-utils/setup.ts'],
    passWithNoTests: true,
  },
});
