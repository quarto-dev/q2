/** `npm run test:demo`: the demo's Playwright tests, with the `source` export condition (see e2e/run.mjs). */
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const cli = join(dirname(require.resolve('@playwright/test/package.json')), 'cli.js');
const env = { ...process.env, NODE_OPTIONS: [process.env.NODE_OPTIONS, '--conditions=source'].filter(Boolean).join(' ') };
const r = spawnSync(process.execPath, [cli, 'test', '--config', join(here, 'tests', 'e2e', 'playwright.config.ts'), ...process.argv.slice(2)], { stdio: 'inherit', env });
process.exit(r.status ?? 1);
