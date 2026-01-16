import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    // Use the same environment as our app
    environment: 'node',
    // Include test files
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
    // Coverage configuration
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html'],
      include: ['src/**/*.ts', 'src/**/*.tsx'],
      exclude: ['src/**/*.test.ts', 'src/**/*.test.tsx', 'src/vite-env.d.ts'],
    },
  },
});
