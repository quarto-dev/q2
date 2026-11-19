/**
 * GE-14, the hub-less demo: a real VS Code window originates a git project to a plain sync server (the in-process test
 * hub on loopback, which needs no sign-in), `q2 preview --hub-url` shows it in a browser, and second peers join as a
 * clone and as a mirror. Then the originator switches branches and everyone follows through the relay.
 *
 * No network beyond loopback. The same run against `wss://sync.automerge.org` is a manual transcript (see the plan).
 */
import * as path from 'node:path';
import { test, type Page } from '@playwright/test';
import { FEATURE_A, MAIN_FILES, buildFixtureRepo } from '../../../../q2-preview-spa/e2e/helpers/gitRepo';
import { noticeLocator, shownText, treeItem, typeInEditor } from '../../../../q2-preview-spa/e2e/helpers/editor';
import { Scene, expect, expectFile, q2SupportsHubUrl, read, waitFor } from '../../helpers/cross-client';
import { ensureWasm, HeadlessPeer } from '../../helpers/hub';
import { launchVscode, type RunningVscode } from '../../helpers/launch';
import { fillInputBox, runCommand } from '../../helpers/palette';
import { execFileSync } from 'node:child_process';

let scene: Scene;
let vscode: RunningVscode | undefined;

test.beforeAll(async () => {
  await ensureWasm();
});
test.beforeEach(async () => {
  scene = new Scene();
  test.skip(!(await q2SupportsHubUrl()), 'the built q2 has no --hub-url (run cargo build --bin q2)');
});
test.afterEach(async () => {
  await vscode?.close();
  vscode = undefined;
  await scene.close();
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

/** L1: the originator. A real VS Code window on the fixture repository runs "Share this folder" against `serverUrl`. */
async function originate(serverUrl: string, dir: string): Promise<{ id: string; command: string }> {
  vscode = await launchVscode({ workspace: dir, env: { QUARTO_HUB_TEST_SYNC_SERVER: serverUrl } });
  const { page } = vscode;
  await runCommand(page, 'Quarto Hub: Share this folder');
  await fillInputBox(page, serverUrl);
  await dialog(page).waitFor({ state: 'visible', timeout: 30_000 });
  const message = await dialog(page).locator('.dialog-message-text').innerText();
  const detail = await dialog(page).locator('.dialog-message-detail').innerText();
  scene.log(`modal: ${message} | ${detail.replace(/\s+/g, ' ')}`);
  expect(message).toBe(`Share this folder on ${serverUrl}?`);
  expect(detail).toContain('Anyone with access to that server and the project link can read and edit it.');
  expect(detail).toMatch(/Project:\s+\S.*\(git repo, branch main\)/);
  expect(detail).toMatch(/Files:\s+\d+ tracked files/);
  await dialog(page).getByRole('button', { name: 'Share' }).click();
  // The panel appears only after the server confirmed the heads; the three forms are copyable.
  const command = await copyForm(page, 'q2 preview command');
  const m = /^q2 preview --hub-url (\S+) --project (\S+) --ui editor --allow-edit$/.exec(command);
  expect(m, `the copied command: ${command}`).not.toBeNull();
  expect(m![1]).toBe(serverUrl);
  for (const form of ['Share link', 'Server and project id']) {
    // The other two forms are listed in the same panel; reopen it from the command and check each copies something.
    await runCommand(page, 'Quarto Hub: Show share information');
    expect((await copyForm(page, form)).length).toBeGreaterThan(8);
  }
  scene.log(`copied: ${command}`);
  return { id: m![2]!, command };
}

async function fixtureRepo(): Promise<string> {
  const dir = await scene.dir('ge14-origin-');
  buildFixtureRepo(dir);
  return dir;
}

test('L1-L4 @permanent: originate from VS Code, show it in a browser through --hub-url, join a clone and a mirror, and follow a branch switch through the relay', async () => {
  const server = await scene.standaloneHub();
  const dir = await fixtureRepo();
  const { id } = await originate(server.url, dir);

  // L2: q2 preview --hub-url shows the project; a browser edit reaches the originator's disk.
  const preview = await scene.previewOver(server.url, id);
  const guest = await scene.guest(preview.url, 'a.qmd');
  await expect.poll(() => shownText(guest.page), { timeout: 60_000 }).toContain('Version MAIN of a.');
  await typeInEditor(guest.page, 'FROM-THE-BROWSER ');
  await expectFile(dir, 'a.qmd', (t) => !!t && t.includes('FROM-THE-BROWSER'), "the browser edit reaches the originator's disk", 60_000);

  // L3: a second peer joins as a clone (its identity check passes) and as a mirror; both sync, the clone attaches by blob.
  const cloneParent = await scene.dir('ge14-clone-');
  const cloneDir = path.join(cloneParent, 'repo');
  execFileSync('git', ['clone', '--quiet', dir, cloneDir], { stdio: 'ignore' });
  const clone = await HeadlessPeer.joinAsClone(cloneDir, server.url, id);
  scene.onCleanup(() => clone.stop());
  const attached = Object.keys(clone.cloneState()!.attached).sort();
  expect(attached).toEqual(['_quarto.yml', 'a.qmd', 'c.qmd', 'img/logo.png', 'index.qmd']);
  const mirrorDir = await scene.dir('ge14-mirror-');
  const mirror = await HeadlessPeer.joinAsMirror(mirrorDir, server.url, id);
  scene.onCleanup(() => mirror.stop());
  await expectFile(mirrorDir, 'a.qmd', (t) => !!t && t.includes('FROM-THE-BROWSER'), 'the mirror sees the browser edit');
  await expectFile(cloneDir, 'a.qmd', (t) => !!t && t.includes('FROM-THE-BROWSER'), 'the clone sees the browser edit');

  // L4: the originator runs git switch; the browser follows through the relay with the notice, the mirror follows,
  // and the clone on main stays in its own doc. (The browser edit made the originator's a.qmd dirty; a commit on main
  // first keeps the switch clean, as a user would.)
  execFileSync('git', ['-c', 'user.name=GE', '-c', 'user.email=ge@example.invalid', 'commit', '-qam', 'keep the browser edit'], { cwd: dir });
  const cloneBefore = await read(cloneDir, 'a.qmd');
  execFileSync('git', ['switch', '--quiet', 'feature'], { cwd: dir });
  await expect.poll(() => shownText(guest.page), { timeout: 60_000 }).toContain('Version FEATURE of a.');
  await expect(noticeLocator(guest.page)).toContainText("project owner's checked-out version changed", { timeout: 30_000 });
  await expectFile(mirrorDir, 'a.qmd', (t) => t === FEATURE_A, 'the mirror follows to the feature text', 60_000);
  await expect(treeItem(guest.page, 'b.qmd')).toBeVisible({ timeout: 30_000 });
  expect(await read(cloneDir, 'a.qmd')).toBe(cloneBefore);
  expect(MAIN_FILES['a.qmd']).toBeDefined();
});

test('L5 @exploratory: an originator that goes offline, switches branch, and restarts re-points the paths; until then the browser is stale (the accepted gap)', async () => {
  const server = await scene.standaloneHub();
  const dir = await fixtureRepo();
  const { id } = await originate(server.url, dir);
  const preview = await scene.previewOver(server.url, id);
  const guest = await scene.guest(preview.url, 'a.qmd');
  await expect.poll(() => shownText(guest.page), { timeout: 60_000 }).toContain('Version MAIN of a.');

  await vscode!.close();
  vscode = undefined;
  execFileSync('git', ['switch', '--quiet', 'feature'], { cwd: dir });
  // Nothing is running that could follow the switch: the browser still shows main.
  await expect.poll(() => shownText(guest.page)).toContain('Version MAIN of a.');

  vscode = await launchVscode({ workspace: dir, env: { QUARTO_HUB_TEST_SYNC_SERVER: server.url } });
  await expect.poll(() => shownText(guest.page), { timeout: 90_000 }).toContain('Version FEATURE of a.');
});
