/**
 * Static-file resource sync end-to-end (Phase D.3, bd-kw93.9).
 *
 * Verifies that non-qmd project assets — CSS under `_extensions/` and
 * image files like SVG — round-trip through the filesystem watcher →
 * samod binary-doc sync → SPA notification pipeline that B.1 broadened
 * and D.3 extends. The acceptance is "an on-disk edit triggers a
 * re-render attempt in the SPA within ~2 s." The visible signal is
 * `window.__renderTicks`, a counter PreviewApp bumps every time the
 * render `useEffect` completes a render attempt (success or caught
 * failure).
 *
 * Deferred (documented in plan §D.3): asserting that the CSS rule is
 * actually applied to the rendered iframe DOM. The preview-pipeline
 * pickup of `_extensions/`-supplied CSS is a separate question — file
 * a follow-up if a smoke against a real project reveals the styling
 * never lands. The watcher → sync → notify pipeline is the contract
 * D.3 pins.
 */

import { test, expect, type Page } from '@playwright/test';
import { writeFile } from 'node:fs/promises';
import path from 'node:path';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

/** Returns the current value of `window.__renderTicks` (0 if unset). */
async function getRenderTicks(page: Page): Promise<number> {
  return await page.evaluate(() => {
    const w = window as unknown as { __renderTicks?: number };
    return w.__renderTicks ?? 0;
  });
}

/** Wait until `__renderTicks` exceeds `baseline` (i.e. at least one
 * more render fired). Throws on timeout — D.3 budget is ~2 s, ceiling
 * 5 s for CI variance. */
async function waitForRenderTickAbove(
  page: Page,
  baseline: number,
  timeoutMs = 5_000,
) {
  await page.waitForFunction(
    (b) => {
      const w = window as unknown as { __renderTicks?: number };
      return (w.__renderTicks ?? 0) > b;
    },
    baseline,
    { timeout: timeoutMs },
  );
}

const INITIAL_CSS = `.q2-test-sentinel { color: rgb(123, 45, 67); }
`;

const EDITED_CSS = `.q2-test-sentinel { color: rgb(200, 100, 50); }
`;

const INITIAL_SVG = `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <circle cx="50" cy="50" r="40" fill="rgb(123, 45, 67)"/>
</svg>
`;

const EDITED_SVG = `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect x="10" y="10" width="80" height="80" fill="rgb(200, 100, 50)"/>
</svg>
`;

const QMD_CONTENT = `# Hello

[A sentinel paragraph]{.q2-test-sentinel}

![](assets/logo.svg)
`;

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [
      { path: 'index.qmd', content: QMD_CONTENT },
      { path: '_extensions/sentinel/sentinel.css', content: INITIAL_CSS },
      { path: 'assets/logo.svg', content: INITIAL_SVG },
    ],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('editing CSS in _extensions/ triggers a re-render', async ({ page }) => {
  await page.goto(server.url);
  // Wait for the first render to complete. The basic-preview spec
  // pins the heading-in-iframe contract; here we just need the
  // counter to start ticking.
  await page.waitForFunction(
    () => {
      const w = window as unknown as { __renderTicks?: number };
      return (w.__renderTicks ?? 0) >= 1;
    },
    null,
    { timeout: 30_000 },
  );

  const baseline = await getRenderTicks(page);

  // Edit the CSS asset on disk. The watcher allow-list was
  // extended for `.css` in D.3 (crates/quarto-hub/src/watch.rs);
  // without that change this write would be silently ignored.
  await writeFile(
    path.join(server.projectDir, '_extensions', 'sentinel', 'sentinel.css'),
    EDITED_CSS,
  );

  await waitForRenderTickAbove(page, baseline);

  const after = await getRenderTicks(page);
  expect(after).toBeGreaterThan(baseline);
});

test('editing an image (SVG) triggers a re-render', async ({ page }) => {
  await page.goto(server.url);
  await page.waitForFunction(
    () => {
      const w = window as unknown as { __renderTicks?: number };
      return (w.__renderTicks ?? 0) >= 1;
    },
    null,
    { timeout: 30_000 },
  );

  const baseline = await getRenderTicks(page);

  // SVG was already in the B.1 image allow-list; this test pins
  // that the existing image-sync path actually fires through to a
  // re-render in the SPA.
  await writeFile(path.join(server.projectDir, 'assets', 'logo.svg'), EDITED_SVG);

  await waitForRenderTickAbove(page, baseline);

  const after = await getRenderTicks(page);
  expect(after).toBeGreaterThan(baseline);
});
