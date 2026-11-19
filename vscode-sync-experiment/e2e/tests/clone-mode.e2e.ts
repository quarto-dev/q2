/**
 * GE-10 Playwright tests: clone mode in a real VS Code, a real git repository, and the in-process test hub. A project
 * is published with the headless peer's `publishGit`; the workspace is a clone of the same repository. Nobody watches
 * the window: assertions read the disk, the status bar, the toasts, the sidecar, and the hub's docs.
 *
 * Permanent-subset candidates (local hub only, deterministic): every test here. (GE-14 decides.)
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { FILES_DETACHED, FILES_JOINED_LIVE, formatNotice } from '@quarto/quarto-git-epochs';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { OtherPeer } from '../../src/core/clone/clone-harness';
import { cleanupGitTempDirs, cloneRepo, commitAll, git, put, repoWith } from '../../src/core/clone/git-fixtures';
import { publishGit, type GitPublication } from '../../src/headless/publish-git';
import { findDoc } from '../../src/core/repo';
import { ensureWasm, readText, startHub, waitFor } from '../helpers/hub';
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
  await vscode?.close();
  vscode = undefined;
  for (const c of cleanup.splice(0).reverse()) await c().catch(() => {});
  cleanupGitTempDirs();
});

const toasts = (page: Page) => page.locator('.notifications-toasts');
const statusBar = (page: Page) => page.locator('.statusbar');

interface World {
  origin: string;
  clone: string;
  pub: GitPublication;
  other: OtherPeer;
}

/** A published git project, a clone with a `side` branch (a.qmd changed), and a second peer on the hub. */
async function world(): Promise<World> {
  const origin = repoWith({ 'a.qmd': 'A\n', 'b.qmd': 'B\n' });
  const pub = await publishGit(origin, hub.url, hub);
  const clone = cloneRepo(origin);
  git(clone, 'switch', '-q', '-c', 'side');
  put(clone, 'a.qmd', 'A2\n');
  commitAll(clone, 'a2');
  git(clone, 'switch', '-q', 'main');
  const other = new OtherPeer(hub);
  await other.open(pub.indexDocId);
  cleanup.push(() => other.stop(), () => pub.stop());
  return { origin, clone, pub, other };
}

const open = async (workspace: string) =>
  (vscode = await launchVscode({ workspace, env: { QUARTO_HUB_TEST_SYNC_SERVER: hub.url } }));

async function attach(page: Page, indexDocId: string): Promise<void> {
  await runCommand(page, 'Quarto Hub: Attach Folder to Project');
  await fillInputBox(page, indexDocId);
}

/** On a failed expectation, print what is on disk: the debugging aid for a window nobody watches. */
async function dump(clone: string): Promise<void> {
  for (const f of ['a.qmd', '.quarto-hub/git-epochs.json', '.quarto-hub/events.jsonl']) {
    console.log(`--- ${f}\n${(await readText(path.join(clone, f))) ?? '(missing)'}`);
  }
}

const sideBlob = (clone: string, rel: string) => git(clone, 'rev-parse', `side:${rel}`);

test('attach succeeds and the status bar says clone', async () => {
  const w = await world();
  const { page } = await open(w.clone);
  await attach(page, w.pub.indexDocId);
  await expect(statusBar(page)).toContainText('Quarto Hub: clone', { timeout: 60_000 });
  const meta = JSON.parse(await fs.readFile(path.join(w.clone, '.quarto-hub', 'project.json'), 'utf8'));
  expect(meta).toMatchObject({ mode: 'clone', indexDocId: w.pub.indexDocId });
  expect(meta.repo.roots.length).toBeGreaterThan(0);
  expect(git(w.clone, 'status', '--porcelain')).not.toContain('.quarto-hub');
});

test('attach to an unrelated repository is refused and a mirror is offered', async () => {
  const w = await world();
  const unrelated = repoWith({ 'index.qmd': 'x\n' }, 'unrelated history');
  const { page } = await open(unrelated);
  await attach(page, w.pub.indexDocId);
  await expect(toasts(page)).toContainText('does not share a root commit', { timeout: 30_000 });
  await expect(toasts(page).getByRole('button', { name: 'Create a mirror folder instead' })).toBeVisible();
  await expect(fs.stat(path.join(unrelated, '.quarto-hub', 'project.json'))).rejects.toThrow();
});

test('a git switch to a branch another peer already created attaches to that doc and shows the join notice', async () => {
  const w = await world();
  const doc = await w.other.register('a.qmd', sideBlob(w.clone, 'a.qmd'), 'A2 plus live edits\n');
  const { page } = await open(w.clone);
  await attach(page, w.pub.indexDocId);
  await expect(statusBar(page)).toContainText('Quarto Hub: clone', { timeout: 60_000 });
  git(w.clone, 'switch', '-q', 'side');
  await expect(toasts(page)).toContainText(formatNotice(FILES_JOINED_LIVE, { n: 1 }), { timeout: 30_000 }).catch(async (e) => {
    await dump(w.clone);
    throw e;
  });
  await waitFor(async () => (await readText(path.join(w.clone, 'a.qmd'))) === 'A2 plus live edits\n', 'the shared doc text on disk');
  expect((await findDoc<{ text: string }>(w.other.repo, doc)).doc()!.text).toBe('A2 plus live edits\n');
});

test('a dirty file detaches, "Adopt" asks first and then takes the doc', async () => {
  const w = await world();
  await w.other.register('a.qmd', sideBlob(w.clone, 'a.qmd'), 'A2 shared\n');
  const { page } = await open(w.clone);
  await attach(page, w.pub.indexDocId);
  await expect(statusBar(page)).toContainText('Quarto Hub: clone', { timeout: 60_000 });
  git(w.clone, 'switch', '-q', 'side');
  await fs.writeFile(path.join(w.clone, 'a.qmd'), 'my own edit\n'); // inside the quiet window
  await expect(toasts(page)).toContainText(formatNotice(FILES_DETACHED, { n: 1 }), { timeout: 30_000 });
  await expect(statusBar(page)).toContainText('1 detached');
  expect(await readText(path.join(w.clone, 'a.qmd'))).toBe('my own edit\n');
  await toasts(page).getByRole('button', { name: 'Adopt shared version' }).click();
  await expect(toasts(page)).toContainText('uncommitted changes', { timeout: 15_000 }); // ADOPT_CONFIRM
  await toasts(page).getByRole('button', { name: 'Replace' }).click();
  await waitFor(async () => (await readText(path.join(w.clone, 'a.qmd'))) === 'A2 shared\n', 'the shared version adopted');
  await expect(statusBar(page)).not.toContainText('detached');
});

test('"Stay detached" survives closing and reopening the window', async () => {
  const w = await world();
  await w.other.register('a.qmd', sideBlob(w.clone, 'a.qmd'), 'A2 shared\n');
  const first = await open(w.clone);
  await attach(first.page, w.pub.indexDocId);
  await expect(statusBar(first.page)).toContainText('Quarto Hub: clone', { timeout: 60_000 });
  git(w.clone, 'switch', '-q', 'side');
  await fs.writeFile(path.join(w.clone, 'a.qmd'), 'my own edit\n');
  await toasts(first.page).getByRole('button', { name: 'Stay detached' }).click({ timeout: 30_000 });
  await first.close();
  const second = await open(w.clone);
  await expect(statusBar(second.page)).toContainText('1 detached', { timeout: 60_000 }).catch(async (e) => {
    await dump(w.clone);
    throw e;
  });
  expect(await readText(path.join(w.clone, 'a.qmd'))).toBe('my own edit\n');
});

test('an offline git switch while the window is closed runs arrival at startup and nothing from disk reaches a doc', async () => {
  const w = await world();
  const aDoc = w.pub.docs['a.qmd']!.docId;
  const first = await open(w.clone);
  await attach(first.page, w.pub.indexDocId);
  await expect(statusBar(first.page)).toContainText('Quarto Hub: clone', { timeout: 60_000 });
  await first.close();
  git(w.clone, 'switch', '-q', 'side'); // offline
  const second = await open(w.clone);
  await expect(statusBar(second.page)).toContainText('Quarto Hub: clone', { timeout: 60_000 });
  await waitFor(async () => (await fs.readFile(path.join(w.clone, '.quarto-hub', 'events.jsonl'), 'utf8').catch(() => '')).includes('"switched"'), 'the arrival event');
  expect((await findDoc<{ text: string }>(w.other.repo, aDoc)).doc()!.text).toBe('A\n'); // the old doc is untouched
  expect(await readText(path.join(w.clone, 'a.qmd'))).toBe('A2\n');
});
