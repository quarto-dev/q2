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
// `--timing` selects the checkout-reload timing test (its own config; see tests/checkout-reload.timing.ts).
const args = process.argv.slice(2);
const timing = args.includes('--timing');
// `--cross` selects the cross-client scenarios (GE-14; tests/cross-client, its own config).
const cross = args.includes('--cross');
const config = join(here, timing ? 'playwright.timing.config.ts' : cross ? 'playwright.cross-client.config.ts' : 'playwright.config.ts');
const result = spawnSync(process.execPath, [cli, 'test', '--config', config, ...args.filter((a) => a !== '--timing' && a !== '--cross')], {
  stdio: 'inherit',
  env,
});
process.exit(result.status ?? 1);
