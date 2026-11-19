/**
 * The demo's end-to-end tests (DM13, E1 to E8) for the presets that exist. Each starts the real launcher machinery
 * (`Demo`): a real hub, a real shell in a PTY, the xterm.js page, real git, and the real R and Python engines.
 */
import { test, expect } from '@playwright/test';
import { editorText, noticeLocator } from '../../../../q2-preview-spa/e2e/helpers/editor';
import { StoryFailure } from '../../src/driver';
import { storyFor } from '../../stories';
import { playStory, startDemo, startFreePlay, stopAll } from './helpers';

test.afterEach(stopAll);

const storyLog = (d: { events: { source: string; kind: string; summary: string }[] }) => d.events.filter((e) => e.source === 'story');

const PRESETS = ['hub-browser', 'hub-vscode-host', 'hub-vscode-clone', 'hubless'] as const;

for (const preset of PRESETS) {
  test(`E1: ${preset} story in --fast: every check passes and the step log equals the story`, async () => {
    const d = await startDemo({ preset, fast: true, typedHub: true });
    await playStory(d);
    const expected = storyFor(d).map((s) => s.id);
    expect(storyLog(d).map((e) => `${e.kind}:${e.summary}`)).toEqual(expected.map((id) => `pass:${id}`));
    // The live invariant checks stayed silent through the whole story.
    expect(d.events.filter((e) => e.source === 'invariant')).toEqual([]);
  });
}

test('E2: engines: the guest shows the R and Python charts, and the R chart re-executes across the switch', async () => {
  const d = await startFreePlay();
  // Both papers render a chart (live chunks, executed by the hub).
  await expect(d.hostEditor!.page.frameLocator('iframe').locator('img').first()).toBeVisible({ timeout: 120_000 });
  await expect(d.guestEditor!.page.frameLocator('iframe').locator('img').first()).toBeVisible({ timeout: 120_000 });
  const before = await d.hostEditor!.page.frameLocator('iframe').locator('img').first().getAttribute('src');
  d.git('switch', 'feature');
  await expect
    .poll(async () => d.hostEditor!.page.frameLocator('iframe').locator('img').first().getAttribute('src'), { timeout: 120_000 })
    .not.toBe(before);
});

test('E3: free play: a person types git switch into the terminal; the guest follows and the observer narrates', async () => {
  const d = await startFreePlay({ narration: 'strip' });
  const term = d.terminalPages[0]!.page;
  await term.locator('.xterm').click();
  await term.keyboard.type('git switch feature');
  await term.keyboard.press('Enter');
  await expect(editorText(d.hostEditor!.page)).toContainText('After reanalysis', { timeout: 60_000 });
  await expect(noticeLocator(d.guestEditor!.page)).toHaveCount(1, { timeout: 60_000 });
  expect(d.git('branch', '--show-current').trim()).toBe('feature');
  await expect(term.getByTestId('strip')).toContainText('The hub switched branches', { timeout: 30_000 });
  expect(d.typed).toContain('git switch feature');
});

test('E4: fault injection: a conflict marker in a doc raises the red warning', async () => {
  const d = await startFreePlay();
  const { writeFileSync } = await import('node:fs');
  const p = `${d.journal}/index.qmd`;
  const { readFileSync } = await import('node:fs');
  writeFileSync(p, readFileSync(p, 'utf8') + '\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> other\n');
  await expect(d.terminalPages[0]!.page.getByTestId('strip')).toContainText('conflict marker in index.qmd', { timeout: 30_000 });
  expect(d.events.some((e) => e.source === 'invariant' && e.kind === 'conflict-marker')).toBe(true);
});

test('E4b: fault injection: the hub disk differing from the pointed doc raises the warning', async () => {
  const d = await startFreePlay();
  // The observer's reader says the pointed doc holds other text than the disk (a stand-in for a lost write).
  d.docReader = { pointerTexts: async (paths) => Object.fromEntries(paths.map((p) => [p, 'not what the disk holds'])), close: async () => {} };
  await expect(d.terminalPages[0]!.page.getByTestId('strip')).toContainText('hub disk differs from the pointed doc', { timeout: 60_000 });
  expect(d.events.some((e) => e.source === 'invariant' && e.kind === 'disk-differs-from-pointer')).toBe(true);
});

test('E4c: fault injection: a clone on another branch whose disk holds the hub branch text raises the warning', async () => {
  const d = await startFreePlay({ preset: 'hub-vscode-clone' });
  d.git('switch', 'feature');
  await expect.poll(() => d.shown('host'), { timeout: 60_000 }).toContain('After reanalysis');
  const { execFileSync } = await import('node:child_process');
  const text = execFileSync('git', ['show', 'feature:celia-2026-tip-extension-lmm.qmd'], { cwd: d.journal, encoding: 'utf8' });
  (await import('node:fs')).writeFileSync(`${d.guestDir}/celia-2026-tip-extension-lmm.qmd`, text);
  await expect(d.terminalPages[0]!.page.getByTestId('strip')).toContainText('is on main, not feature, but its disk was rewritten', { timeout: 60_000 });
  expect(d.events.some((e) => e.source === 'invariant' && e.kind === 'clone-disk-rewritten')).toBe(true);
});

test('E5: hub-browser: reset restores the journal and a fresh hub; the windows stay open and follow it', async () => {
  const d = await startFreePlay();
  const firstId = d.projectId;
  d.git('switch', 'feature');
  await expect(editorText(d.hostEditor!.page)).toContainText('After reanalysis', { timeout: 60_000 });
  d.hostTerminal.write('t'); // a stray key left on the input line must not break the reset's own `cd`
  await d.reset();
  expect(d.hostTerminal.output()).not.toContain('command not found');
  expect(d.git('branch', '--show-current').trim()).toBe('main');
  expect(d.projectId).not.toBe(firstId);
  expect(d.hostEditor!.page.isClosed()).toBe(false);
  await expect(editorText(d.hostEditor!.page)).not.toContainText('After reanalysis', { timeout: 60_000 });
  await expect(editorText(d.guestEditor!.page)).toContainText('Fusion', { timeout: 60_000 });
  expect(d.events.some((e) => e.source === 'demo' && e.kind === 'reset')).toBe(true);
});

for (const preset of ['hub-vscode-host', 'hub-vscode-clone', 'hubless'] as const) {
  test(`E5: ${preset}: reset restores the journal and a fresh project; the editors come back on it`, async () => {
    const d = await startFreePlay({ preset });
    const firstId = d.projectId;
    d.git('switch', 'feature');
    await expect.poll(() => d.shown('host'), { timeout: 60_000 }).toContain('After reanalysis');
    await d.reset();
    expect(d.git('branch', '--show-current').trim()).toBe('main');
    expect(d.projectId).not.toBe(firstId);
    expect(d.terminalPages[0]!.page.isClosed()).toBe(false);
    await expect.poll(() => d.shown('host'), { timeout: 90_000 }).toContain('Tip extension');
    expect(await d.shown('host')).not.toContain('After reanalysis');
    if (preset === 'hub-vscode-clone') await expect.poll(() => d.shown('guest'), { timeout: 90_000 }).toContain('Fusion');
    expect(d.events.some((e) => e.source === 'demo' && e.kind === 'reset')).toBe(true);
  });
}

for (const where of ['inline', 'strip', 'both', 'off'] as const) {
  test(`E6: captions=${where}`, async () => {
    const d = await startDemo({ captions: where, fast: false });
    d.say('caption', 'CAPTION-MARKER');
    const page = d.terminalPages[0]!.page;
    const inline = page.locator('.xterm-rows');
    const strip = page.getByTestId('strip');
    if (where === 'inline' || where === 'both') await expect(inline).toContainText('CAPTION-MARKER');
    else await expect(inline).not.toContainText('CAPTION-MARKER');
    if (where === 'strip' || where === 'both') await expect(strip).toContainText('CAPTION-MARKER');
    else await expect(strip).not.toContainText('CAPTION-MARKER');
    // Never in the shell's input.
    expect(d.hostTerminal.output()).not.toContain('CAPTION-MARKER');
  });
}

test('E7: layout: each window reports the bounds the layout computed (headed; needs a display)', async () => {
  const d = await startFreePlay({ headless: false });
  const bounds = async (w: { context: import('@playwright/test').BrowserContext; page: import('@playwright/test').Page }) => {
    const cdp = await w.context.newCDPSession(w.page);
    const { windowId } = (await cdp.send('Browser.getWindowForTarget')) as { windowId: number };
    return ((await cdp.send('Browser.getWindowBounds', { windowId })) as { bounds: Record<string, number> }).bounds;
  };
  const want = d.layout;
  for (const [w, r] of [
    [d.hostEditor!, want.hostEditor],
    [d.terminalPages[0]!, want.hostTerminal],
    [d.guestEditor!, want.guestEditor],
  ] as const) {
    const b = await bounds(w);
    expect({ left: b.left, top: b.top, width: b.width, height: b.height }).toEqual({ left: r.left, top: r.top, width: r.width, height: r.height });
  }
});

test('E8: control: against a hub that ignores git, the story fails at the switch beat', async () => {
  const d = await startDemo({ fast: true, typedHub: true, hubGit: 'ignore' });
  const err = await playStory(d).then(
    () => undefined,
    (e) => e,
  );
  expect(err).toBeInstanceOf(StoryFailure);
  expect((err as StoryFailure).stepId).toBe('switch-feature');
});

test('E7b: layout: the VS Code windows report the bounds the layout computed (headed)', async () => {
  const d = await startFreePlay({ preset: 'hub-vscode-clone', headless: false });
  const b = await d.vscode.guest!.app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getBounds());
  const r = d.layout.guestEditor;
  expect(b).toEqual({ x: r.left, y: r.top, width: r.width, height: r.height });
  const h = await startFreePlay({ preset: 'hubless', headless: false });
  const hb = await h.vscode.host!.app.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]!.getBounds());
  const hr = h.layout.hostEditor;
  expect(hb).toEqual({ x: hr.left, y: hr.top, width: hr.width, height: hr.height });
});
