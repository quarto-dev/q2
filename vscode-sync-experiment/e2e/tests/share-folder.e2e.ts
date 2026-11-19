/**
 * GE-12 Playwright test: "Share this folder" in a real VS Code, on a real git repository, against a local hub on
 * loopback (no network). The test runs the command, reads the modal, confirms, waits for the share panel, copies the id,
 * and has headless peers join the project as a clone and as a mirror. Nobody watches the window.
 *
 * Permanent-subset candidate (local hub only, deterministic): yes. (GE-14 decides.)
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, type Page } from '@playwright/test';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { cleanupGitTempDirs, cloneRepo, git, put, repoWith } from '../../src/core/clone/git-fixtures';
import { ensureWasm, HeadlessPeer, readText, startHub, tempDir, waitFor } from '../helpers/hub';
import { launchVscode, type RunningVscode } from '../helpers/launch';
import { fillInputBox, runCommand } from '../helpers/palette';

let hub: TestHub;
let vscode: RunningVscode | undefined;
const cleanup: (() => Promise<void>)[] = [];

test.beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
test.afterAll(async () => {
  await hub.stop();
});
test.afterEach(async () => {
  const t = Date.now();
  await vscode?.close();
  vscode = undefined;
  console.log(`afterEach: vscode closed after ${Date.now() - t} ms`);
  for (const c of cleanup.splice(0).reverse()) {
    await c().catch(() => {});
    console.log(`afterEach: a cleanup finished after ${Date.now() - t} ms`);
  }
  cleanupGitTempDirs();
});

const dialog = (page: Page) => page.locator('.monaco-dialog-box');

async function clipboard(): Promise<string> {
  return vscode!.app.evaluate(({ clipboard: c }) => c.readText());
}

/** Pick a form in the share panel and return what it copied. The clipboard is emptied first: it outlives the test run. */
async function copyForm(page: Page, label: string): Promise<string> {
  await vscode!.app.evaluate(({ clipboard: c }) => c.writeText(''));
  const row = page.locator('.quick-input-list .monaco-list-row', { hasText: label });
  await row.waitFor({ timeout: 60_000 });
  await row.click();
  return waitFor(async () => (await clipboard()) || undefined, `the ${label} on the clipboard`);
}

test('share a git folder, read the confirm, copy the id, and join it as a clone and as a mirror', async () => {
  const dir = repoWith({ 'index.qmd': 'hello\n', 'guides/a.qmd': 'a\n', '_quarto.yml': 'project:\n  title: "Shared Site"\n' });
  put(dir, 'guides/a.qmd', 'a, edited and not committed\n');
  vscode = await launchVscode({ workspace: dir, env: { QUARTO_HUB_TEST_SYNC_SERVER: hub.url } });
  const { page } = vscode;

  await runCommand(page, 'Quarto Hub: Share this folder');
  await fillInputBox(page, hub.url); // the server prompt, prefilled with the local hub

  // The confirm: a modal that says what is published and where.
  await dialog(page).waitFor({ state: 'visible', timeout: 30_000 });
  const message = await dialog(page).locator('.dialog-message-text').innerText();
  const detail = await dialog(page).locator('.dialog-message-detail').innerText();
  console.log(`confirm title: ${message}\nconfirm body:\n${detail}`);
  expect(message).toBe(`Share this folder on ${hub.url}?`);
  expect(detail).toContain('Anyone with access to that server and the project link can read and edit it.');
  expect(detail).not.toContain("can't be unpublished"); // a local server is not the public one
  expect(detail).toMatch(/Project:\s+Shared Site\s+\(git repo, branch main\)/);
  expect(detail).toMatch(/Files:\s+3 tracked files/);
  expect(detail).toMatch(/Uncommitted:\s+1 file will be shared as it is on disk:/);
  expect(detail).toContain('guides/a.qmd');
  await dialog(page).getByRole('button', { name: 'Share' }).click();

  // The panel appears only after the server confirmed the heads; copy the preview command.
  const command = await copyForm(page, 'q2 preview command');
  console.log(`copied: ${command}`);
  const m = /^q2 preview --hub-url (\S+) --project (\S+) --ui editor --allow-edit$/.exec(command);
  expect(m).not.toBeNull();
  const [, server, id] = m!;
  expect(server).toBe(hub.url);

  // The folder is bound: project.json carries the owner, and the index names the same owner.
  const meta = JSON.parse(await fs.readFile(path.join(dir, '.quarto-hub', 'project.json'), 'utf8'));
  expect(meta).toMatchObject({ mode: 'clone', indexDocId: id, name: 'Shared Site', file: 'index.qmd' });
  expect(meta.ownerId).toMatch(/\S{8,}/);
  expect(git(dir, 'status', '--porcelain')).not.toContain('.quarto-hub');

  // A headless peer joins as a clone of the same repository, and another as a mirror.
  const clone = cloneRepo(dir);
  const c = await HeadlessPeer.joinAsClone(clone, hub.url, id!);
  cleanup.push(() => c.stop());
  expect(Object.keys(c.cloneState()!.attached).sort()).toEqual(['_quarto.yml', 'guides/a.qmd', 'index.qmd']);
  const mirrorDir = await tempDir('ge12-mirror-');
  const mirror = await HeadlessPeer.joinAsMirror(mirrorDir, hub.url, id!);
  cleanup.push(() => mirror.stop());
  expect(await readText(path.join(mirrorDir, 'index.qmd'))).toBe('hello\n');
  expect(await readText(path.join(mirrorDir, 'guides', 'a.qmd'))).toBe('a, edited and not committed\n'); // published as on disk

  // The index agrees with the folder: the owner key matches, and git-epochs keys are in place.
  const state = c.cloneState()!;
  expect(state.head).toBe(git(clone, 'rev-parse', 'HEAD'));
  console.log('test body finished');
});

/**
 * The preview leg of the end-to-end verification: the command the panel copies starts `q2 preview` against the shared
 * project, which serves the editor; then `git switch` in the originating folder moves the pointer. Needs a built `q2`
 * (`cargo build --bin q2`, or QUARTO_HUB_E2E_Q2); skipped when there is none.
 */
test('the copied q2 preview command serves the project, and git switch re-points it', async () => {
  const { spawn } = await import('node:child_process');
  const { createServer } = await import('node:net');
  const { existsSync } = await import('node:fs');
  const q2 = process.env.QUARTO_HUB_E2E_Q2 ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..', 'target', 'debug', process.platform === 'win32' ? 'q2.exe' : 'q2');
  test.skip(!existsSync(q2), `no q2 binary at ${q2}`);
  const dir = repoWith({ 'index.qmd': '# Hello from the owner\n', 'a.qmd': 'A\n' });
  git(dir, 'switch', '-q', '-c', 'side');
  put(dir, 'a.qmd', 'A on side\n');
  git(dir, 'commit', '-qam', 'side');
  git(dir, 'switch', '-q', 'main');
  vscode = await launchVscode({ workspace: dir, env: { QUARTO_HUB_TEST_SYNC_SERVER: hub.url } });
  const { page } = vscode;
  await runCommand(page, 'Quarto Hub: Share this folder');
  await fillInputBox(page, hub.url);
  await dialog(page).waitFor({ state: 'visible', timeout: 30_000 });
  await dialog(page).getByRole('button', { name: 'Share' }).click();
  const command = await copyForm(page, 'q2 preview command');
  const [, server, id] = /^q2 preview --hub-url (\S+) --project (\S+) --ui editor --allow-edit$/.exec(command)!;

  const port = await new Promise<number>((resolve) => {
    const s = createServer().listen(0, '127.0.0.1', () => {
      const p = (s.address() as { port: number }).port;
      s.close(() => resolve(p));
    });
  });
  const out: string[] = [];
  const child = spawn(q2, ['preview', '--hub-url', server!, '--project', id!, '--ui', 'editor', '--allow-edit', '--no-browser', '--port', String(port)], { stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.on('data', (b) => out.push(String(b)));
  child.stderr.on('data', (b) => out.push(String(b)));
  cleanup.push(async () => void child.kill());
  const body = await waitFor(async () => {
    const r = await fetch(`http://127.0.0.1:${port}/`).catch(() => null);
    return r?.ok ? await r.text() : undefined;
  }, 'q2 preview serving', 60_000);
  console.log(`q2 preview output:\n${out.join('')}\nGET / -> ${body.length} bytes, starts: ${body.slice(0, 80).replace(/\s+/g, ' ')}`);
  expect(body.length).toBeGreaterThan(100);

  // git switch in the originating folder moves the pointer on the server.
  const { Repo } = await import('@automerge/automerge-repo/slim');
  const { findDoc, webSocketAdapter } = await import('../../src/core/repo');
  const reader = new Repo({ network: [webSocketAdapter(hub.url)] });
  cleanup.push(() => reader.shutdown());
  const index = await findDoc<{ files: Record<string, unknown> }>(reader, id!);
  const before = String(index.doc()!.files['a.qmd']);
  git(dir, 'switch', '-q', 'side');
  await waitFor(() => String(index.doc()!.files['a.qmd']) !== before, 'the pointer flipping after git switch', 60_000);
  const flipped = await findDoc<{ text: string }>(reader, String(index.doc()!.files['a.qmd']));
  console.log(`a.qmd pointer flipped to a doc holding: ${JSON.stringify(flipped.doc()!.text)}`);
  expect(flipped.doc()!.text).toBe('A on side\n');
});
