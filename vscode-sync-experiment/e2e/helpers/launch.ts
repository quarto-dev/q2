/**
 * Launch a real VS Code with the extension under development, driven by
 * Playwright's Electron support (`_electron.launch`), and give the test a
 * `Page` for the workbench window.
 *
 * VS Code is downloaded once by `@vscode/test-electron` and cached under
 * `.vscode-test/` (pinned version below). Each launch gets a throwaway
 * user-data dir and extensions dir.
 */
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { downloadAndUnzipVSCode } from '@vscode/test-electron';
import { _electron, type ElectronApplication, type Page } from '@playwright/test';

const here = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(here, '..', '..');

/**
 * The VS Code the smoke tests run on. Bump deliberately. It must ship Node 22 or
 * later: the extension's sync client uses the global `WebSocket`, which Node 20
 * (VS Code 1.96, for one) lacks, so sync fails there with "WebSocket is not defined"
 * even though engines.vscode says ^1.90.0 (found in GE-8; see the plan's notes).
 */
export const VSCODE_VERSION = process.env.QUARTO_HUB_E2E_VSCODE_VERSION ?? '1.101.2';

export interface LaunchOptions {
  /** The folder to open as the workspace. */
  workspace: string;
  /** Extra environment for VS Code (and so for the extension host). */
  env?: Record<string, string>;
}

export interface RunningVscode {
  app: ElectronApplication;
  page: Page;
  userDataDir: string;
  /** Close VS Code and delete its throwaway directories. */
  close(): Promise<void>;
}

/** The path of the VS Code executable, downloading it on first use. */
export async function vscodeExecutable(): Promise<string> {
  return downloadAndUnzipVSCode({ version: VSCODE_VERSION, cachePath: path.join(extensionRoot, '.vscode-test') });
}

export async function launchVscode(options: LaunchOptions): Promise<RunningVscode> {
  const executablePath = await vscodeExecutable();
  // Short paths: macOS limits unix socket paths to about 100 characters.
  const base = process.platform === 'win32' ? os.tmpdir() : '/tmp';
  const userDataDir = await fs.mkdtemp(path.join(base, 'qhv-ud-'));
  const extensionsDir = await fs.mkdtemp(path.join(base, 'qhv-ex-'));
  await fs.mkdir(path.join(userDataDir, 'User'), { recursive: true });
  await fs.writeFile(
    path.join(userDataDir, 'User', 'settings.json'),
    JSON.stringify(
      {
        // Playwright cannot click native modal dialogs; GE-11 and GE-12 use modal confirms.
        'window.dialogStyle': 'custom',
        'workbench.startupEditor': 'none',
        'workbench.tips.enabled': false,
        'security.workspace.trust.enabled': false,
        'update.mode': 'none',
        'telemetry.telemetryLevel': 'off',
        'extensions.autoUpdate': false,
        'extensions.autoCheckUpdates': false,
        'files.autoSave': 'afterDelay',
        'files.autoSaveDelay': 300,
        'editor.formatOnSave': false,
      },
      null,
      2,
    ),
  );
  const app = await _electron.launch({
    executablePath,
    args: [
      `--extensionDevelopmentPath=${extensionRoot}`,
      `--user-data-dir=${userDataDir}`,
      `--extensions-dir=${extensionsDir}`,
      '--disable-workspace-trust',
      '--skip-release-notes',
      '--skip-welcome',
      '--disable-updates',
      '--disable-extensions',
      // Containers: no sandbox helper, no GPU.
      '--no-sandbox',
      '--disable-gpu',
      options.workspace,
    ],
    // NODE_OPTIONS belongs to the test runner (see playwright.config.ts), not to VS Code.
    env: { ...(process.env as Record<string, string>), NODE_OPTIONS: '', ...options.env },
  });
  const page = await app.firstWindow();
  await page.waitForSelector('.monaco-workbench', { timeout: 60_000 });
  return {
    app,
    page,
    userDataDir,
    async close() {
      await app.close().catch(() => {});
      await fs.rm(userDataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }).catch(() => {});
      await fs.rm(extensionsDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }).catch(() => {});
    },
  };
}
