/**
 * GE-11 Playwright tests: Join session and Leave session in a real VS Code against a real `q2 preview --allow-edit` on
 * loopback (no network). The ticket case uses a stub `q2` that forwards TCP to that hub, as the real proxy would.
 *
 * Permanent-subset candidates (local only, deterministic): every test here. (GE-14 decides.)
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { pidAlive, writeStubQ2 } from '../../src/core/stub-q2';
import { ensureWasm, tempDir, waitFor } from '../helpers/hub';
import { launchVscode, type RunningVscode } from '../helpers/launch';
import { fillInputBox, runCommand } from '../helpers/palette';
import { startQ2Hub, type Q2Hub } from '../helpers/q2-hub';

const TICKET = 'q2previewE2eSecretTicket-0123456789';
let vscode: RunningVscode | undefined;
let hub: Q2Hub | undefined;
const dirs: string[] = [];

test.beforeAll(async () => {
  await ensureWasm();
});
test.afterEach(async () => {
  await vscode?.close();
  vscode = undefined;
  await hub?.stop();
  hub = undefined;
  for (const d of dirs.splice(0)) await fs.rm(d, { recursive: true, force: true }).catch(() => {});
});

const statusBar = (page: Page) => page.locator('.statusbar');
const folder = async (prefix: string) => {
  const d = await fs.realpath(await tempDir(prefix));
  dirs.push(d);
  await fs.writeFile(path.join(d, '_quarto.yml'), 'project:\n  type: default\n');
  await fs.writeFile(path.join(d, 'index.qmd'), '# Hello\n');
  return d;
};
const readMeta = async (root: string) => JSON.parse(await fs.readFile(path.join(root, '.quarto-hub', 'project.json'), 'utf8'));
/** On failure, print what the extension logged: the output channel is a log file under the user-data directory. */
async function dumpLogs(v: RunningVscode, dir?: string): Promise<void> {
  const scan = async (d: string): Promise<void> => {
    for (const e of await fs.readdir(d, { withFileTypes: true }).catch(() => [])) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) await scan(p);
      else if (/Quarto Hub/.test(e.name)) console.log(`--- ${p}\n${await fs.readFile(p, 'utf8')}`);
    }
  };
  await scan(path.join(v.userDataDir, 'logs'));
  if (dir) console.log(`--- project.json\n${await fs.readFile(path.join(dir, '.quarto-hub', 'project.json'), 'utf8').catch(() => '(missing)')}`);
}
const pickerItems = (page: Page) => page.locator('.quick-input-list .monaco-list-row');

async function join(page: Page, input: string): Promise<void> {
  await runCommand(page, 'Quarto Hub: Join Session');
  await fillInputBox(page, input);
  await pickerItems(page).first().waitFor({ timeout: 60_000 });
}
const pick = (page: Page, label: string) => pickerItems(page).filter({ hasText: label }).first().click();
const confirm = (page: Page, label: string) => page.locator('.dialog-buttons .monaco-button', { hasText: label }).first().click();

test('(a) an http address of the hub\'s own directory offers Editor only first, with a confirm', async () => {
  const dir = await folder('ge11-own-');
  hub = await startQ2Hub(dir);
  vscode = await launchVscode({ workspace: dir });
  const { page } = vscode;
  await join(page, hub.httpUrl + '/');
  const labels = await pickerItems(page).allInnerTexts();
  expect(labels[0]).toContain('Editor only');
  expect(labels[1]).toContain('Create a mirror folder');
  expect(labels.join('\n')).not.toContain('Attach');
  await pick(page, 'Editor only');
  await confirm(page, 'Use as editor only');
  await expect(statusBar(page)).toContainText('Quarto Hub: editor only', { timeout: 60_000 }).catch(async (e) => {
    await dumpLogs(vscode!, dir);
    console.log('--- hub log\n' + hub!.log());
    throw e;
  });
  expect(await readMeta(dir)).toMatchObject({ mode: 'editor-only', indexDocId: hub.indexDocId, syncServer: hub.wsUrl });
});

test('(b) a ws URL from another folder creates a mirror inside the workspace folder and opens it', async () => {
  const hubDir = await folder('ge11-hubdir-');
  const work = await folder('ge11-work-');
  hub = await startQ2Hub(hubDir);
  vscode = await launchVscode({ workspace: work });
  const { page } = vscode;
  await join(page, hub.wsUrl);
  expect(await pickerItems(page).allInnerTexts()).toHaveLength(1);
  await pick(page, 'Create a mirror folder');
  const mirror = await waitFor(async () => {
    const entries = await fs.readdir(work, { withFileTypes: true });
    for (const e of entries) {
      if (e.isDirectory() && (await fs.stat(path.join(work, e.name, '.quarto-hub', 'project.json')).catch(() => null))) return path.join(work, e.name);
    }
    return null;
  }, 'the mirror folder', 60_000);
  expect(await readMeta(mirror)).toMatchObject({ mode: 'mirror', indexDocId: hub.indexDocId, syncServer: hub.wsUrl });
  expect(await fs.readFile(path.join(mirror, 'index.qmd'), 'utf8')).toBe('# Hello\n');
  await expect(statusBar(page)).toContainText('Quarto Hub: mirror', { timeout: 60_000 });
});

test('(c) and (e) a ticket joins through the spawned proxy, leaves no ticket on disk, and Leave ends the proxy', async () => {
  const dir = await folder('ge11-ticket-');
  hub = await startQ2Hub(dir);
  const stubDir = await tempDir('ge11-stub-');
  dirs.push(stubDir);
  const stub = await writeStubQ2(stubDir);
  vscode = await launchVscode({
    workspace: dir,
    settings: { 'quartoHub.q2Path': stub.q2Path },
    env: stub.env('forward', { STUB_Q2_TARGET: String(hub.port) }),
  });
  const { page } = vscode;
  await join(page, TICKET);
  // The ticket kind is remote-looking and the hub's root matches the folder: Mirror first, Editor only last.
  const labels = await pickerItems(page).allInnerTexts();
  expect(labels[0]).toContain('Create a mirror folder');
  expect(labels[1]).toContain('Editor only');
  await pick(page, 'Editor only');
  await confirm(page, 'Use as editor only');
  await expect(statusBar(page)).toContainText('Quarto Hub: editor only', { timeout: 60_000 });
  const meta = await readMeta(dir);
  const args: string[] = JSON.parse(await fs.readFile(path.join(stubDir, 'args.json'), 'utf8'));
  const port = args[args.indexOf('--port') + 1];
  expect(meta.syncServer).toBe(`ws://127.0.0.1:${port}/ws`);
  expect(args).toEqual(expect.arrayContaining(['--join', TICKET, '--no-browser', '--exit-on-stdin-close']));
  // No ticket in project.json or anywhere VS Code logged under its user-data directory (the output channel is a log file there).
  expect(JSON.stringify(meta)).not.toContain(TICKET);
  const hits: string[] = [];
  const scan = async (d: string): Promise<void> => {
    for (const e of await fs.readdir(d, { withFileTypes: true }).catch(() => [])) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) await scan(p);
      else {
        const text = await fs.readFile(p, 'utf8').catch(() => '');
        if (text.includes(TICKET)) hits.push(p);
      }
    }
  };
  await scan(path.join(vscode.userDataDir, 'logs'));
  expect(hits).toEqual([]);

  const pid = Number(await fs.readFile(path.join(stubDir, 'pid'), 'utf8'));
  expect(pidAlive(pid)).toBe(true);
  await runCommand(page, 'Quarto Hub: Leave Session');
  await waitFor(() => !pidAlive(pid), 'the stub proxy to exit', 30_000);
  await expect(statusBar(page)).not.toContainText('Quarto Hub: editor only');
});

test('(d) a bound folder is offered rebind first', async () => {
  const hubDir = await folder('ge11-hubdir2-');
  const work = await folder('ge11-bound-');
  hub = await startQ2Hub(hubDir);
  await fs.mkdir(path.join(work, '.quarto-hub'), { recursive: true });
  await fs.writeFile(
    path.join(work, '.quarto-hub', 'project.json'),
    JSON.stringify({ indexDocId: hub.indexDocId, syncServer: hub.wsUrl, createdAt: new Date().toISOString(), mode: 'mirror' }),
  );
  vscode = await launchVscode({ workspace: work });
  const { page } = vscode;
  await join(page, hub.httpUrl);
  const labels = await pickerItems(page).allInnerTexts();
  expect(labels[0]).toContain('Rebind this folder to the new session');
  expect(labels[1]).toContain('Create a mirror folder');
  await page.keyboard.press('Escape');
});
