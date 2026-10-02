/**
 * The pdf.js viewer for compiled PDFs (host phase H9): a recompile keeps the reader's page and
 * zoom, a different file starts over. Drives the production chain (`DownloadController`) into the
 * viewer through the test hook. Needs a VITE_E2E=1 build with the pandoc, typst and pdf.js assets.
 * The name matches the webkit project's `{pandoc,typst}-*` pattern.
 */
import { expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';

const PAGES = 12;

function document(marker: string): string {
  const body = Array.from({ length: PAGES }, (_, i) => `# Page ${i + 1}\n\nBody of page ${i + 1}${i === PAGES - 1 ? ` ${marker}` : ''}.\n`);
  return `---\ntitle: Viewer\n---\n\n${body.join('\n```{=typst}\n#pagebreak()\n```\n\n')}`;
}

async function boot(page: Page) {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
    await window.__quartoTest!.wasmRenderer.initWasm();
    window.__quartoTest!.wasmRenderer.vfsClear();
  });
}

const write = (page: Page, path: string, text: string) =>
  page.evaluate(({ path, text }) => window.__quartoTest!.wasmRenderer.vfsAddFile(path, text), { path, text });

const render = (page: Page, path: string) =>
  page.evaluate(async (path) => (await window.__quartoTest!.pandoc.startDownload(path, 'pdf')).phase, path);

/** What the reader sees, once the viewer has finished laying out (and restoring) a document. */
const viewState = (page: Page) =>
  page.evaluate(() => {
    const frame = document.querySelector<HTMLIFrameElement>('#pdf-viewer-host iframe');
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const app = (frame?.contentWindow as any)?.PDFViewerApplication;
    const v = app?.pdfViewer;
    if (!v?.pagesCount || !app.pdfDocument) return null;
    return { page: v.currentPageNumber as number, scale: Math.round(v.currentScale * 100) / 100, pages: v.pagesCount as number, marker: app.pdfDocument.fingerprints[0] as string };
  });

test.describe('PDF viewer', () => {
  test.setTimeout(240_000);

  test('a recompile keeps the page and zoom; another file starts over', async ({ page }) => {
    await boot(page);
    await page.evaluate(() => window.__quartoTest!.pandoc.mountPdfViewer());
    await write(page, '/doc/a.qmd', document('first'));
    expect(await render(page, '/doc/a.qmd')).toBe('done');
    await expect.poll(() => viewState(page), { timeout: 60_000 }).toMatchObject({ pages: PAGES, page: 1 });

    // The reader moves to page 6 at 150%.
    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const app = (document.querySelector<HTMLIFrameElement>('#pdf-viewer-host iframe')!.contentWindow as any).PDFViewerApplication;
      app.pdfViewer.currentScaleValue = '1.5';
      app.pdfViewer.currentPageNumber = 6;
    });
    await expect.poll(() => viewState(page)).toMatchObject({ page: 6, scale: 1.5 });
    const before = (await viewState(page))!;

    // An edit that changes the PDF (so its own /ID changes); the viewer reopens in place.
    await write(page, '/doc/a.qmd', document('second'));
    expect(await render(page, '/doc/a.qmd')).toBe('done');
    await expect.poll(async () => (await viewState(page))?.page, { timeout: 60_000 }).toBe(6);
    expect(await viewState(page)).toMatchObject({ page: 6, scale: 1.5, pages: PAGES, marker: before.marker });

    // A different file does not inherit the position.
    await write(page, '/doc/b.qmd', document('other'));
    expect(await render(page, '/doc/b.qmd')).toBe('done');
    await expect.poll(() => viewState(page), { timeout: 60_000 }).toMatchObject({ page: 1, pages: PAGES });
    expect((await viewState(page))!.scale).not.toBe(1.5);
  });

  test('the preview pane: it compiles on mount, an edit recompiles in place and keeps the reader where they were', async ({ page }) => {
    await boot(page);
    await write(page, '/doc/p.qmd', document('one'));
    // The pane reads the VFS; the content prop is what triggers a recompile.
    const pane = (marker: string) =>
      page.evaluate((m) => (window as unknown as { __pane?: { update(c: string): void } }).__pane?.update(m), marker);
    await page.evaluate(() => {
      (window as unknown as { __pane: unknown }).__pane = window.__quartoTest!.pandoc.mountPdfPreviewPane('/doc/p.qmd', 'one');
    });
    await expect(page.getByTestId('pdf-preview-loading')).toBeVisible();
    await expect.poll(() => viewState(page), { timeout: 120_000 }).toMatchObject({ pages: PAGES, page: 1 });

    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const app = (document.querySelector<HTMLIFrameElement>('#pdf-viewer-host iframe')!.contentWindow as any).PDFViewerApplication;
      app.pdfViewer.currentScaleValue = '1.5';
      app.pdfViewer.currentPageNumber = 7;
    });
    await expect.poll(() => viewState(page)).toMatchObject({ page: 7, scale: 1.5 });

    // While the edit recompiles and swaps in, the frame on top must never be an empty viewer (the flash).
    await page.evaluate(() => {
      const w = window as unknown as { __empty: number; __sample: boolean };
      w.__empty = 0;
      w.__sample = true;
      const tick = () => {
        const frames = [...document.querySelectorAll<HTMLIFrameElement>('#pdf-viewer-host iframe')];
        const top = frames.sort((a, b) => Number(b.style.zIndex) - Number(a.style.zIndex))[0];
        if (!top?.contentDocument?.querySelector('.page canvas')) w.__empty++;
        if (w.__sample) requestAnimationFrame(tick);
      };
      tick();
    });
    await write(page, '/doc/p.qmd', document('two'));
    await pane('two');
    await expect(page.getByTestId('pdf-preview-status')).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId('pdf-preview-status')).toBeHidden({ timeout: 60_000 });
    // The compile is done; the viewer then reopens the new bytes in place.
    await expect.poll(() => viewState(page), { timeout: 30_000 }).toMatchObject({ page: 7, scale: 1.5, pages: PAGES });
    await expect.poll(() => page.evaluate(() => document.querySelectorAll('#pdf-viewer-host iframe').length)).toBe(1);
    expect(await page.evaluate(() => { const w = window as unknown as { __empty: number; __sample: boolean }; w.__sample = false; return w.__empty; })).toBe(0);

    // A broken edit keeps the last PDF under an error banner.
    await write(page, '/doc/p.qmd', '---\ntitle: Bad\n---\n\n```{=typst}\n#no-such-function()\n```\n');
    await pane('bad');
    await expect(page.getByTestId('pdf-preview-error')).toBeVisible({ timeout: 60_000 });
    await expect.poll(() => viewState(page)).toMatchObject({ page: 7, pages: PAGES });
  });
});
