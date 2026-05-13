import { defineConfig } from 'vitest/config';

export default defineConfig({
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
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
});
