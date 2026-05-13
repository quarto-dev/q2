import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config';
import path from 'path';

// q2-preview-spa unit-test config. Inherits `/src/wasm-js-bridge` +
// `wasm-quarto-hub-client` aliases from vite.config.ts via mergeConfig
// (those need to work at vite-build time too, so they live there).
//
// The workspace-package aliases below are *test-only*: vitest doesn't
// honor the `source` exports condition on fresh clones the way Vite's
// prod build does, so we point at source explicitly. Same pattern as
// hub-client and preview-renderer.
export default mergeConfig(
  viteConfig,
  defineConfig({
    resolve: {
      alias: {
        '@quarto/preview-renderer': path.resolve(__dirname, '../ts-packages/preview-renderer/src'),
        '@quarto/preview-runtime': path.resolve(__dirname, '../ts-packages/preview-runtime/src'),
        '@quarto/quarto-automerge-schema': path.resolve(__dirname, '../ts-packages/quarto-automerge-schema/src/index.ts'),
        '@quarto/quarto-sync-client': path.resolve(__dirname, '../ts-packages/quarto-sync-client/src/index.ts'),
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
  }),
);
