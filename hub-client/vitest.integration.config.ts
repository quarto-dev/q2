import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    // Use jsdom for DOM APIs and React component testing
    environment: 'jsdom',
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
});
