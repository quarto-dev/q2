import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config';
import path from 'path';

// Integration tests: jsdom + jest-dom + fake-indexeddb. Workspace
// aliases below as in vitest.config.ts.
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
      environment: 'jsdom',
      globals: true,
      include: [
        'src/**/*.integration.test.ts',
        'src/**/*.integration.test.tsx',
      ],
      setupFiles: ['./src/test-utils/setup.ts'],
      passWithNoTests: true,
    },
  }),
);
