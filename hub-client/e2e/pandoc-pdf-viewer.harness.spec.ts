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
});
