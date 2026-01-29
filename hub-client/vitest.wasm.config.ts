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
        // Map it to the actual source directory for tests.
        '/src': path.resolve(__dirname, 'src'),
      },
    },
  }),
);
