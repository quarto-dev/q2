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
      include: ['src/**/*.wasm.test.{ts,tsx}'],
      // Use node environment - WASM doesn't need DOM
      environment: 'node',
      // pandoc.wasm needs the WebAssembly exnref proposal; Node 24 only takes the flag
      // here (NODE_OPTIONS rejects it). V8 flags are not reliable under threads, so the
      // fork pool is required (src/pandoc/pandocHost.wasm.test.ts asserts both).
      pool: 'forks',
      execArgv: ['--experimental-wasm-exnref'],
      // Pass even when no test files are found (initially)
      passWithNoTests: true,
      // Hang detection only — deliberately loose (5-10x typical duration),
      // NOT a performance budget. The smoke-all sweep runs every fixture in
      // one `it` and legitimately takes ~25s on slow CI runners; a 30s
      // timeout flaked on main (2026-07-30). If this trips, something is
      // wedged, not slow. See
      // claude-notes/research/2026-07-30-test-timeouts-are-hang-detection.md
      testTimeout: 120000,
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
        '@quarto/pandoc-host': path.resolve(__dirname, '../ts-packages/pandoc-host/src/index.ts'),
        // A directory alias, so `@quarto/typst-host/fontBundle` resolves too (the main thread imports only that).
        '@quarto/typst-host': path.resolve(__dirname, '../ts-packages/typst-host/src'),
        '@quarto/preview-renderer': path.resolve(__dirname, '../ts-packages/preview-renderer/src'),
        '@quarto/preview-runtime': path.resolve(__dirname, '../ts-packages/preview-runtime/src'),
        '@quarto/quarto-automerge-schema': path.resolve(__dirname, '../ts-packages/quarto-automerge-schema/src/index.ts'),
        '@quarto/quarto-sync-client': path.resolve(__dirname, '../ts-packages/quarto-sync-client/src/index.ts'),
      },
    },
  }),
);
