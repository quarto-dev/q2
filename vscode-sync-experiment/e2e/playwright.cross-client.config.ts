import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';
import base from './playwright.config';

const here = path.dirname(fileURLToPath(import.meta.url));

/**
 * The cross-client scenarios (GE-14): a real `q2 preview` hub, a chromium guest, headless peers, and (for the hub-less
 * demo) a real VS Code window. `npm run test:e2e:cross` runs the permanent subset (tests tagged `@permanent`); pass
 * `GE14_ALL=1` to include the exploratory ones. Needs `target/debug/q2` built with the hub-client embed
 * (`cargo xtask build-hub-client-embed` before `cargo build --bin q2`) and Playwright's chromium.
 */
export default defineConfig({
  ...base,
  testDir: path.join(here, 'tests', 'cross-client'),
  testMatch: '**/*.e2e.ts',
  testIgnore: undefined,
  // Playwright ANDs a command-line --grep with this one, so the exploratory tests are opted in by environment.
  grep: process.env.GE14_ALL === '1' ? undefined : /@permanent/,
  timeout: 240_000,
  expect: { timeout: 30_000 },
});
