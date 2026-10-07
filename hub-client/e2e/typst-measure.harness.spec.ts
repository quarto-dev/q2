/**
 * Browser measurements for the typst worker (pandoc-host H7): first-download and cached-load
 * latency, per-compile latency, the gzip size of the vendored typst-assets export, and peak
 * process memory with a typst compile resident next to a pandoc render and the Rust wasm.
 * Opt-in (timing numbers are not assertions):
 *
 *   node scripts/fetch-pandoc-wasm.mjs --require
 *   npm run build -w ts-packages/typst-host && VITE_E2E=1 npm run build
 *   Q2_MEASURE=1 Q2_MEASURE_OUT=/tmp/measure npx playwright test --config playwright.harness.config.ts \
 *     e2e/typst-measure.harness.spec.ts --retries=0 --workers=1
 *
 * Modelled on pandoc-measure.harness.spec.ts: it launches its own Chromium and WebKit and
 * writes one JSON file per browser to `Q2_MEASURE_OUT`. Process memory is the summed RSS of
 * every process whose command line contains `ms-playwright` (sampled every 100 ms for the
 * peak), so close other Playwright browsers first. Results are recorded in the evidence file
 * (claude-notes/research/2026-10-01-pandoc-wasm-evidence.md, section H7).
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { chromium, expect, test, webkit, type Browser, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';

const OUT = process.env.Q2_MEASURE_OUT ?? '/tmp/q2-measure';
const BASE = 'http://localhost:5173/';

test.skip(!process.env.Q2_MEASURE, 'opt-in: set Q2_MEASURE=1');

/** Summed RSS (MB) of the Playwright-launched browser processes. */
function rssMb(): number {
  const out = execFileSync('ps', ['-axo', 'rss=,command='], { maxBuffer: 1 << 26 }).toString('utf8');
  let kb = 0;
  for (const line of out.split('\n')) {
    if (!line.includes('ms-playwright')) continue;
    kb += Number(line.trim().split(/\s+/)[0]) || 0;
  }
  return Math.round(kb / 1024);
}

/** Run `fn` while sampling process RSS; returns the result and the peak seen. */
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

const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

/** A multi-page document with a table, a vendored package and some math, to give the compiler real work. */
const DOC = [
  '#import "@preview/fontawesome:0.5.0": fa-icon',
  '= Measure #fa-icon("heart")',
  ...Array.from({ length: 12 }, (_, i) => `== Section ${i}\n${'Lorem ipsum dolor sit amet, consectetur adipiscing elit. '.repeat(30)}\n$ sum_(k=1)^n k = (n(n+1))/2 $\n#table(columns: 3, ..range(30).map(str))\n#pagebreak()`),
].join('\n');

/** Compile once through a fresh runner (or the one on `window`); returns timings and the PDF size. */
async function compile(page: Page, body: string, fresh: boolean) {
  return page.evaluate(
    async ({ body, fresh }) => {
      const { typst } = window.__quartoTest!;
      const w = window as unknown as { __typstRunner?: ReturnType<typeof typst.createRunner> };
      if (fresh || !w.__typstRunner) w.__typstRunner = typst.createRunner({ idleMs: 10 * 60_000 });
      const { runner } = w.__typstRunner;
      const t0 = performance.now();
      const vendored = await typst.vendoredAssets();
      const vendoredMs = Math.round(performance.now() - t0);
      const o = await runner.run({ input: { main: '/doc/main.typ', files: [{ path: '/doc/main.typ', bytes: new TextEncoder().encode(body) }] }, fonts: vendored.fonts, vendoredPackages: vendored.vendoredPackages });
      const rustWasmMb = Math.round((await window.__quartoTest!.pandoc.rustWasmMemoryBytes()) / 1048576);
      return { ok: o.ok, kind: o.ok ? undefined : o.kind, pages: o.ok ? o.pages : undefined, pdfBytes: o.ok ? o.pdf.byteLength : undefined, stats: o.ok ? o.stats : undefined, diagnostics: o.diagnostics.map((d) => (d as { message?: string }).message), vendoredMs, totalMs: Math.round(performance.now() - t0), rustWasmMb };
    },
    { body, fresh },
  );
}

/** A pandoc docx render through the production runner, leaving its module resident. */
async function pandocRender(page: Page) {
  return page.evaluate(async () => {
    const { wasmRenderer, pandoc } = window.__quartoTest!;
    await wasmRenderer.initWasm();
    wasmRenderer.vfsClear();
    wasmRenderer.vfsAddBinaryFile('/project/min.qmd', new TextEncoder().encode('---\ntitle: Min\n---\n\nHello.\n'));
    const t0 = performance.now();
    const r = await pandoc.download('/project/min.qmd', { format: 'docx', sourceDateEpoch: 1_700_000_000, save: false });
    return { ok: r.ok, ms: Math.round(performance.now() - t0) };
  });
}

/** Gzip size of the Rust `get_typst_assets()` export (every file's bytes, concatenated, through CompressionStream). */
async function vendoredExportGzip(page: Page) {
  return page.evaluate(async () => {
    const { wasmRenderer } = window.__quartoTest!;
    await wasmRenderer.initWasm();
    const files = wasmRenderer.getTypstAssets().files;
    const raw = files.reduce((n, f) => n + f.bytes.byteLength, 0);
    const buf = new Uint8Array(raw);
    let at = 0;
    for (const f of files) {
      buf.set(f.bytes, at);
      at += f.bytes.byteLength;
    }
    const gz = await new Response(new Blob([buf]).stream().pipeThrough(new CompressionStream('gzip'))).arrayBuffer();
    return { files: files.length, rawBytes: raw, gzipBytes: gz.byteLength };
  });
}

for (const [name, launcher] of [
  ['chromium', chromium],
  ['webkit', webkit],
] as const) {
  test(`measure typst: ${name}`, async () => {
    test.setTimeout(15 * 60_000);
    let browser: Browser;
    try {
      browser = await launcher.launch();
    } catch (e) {
      test.skip(true, `${name} cannot launch: ${String(e).slice(0, 120)}`);
      return;
    }
    const result: Record<string, unknown> = { browser: name, version: browser.version(), date: new Date().toISOString() };
    try {
      const context = await browser.newContext({ baseURL: BASE });
      const page = await context.newPage();
      await boot(page);
      result.rssIdleMb = rssMb();
      result.vendoredExport = await vendoredExportGzip(page);

      // --- first use: a fresh context has an empty Cache API; this fetches the wasm and the fonts ---
      const first = await withPeakRss(() => compile(page, DOC, true));
      result.firstCompile = { ...first.value, rssStartMb: first.startMb, rssPeakMb: first.peakMb };
      expect(first.value.ok, JSON.stringify(first.value)).toBe(true);

      // --- warm: the same runner, the module resident, a fresh worker per compile ---
      const warm: Awaited<ReturnType<typeof compile>>[] = [];
      for (let i = 0; i < 6; i++) warm.push(await compile(page, DOC, false));
      result.warm = { runs: warm.slice(1).map((w) => w.totalMs), medianMs: median(warm.slice(1).map((w) => w.totalMs)) };

      // --- cached (reload): the Cache API holds the gz, the module is gone ---
      await boot(page);
      const cached = await withPeakRss(() => compile(page, DOC, true));
      result.cachedLoad = { ...cached.value, rssStartMb: cached.startMb, rssPeakMb: cached.peakMb };

      // --- all three resident: Rust wasm (booted) + a pandoc render + a typst compile, then a second of each ---
      await boot(page);
      const rssBoot = rssMb();
      const pandocFirst = await withPeakRss(() => pandocRender(page));
      const typstNext = await withPeakRss(() => compile(page, DOC, true));
      const pandocAgain = await withPeakRss(() => pandocRender(page));
      const typstAgain = await withPeakRss(() => compile(page, DOC, false));
      result.resident = {
        rssBootMb: rssBoot,
        pandocFirst: { ...pandocFirst.value, rssPeakMb: pandocFirst.peakMb },
        typstNext: { ok: typstNext.value.ok, totalMs: typstNext.value.totalMs, rssStartMb: typstNext.startMb, rssPeakMb: typstNext.peakMb, rustWasmMb: typstNext.value.rustWasmMb },
        pandocAgain: { ...pandocAgain.value, rssPeakMb: pandocAgain.peakMb },
        typstAgain: { ok: typstAgain.value.ok, totalMs: typstAgain.value.totalMs, rssStartMb: typstAgain.startMb, rssPeakMb: typstAgain.peakMb },
        rssFinalMb: rssMb(),
      };
      await context.close();
    } finally {
      await browser.close();
      mkdirSync(OUT, { recursive: true });
      writeFileSync(path.join(OUT, `typst-measure-${name}.json`), JSON.stringify(result, null, 2));
    }
  });
}
