/**
 * GE-14, the hub demo: a real `q2 preview --git track --ui editor --allow-edit` hub on a real git repository, a browser
 * guest (Playwright chromium on the hub's editor), a headless mirror, and headless clones, through a commit, a branch
 * switch, a reset, and a switch back. Every assertion waits on the state it asserts (`expect.poll`), never a fixed
 * sleep. No network: the hub runs without `--share`, and peers join by the loopback URL.
 *
 * Tags (see the plan's "The permanent subset"): `@permanent` runs in CI and in `cargo xtask verify --e2e`;
 * `@exploratory` is kept and runs by hand (`--grep @exploratory`).
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { test } from '@playwright/test';
import { FEATURE_A, FEATURE_B, MAIN_FILES } from '../../../../q2-preview-spa/e2e/helpers/gitRepo';
import { gitStatus, noticeLocator, shownText, treeItem, typeInEditor } from '../../../../q2-preview-spa/e2e/helpers/editor';
import { Scene, decision, expect, expectFile, read, readBytes, waitFor } from '../../helpers/cross-client';
import { dumpSidecar } from '../../helpers/registry-dump';
import { ensureWasm, HeadlessPeer } from '../../helpers/hub';
import { launchVscode, type RunningVscode } from '../../helpers/launch';
import { execFileSync } from 'node:child_process';
import { bindFolder } from '../../../src/core/join';
import { activeEditorText, openFile } from '../../helpers/palette';

let scene: Scene;
let vscode: RunningVscode | undefined;

test.beforeAll(async () => {
  await ensureWasm();
});
test.beforeEach(() => {
  scene = new Scene();
});
test.afterEach(async () => {
  await vscode?.close();
  vscode = undefined;
  await scene.close();
});

const MARKERS = /<<<<<<<|>>>>>>>/;
const blobOf = (repo: { git(...a: string[]): string }, rev: string, file: string): string => repo.git('rev-parse', `${rev}:${file}`).trim();

async function appendOnDisk(dir: string, rel: string, text: string): Promise<void> {
  await fs.appendFile(path.join(dir, rel), text);
}

test('H1 @permanent: a hub commit while every peer types causes no swap and no notice, and the registry moves the doc forward', async () => {
  const hub = await scene.trackingHub();
  const oldBlob = blobOf(hub.repo, 'HEAD', 'a.qmd');
  const guest = await scene.guest(hub.handle.url, 'a.qmd');
  const mirror = await scene.mirror(hub.wsUrl, hub.indexDocId);
  const clone = await scene.clone(hub);
  await expectFile(mirror.dir, 'a.qmd', (t) => t === MAIN_FILES['a.qmd'], 'the mirror holds a.qmd');

  // Everyone types, one after another so that no peer writes its disk while another peer's edit is landing there (two
  // disk appends racing a sync write is a test race, not the behavior under test): the browser in the editor, then
  // the clone and the mirror on disk. Each edit must reach every peer before the next is typed.
  const everywhere = async (needle: string) => {
    await expect.poll(async () => (await shownText(guest.page).catch(() => '')).includes(needle), { timeout: 30_000, message: `${needle} in the browser` }).toBe(true);
    for (const [where, dir] of [['hub', hub.dir], ['mirror', mirror.dir], ['clone', clone.dir]] as const) {
      await expectFile(dir, 'a.qmd', (t) => !!t && t.includes(needle), `${needle} on the ${where} disk`);
    }
  };
  await typeInEditor(guest.page, 'GUEST-EDIT ');
  await everywhere('GUEST-EDIT');
  await appendOnDisk(clone.dir, 'a.qmd', 'CLONE-EDIT\n');
  await everywhere('CLONE-EDIT');
  await appendOnDisk(mirror.dir, 'a.qmd', 'MIRROR-EDIT\n');
  await everywhere('MIRROR-EDIT');
  const all = (t: string | undefined) => !!t && ['GUEST-EDIT', 'CLONE-EDIT', 'MIRROR-EDIT'].every((e) => t.includes(e));

  // The hub commits what is on its disk.
  const committed = hub.repo.commitAll('commit while everyone is typing');
  const newBlob = blobOf(hub.repo, committed, 'a.qmd');
  expect(newBlob).not.toBe(oldBlob);
  const status = await decision(hub.handle.url, ['moved-forward']);
  expect(status.decisions.every((d) => d.kind !== 'flipped' && d.kind !== 'seeded')).toBe(true);

  // The browser keeps typing across the commit; no swap and no notice anywhere; every edit survives in every peer.
  await typeInEditor(guest.page, 'AFTER-COMMIT ');
  await everywhere('AFTER-COMMIT');
  await expect(noticeLocator(guest.page)).toHaveCount(0);
  await expect.poll(async () => all(await shownText(guest.page).catch(() => '')), { timeout: 30_000 }).toBe(true);
  await expectFile(mirror.dir, 'a.qmd', all, 'every edit on the mirror');
  await expectFile(clone.dir, 'a.qmd', all, 'every edit on the clone');
  expect(all(await read(hub.dir, 'a.qmd'))).toBe(true);

  // The registry moved forward: an entry under the new blob, none under the old.
  await expect
    .poll(
      async () => {
        const dump = await dumpSidecar(hub.wsUrl, hub.indexDocId);
        const a = dump.entries.filter((e) => e.path === 'a.qmd');
        return { under_new: a.some((e) => e.blob === newBlob), under_old: a.some((e) => e.blob === oldBlob) };
      },
      { timeout: 30_000, message: 'the registry holds a.qmd under the new blob only' },
    )
    .toEqual({ under_new: true, under_old: false });
});

test('H2 @permanent: git switch feature swaps the browser with the owner notice, rewrites the mirror, leaves a clone on main alone, and a clone on feature joins the new doc', async () => {
  const hub = await scene.trackingHub();
  const guest = await scene.guest(hub.handle.url, 'a.qmd');
  const mirror = await scene.mirror(hub.wsUrl, hub.indexDocId);
  const cloneMain = await scene.clone(hub, 'main');
  const cloneFeature = await scene.clone(hub, 'feature');
  await expectFile(mirror.dir, 'a.qmd', (t) => t === MAIN_FILES['a.qmd'], 'the mirror holds a.qmd');
  const mainDisk = await read(cloneMain.dir, 'a.qmd');
  expect(mainDisk).toBe(MAIN_FILES['a.qmd']);

  hub.repo.switch('feature');
  await decision(hub.handle.url, ['flipped', 'seeded']);

  // The browser swaps with the owner notice and shows the feature text.
  await expect.poll(() => shownText(guest.page), { timeout: 30_000 }).toContain('Version FEATURE of a.');
  await expect(noticeLocator(guest.page)).toContainText("project owner's checked-out version changed", { timeout: 30_000 });
  // The mirror's disk equals the feature file.
  await expectFile(mirror.dir, 'a.qmd', (t) => t === FEATURE_A, 'the mirror follows to the feature text');
  // The clone on main stays in its own doc and its disk is untouched.
  expect(await read(cloneMain.dir, 'a.qmd')).toBe(mainDisk);
  // The clone that was already on feature is on the hub's new doc: it holds the feature text, and the hub's pointer for
  // a.qmd is a doc both share (an edit on the clone reaches the hub's disk).
  expect(await read(cloneFeature.dir, 'a.qmd')).toBe(FEATURE_A);
  await appendOnDisk(cloneFeature.dir, 'a.qmd', 'FROM-FEATURE-CLONE\n');
  await expectFile(hub.dir, 'a.qmd', (t) => !!t && t.includes('FROM-FEATURE-CLONE'), 'the feature clone edits the hub doc');
  expect(MARKERS.test((await read(hub.dir, 'a.qmd')) ?? '')).toBe(false);
});

test('H3 @permanent: a clone on the hub\'s branch and the browser exchange edits both ways', async () => {
  const hub = await scene.trackingHub();
  const guest = await scene.guest(hub.handle.url, 'a.qmd');
  const clone = await scene.clone(hub);

  await appendOnDisk(clone.dir, 'a.qmd', 'CLONE-TO-BROWSER\n');
  await expect.poll(() => shownText(guest.page), { timeout: 30_000 }).toContain('CLONE-TO-BROWSER');
  await expectFile(hub.dir, 'a.qmd', (t) => !!t && t.includes('CLONE-TO-BROWSER'), 'the clone edit reaches the hub disk');

  await typeInEditor(guest.page, 'BROWSER-TO-CLONE ');
  await expectFile(clone.dir, 'a.qmd', (t) => !!t && t.includes('BROWSER-TO-CLONE'), 'the browser edit reaches the clone disk');
});

test('H5 @permanent: git reset --hard after a browser edit retires the doc: no revert merges in as an author edit, the pointer moves to a doc equal to disk, the edit stays in the retired doc', async () => {
  const hub = await scene.trackingHub();
  const guest = await scene.guest(hub.handle.url, 'a.qmd');
  const cloneSame = await scene.clone(hub);
  await typeInEditor(guest.page, 'DIRTY-EDIT ');
  await expectFile(hub.dir, 'a.qmd', (t) => !!t && t.includes('DIRTY-EDIT'), 'the browser edit reaches the hub disk');
  await expectFile(cloneSame.dir, 'a.qmd', (t) => !!t && t.includes('DIRTY-EDIT'), 'the clone has the edit too');

  hub.repo.resetHard();
  // The guest moves to a doc equal to HEAD; nothing re-applies the revert as an author edit.
  await expect.poll(() => shownText(guest.page), { timeout: 30_000 }).not.toContain('DIRTY-EDIT');
  expect(await shownText(guest.page)).toContain('Version MAIN of a.');
  await expectFile(hub.dir, 'a.qmd', (t) => t === MAIN_FILES['a.qmd'], 'the hub disk is HEAD');
  await expect
    .poll(async () => (await gitStatus(hub.handle.url)).decisions.some((d) => (d.discarded ?? 0) >= 1), {
      timeout: 30_000,
      message: 'a decision reports the browser edit as discarded',
    })
    .toBe(true);
  // The browser edit stayed in the retired doc: the registry lists it as retired.
  await expect
    .poll(async () => (await dumpSidecar(hub.wsUrl, hub.indexDocId)).retired.some((e) => e.path === 'a.qmd'), {
      timeout: 30_000,
      message: 'the retired doc is in the sidecar',
    })
    .toBe(true);
  // A clone on the same blob stays in the retired doc: its disk keeps the dirty text, and it never overwrites the hub.
  await expectFile(cloneSame.dir, 'a.qmd', (t) => !!t && t.includes('DIRTY-EDIT'), 'the clone stays in the retired doc');
  expect(await read(hub.dir, 'a.qmd')).toBe(MAIN_FILES['a.qmd']);
});

test('H6 @permanent: switching to feature and back lists and unlists b.qmd and c.qmd in the browser and the mirror; a clone on main keeps c.qmd', async () => {
  const hub = await scene.trackingHub();
  const guest = await scene.guest(hub.handle.url, 'a.qmd');
  const mirror = await scene.mirror(hub.wsUrl, hub.indexDocId);
  const clone = await scene.clone(hub);
  await expectFile(mirror.dir, 'c.qmd', (t) => t === MAIN_FILES['c.qmd'], 'the mirror holds c.qmd');
  await expect(treeItem(guest.page, 'c.qmd')).toBeVisible();

  hub.repo.switch('feature');
  await expect(treeItem(guest.page, 'b.qmd')).toBeVisible({ timeout: 30_000 });
  await expect(treeItem(guest.page, 'c.qmd')).toHaveCount(0, { timeout: 30_000 });
  await expectFile(mirror.dir, 'c.qmd', (t) => t === undefined, 'c.qmd leaves the mirror');
  await expectFile(mirror.dir, 'b.qmd', (t) => t === FEATURE_B, 'b.qmd appears in the mirror');
  expect(await read(clone.dir, 'c.qmd')).toBe(MAIN_FILES['c.qmd']); // the clone on main keeps its own

  hub.repo.switch('main');
  await expect(treeItem(guest.page, 'c.qmd')).toBeVisible({ timeout: 30_000 });
  await expect(treeItem(guest.page, 'b.qmd')).toHaveCount(0, { timeout: 30_000 });
  await expectFile(mirror.dir, 'c.qmd', (t) => t === MAIN_FILES['c.qmd'], 'c.qmd is back in the mirror at its old doc');
  await expectFile(mirror.dir, 'b.qmd', (t) => t === undefined, 'b.qmd is gone from the mirror');
});

test('H7 @permanent: a tracked binary follows the switch in the browser tree\'s hub and the mirror, and a clone on main keeps its own bytes', async () => {
  const hub = await scene.trackingHub();
  await scene.guest(hub.handle.url, 'a.qmd');
  const mirror = await scene.mirror(hub.wsUrl, hub.indexDocId);
  const clone = await scene.clone(hub);
  await expect.poll(async () => (await readBytes(mirror.dir, 'img/logo.png'))?.equals(hub.repo.logoMain)).toBe(true);

  hub.repo.switch('feature');
  await expect.poll(async () => (await readBytes(mirror.dir, 'img/logo.png'))?.equals(hub.repo.logoFeature), { timeout: 30_000, message: 'the mirror has the feature image' }).toBe(true);
  expect((await readBytes(clone.dir, 'img/logo.png'))?.equals(hub.repo.logoMain)).toBe(true);

  hub.repo.switch('main');
  await expect.poll(async () => (await readBytes(mirror.dir, 'img/logo.png'))?.equals(hub.repo.logoMain), { timeout: 30_000, message: 'the mirror has the main image again' }).toBe(true);
  expect((await readBytes(clone.dir, 'img/logo.png'))?.equals(hub.repo.logoMain)).toBe(true);
});

test('H8 @exploratory: a rebase that stops for conflicts pauses the hub; guests keep editing and see no conflict marker; one decision runs after it continues', async () => {
  const hub = await scene.trackingHub();
  const guest = await scene.guest(hub.handle.url, 'a.qmd');
  const repo = hub.repo;
  repo.writeFile('a.qmd', '---\ntitle: A\n---\n\nVersion MAIN2 of a.\n');
  repo.commitAll('main edits the same line');
  repo.switch('feature');
  await decision(hub.handle.url, ['flipped', 'seeded']);
  const before = (await (await fetch(new URL('api/preview/git-status', hub.handle.url))).json()).decisions.length;
  expect(repo.rebase('main')).toEqual({ stopped: true });
  await expect.poll(async () => (await (await fetch(new URL('api/preview/git-status', hub.handle.url))).json()).paused, { timeout: 30_000 }).toBe(true);
  await typeInEditor(guest.page, 'DURING-REBASE ');
  expect(MARKERS.test(await shownText(guest.page))).toBe(false);
  repo.writeFile('a.qmd', '---\ntitle: A\n---\n\nVersion RESOLVED of a.\n');
  expect(repo.rebaseContinue()).toEqual({ stopped: false });
  await expect.poll(async () => (await (await fetch(new URL('api/preview/git-status', hub.handle.url))).json()).paused, { timeout: 30_000 }).toBe(false);
  await expect.poll(() => shownText(guest.page), { timeout: 30_000 }).toContain('Version RESOLVED of a.');
  const after = (await (await fetch(new URL('api/preview/git-status', hub.handle.url))).json()).decisions.length;
  expect(after - before).toBeGreaterThanOrEqual(1);
  await waitFor(async () => !MARKERS.test((await read(hub.dir, 'a.qmd')) ?? ''), 'no conflict marker on the hub disk');
});

test('H4 @exploratory: a real VS Code clone that runs git switch feature attaches the doc for that blob, peers on main stay put, and the hub pointer does not move', async () => {
  const hub = await scene.trackingHub();
  const guest = await scene.guest(hub.handle.url, 'a.qmd');
  // Bind a clone folder with the join logic, then open it in a real window (the extension runs clone mode there).
  const clone = await scene.clone(hub);
  await clone.peer.stop();
  vscode = await launchVscode({ workspace: clone.dir });
  await expect(vscode.page.locator('.statusbar')).toContainText('Quarto Hub: clone', { timeout: 60_000 });
  const before = await dumpSidecar(hub.wsUrl, hub.indexDocId);
  const mainDoc = before.entries.find((e) => e.path === 'a.qmd')!.doc;

  execFileSync('git', ['switch', '--quiet', 'feature'], { cwd: clone.dir });
  await expectFile(clone.dir, 'a.qmd', (t) => t === FEATURE_A, 'the clone checkout has the feature text');
  // Arrival attaches the doc for that blob, or creates one and registers it.
  const featureBlob = clone.dir && execFileSync('git', ['rev-parse', 'HEAD:a.qmd'], { cwd: clone.dir, encoding: 'utf8' }).trim();
  await expect
    .poll(async () => (await dumpSidecar(hub.wsUrl, hub.indexDocId)).entries.some((e) => e.path === 'a.qmd' && e.blob === featureBlob), {
      timeout: 60_000,
      message: 'the registry lists a.qmd under the feature blob',
    })
    .toBe(true);
  // The hub's pointer does not move: no decision, the hub disk and the browser stay on main.
  expect((await gitStatus(hub.handle.url)).decisions.some((d) => d.kind === 'flipped')).toBe(false);
  expect(await read(hub.dir, 'a.qmd')).toBe(MAIN_FILES['a.qmd']);
  expect(await shownText(guest.page)).toContain('Version MAIN of a.');
  const after = await dumpSidecar(hub.wsUrl, hub.indexDocId);
  expect(after.entries.find((e) => e.path === 'a.qmd' && e.doc === mainDoc)).toBeDefined();
  void HeadlessPeer;
});

test('H2e @permanent: an editor-only VS Code window on a clone of the hub repository suspends during its own checkout, rebinds to the hub\'s feature doc, shows the feature text, and splices nothing into the hub', async () => {
  const hub = await scene.trackingHub();
  const parent = await scene.dir('ge14-editor-only-');
  const folder = path.join(parent, 'repo');
  execFileSync('git', ['clone', '--quiet', hub.dir, folder], { stdio: 'ignore', env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null' } });
  for (const [k, v] of [['user.name', 'GE'], ['user.email', 'ge@example.invalid'], ['commit.gpgsign', 'false'], ['core.autocrlf', 'false']] as const) {
    execFileSync('git', ['config', k, v], { cwd: folder });
  }
  execFileSync('git', ['fetch', '--quiet', 'origin', 'feature:feature'], { cwd: folder });
  await bindFolder({ root: folder, indexDocId: hub.indexDocId, syncServer: hub.wsUrl, mode: 'editor-only' });
  scene.log(`editor-only clone ${folder} bound to ${hub.wsUrl} (${hub.indexDocId})`);
  vscode = await launchVscode({ workspace: folder });
  const { page } = vscode;
  await expect(page.locator('.statusbar')).toContainText('Quarto Hub: editor only', { timeout: 60_000 });
  await openFile(page, 'a.qmd');
  await expect.poll(() => activeEditorText(page), { timeout: 60_000 }).toContain('Version MAIN of a.');

  // The hub switches, then the editor's own repository does: the window suspends during git's rewrite, then follows the flip.
  hub.repo.switch('feature');
  await decision(hub.handle.url, ['flipped', 'seeded']);
  execFileSync('git', ['switch', '--quiet', 'feature'], { cwd: folder });
  await expect.poll(() => activeEditorText(page), { timeout: 60_000 }).toContain('Version FEATURE of a.');
  expect(await activeEditorText(page)).not.toContain('Version MAIN of a.');
  // Nothing from the window's own git rewrite was spliced into the hub's doc.
  await expectFile(hub.dir, 'a.qmd', (t) => t === FEATURE_A, 'the hub doc holds exactly the feature text', 60_000);
});
