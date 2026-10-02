/**
 * Browser measurements for pandoc.wasm (pandoc-host H3): first-download and cached-load
 * latency, per-render latency, Lua/filter startup, the cost of mounting the share tree,
 * and peak memory with image-heavy documents. Opt-in (timing numbers are not assertions):
 *
 *   VITE_E2E=1 npm run build
 *   Q2_MEASURE=1 Q2_MEASURE_OUT=/tmp/measure npx playwright test --config playwright.harness.config.ts \
 *     e2e/pandoc-measure.harness.spec.ts --retries=0 --workers=1
 *
 * It launches its own Chromium and WebKit (Firefox cannot launch under Playwright here;
 * run spike/ff by hand) against the preview server of the harness config, and writes one
 * JSON file per browser to `Q2_MEASURE_OUT`. Process memory is the summed RSS of every
 * process whose command line contains `ms-playwright` (sampled every 100 ms for the peak),
 * so close other Playwright browsers first. Results are recorded in the evidence file
 * (claude-notes/research/2026-10-01-pandoc-wasm-evidence.md, section H3).
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { chromium, expect, test, webkit, type Browser, type BrowserContext, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import { fixtureFiles, goldenFixtures } from '../src/test-utils/goldenFixtures';

// `--expose-gc` for Chromium is a launch argument below.
const OUT = process.env.Q2_MEASURE_OUT ?? '/tmp/q2-measure';
const SDE = 1_700_000_000;
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

type Seed = { path: string; base64: string }[];

/** Seed the VFS, then run the dev harness and return a serialisable summary. */
async function render(page: Page, files: Seed, qmd: string) {
  return page.evaluate(
    async ({ files, qmd, sde }) => {
      const { wasmRenderer, pandoc } = window.__quartoTest!;
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      for (const f of files) wasmRenderer.vfsAddBinaryFile(`/project/${f.path}`, Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0)));
      const r = await pandoc.download(`/project/${qmd}`, { format: 'docx', sourceDateEpoch: sde, save: false });
      return { ok: r.ok, failure: r.failure, requestError: r.requestError, messages: r.diagnostics.map((d) => (d as { message?: string; title?: string }).message ?? (d as { title?: string }).title), timings: r.timings, notices: r.notices, bytes: r.output?.byteLength };
    },
    { files, qmd, sde: SDE },
  );
}

const median = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

/**
 * Image-heavy render in two steps, so process memory can be read between them: `seedImages`
 * puts `count` images of about `mb` MB each in the VFS (PNG with stored deflate blocks of
 * xorshift noise: valid, incompressible like a photo) and `renderSeeded` runs the harness.
 * The generator's own buffers are garbage after the first call returns (Chromium is
 * launched with `--expose-gc` and collected between the steps), so the second step's growth
 * is the render chain's.
 */
async function seedImages(page: Page, count: number, mb: number) {
  return page.evaluate(
    async ({ count, mb }) => {
      const { wasmRenderer } = window.__quartoTest!;
      await wasmRenderer.initWasm();
      wasmRenderer.vfsClear();
      const crcTable = new Uint32Array(256).map((_, n) => {
        let c = n;
        for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
        return c >>> 0;
      });
      const crc = (b: Uint8Array) => {
        let c = 0xffffffff;
        for (let i = 0; i < b.length; i++) c = crcTable[(c ^ b[i]) & 0xff] ^ (c >>> 8);
        return (c ^ 0xffffffff) >>> 0;
      };
      const u32 = (n: number) => new Uint8Array([(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255]);
      const chunk = (type: string, data: Uint8Array) => {
        const t = new TextEncoder().encode(type);
        const body = new Uint8Array(4 + data.length);
        body.set(t);
        body.set(data, 4);
        return [u32(data.length), body, u32(crc(body))];
      };
      const png = (bytesTarget: number, seed: number) => {
        const w = 2000;
        const h = Math.max(1, Math.round(bytesTarget / (w * 3 + 1)));
        const raw = new Uint8Array(h * (w * 3 + 1));
        let x = seed | 1;
        for (let i = 0; i < raw.length; i++) {
          x ^= x << 13;
          x ^= x >>> 17;
          x ^= x << 5;
          raw[i] = x & 255;
        }
        for (let r = 0; r < h; r++) raw[r * (w * 3 + 1)] = 0; // filter: none
        const blocks: Uint8Array[] = [new Uint8Array([0x78, 0x01])];
        let a = 1;
        let b = 0;
        for (let i = 0; i < raw.length; i++) {
          a = (a + raw[i]) % 65521;
          b = (b + a) % 65521;
        }
        for (let off = 0; off < raw.length; off += 65535) {
          const len = Math.min(65535, raw.length - off);
          const last = off + len >= raw.length ? 1 : 0;
          blocks.push(new Uint8Array([last, len & 255, len >>> 8, ~len & 255, (~len >>> 8) & 255]), raw.subarray(off, off + len));
        }
        blocks.push(u32(((b << 16) | a) >>> 0));
        const z = new Uint8Array(blocks.reduce((n, p) => n + p.length, 0));
        let o = 0;
        for (const p of blocks) {
          z.set(p, o);
          o += p.length;
        }
        const ihdr = new Uint8Array(13);
        ihdr.set(u32(w), 0);
        ihdr.set(u32(h), 4);
        ihdr.set([8, 2, 0, 0, 0], 8);
        const parts = [new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]), ...chunk('IHDR', ihdr), ...chunk('IDAT', z), ...chunk('IEND', new Uint8Array(0))];
        const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
        o = 0;
        for (const p of parts) {
          out.set(p, o);
          o += p.length;
        }
        return out;
      };
      let md = '---\ntitle: Images\n---\n\n';
      let total = 0;
      for (let i = 0; i < count; i++) {
        const bytes = png(mb * 1024 * 1024, 12345 + i);
        total += bytes.length;
        wasmRenderer.vfsAddBinaryFile(`/project/img${i}.png`, bytes);
        md += `![Figure ${i}](img${i}.png)\n\n`;
      }
      wasmRenderer.vfsAddFile('/project/images.qmd', md);
      (globalThis as { gc?: () => void }).gc?.();
      return total;
    },
    { count, mb },
  );
}

async function renderSeeded(page: Page) {
  return page.evaluate(async (sde) => {
    const { pandoc } = window.__quartoTest!;
    const t0 = performance.now();
    const r = await pandoc.download('/project/images.qmd', { format: 'docx', sourceDateEpoch: sde, save: false });
    return {
      ok: r.ok,
      failure: r.failure,
      requestError: r.requestError,
      messages: r.diagnostics.map((d) => (d as { message?: string; title?: string }).message ?? (d as { title?: string }).title),
      timings: r.timings,
      outputBytes: r.output?.byteLength,
      totalMs: Math.round(performance.now() - t0),
    };
  }, SDE);
}

const fixtureSeed = (qmd: string): Seed => {
  const f = goldenFixtures().find((x) => x.qmd === qmd)!;
  return fixtureFiles(f).map((x) => ({ path: x.path, base64: Buffer.from(x.bytes).toString('base64') }));
};

const MINIMAL: Seed = [{ path: 'min.qmd', base64: Buffer.from('---\ntitle: Min\n---\n\nHello.\n').toString('base64') }];

for (const [name, launcher] of [
  ['chromium', chromium],
  ['webkit', webkit],
] as const) {
  test(`measure: ${name}`, async () => {
    test.setTimeout(20 * 60_000);
    let browser: Browser;
    try {
      browser = await launcher.launch(name === 'chromium' ? { args: ['--js-flags=--expose-gc'] } : {});
    } catch (e) {
      test.skip(true, `${name} cannot launch: ${String(e).slice(0, 120)}`);
      return;
    }
    const result: Record<string, unknown> = { browser: name, version: browser.version(), date: new Date().toISOString() };
    try {
      // --- first download: a fresh context has an empty Cache API ---
      const context: BrowserContext = await browser.newContext({ baseURL: BASE });
      const page = await context.newPage();
      await boot(page);
      const idle = rssMb();
      result.rssIdleMb = idle;
      const first = await withPeakRss(() => render(page, fixtureSeed('callouts.qmd'), 'callouts.qmd'));
      result.firstDownload = { ...first.value, rssStartMb: first.startMb, rssPeakMb: first.peakMb };
      expect(first.value.ok, JSON.stringify(first.value)).toBe(true);

      // --- warm module, fresh instance per render ---
      const warm: Awaited<ReturnType<typeof render>>[] = [];
      for (let i = 0; i < 7; i++) warm.push(await render(page, fixtureSeed('callouts.qmd'), 'callouts.qmd'));
      const pick = (k: 'runnerMs' | 'instanceMs' | 'runMs' | 'mountMs' | 'requestMs') => warm.slice(1).map((w) => (w.timings as unknown as Record<string, number>)[k]);
      result.warmCallouts = {
        runs: warm.slice(1).map((w) => w.timings),
        medianRunnerMs: median(pick('runnerMs')),
        medianInstanceMs: median(pick('instanceMs')),
        medianRunMs: median(pick('runMs')),
        medianMountMs: median(pick('mountMs')),
        medianRequestMs: median(pick('requestMs')),
        memoryBytes: warm[warm.length - 1].timings.memoryBytes,
        shareTreeFiles: warm[0].timings.shareTreeFiles,
        shareTreeBytes: warm[0].timings.shareTreeBytes,
      };

      // --- filter-chain startup: a one-paragraph docx (full filters) vs the smoke job (one trivial filter) ---
      const minimal: Awaited<ReturnType<typeof render>>[] = [];
      for (let i = 0; i < 6; i++) minimal.push(await render(page, MINIMAL, 'min.qmd'));
      const smoke: number[] = [];
      for (let i = 0; i < 6; i++) {
        smoke.push(
          await page.evaluate(async () => {
            const { pandoc } = window.__quartoTest!;
            const { runner } = pandoc.createRunner();
            const { request, shareTree } = pandoc.smokeJob();
            const o = await runner.run(request, shareTree);
            return o.ok ? o.stats.runMs : -1;
          }),
        );
      }
      result.startup = {
        minimalDocxRunMs: minimal.slice(1).map((m) => m.timings.runMs),
        minimalDocxMedianRunMs: median(minimal.slice(1).map((m) => m.timings.runMs ?? 0)),
        minimalDocxMedianMountMs: median(minimal.slice(1).map((m) => m.timings.mountMs ?? 0)),
        smokeRunMs: smoke.slice(1),
        smokeMedianRunMs: median(smoke.slice(1)),
      };

      // --- cached (reload): the Cache API holds the gz, the module is gone ---
      await boot(page);
      const cached = await render(page, fixtureSeed('callouts.qmd'), 'callouts.qmd');
      result.cachedLoad = cached;

      // --- image-heavy documents: peak linear memory and process RSS ---
      const images: unknown[] = [];
      for (const [count, mb] of [
        [4, 5],
        [10, 10],
        [12, 24],
      ] as const) {
        await boot(page); // a clean main thread; the cached gz keeps the load short
        const imageBytes = await seedImages(page, count, mb);
        const r = await withPeakRss(() => renderSeeded(page));
        images.push({ count, mbEach: mb, imageBytes, ...r.value, rssSeededMb: r.startMb, rssPeakMb: r.peakMb });
      }
      result.images = images;
      await context.close();
    } finally {
      await browser.close();
      mkdirSync(OUT, { recursive: true });
      writeFileSync(path.join(OUT, `measure-${name}.json`), JSON.stringify(result, null, 2));
    }
  });
}
