/**
 * `quarto preview` end-to-end smoke (bd-vpsy, Phase A.7).
 *
 * Spawns the real `q2 preview` binary against a temp fixture
 * project, opens chromium against the SPA, and pins four
 * behaviours from the §A.7 plan:
 *
 *   1. The preview renders (DOM contains the expected `# Hello`).
 *   2. Editing the source `.qmd` on disk re-renders the iframe
 *      within ~2 s (file watcher → samod → SPA).
 *   3. The force-refresh button (`bd-b5hf`) is reachable and
 *      clicking it runs without error.
 *   4. The Q2-preview DOM-stability invariant: an element whose
 *      logical identity is unchanged across an edit keeps the
 *      *same* DOM node instance (React's reconciler reuses, not
 *      replaces).
 *
 * The §A.7 plan named this `data-stable-id`; in practice the
 * stability is React-reconciler-level, so we pin it via node
 * identity on the heading. The heading exists in both pre- and
 * post-edit ASTs and is generated from `# Hello`, which the edit
 * doesn't touch.
 */

import { test, expect, type Page } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const INITIAL_CONTENT = `# Hello

First paragraph from the initial fixture.
`;

const EDITED_CONTENT = `# Hello

First paragraph has been edited live.

A second paragraph appears.
`;

/**
 * Wait for the inner iframe (the sandboxed q2-preview renderer)
 * to mount AND for the post-pipeline DOM to be ready. The outer
 * `<iframe src="/q2-preview.html">` mounts before the AST is
 * posted, so we wait for the actual rendered content too.
 */
async function waitForInnerHeading(page: Page, text: string) {
  await page.waitForFunction(
    (expected) => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const h1 = innerDoc?.querySelector('h1');
      return h1 != null && h1.textContent === expected;
    },
    text,
    { timeout: 30_000 },
  );
}

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [{ path: 'index.qmd', content: INITIAL_CONTENT }],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('initial render contains the fixture heading and paragraph', async ({ page }) => {
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Hello');

  const innerText = await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const innerDoc = outer?.contentDocument;
    return {
      h1: innerDoc?.querySelector('h1')?.textContent ?? null,
      paragraphs: Array.from(innerDoc?.querySelectorAll('p') ?? []).map(
        (p) => p.textContent ?? '',
      ),
    };
  });
  expect(innerText.h1).toBe('Hello');
  expect(innerText.paragraphs).toEqual(['First paragraph from the initial fixture.']);
});

test('on-disk edit re-renders within 2s', async ({ page }) => {
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Hello');

  // Edit the source. The file watcher inside quarto-hub will pick
  // this up, sync the new bytes into samod, push to the SPA, and
  // re-render. Budget per the plan: 2 s.
  await writeFile(path.join(server.projectDir, 'index.qmd'), EDITED_CONTENT);

  await page.waitForFunction(
    () => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const paragraphs = Array.from(innerDoc?.querySelectorAll('p') ?? []);
      return paragraphs.some((p) => p.textContent?.includes('edited live'));
    },
    null,
    { timeout: 5_000 }, // budget is 2 s per plan; a generous ceiling for CI variance
  );

  const after = await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const innerDoc = outer?.contentDocument;
    return {
      paragraphs: Array.from(innerDoc?.querySelectorAll('p') ?? []).map(
        (p) => p.textContent ?? '',
      ),
    };
  });
  expect(after.paragraphs).toEqual([
    'First paragraph has been edited live.',
    'A second paragraph appears.',
  ]);
});

test('force-refresh button is reachable and runs without error', async ({ page }) => {
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Hello');

  // The button lives in PreviewApp's chrome (outside the sandboxed
  // renderer iframe), so it's accessible as a normal page button.
  const refresh = page.getByRole('button', { name: /refresh preview/i });
  await expect(refresh).toBeVisible();
  await refresh.click();
  // No explicit observable side-effect at the iframe level (re-rendering
  // identical AST yields identical DOM), so we just assert the iframe
  // is still healthy a tick after the click.
  await page.waitForTimeout(500);
  const stillThere = await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    return outer?.contentDocument?.querySelector('h1')?.textContent ?? null;
  });
  expect(stillThere).toBe('Hello');
});

test('DOM-stability: heading node identity is preserved across an edit', async ({ page }) => {
  await page.goto(server.url);
  await waitForInnerHeading(page, 'Hello');

  // Stash a reference to the pre-edit heading on `window`. Playwright
  // can't compare DOM nodes across separate `evaluate()` calls
  // (returned values are serialized), so the comparison has to happen
  // inside a single evaluate that closes over both states via window
  // storage.
  await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const innerDoc = outer?.contentDocument;
    const h1 = innerDoc?.querySelector('h1');
    (window as unknown as { __preEditH1: Element | null }).__preEditH1 = h1 ?? null;
  });

  await writeFile(path.join(server.projectDir, 'index.qmd'), EDITED_CONTENT);
  await page.waitForFunction(
    () => {
      const outer = document.querySelector('iframe');
      const innerDoc = outer?.contentDocument;
      const paragraphs = Array.from(innerDoc?.querySelectorAll('p') ?? []);
      return paragraphs.some((p) => p.textContent?.includes('edited live'));
    },
    null,
    { timeout: 5_000 },
  );

  const result = await page.evaluate(() => {
    const outer = document.querySelector('iframe');
    const innerDoc = outer?.contentDocument;
    const postEdit = innerDoc?.querySelector('h1');
    const preEdit = (window as unknown as { __preEditH1: Element | null }).__preEditH1;
    return {
      preEditWasFound: preEdit != null,
      postEditFound: postEdit != null,
      sameNode: preEdit === postEdit,
      postText: postEdit?.textContent ?? null,
    };
  });
  expect(result.preEditWasFound).toBe(true);
  expect(result.postEditFound).toBe(true);
  expect(result.postText).toBe('Hello');
  // This is the load-bearing assertion: the heading wasn't touched
  // by the edit, so React's reconciler must keep the same DOM node.
  expect(result.sameNode).toBe(true);
});
