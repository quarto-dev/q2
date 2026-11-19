// Smoke test: launch VS Code with this extension and run suite.cjs.
//
//   npm run test:smoke            # downloads a VS Code build (cached)
//   VSCODE_EXECUTABLE=... npm run test:smoke   # use an installed one
//
// The extension finds q2 the usual way; set Q2_SMOKE_BINARY to point the
// test workspace's q2.path at a specific build (e.g. target/debug/q2).
import { runTests } from '@vscode/test-electron';
import { mkdtempSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const extensionDevelopmentPath = resolve(here, '..');
const userDataDir = mkdtempSync(join(tmpdir(), 'q2-vscode-smoke-'));
if (process.env.Q2_SMOKE_BINARY) {
  mkdirSync(join(userDataDir, 'User'), { recursive: true });
  writeFileSync(
    join(userDataDir, 'User', 'settings.json'),
    JSON.stringify({ 'q2.path': resolve(process.env.Q2_SMOKE_BINARY) }),
  );
}

await runTests({
  extensionDevelopmentPath,
  extensionTestsPath: join(here, 'suite.cjs'),
  vscodeExecutablePath: process.env.VSCODE_EXECUTABLE || undefined,
  launchArgs: [
    join(extensionDevelopmentPath, 'test-workspace'),
    '--disable-extensions',
    `--user-data-dir=${userDataDir}`,
  ],
});
