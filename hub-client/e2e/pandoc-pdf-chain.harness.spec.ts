/**
 * "Download as PDF" in a browser (host phase H8): the production `DownloadController`
 * (Rust request -> pandoc.wasm worker -> typst worker -> a real download) on typst fixtures.
 * Needs a VITE_E2E=1 build with the pandoc and typst assets (`npm run test:harness`) and no hub
 * server. The name matches the webkit project's `{pandoc,typst}-*` pattern.
 */
import { readFile } from 'node:fs/promises';
import { expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import { countPdfPages } from '../../ts-packages/typst-host/src/pdf';

const PNG_B64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==';

async function boot(page: Page) {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
}

async function seed(page: Page, files: { path: string; text?: string; base64?: string }[]) {
  await page.evaluate(
    async ({ files }) => {
      const { wasmRenderer } = window.__quartoTest!;
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      for (const f of files) {
        if (f.base64 !== undefined) wasmRenderer.vfsAddBinaryFile(f.path, Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0)));
        else wasmRenderer.vfsAddFile(f.path, f.text ?? '');
      }
    },
    { files },
  );
}

const start = (page: Page, path: string, options: { cancelOnStage?: string } = {}) =>
  page.evaluate(({ path, options }) => window.__quartoTest!.pandoc.startDownload(path, 'pdf', options), { path, options });

test.describe('Download as PDF (pandoc.wasm then typst)', () => {
  // The first run fetches and compiles both wasm modules.
  test.setTimeout(240_000);

  test('a typst document with an image becomes a two-page PDF download', async ({ page }) => {
    await boot(page);
    await seed(page, [
      { path: '/doc/report.qmd', text: '---\ntitle: Chain\n---\n\n# One\n\n![A pixel](pixel.png)\n\n```{=typst}\n#pagebreak()\n```\n\nPage two.\n' },
      { path: '/doc/pixel.png', base64: PNG_B64 },
    ]);
    const downloaded = page.waitForEvent('download', { timeout: 200_000 });
    const summary = await start(page, '/doc/report.qmd');
    expect(summary, JSON.stringify(summary)).toMatchObject({ phase: 'done', fileName: 'report.pdf' });
    // Both stages showed progress, in order.
    const order = (s: string) => summary.stages.indexOf(s);
    expect(order('typst-loading')).toBeGreaterThanOrEqual(0);
    expect(order('running')).toBeGreaterThan(order('typst-loading'));
    expect(order('typst-compiling')).toBeGreaterThan(order('running'));
    const download = await downloaded;
    expect(download.suggestedFilename()).toBe('report.pdf');
    const file = await download.path();
    const bytes = new Uint8Array(await readFile(file!));
    expect(new TextDecoder().decode(bytes.slice(0, 5))).toBe('%PDF-');
    expect(countPdfPages(bytes)).toBe(2);
  });

  test('a typst error blocks the download and is attributed to the typst stage', async ({ page }) => {
    await boot(page);
    await seed(page, [{ path: '/doc/bad.qmd', text: '---\ntitle: Bad\n---\n\n```{=typst}\n#no-such-function()\n```\n' }]);
    let downloads = 0;
    page.on('download', () => downloads++);
    const summary = await start(page, '/doc/bad.qmd');
    expect(summary).toMatchObject({ phase: 'failed', state: 'typst-error' });
    expect(summary.diagnostics.some((d) => (d as { stage?: string }).stage === 'typst')).toBe(true);
    expect(downloads).toBe(0);
  });

  test('a cancel during the compile reports once and downloads nothing', async ({ page }) => {
    await boot(page);
    await seed(page, [{ path: '/doc/c.qmd', text: '---\ntitle: C\n---\n\nHello.\n' }]);
    let downloads = 0;
    page.on('download', () => downloads++);
    const summary = await start(page, '/doc/c.qmd', { cancelOnStage: 'typst-compiling' });
    expect(summary).toMatchObject({ phase: 'cancelled', cancelledCount: 1 });
    // The page stays usable: a second click completes.
    expect(downloads).toBe(0);
    const downloaded = page.waitForEvent('download', { timeout: 60_000 });
    const again = await start(page, '/doc/c.qmd');
    expect(again).toMatchObject({ phase: 'done', fileName: 'c.pdf' });
    expect((await downloaded).suggestedFilename()).toBe('c.pdf');
  });
});
