/**
 * Run the Playwright smoke tests with the `source` export condition.
 *
 * Workspace packages publish `source` (TypeScript) and `import` (ESM dist)
 * entries only, and Playwright loads test files as CommonJS, so the tests need
 * Node to resolve `source`, as esbuild.mjs and vitest do. The flag has to be on
 * the process that discovers the tests, so it is set here (portably, unlike an
 * inline `NODE_OPTIONS=` in an npm script) before Playwright starts.
 */
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const cli = join(dirname(require.resolve('@playwright/test/package.json')), 'cli.js');
const env = { ...process.env, NODE_OPTIONS: [process.env.NODE_OPTIONS, '--conditions=source'].filter(Boolean).join(' ') };
const result = spawnSync(process.execPath, [cli, 'test', '--config', join(here, 'playwright.config.ts'), ...process.argv.slice(2)], {
  stdio: 'inherit',
  env,
});
process.exit(result.status ?? 1);
