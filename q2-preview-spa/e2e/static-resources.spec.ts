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
 *
 * The project theme test does assert on applied styling: `theme:`
 * stylesheets (`.css` and `.scss`) must reach the preview VFS at
 * startup (else the render fails with Q-14-4 "Theme file not found"),
 * compile in the WASM, and re-theme the open page when edited on disk.
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

/** A theme that only sets the page background, so the test can read it
 * back as the iframe body's computed `background-color`. */
function themeScss(bodyBg: string): string {
  return `/*-- scss:defaults --*/\n$body-bg: ${bodyBg} !default;\n`;
}

let server: PreviewServerHandle;

test.afterEach(async () => {
  await server?.stop();
});

test.describe('_extensions CSS and SVG fixture', () => {
  test.beforeEach(async () => {
    server = await startPreviewServer({
      fixtureFiles: [
        { path: 'index.qmd', content: QMD_CONTENT },
        { path: '_extensions/sentinel/sentinel.css', content: INITIAL_CSS },
        { path: 'assets/logo.svg', content: INITIAL_SVG },
      ],
    });
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
});

test.describe('project theme files fixture', () => {
  test.beforeEach(async () => {
    server = await startPreviewServer({
      fixtureFiles: [
        {
          path: '_quarto.yml',
          content:
            'project:\n  type: default\nformat:\n  html:\n    theme: [t.scss, t2.css]\n',
        },
        { path: 't.scss', content: themeScss('#abcdef') },
        // A `.css` theme file is SCSS-layered like a `.scss` one.
        { path: 't2.css', content: '/*-- scss:rules --*/\nh2 { color: rgb(1, 2, 3); }\n' },
        { path: 'index.qmd', content: '---\ntitle: Themed\n---\n\n## Section\n\ntext\n' },
      ],
    });
  });

  test('project theme stylesheets are applied, and editing one re-themes the page', async ({
    page,
  }) => {
    await page.goto(server.url);
    const frame = page.frameLocator('iframe');
    const body = frame.locator('body');

    // Initial render: both theme files reached the VFS and compiled.
    await expect(body).toHaveCSS('background-color', 'rgb(171, 205, 239)', {
      timeout: 20_000,
    });
    await expect(frame.locator('h2')).toHaveCSS('color', 'rgb(1, 2, 3)');

    // On-disk edit: the watcher re-syncs the stylesheet and the page re-themes.
    await writeFile(path.join(server.projectDir, 't.scss'), themeScss('#123456'));

    await expect(body).toHaveCSS('background-color', 'rgb(18, 52, 86)', {
      timeout: 5_000,
    });
  });
});
