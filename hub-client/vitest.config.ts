import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config';
import path from 'path';

export default mergeConfig(
  viteConfig,
  defineConfig({
    resolve: {
      // Explicit aliases for workspace packages to resolve to source
      // This ensures tests work on fresh clones without building packages first
      alias: {
        '@quarto/quarto-automerge-schema': path.resolve(__dirname, '../ts-packages/quarto-automerge-schema/src/index.ts'),
        '@quarto/quarto-sync-client': path.resolve(__dirname, '../ts-packages/quarto-sync-client/src/index.ts'),
      },
    },
    test: {
      // Use node environment for unit tests (fast, no DOM)
      environment: 'node',
      // Include unit test files, exclude integration and WASM tests
      include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
      exclude: [
        'src/**/*.integration.test.ts',
        'src/**/*.integration.test.tsx',
        'src/**/*.wasm.test.ts',
      ],
      // Pass even when no test files are found
      passWithNoTests: true,
      // Coverage configuration
      coverage: {
        provider: 'v8',
        reporter: ['text', 'html'],
        include: ['src/**/*.ts', 'src/**/*.tsx'],
        exclude: [
          'src/**/*.test.ts',
          'src/**/*.test.tsx',
          'src/**/*.integration.test.ts',
          'src/**/*.integration.test.tsx',
          'src/vite-env.d.ts',
          'src/test-utils/**',
          'src/__mocks__/**',
        ],
      },
    },
  }),
);
