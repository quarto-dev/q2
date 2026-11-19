/**
 * GE-13: the git-epochs hub scenarios that show in the default read-only viewer UI (S1, S2, S4, S5, S8, S9, S10,
 * S12), with `q2 preview --git track` on a real git repo and two browser guests in separate contexts. The viewer
 * re-renders on a swap and shows no replaced-file notice (that is the editor's, in `embed-git-epochs.spec.ts`), but it
 * does show the paused banner. Runs under the `chromium` and `firefox` projects; no jj, no network.
 */

import { test, expect, type Browser, type Page } from '@playwright/test';
import { FEATURE_A, MAIN_FILES } from './helpers/gitRepo';
import { gitStatus, noticeLocator, pausedIndicator, previewBodyText } from './helpers/editor';
import { expectDisk, readDisk, readDiskBytes, startGitHub, waitForDecision } from './helpers/gitScenario';
import type { PreviewServerHandle } from './helpers/previewServer';

let hub: PreviewServerHandle | undefined;
const pages: Page[] = [];

test.afterEach(async () => {
  for (const p of pages.splice(0)) await p.context().close().catch(() => {});
  await hub?.stop();
  hub = undefined;
});

/** Two guests, each in its own context, showing `a.qmd`. */
async function guestsOnA(browser: Browser, url: string): Promise<[Page, Page]> {
  const out: Page[] = [];
  for (let i = 0; i < 2; i++) {
    const page = await (await browser.newContext()).newPage();
    pages.push(page);
    await page.goto(new URL('?page=a.qmd', url).toString());
    await expect.poll(() => previewBodyText(page), { timeout: 90_000 }).toContain('Version MAIN of a.');
    out.push(page);
  }
  return [out[0], out[1]];
}

const shows = (page: Page, text: string, timeout = 30_000) => expect.poll(() => previewBodyText(page), { timeout }).toContain(text);
const MARKERS = /<<<<<<<|>>>>>>>/;

test.describe('git epochs, hub with viewer guests', () => {
  test('S1: an owner commit while the hub runs changes nothing the guests see', async ({ browser }) => {
    hub = await startGitHub({ ui: 'viewer' });
    const [a, b] = await guestsOnA(browser, hub.url);
    const text = '---\ntitle: A\n---\n\nEdited on disk, then committed.\n';
    hub.repo!.writeFile('a.qmd', text);
    await expectDisk(hub, 'a.qmd', (t) => t === text);
    hub.repo!.commitAll('commit the disk edit');
    for (const g of [a, b]) await shows(g, 'Edited on disk, then committed.');
    const status = await waitForDecision(hub, ['moved-forward']);
    expect(status.tracking).toBe(true);
    for (const g of [a, b]) await expect(noticeLocator(g)).toHaveCount(0);
  });

  test('S2: git switch feature re-renders both guests from the feature text, with no notice', async ({ browser }) => {
    hub = await startGitHub({ ui: 'viewer' });
    const [a, b] = await guestsOnA(browser, hub.url);
    hub.repo!.switch('feature');
    for (const g of [a, b]) {
      await shows(g, 'Version FEATURE of a.');
      await expect(noticeLocator(g)).toHaveCount(0);
    }
    expect(await readDisk(hub, 'a.qmd')).toBe(FEATURE_A);
    // Ordinary sync would also show the new text, so the swap itself is read from the status route.
    await waitForDecision(hub, ['flipped']);
  });

  test('S4: git switch main back re-renders the main text', async ({ browser }) => {
    hub = await startGitHub({ ui: 'viewer' });
    const [a] = await guestsOnA(browser, hub.url);
    hub.repo!.switch('feature');
    await shows(a, 'Version FEATURE of a.');
    hub.repo!.switch('main');
    await shows(a, 'Version MAIN of a.');
    expect(await readDisk(hub, 'a.qmd')).toBe(MAIN_FILES['a.qmd']);
  });

  test('S5: a rebase stopped for conflicts shows the paused banner, then clears', async ({ browser }) => {
    hub = await startGitHub({ ui: 'viewer' });
    const [a, b] = await guestsOnA(browser, hub.url);
    const repo = hub.repo!;
    repo.writeFile('a.qmd', '---\ntitle: A\n---\n\nVersion MAIN2 of a.\n');
    repo.commitAll('main edits the same line');
    repo.switch('feature');
    expect(repo.rebase('main')).toEqual({ stopped: true });
    await expect.poll(async () => (await gitStatus(hub!.url)).paused, { timeout: 20_000 }).toBe(true);
    for (const g of [a, b]) {
      await expect(pausedIndicator(g)).toBeVisible({ timeout: 20_000 });
      expect(await previewBodyText(g)).not.toMatch(MARKERS);
    }
    repo.writeFile('a.qmd', '---\ntitle: A\n---\n\nVersion RESOLVED of a.\n');
    expect(repo.rebaseContinue()).toEqual({ stopped: false });
    await expect.poll(async () => (await gitStatus(hub!.url)).paused, { timeout: 20_000 }).toBe(false);
    for (const g of [a, b]) {
      await expect(pausedIndicator(g)).toHaveCount(0, { timeout: 20_000 });
      await shows(g, 'Version RESOLVED of a.');
    }
  });

  test('S8: files added and removed on feature appear in and leave the project, and back', async ({ browser }) => {
    test.setTimeout(150_000);
    hub = await startGitHub({ ui: 'viewer' });
    const [a] = await guestsOnA(browser, hub.url);
    const repo = hub.repo!;
    // A fresh load seeds the page from `?page=`, and a file the index does not list yet falls back to the first page,
    // so each check reloads until the project lists (or stops listing) the file.
    const eventually = (file: string, check: (body: string) => void) =>
      expect(async () => {
        await a.goto(new URL(`?page=${file}`, hub!.url).toString());
        await expect.poll(() => previewBodyText(a), { timeout: 15_000 }).not.toBe('');
        check(await previewBodyText(a));
      }).toPass({ timeout: 60_000 });
    repo.switch('feature');
    await eventually('b.qmd', (body) => expect(body).toContain('The file b exists on feature.'));
    await eventually('c.qmd', (body) => expect(body).not.toContain('The file c exists on main.'));
    repo.switch('main');
    await eventually('c.qmd', (body) => expect(body).toContain('The file c exists on main.'));
    await eventually('b.qmd', (body) => expect(body).not.toContain('The file b exists on feature.'));
  });

  test('S9: a tracked binary follows the checkout byte for byte', async ({ browser }) => {
    hub = await startGitHub({ ui: 'viewer' });
    await guestsOnA(browser, hub.url);
    const repo = hub.repo!;
    repo.switch('feature');
    await expect.poll(async () => (await readDiskBytes(hub!, 'img/logo.png')).equals(repo.logoFeature), { timeout: 30_000 }).toBe(true);
    repo.switch('main');
    await expect.poll(async () => (await readDiskBytes(hub!, 'img/logo.png')).equals(repo.logoMain), { timeout: 30_000 }).toBe(true);
  });

  test('S10: git reset --hard after a dirty disk edit puts the HEAD content back', async ({ browser }) => {
    hub = await startGitHub({ ui: 'viewer' });
    const [a] = await guestsOnA(browser, hub.url);
    hub.repo!.writeFile('a.qmd', '---\ntitle: A\n---\n\nDIRTY-EDIT.\n');
    await shows(a, 'DIRTY-EDIT.');
    hub.repo!.resetHard();
    await shows(a, 'Version MAIN of a.');
    expect(await readDisk(hub, 'a.qmd')).toBe(MAIN_FILES['a.qmd']);
    await waitForDecision(hub, ['reset', 'flipped', 'retired', 'seeded', 'switched']);
  });

  test('S12: control, --git ignore reports tracking false and shows no paused banner', async ({ browser }) => {
    hub = await startGitHub({ ui: 'viewer', git: 'ignore' });
    const [a] = await guestsOnA(browser, hub.url);
    expect((await gitStatus(hub.url)).tracking).toBe(false);
    const repo = hub.repo!;
    repo.writeFile('a.qmd', '---\ntitle: A\n---\n\nVersion MAIN2 of a.\n');
    repo.commitAll('main edit');
    repo.switch('feature');
    repo.rebase('main');
    await expect(pausedIndicator(a)).toHaveCount(0);
    expect((await gitStatus(hub.url)).paused).toBe(false);
    repo.rebaseAbort();
  });
});
