/**
 * Vitest config for WASM end-to-end tests
 *
 * These tests exercise the actual WASM module with vite's module resolution.
 * The WASM module imports from `/src/wasm-js-bridge/...` which needs special handling.
 */
import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config';
import path from 'path';

export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      // Include only WASM test files
      include: ['src/**/*.wasm.test.ts'],
      // Use node environment - WASM doesn't need DOM
      environment: 'node',
      // Pass even when no test files are found (initially)
      passWithNoTests: true,
      // Longer timeout for WASM initialization
      testTimeout: 30000,
    },
    resolve: {
      alias: {
        // The WASM JS file imports from `/src/...` which only works in vite dev server.
        // Map it to the actual source directory for tests. The
        // `/src/wasm-js-bridge` alias from `vite.config.ts` is more
        // specific and wins over this one for bridge imports (mergeConfig
        // unions both into a single object; rollup-plugin-alias matches
        // the longest prefix).
        '/src': path.resolve(__dirname, 'src'),
        '@quarto/preview-renderer': path.resolve(__dirname, '../ts-packages/preview-renderer/src'),
        '@quarto/preview-runtime': path.resolve(__dirname, '../ts-packages/preview-runtime/src'),
        '@quarto/quarto-automerge-schema': path.resolve(__dirname, '../ts-packages/quarto-automerge-schema/src/index.ts'),
        '@quarto/quarto-sync-client': path.resolve(__dirname, '../ts-packages/quarto-sync-client/src/index.ts'),
      },
    },
  }),
);
