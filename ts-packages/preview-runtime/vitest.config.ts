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
      // Workspace-package aliases to source — mirrors the pattern in
      // hub-client/vitest.config.ts. Vitest doesn't honor the `source`
      // export condition on fresh clones, so workspace deps need
      // explicit aliases when no `dist/` has been built.
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
    environment: 'node',
    include: ['src/**/*.test.ts'],
    exclude: ['src/**/*.integration.test.ts'],
    passWithNoTests: true,
  },
});
