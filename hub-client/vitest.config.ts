import { defineConfig } from 'vitest/config';

export default defineConfig({
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
});
