import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      // Mirror hub-client/vite.config.ts. The wasm-quarto-hub-client symlink
      // lives in hub-client/ for now; preview-runtime points at it through
      // the workspace. When the SPA is added in Phase 6 it does the same.
      'wasm-quarto-hub-client': path.resolve(
        __dirname,
        '../../hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client.js',
      ),
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
    exclude: ['src/**/*.integration.test.ts'],
    passWithNoTests: true,
  },
});
