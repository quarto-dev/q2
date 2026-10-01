// Runs spike tests under hub-client's wasm vitest config (so /src/wasm-js-bridge etc. resolve).
//   cd hub-client && npx vitest run --config ../claude-notes/plans/2026-10-01-pandoc-wasm/spike/vitest.spike.config.ts
import { defineConfig, mergeConfig } from 'vitest/config';
import base from '../../../../hub-client/vitest.wasm.config';
import path from 'path';
const hub = path.resolve(__dirname, '../../../../hub-client');
export default mergeConfig(base, defineConfig({
  root: hub,
  test: { include: [path.resolve(__dirname, '*.spike.test.ts')], testTimeout: 300000 },
  server: { fs: { allow: [path.resolve(__dirname, '../../../..')] } },
}));
