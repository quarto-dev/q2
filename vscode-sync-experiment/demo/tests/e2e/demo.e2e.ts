/**
 * The demo's end-to-end tests (DM13, E1 to E8) for the presets that exist. Each starts the real launcher machinery
 * (`Demo`): a real hub, a real shell in a PTY, the xterm.js page, real git, and the real R and Python engines.
 */
import { test, expect } from '@playwright/test';
import { editorText, noticeLocator } from '../../../../q2-preview-spa/e2e/helpers/editor';
import { StoryFailure } from '../../src/driver';
import { hubBrowserStory } from '../../stories/hub-browser';
import { playStory, startDemo, startFreePlay, stopAll } from './helpers';

test.afterEach(stopAll);

const storyLog = (d: { events: { source: string; kind: string; summary: string }[] }) => d.events.filter((e) => e.source === 'story');

test('E1: hub-browser story in --fast: every check passes and the step log equals the story', async () => {
  const d = await startDemo({ fast: true, typedHub: true });
  await playStory(d);
  const expected = hubBrowserStory(d).map((s) => s.id);
  expect(storyLog(d).map((e) => `${e.kind}:${e.summary}`)).toEqual(expected.map((id) => `pass:${id}`));
});

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

test('E5: reset restores the journal and a fresh hub; the windows stay open and follow it', async () => {
  const d = await startFreePlay();
  const firstId = d.projectId;
  d.git('switch', 'feature');
  await expect(editorText(d.hostEditor!.page)).toContainText('After reanalysis', { timeout: 60_000 });
  await d.reset();
  expect(d.git('branch', '--show-current').trim()).toBe('main');
  expect(d.projectId).not.toBe(firstId);
  expect(d.hostEditor!.page.isClosed()).toBe(false);
  await expect(editorText(d.hostEditor!.page)).not.toContainText('After reanalysis', { timeout: 60_000 });
  await expect(editorText(d.guestEditor!.page)).toContainText('Fusion', { timeout: 60_000 });
  expect(d.events.some((e) => e.source === 'demo' && e.kind === 'reset')).toBe(true);
});

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
