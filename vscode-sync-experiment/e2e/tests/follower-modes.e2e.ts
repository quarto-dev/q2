/**
 * GE-9 Playwright tests: the mirror and editor-only follower modes in a real VS
 * Code, against the in-process test hub, with a headless owner standing in for
 * the project owner. Nobody watches the window: assertions read the disk, the
 * editor's rendered text, the status bar, the notification toasts, and the hub's
 * documents.
 *
 * Permanent-subset candidates (no network beyond the local hub, deterministic):
 * every test here. (GE-14 decides.) The checkout timing test is not one of them;
 * it lives in checkout-reload.timing.ts.
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import {
  EDITOR_ONLY_NO_FLIP,
  GIT_OP_EDITS_DROPPED,
  REPLACED_THIS_FILE,
} from '@quarto/quarto-git-epochs';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { git } from '../helpers/git';
import {
  HeadlessOwner,
  HeadlessPeer,
  createProjectFolder,
  ensureWasm,
  readProjectMeta,
  readText,
  sleep,
  startHub,
  tempDir,
  waitFor,
  writeProjectMeta,
} from '../helpers/hub';
import { launchVscode, type RunningVscode } from '../helpers/launch';
import { activeEditorText, openFile } from '../helpers/palette';

let hub: TestHub;
let vscode: RunningVscode | undefined;
const cleanup: (() => Promise<void>)[] = [];
const quiet = { appendLine() {} };

test.beforeAll(async () => {
  await ensureWasm();
  hub = await startHub();
});
test.afterAll(async () => {
  await hub.stop();
});
test.afterEach(async () => {
  await vscode?.close();
  vscode = undefined;
  for (const c of cleanup.splice(0).reverse()) await c().catch(() => {});
});

async function dir(prefix: string): Promise<string> {
  const d = await tempDir(prefix);
  cleanup.push(() => fs.rm(d, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
  return d;
}

async function newOwner(files: Record<string, string>): Promise<HeadlessOwner> {
  const owner = await HeadlessOwner.create(hub, files);
  cleanup.push(() => owner.stop());
  return owner;
}

const toasts = (page: Page) => page.locator('.notifications-toasts');
const statusBar = (page: Page) => page.locator('.statusbar');
const editorText = (page: Page) => activeEditorText(page);

async function typeAtEnd(page: Page, text: string): Promise<void> {
  await page.locator('.editor-instance .monaco-editor').first().click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+End' : 'Control+End');
  await page.keyboard.type(text);
}

test('a mirror with an open dirty buffer follows a flip: stranded edits are counted, the buffer and disk converge', async () => {
  const owner = await newOwner({ 'a.qmd': 'A1\n' });
  const workspace = await dir('ge9-mirror-');
  const root = await createProjectFolder(owner.indexDocId, workspace, quiet, { syncServer: hub.url });
  vscode = await launchVscode({ workspace: root, env: { QUARTO_HUB_TEST_SWAP_TIMEOUT_MS: '60000' } });
  const { page } = vscode;
  await expect(statusBar(page)).toContainText('Quarto Hub: mirror', { timeout: 60_000 });

  await openFile(page, 'a.qmd');
  await expect.poll(() => editorText(page), { timeout: 30_000 }).toContain('A1');
  const oldDoc = owner.docIdOf('a.qmd')!;

  // The new doc is not on the hub yet, so the swap stays pending while the user types.
  owner.flipPath('a.qmd', owner.createDocOffline('A2\n'));
  await owner.flush();
  await sleep(1500);
  await typeAtEnd(page, 'abc');
  const oldHandle = await hub.repo.find<{ text: string }>(oldDoc as never);
  await waitFor(() => (oldHandle.doc()?.text ?? '').includes('abc'), 'the typed text in the old doc', 30_000);

  await owner.publishOffline();
  await expect.poll(() => editorText(page), { timeout: 30_000 }).toContain('A2');
  expect(await editorText(page)).not.toContain('abc');
  await waitFor(async () => (await readText(path.join(root, 'a.qmd'))) === 'A2\n', 'the new text on disk', 30_000);
  await expect(toasts(page)).toContainText(REPLACED_THIS_FILE, { timeout: 30_000 });
  const toast = await toasts(page).innerText();
  console.log('mirror notice:', JSON.stringify(toast));
  expect(toast).toMatch(/\d+ of your recent edits stayed with the previous version\./);
  expect(oldHandle.doc()?.text).toContain('abc'); // the stranded edits stay in the old doc
});

test('a folder with no mode in project.json opens as a mirror', async () => {
  const owner = await newOwner({ 'a.qmd': 'A1\n' });
  const workspace = await dir('ge9-legacy-');
  const seedPeer = await HeadlessPeer.join(hub.url, owner.indexDocId, workspace); // writes project.json without a mode
  await seedPeer.stop();
  expect(JSON.parse((await readText(path.join(workspace, '.quarto-hub', 'project.json')))!).mode).toBeUndefined();
  vscode = await launchVscode({ workspace });
  await expect(statusBar(vscode.page)).toContainText('Quarto Hub: mirror', { timeout: 60_000 });
  owner.flipPath('a.qmd', owner.createDocFor('a.qmd', 'A2\n'));
  await owner.flush();
  await waitFor(async () => (await readText(path.join(workspace, 'a.qmd'))) === 'A2\n', 'the flip on the legacy folder', 60_000);
});

test('the moved message from another peer appears briefly in the window', async () => {
  const owner = await newOwner({ 'a.qmd': 'A1\n' });
  const workspace = await dir('ge9-moved-');
  const root = await createProjectFolder(owner.indexDocId, workspace, quiet, { syncServer: hub.url });
  vscode = await launchVscode({ workspace: root });
  const { page } = vscode;
  await openFile(page, 'a.qmd');
  await expect.poll(() => editorText(page), { timeout: 30_000 }).toContain('A1');
  // Another peer on the same doc announces itself, then leaves because of a version switch.
  // The relay through the hub starts once both sides hold the doc, so send until the window shows it.
  const shown = /Remote Tester switched versions/;
  let seen = false;
  for (let attempt = 0; attempt < 10 && !seen; attempt++) {
    owner.broadcast('a.qmd', { type: 'presence', peerId: 'remote-1', userId: 'r', userName: 'Remote Tester', userColor: '#336699', cursor: null, selection: null });
    await sleep(500);
    owner.broadcast('a.qmd', { type: 'moved', peerId: 'remote-1' });
    for (let i = 0; i < 15 && !seen; i++) {
      seen = shown.test(await statusBar(page).innerText());
      if (!seen) await sleep(100);
    }
  }
  expect(seen, 'the moved text in the status bar').toBe(true);
  // It is transient (about 3 s).
  await expect.poll(async () => shown.test(await statusBar(page).innerText()), { timeout: 15_000 }).toBe(false);
});

/** A folder that is a git repository and is bound to the project in editor-only mode. */
async function editorOnlyRepo(owner: HeadlessOwner, file: string): Promise<string> {
  const workspace = await dir('ge9-eo-');
  const root = await createProjectFolder(owner.indexDocId, workspace, quiet, { syncServer: hub.url });
  await git(root, 'init', '-q', '-b', 'main');
  await git(root, 'config', 'user.email', 'e2e@example.invalid');
  await git(root, 'config', 'user.name', 'E2E');
  await git(root, 'config', 'commit.gpgsign', 'false');
  await git(root, 'add', file);
  await git(root, 'commit', '-q', '-m', 'initial');
  const meta = (await readProjectMeta(root))!;
  await writeProjectMeta(root, { ...meta, mode: 'editor-only' });
  return root;
}

test('editor-only in a git repo: a git rewrite under a lock sends no edit, and the hub flip rebinds the buffer', async () => {
  const owner = await newOwner({ 'a.qmd': 'A1\n' });
  const root = await editorOnlyRepo(owner, 'a.qmd');
  vscode = await launchVscode({ workspace: root });
  const { page } = vscode;
  await expect(statusBar(page)).toContainText('Quarto Hub: editor only', { timeout: 60_000 });
  await openFile(page, 'a.qmd');
  await expect.poll(() => editorText(page), { timeout: 30_000 }).toContain('A1');
  // The extension keeps its own folder out of `git status`.
  await waitFor(async () => ((await readText(path.join(root, '.git', 'info', 'exclude'))) ?? '').includes('.quarto-hub/'), '.quarto-hub/ in info/exclude', 30_000);
  const oldDoc = owner.docIdOf('a.qmd')!;
  const oldHeads = (await hub.repo.find<{ text: string }>(oldDoc as never)).heads();

  // git takes its lock and rewrites the file the way a checkout does.
  await fs.writeFile(path.join(root, '.git', 'index.lock'), '');
  await sleep(1000);
  await fs.writeFile(path.join(root, 'a.qmd'), 'A2\n');
  await expect.poll(() => editorText(page), { timeout: 30_000 }).toContain('A2'); // VS Code reloaded the buffer
  await sleep(2000);
  // No splice reached the hub's doc.
  expect((await hub.repo.find<{ text: string }>(oldDoc as never)).heads()).toEqual(oldHeads);
  expect(owner.textOf('a.qmd')).toBe('A1\n');

  // git finishes; the hub (here, the owner) flips to the new version.
  await fs.rm(path.join(root, '.git', 'index.lock'));
  const newDoc = owner.createDocFor('a.qmd', 'A2\n');
  owner.flipPath('a.qmd', newDoc);
  await owner.flush();
  await sleep(3000);
  expect(await editorText(page)).toContain('A2');
  expect(await editorText(page)).not.toContain('A1');
  expect(owner.textOf('a.qmd')).toBe('A2\n'); // not duplicated, not stale

  // Editing resumes on the new doc.
  await typeAtEnd(page, 'typed');
  await waitFor(() => (owner.textOf('a.qmd') ?? '').includes('typed'), 'editing on the new doc', 30_000);
  console.log('editor-only: new doc text after resume:', JSON.stringify(owner.textOf('a.qmd')));
  expect(await toasts(page).innerText().catch(() => '')).not.toContain(GIT_OP_EDITS_DROPPED);
});

test('editor-only without a flip warns when the wait ends and keeps the buffer on the git text', async () => {
  const owner = await newOwner({ 'a.qmd': 'A1\n' });
  const root = await editorOnlyRepo(owner, 'a.qmd');
  vscode = await launchVscode({ workspace: root, env: { QUARTO_HUB_TEST_FLIP_WAIT_MS: '3000' } });
  const { page } = vscode;
  await expect(statusBar(page)).toContainText('Quarto Hub: editor only', { timeout: 60_000 });
  await openFile(page, 'a.qmd');
  await expect.poll(() => editorText(page), { timeout: 30_000 }).toContain('A1');

  await fs.writeFile(path.join(root, '.git', 'index.lock'), '');
  await sleep(1000);
  await fs.writeFile(path.join(root, 'a.qmd'), 'A2\n');
  await expect.poll(() => editorText(page), { timeout: 30_000 }).toContain('A2');
  await fs.rm(path.join(root, '.git', 'index.lock'));

  await expect(toasts(page)).toContainText(EDITOR_ONLY_NO_FLIP, { timeout: 30_000 });
  console.log('editor-only warning:', JSON.stringify(await toasts(page).innerText()));
  expect(await editorText(page)).toContain('A2'); // the old doc's text was never written into the buffer
  expect(await editorText(page)).not.toContain('A1');
  expect(owner.textOf('a.qmd')).toBe('A1\n');
});
