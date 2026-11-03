/**
 * The pdf.js viewer for compiled PDFs (host phase H9): a recompile keeps the reader's page and
 * zoom, a different file starts over. Drives the production chain (`DownloadController`) into the
 * viewer through the test hook. Needs a VITE_E2E=1 build with the pandoc, typst and pdf.js assets.
 * The name matches the webkit project's `{pandoc,typst}-*` pattern.
 *
 * Synchronisation: every wait is on the state the next step needs, never on a proxy for it.
 * pdf.js exposes `pagesCount` and `pdfDocument` as soon as the document is open, before any page is
 * drawn and before the viewer promotes the frame; a step that proceeded on those alone armed the
 * flash guard against an unpainted first frame (the `__empty` 2–6 flake) and issued the edit while
 * the pane had no PDF yet (the "Updating" status never seen), see
 * claude-notes/plans/2026-11-02-pdf-viewer-harness-flake-root-cause.md. So "a PDF is on screen" is
 * the promoted frame (`iframe[title="PDF preview"]`, set in the same synchronous step that makes it
 * live, after its page has painted), and "the reader is at page N" includes that page's view being
 * in pdf.js's FINISHED state. The patched worker reports one fingerprint for every document, so a
 * replacement is told from its predecessor by tagging the live frame before the edit.
 */
import { expect, test, type Page, type TestInfo } from '@playwright/test';
import type {} from './helpers/testHooks';

const PAGES = 12;
/** The promoted (live) frame: `VIEWER_TITLE` in pdfViewer.ts. */
const LIVE = '#pdf-viewer-host iframe[title="PDF preview"]';
const FRAMES = '#pdf-viewer-host iframe';
/** Short polls for state that is one or two frames away; the long waits are locator assertions. */
const FAST = { intervals: [25] };

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
  page.evaluate(async (path) => (await window.__quartoTest!.pandoc.startDownload(path, 'typst-pdf')).phase, path);

/**
 * What the reader sees in the live frame, or null until one is promoted: page, zoom, page count,
 * and whether the current page is drawn at that layout (pdf.js `RenderingStates.FINISHED`).
 */
const viewState = (page: Page) =>
  page.evaluate((live) => {
    const frame = document.querySelector<HTMLIFrameElement>(live);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const app = (frame?.contentWindow as any)?.PDFViewerApplication;
    const v = app?.pdfViewer;
    if (!v?.pagesCount || !app.pdfDocument) return null;
    return {
      page: v.currentPageNumber as number,
      scale: Math.round(v.currentScale * 100) / 100,
      pages: v.pagesCount as number,
      marker: app.pdfDocument.fingerprints[0] as string,
      painted: v.getPageView(v.currentPageNumber - 1)?.renderingState === 3,
    };
  }, LIVE);

/** A document is on screen: one promoted frame, its current page drawn. */
async function shown(page: Page, timeout: number) {
  await expect(page.locator(LIVE)).toHaveCount(1, { timeout });
  await expect.poll(() => viewState(page), { ...FAST, timeout: 30_000 }).toMatchObject({ pages: PAGES, painted: true });
}

/** Tags the live frame, so `replaced` can tell the next document from this one. */
const tagLive = (page: Page) => page.evaluate((live) => document.querySelector(live)?.setAttribute('data-e2e-old', ''), LIVE);

/** A document other than the tagged one is on screen (promoted, so painted) and the tagged one is gone. */
async function replaced(page: Page, timeout: number) {
  await expect(page.locator(`${LIVE}:not([data-e2e-old])`)).toHaveCount(1, { timeout });
  await expect(page.locator(FRAMES)).toHaveCount(1);
  await expect.poll(() => viewState(page), { ...FAST, timeout: 30_000 }).toMatchObject({ pages: PAGES, painted: true });
}

/** The reader moves to `pageNumber` at `scale`; returns once that page is drawn at that zoom. */
async function goTo(page: Page, pageNumber: number, scale: number) {
  await page.evaluate(
    ([live, n, s]) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const app = (document.querySelector<HTMLIFrameElement>(live)!.contentWindow as any).PDFViewerApplication;
      app.pdfViewer.currentScaleValue = String(s);
      app.pdfViewer.currentPageNumber = n;
    },
    [LIVE, pageNumber, scale] as const,
  );
  await expect.poll(() => viewState(page), { ...FAST, timeout: 60_000 }).toMatchObject({ page: pageNumber, scale, painted: true });
}

interface GuardReport {
  /** rAF samples in which the topmost host frame held no drawn page. */
  empty: number;
  /** The first of those, with every host frame at that moment. */
  samples: { t: number; frames: { z: string; title: string; hasDoc: boolean; canvases: number; state: number }[] }[];
  /** The pane rendered its "Updating" status at some point after arming. */
  sawStatus: boolean;
  /** Host and pane changes since arming: frames added/removed/promoted, overlays shown/hidden. */
  timeline: { t: number; event: string }[];
}

/**
 * Arms the flash guard: from now on, on every animation frame, the frame on top must hold a drawn page
 * (the invariant of pdfViewer.ts: the frame on top is never an empty viewer). Also latches the pane's
 * "Updating" status, which lives for only a few hundred milliseconds on a fast machine, and records a
 * timeline of the swap, so a failure explains itself. Read back with `readGuard`.
 */
function armGuard(page: Page) {
  return page.evaluate((framesSel) => {
    const w = window as unknown as { __guard: GuardReport & { on: boolean } };
    const t0 = performance.now();
    const t = () => Math.round(performance.now() - t0);
    const g: GuardReport & { on: boolean } = { on: true, empty: 0, samples: [], sawStatus: false, timeline: [] };
    w.__guard = g;
    const describe = (n: Node) => {
      const el = n as HTMLElement;
      return el.getAttribute?.('data-testid') ?? (el.tagName === 'IFRAME' ? `iframe z=${el.style.zIndex} title=${JSON.stringify(el.title)}` : el.tagName);
    };
    new MutationObserver((muts) => {
      for (const m of muts) {
        if (m.type === 'attributes') {
          const el = m.target as HTMLElement;
          if (el.tagName === 'IFRAME' && m.attributeName === 'title') g.timeline.push({ t: t(), event: `promoted ${describe(el)}` });
          continue;
        }
        m.addedNodes.forEach((n) => {
          if (n.nodeType !== Node.ELEMENT_NODE) return;
          if ((n as HTMLElement).getAttribute('data-testid') === 'pdf-preview-status') g.sawStatus = true;
          g.timeline.push({ t: t(), event: `added ${describe(n)}` });
        });
        m.removedNodes.forEach((n) => n.nodeType === Node.ELEMENT_NODE && g.timeline.push({ t: t(), event: `removed ${describe(n)}` }));
      }
    }).observe(document.getElementById('pdf-viewer-host')!, { childList: true, subtree: true, attributes: true, attributeFilter: ['title'] });
    const tick = () => {
      const frames = [...document.querySelectorAll<HTMLIFrameElement>(framesSel)];
      const top = frames.sort((a, b) => Number(b.style.zIndex) - Number(a.style.zIndex))[0];
      if (!top?.contentDocument?.querySelector('.page canvas')) {
        g.empty++;
        if (g.samples.length < 20) {
          g.samples.push({
            t: t(),
            frames: frames.map((f) => {
              // eslint-disable-next-line @typescript-eslint/no-explicit-any
              const v = (f.contentWindow as any)?.PDFViewerApplication?.pdfViewer;
              return {
                z: f.style.zIndex,
                title: f.title,
                hasDoc: !!f.contentDocument,
                canvases: f.contentDocument?.querySelectorAll('.page canvas').length ?? -1,
                state: v?.getPageView?.(v.currentPageNumber - 1)?.renderingState ?? -1,
              };
            }),
          });
        }
      }
      if (g.on) requestAnimationFrame(tick);
    };
    tick();
  }, FRAMES);
}

/** Stops the guard and returns what it saw; attaches the report to the test when something is off. */
async function readGuard(page: Page, testInfo: TestInfo): Promise<GuardReport> {
  const g = await page.evaluate(() => {
    const w = window as unknown as { __guard: GuardReport & { on: boolean } };
    w.__guard.on = false;
    const { on: _on, ...report } = w.__guard;
    return report;
  });
  if (g.empty > 0 || !g.sawStatus) await testInfo.attach('pdf-viewer-guard.json', { body: JSON.stringify(g, null, 2), contentType: 'application/json' });
  return g;
}

test.describe('PDF viewer', () => {
  test.setTimeout(240_000);

  test('a recompile keeps the page and zoom; another file starts over', async ({ page }) => {
    await boot(page);
    await page.evaluate(() => window.__quartoTest!.pandoc.mountPdfViewer());
    await write(page, '/doc/a.qmd', document('first'));
    expect(await render(page, '/doc/a.qmd')).toBe('done');
    await shown(page, 60_000);
    expect(await viewState(page)).toMatchObject({ pages: PAGES, page: 1 });

    // The reader moves to page 6 at 150%.
    await goTo(page, 6, 1.5);
    const before = (await viewState(page))!;

    // An edit that changes the PDF (so its own /ID changes); the viewer reopens in place.
    await tagLive(page);
    await write(page, '/doc/a.qmd', document('second'));
    expect(await render(page, '/doc/a.qmd')).toBe('done');
    await replaced(page, 60_000);
    expect(await viewState(page)).toMatchObject({ page: 6, scale: 1.5, pages: PAGES, marker: before.marker, painted: true });

    // A different file does not inherit the position.
    await tagLive(page);
    await write(page, '/doc/b.qmd', document('other'));
    expect(await render(page, '/doc/b.qmd')).toBe('done');
    await replaced(page, 60_000);
    const other = (await viewState(page))!;
    expect(other).toMatchObject({ page: 1, pages: PAGES, painted: true });
    expect(other.scale).not.toBe(1.5);
  });

  test('the preview pane: it compiles on mount, an edit recompiles in place and keeps the reader where they were', async ({ page }, testInfo) => {
    await boot(page);
    await write(page, '/doc/p.qmd', document('one'));
    // The pane reads the VFS; the content prop is what triggers a recompile.
    const pane = (marker: string) =>
      page.evaluate((m) => (window as unknown as { __pane?: { update(c: string): void } }).__pane?.update(m), marker);
    await page.evaluate(() => {
      (window as unknown as { __pane: unknown }).__pane = window.__quartoTest!.pandoc.mountPdfPreviewPane('/doc/p.qmd', 'one');
    });
    await expect(page.getByTestId('pdf-preview-loading')).toBeVisible();
    // The first compile fetches the modules, then the viewer must draw and promote the frame; WebKit
    // CI runners take tens of seconds for the first part (bd-ynqwyyi7).
    await shown(page, 120_000);
    expect(await viewState(page)).toMatchObject({ pages: PAGES, page: 1 });
    // The pane has a PDF (the first show() resolved), so its spinner is gone and an edit will show "Updating".
    await expect(page.getByTestId('pdf-preview-loading')).toBeHidden();

    await goTo(page, 7, 1.5);

    // While the edit recompiles and swaps in, the frame on top must never be an empty viewer (the flash),
    // and the pane must say it is updating.
    await armGuard(page);
    await tagLive(page);
    await write(page, '/doc/p.qmd', document('two'));
    await pane('two');
    // The compile, then the viewer reopening the new bytes in place (debounce + worker acquisition +
    // compile can take tens of seconds on a loaded WebKit runner, then up to the viewer's 30 s watchdog).
    await replaced(page, 120_000);
    await expect(page.getByTestId('pdf-preview-status')).toBeHidden();
    expect(await viewState(page)).toMatchObject({ page: 7, scale: 1.5, pages: PAGES, painted: true });
    const guard = await readGuard(page, testInfo);
    expect(guard.sawStatus, 'the pane showed its "Updating" status while recompiling').toBe(true);
    expect(guard.empty, 'animation frames in which the top viewer frame held no drawn page').toBe(0);

    // A broken edit keeps the last PDF under an error banner.
    await write(page, '/doc/p.qmd', '---\ntitle: Bad\n---\n\n```{=typst}\n#no-such-function()\n```\n');
    await pane('bad');
    await expect(page.getByTestId('pdf-preview-error')).toBeVisible({ timeout: 60_000 });
    expect(await viewState(page)).toMatchObject({ page: 7, pages: PAGES, painted: true });
    await expect(page.locator(FRAMES)).toHaveCount(1);
  });
});
