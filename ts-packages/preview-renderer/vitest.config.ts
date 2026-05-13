import { defineConfig } from 'vitest/config';
import path from 'path';

export default defineConfig({
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      // Resolve workspace packages to source so tests work on a fresh
      // clone (no built dist required). Mirrors hub-client/vitest.config.ts.
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
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    exclude: [
      'src/**/*.integration.test.ts',
      'src/**/*.integration.test.tsx',
    ],
    passWithNoTests: true,
  },
});
