import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';
import base from './playwright.config';

const here = path.dirname(fileURLToPath(import.meta.url));

/**
 * The cross-client scenarios (GE-14): a real `q2 preview` hub, a chromium guest, headless peers, and (for the hub-less
 * demo) a real VS Code window. `npm run test:e2e:cross` runs the permanent subset (tests tagged `@permanent`); pass
 * `--grep @exploratory` for the exploratory ones. Needs `target/debug/q2` built with the hub-client embed
 * (`cargo xtask build-hub-client-embed` before `cargo build --bin q2`) and Playwright's chromium.
 */
export default defineConfig({
  ...base,
  testDir: path.join(here, 'tests', 'cross-client'),
  testMatch: '**/*.e2e.ts',
  testIgnore: undefined,
  grep: /@permanent/,
  timeout: 240_000,
  expect: { timeout: 30_000 },
});
