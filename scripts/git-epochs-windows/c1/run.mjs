/**
 * Runs the c1 scenario (GE-17) with the `source` export condition, as vscode-sync-experiment/e2e/run.mjs does.
 * Usage: node scripts/git-epochs-windows/c1/run.mjs   (env C1_OUT = transcript path, Q2_BINARY = the q2 to drive)
 */
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const cli = join(dirname(require.resolve('@playwright/test/package.json')), 'cli.js');
const env = { ...process.env, NODE_OPTIONS: [process.env.NODE_OPTIONS, '--conditions=source'].filter(Boolean).join(' ') };
const r = spawnSync(process.execPath, [cli, 'test', '--config', join(here, 'playwright.config.ts'), ...process.argv.slice(2)], { stdio: 'inherit', env });
process.exit(r.status ?? 1);
