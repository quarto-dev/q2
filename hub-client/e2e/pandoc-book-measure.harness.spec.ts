/**
 * Whole-book request measurements (pandoc-wasm R9 task 9). Opt-in: the numbers are recorded in
 * the plan's Handoff log, not asserted (no budget is imposed; Q-9-4):
 *
 *   VITE_E2E=1 npm run build
 *   Q2_MEASURE=1 Q2_MEASURE_OUT=/tmp/q2-book-measure npx playwright test \
 *     --config playwright.harness.config.ts e2e/pandoc-book-measure.harness.spec.ts \
 *     --retries=0 --workers=1 --project=chromium
 *
 * Drives the real wasm export (`renderPandocRequest`, no pandoc run) in Chromium and records, per
 * scenario: wall time to a request, time to the first `onProgress` call (`pass_one` over every
 * project file plus `pre_render`; it is paid on every click), request size, the JS heap +
 * ArrayBuffer delta after a GC, the Rust wasm linear memory (it never shrinks) and the summed
 * process RSS peak (`ps`, every process whose command line contains `ms-playwright`).
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { chromium, expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import { seedImages } from './helpers/seedImages';

const OUT = process.env.Q2_MEASURE_OUT ?? '/tmp/q2-book-measure';
// Another worktree may already serve its own build on 5173: point this at your own preview server.
const BASE = process.env.Q2_MEASURE_BASE ?? 'http://localhost:5173/';
const MB = 1024 * 1024;
const SDE = 1_700_000_000;

test.skip(!process.env.Q2_MEASURE, 'opt-in: set Q2_MEASURE=1');

function rssMb(): number {
  const out = execFileSync('ps', ['-axo', 'rss=,command='], { maxBuffer: 1 << 26 }).toString('utf8');
  let kb = 0;
  for (const line of out.split('\n')) {
    if (!line.includes('ms-playwright')) continue;
    kb += Number(line.trim().split(/\s+/)[0]) || 0;
  }
  return Math.round(kb / 1024);
}

async function withPeakRss<T>(fn: () => Promise<T>): Promise<{ value: T; peakMb: number; startMb: number }> {
  const startMb = rssMb();
  let peak = startMb;
  const timer = setInterval(() => (peak = Math.max(peak, rssMb())), 100);
  try {
    return { value: await fn(), peakMb: peak, startMb };
  } finally {
    clearInterval(timer);
    peak = Math.max(peak, rssMb());
  }
}

async function boot(page: Page) {
  await page.goto(BASE);
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
}

interface Row {
  ok: boolean;
  error?: string;
  chapters?: number;
  stats?: string;
  diag?: string[];
  scope?: string;
  totalMs: number;
  firstProgressMs?: number;
  requestMb?: number;
  rustMb: number;
  previewMs?: number[];
  previewError?: string;
}

/**
 * A book of `n` short chapters (and an `index.qmd`) in the VFS. Each chapter has a heading, a cross-reference
 * and a few paragraphs, so the per-chapter cost is a realistic small page, not an empty one.
 */
async function seedBook(page: Page, n: number) {
  await page.evaluate(
    async ({ n }) => {
      const { wasmRenderer } = window.__quartoTest!;
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      const chapters = ['index.qmd', ...Array.from({ length: n }, (_, i) => `c${i + 1}.qmd`)];
      wasmRenderer.vfsAddFile(
        '/project/_quarto.yml',
        `project:\n  type: book\nbook:\n  title: Measured\n  chapters:\n${chapters.map((c) => `    - ${c}`).join('\n')}\n`,
      );
      wasmRenderer.vfsAddFile('/project/index.qmd', '# Preface\n\nHello.\n');
      const para = 'Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore. ';
      for (let i = 1; i <= n; i++) {
        wasmRenderer.vfsAddFile(
          `/project/c${i}.qmd`,
          `# Chapter ${i} {#sec-c${i}}\n\n${para.repeat(3)}\n\nSee @sec-c${Math.max(1, i - 1)}.\n\n## Part\n\n${para.repeat(2)}\n`,
        );
      }
    },
    { n },
  );
}

/** One request through the export; `concurrentPreview` loops preview renders while it runs. */
async function request(page: Page, file: string, format: string, concurrentPreview = false): Promise<Row> {
  return page.evaluate(
    async ({ file, format, concurrentPreview, sde }) => {
      const { wasmRenderer } = window.__quartoTest!;
      const rustMb = async () => Math.round((await window.__quartoTest!.pandoc.rustWasmMemoryBytes()) / 1048576);
      const t0 = performance.now();
      let firstProgressMs: number | undefined;
      const previewMs: number[] = [];
      let previewError: string | undefined;
      let stop = false;
      let previews: Promise<void> | undefined;
      if (concurrentPreview) {
        // The preview pane recompiling while the download runs: renders interleave at the loop's yields.
        previews = (async () => {
          while (!stop) {
            const p0 = performance.now();
            try {
              await wasmRenderer.renderPageForPreview('/project/index.qmd');
            } catch (e) {
              previewError = String(e);
              return;
            }
            previewMs.push(Math.round(performance.now() - p0));
            await new Promise((r) => setTimeout(r, 0));
          }
        })();
      }
      try {
        const out = await wasmRenderer.renderPandocRequest(file, format, {
          sourceDateEpoch: sde,
          onProgress: () => {
            firstProgressMs ??= Math.round(performance.now() - t0);
          },
        });
        stop = true;
        await previews;
        const req = out.request;
        const bytes = req
          ? [...req.files, ...req.resource_refs].reduce((n, f) => n + f.bytes.byteLength, 0)
          : undefined;
        return {
          ok: out.success,
          error: out.success ? undefined : out.error,
          stats: JSON.stringify(out.stats),
          diag: out.diagnostics.map((d: { title: string }) => d.title).slice(0, 3),
          chapters: out.stats?.book?.chapters,
          scope: out.stats?.book?.scope,
          totalMs: Math.round(performance.now() - t0),
          firstProgressMs,
          requestMb: bytes === undefined ? undefined : Math.round((bytes / 1048576) * 10) / 10,
          rustMb: await rustMb(),
          previewMs,
          previewError,
        };
      } catch (e) {
        stop = true;
        await previews?.catch(() => {});
        return { ok: false, error: `THROWN: ${String(e)}`, totalMs: Math.round(performance.now() - t0), rustMb: await rustMb(), previewMs, previewError };
      }
    },
    { file, format, concurrentPreview, sde: SDE },
  );
}

test('measure: whole-book request (chromium)', async () => {
  test.setTimeout(30 * 60_000);
  const browser = await chromium.launch({ args: ['--js-flags=--expose-gc'] });
  const result: Record<string, unknown> = { browser: 'chromium', version: browser.version(), date: new Date().toISOString() };
  try {
    const context = await browser.newContext({ baseURL: BASE });
    const page = await context.newPage();
    const crashes: string[] = [];
    page.on('crash', () => crashes.push('page crashed'));
    await boot(page);
    const cdp = await context.newCDPSession(page);
    const heapMb = async () => {
      await cdp.send('HeapProfiler.collectGarbage');
      const u = await cdp.send('Runtime.getHeapUsage');
      return Math.round((u.usedSize + u.backingStorageSize) / MB);
    };
    result.rssIdleMb = rssMb();

    // --- small to large books, typst; chapter-alone shows what `pass_one` alone costs ---
    const books: Record<string, unknown> = {};
    for (const n of [10, 50, 200]) {
      await boot(page);
      await seedBook(page, n);
      const heapBefore = await heapMb();
      const warm = await request(page, '/project/c1.qmd', 'typst'); // module warm-up and first-use costs
      const runs: Row[] = [];
      for (let i = 0; i < 3; i++) runs.push(await request(page, '/project/c1.qmd', 'typst'));
      const alone: Row[] = [];
      for (let i = 0; i < 3; i++) {
        alone.push(
          await page.evaluate(async (sde) => {
            const { wasmRenderer } = window.__quartoTest!;
            const t0 = performance.now();
            const out = await wasmRenderer.renderPandocRequest('/project/c1.qmd', 'typst', { sourceDateEpoch: sde, scope: 'chapter' });
            return { ok: out.success, error: out.error, totalMs: Math.round(performance.now() - t0), rustMb: 0 };
          }, SDE),
        );
      }
      const epub = await request(page, '/project/c1.qmd', 'epub');
      const heapAfter = await heapMb();
      books[`${n}-chapters`] = {
        firstRun: warm,
        bookRuns: runs,
        chapterAloneMs: alone.map((a) => a.totalMs),
        epub,
        heapDeltaAfterGcMb: heapAfter - heapBefore,
      };
      expect(runs[0].ok, JSON.stringify(runs[0])).toBe(true);
    }
    result.books = books;

    // --- the concurrent preview recompile, on the 200-chapter book (still seeded) ---
    {
      const baseline: number[] = [];
      for (let i = 0; i < 5; i++) {
        baseline.push(
          await page.evaluate(async () => {
            const { wasmRenderer } = window.__quartoTest!;
            const t0 = performance.now();
            await wasmRenderer.renderPageForPreview('/project/index.qmd');
            return Math.round(performance.now() - t0);
          }),
        );
      }
      const withBook = await withPeakRss(() => request(page, '/project/c1.qmd', 'typst', true));
      result.concurrentPreview = { previewAloneMs: baseline, bookWithPreview: withBook.value, rssPeakMb: withBook.peakMb, rssStartMb: withBook.startMb };
    }

    // --- near-cap, image-heavy: about 250 MiB of images in five chapters ---
    {
      await boot(page);
      const payload = await seedImages(page, 25, 10);
      await page.evaluate(() => {
        const { wasmRenderer } = window.__quartoTest!;
        // Five chapters of five images each, plus the home page (seedImages wrote `images.qmd`).
        wasmRenderer.vfsRemoveFile('/project/images.qmd');
        const chapters = ['index.qmd', 'c1.qmd', 'c2.qmd', 'c3.qmd', 'c4.qmd', 'c5.qmd'];
        wasmRenderer.vfsAddFile('/project/_quarto.yml', `project:\n  type: book\nbook:\n  title: Heavy\n  chapters:\n${chapters.map((c) => `    - ${c}`).join('\n')}\n`);
        wasmRenderer.vfsAddFile('/project/index.qmd', '# Preface\n\nHello.\n');
        for (let c = 0; c < 5; c++) {
          let md = `# Chapter ${c + 1}\n\n`;
          for (let k = 0; k < 5; k++) md += `![Figure ${c * 5 + k}](img${c * 5 + k}.png)\n\n`;
          wasmRenderer.vfsAddFile(`/project/c${c + 1}.qmd`, md);
        }
      });
      const heapBefore = await heapMb();
      const rssSeeded = rssMb();
      const rows: Record<string, unknown> = { payloadMb: Math.round(payload / MB), rssSeededMb: rssSeeded, heapBeforeMb: heapBefore };
      for (const format of ['epub', 'pdf']) {
        const r = await withPeakRss(() => request(page, '/project/c1.qmd', format));
        rows[format] = { ...r.value, rssPeakMb: r.peakMb, rssStartMb: r.startMb, heapDeltaMb: (await heapMb()) - heapBefore };
      }
      // A preview-pane snapshot held at the same time: a preview render keeps its own copy of the VFS.
      const held = await withPeakRss(async () => {
        return page.evaluate(async (sde) => {
          const { wasmRenderer } = window.__quartoTest!;
          const t0 = performance.now();
          const preview = wasmRenderer.renderPageForPreview('/project/index.qmd');
          const out = await wasmRenderer.renderPandocRequest('/project/c1.qmd', 'epub', { sourceDateEpoch: sde });
          await preview;
          return { ok: out.success, error: out.error, totalMs: Math.round(performance.now() - t0) };
        }, SDE);
      });
      rows.epubWithPreviewInFlight = { ...held.value, rssPeakMb: held.peakMb, rssStartMb: held.startMb };
      result.nearCap = rows;
    }

    // --- past the cap: what a `memory.grow` failure does ---
    {
      await boot(page);
      const payload = await seedImages(page, 60, 10); // ~600 MB of images
      await page.evaluate(() => {
        const { wasmRenderer } = window.__quartoTest!;
        wasmRenderer.vfsRemoveFile('/project/images.qmd');
        let md = '# Everything\n\n';
        for (let k = 0; k < 60; k++) md += `![Figure ${k}](img${k}.png)\n\n`;
        wasmRenderer.vfsAddFile('/project/_quarto.yml', 'project:\n  type: book\nbook:\n  title: Huge\n  chapters:\n    - index.qmd\n    - c1.qmd\n');
        wasmRenderer.vfsAddFile('/project/index.qmd', '# Preface\n\nHello.\n');
        wasmRenderer.vfsAddFile('/project/c1.qmd', md);
      });
      const failing = await withPeakRss(() => request(page, '/project/c1.qmd', 'epub'));
      // Is the module usable afterwards?
      let after: unknown;
      try {
        after = await page.evaluate(async (sde) => {
          const { wasmRenderer } = window.__quartoTest!;
          wasmRenderer.vfsClear();
          wasmRenderer.vfsAddFile('/project/small.qmd', '# Small\n\nHello.\n');
          const out = await wasmRenderer.renderPandocRequest('/project/small.qmd', 'docx', { sourceDateEpoch: sde });
          return { ok: out.success, error: out.error };
        }, SDE);
      } catch (e) {
        after = { threw: String(e) };
      }
      result.pastTheCap = { payloadMb: Math.round(payload / MB), request: failing.value, rssPeakMb: failing.peakMb, afterwards: after, crashes };
    }
    await context.close();
  } finally {
    await browser.close();
    mkdirSync(OUT, { recursive: true });
    writeFileSync(path.join(OUT, 'book-measure-chromium.json'), JSON.stringify(result, null, 2));
  }
});
