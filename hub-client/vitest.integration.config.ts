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
      // Use jsdom for DOM APIs and React component testing
      environment: 'jsdom',
      // Enable globals like expect, describe, it for jest-dom compatibility
      globals: true,
      // Only include integration test files
      include: ['src/**/*.integration.test.ts', 'src/**/*.integration.test.tsx'],
      // Setup file for DOM polyfills and test utilities
      setupFiles: ['./src/test-utils/setup.ts'],
      // Inline problematic dependencies
      deps: {
        inline: ['@monaco-editor/react'],
      },
      // Pass even when no test files are found (initially)
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
