import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    // Tests that start an in-process hub and several repos need room.
    testTimeout: 30_000,
  },
  resolve: {
    // Workspace deps compile from source, as in esbuild.mjs.
    conditions: ['source'],
  },
});
