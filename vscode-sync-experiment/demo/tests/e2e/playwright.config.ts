import { defineConfig } from '@playwright/test';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
/** The demo's end-to-end tests (DM13): real hubs, real shells, real engines, and (E7) real windows. Run by `npm run test:demo`. */
export default defineConfig({
  testDir: here,
  testMatch: '**/*.e2e.ts',
  workers: 1,
  fullyParallel: false,
  timeout: 300_000,
  expect: { timeout: 30_000 },
  reporter: [['list']],
});
