import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    exclude: ['dist/**', 'node_modules/**'],
    // globalSetup: build the wasm-qmd-parser nodejs pkg once when missing
    // or stale (main process). setupFiles: point QUARTO_QMD_PARSER_SPEC at
    // it in every worker (env set in globalSetup does not propagate). The
    // package is a build artifact, never an npm dependency (CAP-11).
    globalSetup: ['./scripts/vitest-global-setup.mjs'],
    setupFiles: ['./src/test-setup.ts'],
  },
});
