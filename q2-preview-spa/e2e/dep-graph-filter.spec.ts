/**
 * Dep-graph filter for re-renders (Phase D.6, bd-kw93.12).
 *
 * Pins the *negative* case Phase B.4 relaxed and D.6 restores: editing
 * a `.qmd` that the active page doesn't include should NOT trigger a
 * re-render. Today's filter is single-hop include-shortcode parsing
 * (see `crates/quarto-preview/src/deps.rs`). Other relevance channels
 * (image refs, bibliography, etc.) are deliberately out of scope —
 * non-qmd files always pass the filter, so D.3's CSS/SVG specs stay
 * green.
 *
 * Spec layout:
 *
 *   1. **Negative case** — `index.qmd` doesn't reference
 *      `sibling.qmd`; editing `sibling.qmd` must NOT bump
 *      `__renderTicks` within 2 s.
 *   2. **Positive (self)** — editing the active `.qmd` itself must
 *      bump `__renderTicks`. Sanity check that the filter doesn't
 *      mistakenly block the active page's own edits.
 *
 * The include-shortcode propagation pinned by Phase B.3's
 * `include-shortcode.spec.ts` covers the *positive (dep)* case
 * (edit an included file → active page re-renders) and continues
 * to pass with the filter in place.
 *
 * bd-0mji's acceptance criteria #1 (positive) and #2 (negative)
 * close out via this spec + the existing include-shortcode spec.
 */

import { test, expect, type Page } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

const INDEX_INITIAL = `# Index

This is the active page. It deliberately does not reference any
sibling files.
`;

const SIBLING_INITIAL = `# Sibling

An unrelated .qmd file the index page never includes.
`;

const SIBLING_EDITED = `# Sibling

An unrelated .qmd file the index page never includes.

An extra paragraph appears.
`;

const INDEX_EDITED = `# Index

This is the active page (edited).
`;

async function getRenderTicks(page: Page): Promise<number> {
  return await page.evaluate(() => {
    const w = window as unknown as { __renderTicks?: number };
    return w.__renderTicks ?? 0;
  });
}

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [
      { path: 'index.qmd', content: INDEX_INITIAL },
      { path: 'sibling.qmd', content: SIBLING_INITIAL },
    ],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('editing an unrelated sibling .qmd does NOT trigger a re-render', async ({ page }) => {
  await page.goto(server.url);

  // Wait for the initial render. The first non-zero tick confirms
  // the boot path completed; we then wait an extra moment to let
  // the deps fetch settle so the filter is fully armed.
  await page.waitForFunction(
    () => {
      const w = window as unknown as { __renderTicks?: number };
      return (w.__renderTicks ?? 0) >= 1;
    },
    null,
    { timeout: 30_000 },
  );
  // Give the deps fetch ~1 s to land. Without it the filter falls
  // open (deps === null) and the assertion below would race the
  // pre-D.6 behaviour.
  await page.waitForTimeout(1000);

  const baseline = await getRenderTicks(page);

  // Edit the unrelated sibling. The watcher fires, samod syncs the
  // new text, `onFileContent('sibling.qmd', …)` fires in the SPA,
  // and the D.6 filter should drop it.
  await writeFile(path.join(server.projectDir, 'sibling.qmd'), SIBLING_EDITED);

  // Wait 2 seconds. If the filter is working, no tick should fire.
  // 2 s is the same budget the basic-preview re-render test uses;
  // a real re-render would land well within it.
  await page.waitForTimeout(2000);

  const after = await getRenderTicks(page);
  expect(after).toBe(baseline);
});

test('editing the active .qmd itself DOES trigger a re-render', async ({ page }) => {
  await page.goto(server.url);

  await page.waitForFunction(
    () => {
      const w = window as unknown as { __renderTicks?: number };
      return (w.__renderTicks ?? 0) >= 1;
    },
    null,
    { timeout: 30_000 },
  );
  await page.waitForTimeout(1000);

  const baseline = await getRenderTicks(page);

  await writeFile(path.join(server.projectDir, 'index.qmd'), INDEX_EDITED);

  await page.waitForFunction(
    (b) => {
      const w = window as unknown as { __renderTicks?: number };
      return (w.__renderTicks ?? 0) > b;
    },
    baseline,
    { timeout: 5_000 },
  );

  const after = await getRenderTicks(page);
  expect(after).toBeGreaterThan(baseline);
});
