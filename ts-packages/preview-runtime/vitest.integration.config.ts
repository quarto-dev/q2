import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      'wasm-quarto-hub-client': path.resolve(
        __dirname,
        '../../hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client.js',
      ),
      '@quarto/quarto-automerge-schema': path.resolve(
        __dirname,
        '../quarto-automerge-schema/src/index.ts',
      ),
      '@quarto/quarto-sync-client': path.resolve(
        __dirname,
        '../quarto-sync-client/src/index.ts',
      ),
      '@quarto/preview-renderer': path.resolve(
        __dirname,
        '../preview-renderer/src',
      ),
    },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['src/**/*.integration.test.ts'],
    setupFiles: ['./src/test-utils/setup.ts'],
    passWithNoTests: true,
  },
});
