import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config';
import path from 'path';

export default mergeConfig(
  viteConfig,
  defineConfig({
    resolve: {
      alias: {
        '@quarto/quarto-automerge-schema': path.resolve(__dirname, '../ts-packages/quarto-automerge-schema/src/index.ts'),
        '@quarto/quarto-sync-client': path.resolve(__dirname, '../ts-packages/quarto-sync-client/src/index.ts'),
      },
    },
    test: {
      environment: 'node',
      include: ['src/**/*.bench.ts'],
      passWithNoTests: false,
      // Disable test timeouts at the config level — bench-level
      // `timeout` overrides set per-it take precedence anyway.
      testTimeout: 600_000,
    },
  }),
);
