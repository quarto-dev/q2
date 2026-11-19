import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';

// e2e/ and src/ each carry a package.json with "type": "module", so Playwright
// loads this file and the core it imports as ESM (see e2e/run.mjs).
const here = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  testDir: path.join(here, 'tests'),
  testMatch: '**/*.e2e.ts',
  // The cross-client scenarios (GE-14) need a built q2, the hub-client embed, and chromium; they have their own config.
  testIgnore: '**/cross-client/**',
  // One VS Code at a time: each launch is heavy, and the tests share the download cache.
  workers: 1,
  fullyParallel: false,
  retries: 0,
  // The first run downloads VS Code; a launch plus a sync round trip takes a while.
  timeout: 180_000,
  expect: { timeout: 30_000 },
  reporter: [['list']],
  outputDir: path.join(here, '..', 'test-results'),
});
