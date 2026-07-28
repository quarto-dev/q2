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
  },
});
