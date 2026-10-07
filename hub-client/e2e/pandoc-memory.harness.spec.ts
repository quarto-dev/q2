/**
 * Memory budget for image-heavy documents (host phase H6). The budget is the one H3
 * measured (evidence section 12): pandoc's own linear memory is ~2-3x the image payload
 * plus ~50 MB, and the browser process grows ~11-17x the payload while a render runs.
 * This spec asserts the parts a CI browser can see without process-level tooling:
 *
 *  - pandoc's linear memory (`timings.memoryBytes`) stays within the budget, every browser;
 *  - the main thread's Rust wasm memory (VFS, request copies; it never shrinks) reaches a
 *    plateau instead of growing with every render, every browser;
 *  - the main thread's JS heap, after a forced GC, returns near its pre-render size, so a
 *    finished render retains no copies of the images or the output (Chromium, via CDP:
 *    `measureUserAgentSpecificMemory` needs cross-origin isolation, which the app lacks).
 *
 * Process RSS, where the large numbers show up, is read by the opt-in
 * pandoc-measure.harness.spec.ts (`ps`), not here.
 */
import { expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import { seedImages } from './helpers/seedImages';

const MB = 1024 * 1024;
const SDE = 1_700_000_000;
const RENDERS = 3;

async function boot(page: Page) {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
}

test('an image-heavy docx stays within the memory budget and a repeat render does not grow it', async ({ page, browserName }) => {
  test.setTimeout(5 * 60_000);
  await boot(page);

  const cdp = browserName === 'chromium' ? await page.context().newCDPSession(page) : undefined;
  // JS heap plus ArrayBuffer backing stores (the images and the output live in the latter,
  // which `Performance.getMetrics`' JSHeapUsedSize does not count), after a forced GC.
  const heapMb = async () => {
    if (!cdp) return undefined;
    await cdp.send('HeapProfiler.collectGarbage');
    const u = await cdp.send('Runtime.getHeapUsage');
    return Math.round((u.usedSize + u.backingStorageSize) / MB);
  };

  const payload = await seedImages(page, 10, 10); // ten ~10 MB incompressible PNGs
  expect(payload).toBeGreaterThan(90 * MB);
  const heapBefore = await heapMb();
  const rustBefore = await page.evaluate(() => window.__quartoTest!.pandoc.rustWasmMemoryBytes());

  const rows: { pandocMb: number; rustMb: number; heapMb?: number; outputMb: number; ms: number }[] = [];
  for (let i = 0; i < RENDERS; i++) {
    const r = await page.evaluate(async (sde) => {
      const t0 = performance.now();
      const o = await window.__quartoTest!.pandoc.download('/project/images.qmd', { format: 'docx', sourceDateEpoch: sde, save: false });
      return {
        ok: o.ok,
        detail: o.ok ? '' : JSON.stringify({ failure: o.failure, requestError: o.requestError, diagnostics: o.diagnostics }),
        pandocBytes: o.timings.memoryBytes ?? 0,
        outputBytes: o.output?.byteLength ?? 0,
        rustBytes: await window.__quartoTest!.pandoc.rustWasmMemoryBytes(),
        ms: Math.round(performance.now() - t0),
      };
    }, SDE);
    expect(r.ok, r.detail).toBe(true);
    rows.push({ pandocMb: Math.round(r.pandocBytes / MB), rustMb: Math.round(r.rustBytes / MB), heapMb: await heapMb(), outputMb: Math.round(r.outputBytes / MB), ms: r.ms });
  }
  console.log(`memory: ${browserName} payload ${Math.round(payload / MB)} MB, before: rust ${Math.round(rustBefore / MB)} MB heap ${heapBefore} MB; renders ${JSON.stringify(rows)}`);
  test.info().annotations.push({ type: 'memory', description: JSON.stringify(rows) });

  // Bounds from the H3/H6 measurements with ~1.5x headroom (105 MB of images, Chromium and
  // WebKit alike: pandoc 203 MB = 1.9x payload, Rust wasm 494 MB = 4.7x payload; at the
  // 300 MB limit 2.7x and 4.4x). A bound that fails means the chain started copying more.
  for (const r of rows) {
    expect(r.pandocMb, 'pandoc linear memory').toBeLessThanOrEqual(64 + Math.round((3.5 * payload) / MB));
    expect(r.rustMb, 'Rust wasm linear memory (never shrinks)').toBeLessThanOrEqual(128 + Math.round((6 * payload) / MB));
    expect(r.outputMb, 'the docx carries the images').toBeGreaterThanOrEqual(Math.round(payload / MB) - 5);
  }
  // A repeat render reuses what the first one grew: no per-render leak in either memory.
  expect(rows[RENDERS - 1].rustMb, 'Rust wasm grows with every render').toBeLessThanOrEqual(rows[0].rustMb + 16);
  expect(rows[RENDERS - 1].pandocMb).toBeLessThanOrEqual(rows[0].pandocMb + 16);
  if (cdp) {
    // After a GC the page holds no copies of the images or of the output (Chromium only).
    expect(rows[RENDERS - 1].heapMb! - heapBefore!, 'JS heap and ArrayBuffers retained after the renders').toBeLessThanOrEqual(64);
  }
});
