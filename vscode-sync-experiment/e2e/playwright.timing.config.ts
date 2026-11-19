import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@playwright/test';
import base from './playwright.config';

const here = path.dirname(fileURLToPath(import.meta.url));

/** The checkout-reload timing test: its own script (`npm run test:e2e:timing`), not part of the normal run. */
export default defineConfig({
  ...base,
  testDir: path.join(here, 'tests'),
  testMatch: '**/*.timing.ts',
  timeout: 300_000,
});
