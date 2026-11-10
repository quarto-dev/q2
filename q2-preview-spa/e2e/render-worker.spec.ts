/**
 * The preview renders in a dedicated worker (preview VFS contract D3).
 *
 * Pins the move itself: the page starts the render worker, the page's own
 * realm never fetches the engine's `.wasm` (only the worker does), and
 * what the main thread reads back from the render (the compiled theme
 * CSS, through the VFS mirror) reaches the iframe.
 */

import { test, expect } from '@playwright/test';
import { startPreviewServer, type PreviewServerHandle } from './helpers/previewServer';

let server: PreviewServerHandle;

test.beforeEach(async () => {
  server = await startPreviewServer({
    fixtureFiles: [
      {
        path: 'index.qmd',
        content: '---\ntitle: Worker\nformat:\n  q2-preview:\n    theme: cosmo\n---\n\n# In a worker\n\nRendered off the main thread.\n',
      },
    ],
  });
});

test.afterEach(async () => {
  await server?.stop();
});

test('the engine runs in a worker and the page reads its results', async ({ page }) => {
  await page.goto(server.url);
  await expect
    .poll(
      () =>
        page.evaluate(() =>
          Array.from(document.querySelector('iframe')?.contentDocument?.querySelectorAll('p') ?? []).some(
            (p) => p.textContent === 'Rendered off the main thread.',
          ),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);

  expect(page.workers().some((w) => w.url().includes('engine.worker'))).toBe(true);

  const pageFetchedWasm = await page.evaluate(() =>
    performance
      .getEntriesByType('resource')
      .some((e) => e.name.includes('wasm_quarto_hub_client_bg')),
  );
  expect(pageFetchedWasm).toBe(false);

  // The theme CSS was compiled in the worker and reached the iframe.
  await expect
    .poll(
      () =>
        page.evaluate(() => {
          const doc = document.querySelector('iframe')?.contentDocument;
          if (!doc) return 0;
          return Array.from(doc.styleSheets).reduce((n, sheet) => {
            try {
              return n + sheet.cssRules.length;
            } catch {
              return n;
            }
          }, 0);
        }),
      { timeout: 10_000 },
    )
    .toBeGreaterThan(100);
});
