/**
 * The preview's render entry goes through the serial render queue
 * (preview VFS contract D9). A run of edits must end on a render of the
 * latest content, whole: no render of an older version lands after it,
 * and no render mixes versions.
 *
 * The watcher debounces disk events for 500 ms, so edits written faster
 * than that collapse before they reach the browser; the spec spaces them
 * just past the debounce so each one reaches the SPA as its own
 * content change, as fast as a real editor's saves can arrive. The queue's
 * overlap and coalescing rules are pinned deterministically in
 * `ts-packages/preview-runtime/src/renderPageForPreview.test.ts`; this
 * spec checks the same contract end to end.
 */

import { test, expect, type Page } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const version = (n: number) =>
  `# Version ${n}\n\n${Array.from({ length: 40 }, (_, i) => `Paragraph ${i} of version ${n}.`).join('\n\n')}\n`;

async function paragraphs(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const innerDoc = document.querySelector('iframe')?.contentDocument;
    return Array.from(innerDoc?.querySelectorAll('p') ?? []).map((p) => p.textContent ?? '');
  });
}

async function heading(page: Page): Promise<string | null> {
  return page.evaluate(() => {
    const innerDoc = document.querySelector('iframe')?.contentDocument;
    return innerDoc?.querySelector('h1')?.textContent ?? null;
  });
}

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [{ path: 'index.qmd', content: version(0) }],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('a run of edits ends on a whole render of the latest content', async ({ page }) => {
  await page.goto(server.url);
  await expect.poll(() => heading(page), { timeout: 30_000 }).toBe('Version 0');

  const file = path.join(server.projectDir, 'index.qmd');
  for (const n of [1, 2, 3]) {
    await writeFile(file, version(n));
    await page.waitForTimeout(600);
  }

  await expect.poll(() => heading(page), { timeout: 10_000 }).toBe('Version 3');
  // Settle: nothing older may land after the latest render.
  await page.waitForTimeout(1500);
  expect(await heading(page)).toBe('Version 3');
  const ps = await paragraphs(page);
  expect(ps).toHaveLength(40);
  expect(ps.every((p) => p.endsWith('of version 3.'))).toBe(true);
});
