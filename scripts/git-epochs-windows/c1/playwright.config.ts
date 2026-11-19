import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';

const here = path.dirname(fileURLToPath(import.meta.url));

/** GE-17 item c1: the scripted scenario against the real q2 and git. One test, no browser, no parallelism. */
export default defineConfig({
  testDir: here,
  testMatch: '**/*.e2e.ts',
  timeout: 600_000,
  expect: { timeout: 60_000 },
  workers: 1,
  fullyParallel: false,
  reporter: [['list']],
  outputDir: path.join(here, 'test-results'),
});
