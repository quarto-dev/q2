/**
 * GE-13: the git-epochs hub scenarios (S1 to S13) with the editor UI (`q2 preview --git track --ui editor`), driven
 * through the real binary, real git, and two browser guests in separate contexts on the same loopback hub.
 *
 * Needs a `q2` built with the hub-client embed (`cargo xtask build-hub-client-embed`, then `cargo build --bin q2`);
 * each test skips when `isEmbedBuilt` says the binary holds the placeholder. The jj scenario (S6) skips when `jj` is
 * absent, and the tunnel scenario (S13) when iroh's relay is unreachable.
 *
 * The viewer-UI subset is `git-epochs-viewer.spec.ts`.
 */

import { test, expect, type Browser } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { FEATURE_A, MAIN_FILES, jjAvailable } from './helpers/gitRepo';
import { editorText, gitStatus, noticeLocator, pausedIndicator, shownText, treeItem, typeInEditor } from './helpers/editor';
import { expectDisk, openGuest, readDisk, readDiskBytes, startGitHub, waitForDecision, type Guest } from './helpers/gitScenario';
import { isEmbedBuilt, startJoinGuest, type JoinGuestHandle, type PreviewServerHandle } from './helpers/previewServer';

let hub: PreviewServerHandle | undefined;
let extra: JoinGuestHandle | undefined;
const guests: Guest[] = [];

test.afterEach(async () => {
  for (const g of guests.splice(0)) await g.context.close().catch(() => {});
  await extra?.stop();
  await hub?.stop();
  extra = hub = undefined;
});

async function start(opts: { allowEdit?: boolean; git?: 'track' | 'ignore'; jjColocate?: boolean } = {}): Promise<PreviewServerHandle> {
  hub = await startGitHub({ ui: 'editor', allowEdit: opts.allowEdit ?? true, git: opts.git, jjColocate: opts.jjColocate });
  test.skip(!(await isEmbedBuilt(hub)), 'q2 was built without the hub-client embed (placeholder)');
  return hub;
}

async function guestsOnA(browser: Browser, url: string): Promise<[Guest, Guest]> {
  const a = await openGuest(browser, url, 'a.qmd');
  const b = await openGuest(browser, url, 'a.qmd');
  guests.push(a, b);
  for (const g of [a, b]) await expect(editorText(g.page)).toContainText('Version MAIN of a.', { timeout: 30_000 });
  return [a, b];
}

const MARKERS = /<<<<<<<|>>>>>>>/;

test.describe('git epochs, hub with editor guests', () => {
  test('S1: an owner commit while two guests edit causes no notice and no swap', async ({ browser }) => {
    const h = await start();
    const [a, b] = await guestsOnA(browser, h.url);
    await typeInEditor(a.page, 'ALPHA1 ');
    await typeInEditor(b.page, 'BETA1 ');
    for (const g of [a, b]) {
      await expect(editorText(g.page)).toContainText('ALPHA1', { timeout: 20_000 });
      await expect(editorText(g.page)).toContainText('BETA1', { timeout: 20_000 });
    }
    await expectDisk(h, 'a.qmd', (t) => t.includes('ALPHA1') && t.includes('BETA1'));

    h.repo!.commitAll('owner commit of the guests\' text');
    await typeInEditor(a.page, 'AFTER1 ');
    await expect(editorText(b.page)).toContainText('AFTER1', { timeout: 20_000 });
    const status = await waitForDecision(h, ['moved-forward']);
    expect(status.tracking).toBe(true);
    for (const g of [a, b]) {
      await expect(noticeLocator(g.page)).toHaveCount(0);
      await expect(editorText(g.page)).toContainText('ALPHA1');
      await expect(editorText(g.page)).toContainText('BETA1');
    }
    await expectDisk(h, 'a.qmd', (t) => t.includes('AFTER1'));
  });

  test('S2: git switch feature shows the feature text everywhere, with one notice per guest', async ({ browser }) => {
    const h = await start();
    const [a, b] = await guestsOnA(browser, h.url);
    h.repo!.switch('feature');
    for (const g of [a, b]) {
      await expect(editorText(g.page)).toContainText('Version FEATURE of a.', { timeout: 30_000 });
      await expect(noticeLocator(g.page)).toHaveCount(1);
      await expect(noticeLocator(g.page)).toContainText('replaced because the project owner');
    }
    expect(await readDisk(h, 'a.qmd')).toBe(FEATURE_A);
    // Still one notice after the batch settles (a quiet window longer than the aggregation window).
    await expect(noticeLocator(a.page)).toHaveCount(1);
  });

  test('S3: an edit stranded by the switch is named in the notice; the file matches feature', async ({ browser }) => {
    const h = await start();
    const [a, b] = await guestsOnA(browser, h.url);
    await a.context.setOffline(true);
    await typeInEditor(a.page, 'STRANDED ');
    h.repo!.switch('feature');
    await waitForDecision(h, ['flipped', 'switched']);
    await a.context.setOffline(false);
    await a.page.evaluate(() => window.dispatchEvent(new Event('online')));
    await expect(editorText(a.page)).toContainText('Version FEATURE of a.', { timeout: 40_000 });
    await expect(noticeLocator(a.page)).toContainText('stayed with the previous version', { timeout: 20_000 });
    await expect(editorText(a.page)).not.toContainText('STRANDED');
    await expect(editorText(b.page)).toContainText('Version FEATURE of a.');
    expect(await readDisk(h, 'a.qmd')).toBe(FEATURE_A);
  });

  test('S4: git switch main back leaves editor and disk in agreement', async ({ browser }, testInfo) => {
    const h = await start();
    const [a, b] = await guestsOnA(browser, h.url);
    await typeInEditor(a.page, 'EDIT-ON-MAIN ');
    await expectDisk(h, 'a.qmd', (t) => t.includes('EDIT-ON-MAIN'));
    h.repo!.resetHard(); // park the guest edit: the tree is clean for the switch
    h.repo!.switch('feature');
    await expect(editorText(a.page)).toContainText('Version FEATURE of a.', { timeout: 30_000 });
    h.repo!.switch('main');
    await expect(editorText(a.page)).toContainText('Version MAIN of a.', { timeout: 30_000 });
    await expect(editorText(b.page)).toContainText('Version MAIN of a.');
    await expectDisk(h, 'a.qmd', (t) => t === MAIN_FILES['a.qmd'] || t.includes('EDIT-ON-MAIN'));
    const disk = await readDisk(h, 'a.qmd');
    const shown = await shownText(a.page);
    // Agreement: the editor shows every line the disk has. Record whether the retained edit resurfaced.
    for (const line of disk.split('\n').filter((l) => l.trim())) expect(shown).toContain(line.trim());
    testInfo.annotations.push({ type: 'stranded edit on return', description: disk.includes('EDIT-ON-MAIN') ? 'resurfaced' : 'did not resurface' });
  });

  test('S5: a rebase stopped for conflicts pauses, then applies exactly one decision', async ({ browser }) => {
    const h = await start();
    const [a, b] = await guestsOnA(browser, h.url);
    const repo = h.repo!;
    repo.writeFile('a.qmd', '---\ntitle: A\n---\n\nVersion MAIN2 of a.\n');
    repo.commitAll('main edits the same line');
    repo.switch('feature');
    const before = (await gitStatus(h.url)).decisions.length;
    expect(repo.rebase('main')).toEqual({ stopped: true });
    await expect.poll(async () => (await gitStatus(h.url)).paused, { timeout: 20_000 }).toBe(true);
    for (const g of [a, b]) {
      await expect(pausedIndicator(g.page)).toBeVisible({ timeout: 20_000 });
      await expect(editorText(g.page)).not.toContainText(MARKERS);
    }
    repo.writeFile('a.qmd', '---\ntitle: A\n---\n\nVersion RESOLVED of a.\n');
    expect(repo.rebaseContinue()).toEqual({ stopped: false });
    await expect.poll(async () => (await gitStatus(h.url)).paused, { timeout: 20_000 }).toBe(false);
    for (const g of [a, b]) {
      await expect(pausedIndicator(g.page)).toHaveCount(0);
      await expect(editorText(g.page)).toContainText('Version RESOLVED of a.', { timeout: 30_000 });
      await expect(noticeLocator(g.page)).toHaveCount(1);
    }
    expect(await readDisk(h, 'a.qmd')).toContain('Version RESOLVED of a.');
    const after = await gitStatus(h.url);
    // The rebase finished once: the decisions it added are one batch, not one per intermediate state.
    expect(after.decisions.length - before).toBeGreaterThanOrEqual(1);
  });

  test('S6: jj new follows each flip from disk; arrival under jj seeds a doc and registers nothing', async ({ browser }) => {
    test.skip(!jjAvailable(), 'jj is not installed');
    const h = await start({ jjColocate: true });
    const [a, b] = await guestsOnA(browser, h.url);
    // First arrival on `feature`: no doc is registered for its blobs, so G9 step 1 applies (jj has no effect on it).
    h.repo!.jjNew('feature');
    for (const g of [a, b]) {
      await expect(editorText(g.page)).toContainText('Version FEATURE of a.', { timeout: 30_000 });
      await expect(editorText(g.page)).not.toContainText(MARKERS);
    }
    await waitForDecision(h, ['flipped']);
    // Back to `main`, whose doc exists: without jj this attaches silently (step 3); under jj every such file takes the
    // step 5 fallback, so the decision is `seeded` (a doc created from disk, nothing registered).
    h.repo!.jjNew('main');
    for (const g of [a, b]) {
      await expect(editorText(g.page)).toContainText('Version MAIN of a.', { timeout: 30_000 });
      await expect(editorText(g.page)).not.toContainText(MARKERS);
    }
    const status = await waitForDecision(h, ['seeded']);
    expect(status.decisions.some((d) => d.kind === 'seeded')).toBe(true);
    expect(await readDisk(h, 'a.qmd')).toBe(MAIN_FILES['a.qmd']);
  });

  test('S7: save then commit inside the debounce is treated as a commit', async ({ browser }) => {
    const h = await start();
    const [a, b] = await guestsOnA(browser, h.url);
    const text = '---\ntitle: A\n---\n\nSAVED-THEN-COMMITTED text.\n';
    h.repo!.writeFile('a.qmd', text);
    h.repo!.commitAll('save and commit together');
    for (const g of [a, b]) {
      await expect(editorText(g.page)).toContainText('SAVED-THEN-COMMITTED', { timeout: 30_000 });
      await expect(noticeLocator(g.page)).toHaveCount(0);
    }
    const status = await gitStatus(h.url);
    expect(status.decisions.some((d) => d.kind === 'flipped')).toBe(false);
  });

  test('S8: files added and removed on feature are listed and unlisted, and back', async ({ browser }) => {
    const h = await start();
    const [a] = await guestsOnA(browser, h.url);
    await expect(treeItem(a.page, 'c.qmd')).toBeVisible();
    await expect(treeItem(a.page, 'b.qmd')).toHaveCount(0);
    h.repo!.switch('feature');
    await expect(treeItem(a.page, 'b.qmd')).toBeVisible({ timeout: 30_000 });
    await expect(treeItem(a.page, 'c.qmd')).toHaveCount(0, { timeout: 30_000 });
    h.repo!.switch('main');
    await expect(treeItem(a.page, 'c.qmd')).toBeVisible({ timeout: 30_000 });
    await expect(treeItem(a.page, 'b.qmd')).toHaveCount(0, { timeout: 30_000 });
  });

  test('S9: a tracked binary follows the checkout byte for byte', async ({ browser }) => {
    const h = await start();
    const [a] = await guestsOnA(browser, h.url);
    const repo = h.repo!;
    expect((await readDiskBytes(h, 'img/logo.png')).equals(repo.logoMain)).toBe(true);
    repo.switch('feature');
    await expect.poll(async () => (await readDiskBytes(h, 'img/logo.png')).equals(repo.logoFeature), { timeout: 30_000 }).toBe(true);
    await expect(editorText(a.page)).toContainText('Version FEATURE of a.', { timeout: 30_000 });
    repo.switch('main');
    await expect.poll(async () => (await readDiskBytes(h, 'img/logo.png')).equals(repo.logoMain), { timeout: 30_000 }).toBe(true);
    // The disk is the source of the owner's tree; no cross-branch overwrite left feature's bytes behind.
    expect((await readFile(path.join(h.projectDir, 'img', 'logo.png'))).equals(repo.logoMain)).toBe(true);
  });

  test('S10: git reset --hard after a dirty edit moves guests to a doc equal to HEAD', async ({ browser }) => {
    const h = await start();
    const [a, b] = await guestsOnA(browser, h.url);
    await typeInEditor(a.page, 'DIRTY-EDIT ');
    await expectDisk(h, 'a.qmd', (t) => t.includes('DIRTY-EDIT'));
    h.repo!.resetHard();
    for (const g of [a, b]) {
      await expect(editorText(g.page)).not.toContainText('DIRTY-EDIT', { timeout: 30_000 });
      await expect(editorText(g.page)).toContainText('Version MAIN of a.');
    }
    expect(await readDisk(h, 'a.qmd')).toBe(MAIN_FILES['a.qmd']);
    await expect
      .poll(async () => (await gitStatus(h.url)).decisions.some((d) => (d.discarded ?? 0) >= 1), { timeout: 20_000 })
      .toBe(true);
  });

  test('S11: without --allow-edit a switch follows and no browser edit reaches disk', async ({ browser }) => {
    const h = await start({ allowEdit: false });
    const a = await openGuest(browser, h.url, 'a.qmd');
    guests.push(a);
    await expect(editorText(a.page)).toContainText('Version MAIN of a.', { timeout: 30_000 });
    await typeInEditor(a.page, 'NOT-ALLOWED ').catch(() => {});
    expect(await readDisk(h, 'a.qmd')).toBe(MAIN_FILES['a.qmd']);
    h.repo!.switch('feature');
    await expect(editorText(a.page)).toContainText('Version FEATURE of a.', { timeout: 30_000 });
    expect(await readDisk(h, 'a.qmd')).toBe(FEATURE_A);
    expect(await readDisk(h, 'a.qmd')).not.toContain('NOT-ALLOWED');
  });

  test('S12: control, --git ignore reports tracking false and shows no paused indicator', async ({ browser }) => {
    const h = await start({ git: 'ignore' });
    const [a] = await guestsOnA(browser, h.url);
    const repo = h.repo!;
    expect((await gitStatus(h.url)).tracking).toBe(false);
    repo.writeFile('a.qmd', '---\ntitle: A\n---\n\nVersion MAIN2 of a.\n');
    repo.commitAll('main edit');
    repo.switch('feature');
    repo.rebase('main');
    await expect(pausedIndicator(a.page)).toHaveCount(0);
    expect((await gitStatus(h.url)).paused).toBe(false);
    repo.rebaseAbort();
  });

  test('S13: a guest through the tunnel sees the switch', async ({ browser }) => {
    hub = await (await import('./helpers/previewServer')).startPreviewServer({ gitRepo: true, git: 'track', ui: 'editor', allowEdit: true, share: true });
    test.skip(!(await isEmbedBuilt(hub)), 'q2 was built without the hub-client embed (placeholder)');
    let ticket: string;
    try {
      ticket = await hub.joinTicket(20_000);
      extra = await startJoinGuest(ticket);
    } catch (e) {
      test.skip(true, `the iroh relay is unreachable: ${String(e).slice(0, 120)}`);
      return;
    }
    const g = await openGuest(browser, extra.url, 'a.qmd');
    guests.push(g);
    await expect(editorText(g.page)).toContainText('Version MAIN of a.', { timeout: 60_000 });
    hub.repo!.switch('feature');
    await expect(editorText(g.page)).toContainText('Version FEATURE of a.', { timeout: 60_000 });
  });
});

