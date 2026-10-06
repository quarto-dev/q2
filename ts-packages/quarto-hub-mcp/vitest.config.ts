import * as path from 'node:path';
import { pathToFileURL } from 'node:url';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    exclude: ['dist/**', 'node_modules/**'],
    env: {
      // local-render loads the prebundled WASM host next to itself at
      // runtime (dist/), but under vitest import.meta.url points into
      // src/ — steer it at the build artifact. `npm run build` must
      // have run (the live tests already require dist/index.js).
      QUARTO_HUB_MCP_WASM_HOST: pathToFileURL(
        path.resolve(__dirname, 'dist/wasm-host.mjs'),
      ).href,
    },
    // globalSetup: build the wasm-qmd-parser nodejs pkg once when missing
    // or stale (main process). setupFiles: point QUARTO_QMD_PARSER_SPEC at
    // it in every worker (env set in globalSetup does not propagate). The
    // package is a build artifact, never an npm dependency (CAP-11).
    globalSetup: ['./scripts/vitest-global-setup.mjs'],
    setupFiles: ['./src/test-setup.ts'],
  },
});
