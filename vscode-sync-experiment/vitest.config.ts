import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    // Tests that start an in-process hub and several repos need room.
    testTimeout: 30_000,
  },
  // Workspace deps compile from source, as in esbuild.mjs: the unit tests need
  // no built dist/ of the other packages (`ssr` covers the server-side module graph).
  resolve: { conditions: ['source'] },
  ssr: { resolve: { conditions: ['source'], externalConditions: ['source'] } },
});
