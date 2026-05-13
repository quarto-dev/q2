import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
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
