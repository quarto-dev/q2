/**
 * Memory and wall time of a document-import run (plan P1 T8, epic I19): a ~25 MB docx of
 * incompressible images (plus `Q2_IMPORT_DOCX=<file>` for a realistic large document, and
 * `Q2_IMPORT_SWEEP=1` for the adversarial BMP docx that extracts to about `collected_total_bytes`).
 * Opt-in (the numbers are not assertions, and the run takes minutes):
 *
 *   VITE_E2E=1 npm run build
 *   Q2_MEASURE=1 [Q2_IMPORT_SWEEP=1] Q2_MEASURE_OUT=/tmp/measure npx playwright test -c playwright.harness.config.ts \
 *     pandoc-import-measure --project=chromium --retries=0 --workers=1
 *
 * once per project (`--project=webkit`, `--project=firefox`). Unlike `pandoc-measure.harness.spec.ts`
 * this runs in the project's own browser; process memory is the summed RSS of every process whose
 * command line contains `ms-playwright`, sampled every 100 ms for the peak, so close other
 * Playwright browsers first. The trigger (epic I19) is Chromium `peakMb - startMb` over 1024 for
 * either file; WebKit and Firefox numbers are for magnitude only. One JSON per project goes to
 * `Q2_MEASURE_OUT` (a directory, or a `.json` path that gets `-<project>` before the extension).
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import { compressibleDocx, imageHeavyDocx, tinyDocx } from './helpers/importDocx';

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
  await page.goto('/'); // the project's baseURL
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
}

const runImport = (page: Page, docx: Uint8Array) =>
  page.evaluate((b64) => window.__quartoTest!.pandoc.runImport(b64, 'docx', { wallTimeoutMs: 300_000 }), Buffer.from(docx).toString('base64'));

test('measure: document import', async ({ context }, testInfo) => {
  test.setTimeout(20 * 60_000);
  const project = testInfo.project.name;
  const result: Record<string, unknown> = { project, date: new Date().toISOString() };
  try {
    const runs: Record<string, unknown> = {};
    // Default: the image-heavy docx (the 25 MB source cap's real worst case), plus a realistic large document when
    // `Q2_IMPORT_DOCX` names one (the Moby Dick spike's docx; see claude-notes/research/2026-10-04-moby-dick-import-spike/).
    // `Q2_IMPORT_SWEEP=1` adds the compressible BMP docx (12 x 24 MiB extracted, an adversarial input Gordon ruled out
    // as a concern) and smaller extracted totals (8, 6 and 4 BMPs), to find the total that fits a budget.
    const files: [string, Uint8Array][] = [
      ['imageHeavy', imageHeavyDocx()],
      ...(process.env.Q2_IMPORT_DOCX ? ([['realDocument', new Uint8Array(readFileSync(process.env.Q2_IMPORT_DOCX))]] as [string, Uint8Array][]) : []),
      ...(process.env.Q2_IMPORT_SWEEP
        ? ([12, 8, 6, 4] as const).map((n): [string, Uint8Array] => [n === 12 ? 'compressible' : `compressible${n}`, compressibleDocx(n)])
        : []),
    ];
    for (const [name, docx] of files) {
      // A fresh page per file, so one run's leftovers are not the next one's baseline.
      const page = await context.newPage();
      try {
        await boot(page);
        // Load the wasm (the runner and its loader stay resident) so the measured run is not the first download.
        const warmup = await runImport(page, tinyDocx());
        expect(warmup.ok, JSON.stringify(warmup)).toBe(true);
        await new Promise((r) => setTimeout(r, 2000)); // let the warm-up's memory settle
        const m = await withPeakRss(() => runImport(page, docx));
        runs[name] = { docxBytes: docx.byteLength, ...m.value, rssStartMb: m.startMb, rssPeakMb: m.peakMb, growthMb: m.peakMb - m.startMb };
        expect(m.value.ok, JSON.stringify(m.value)).toBe(true);
      } finally {
        await page.close();
      }
    }
    result.runs = runs;
  } finally {
    const out = process.env.Q2_MEASURE_OUT ?? '/tmp/q2-measure';
    const file = out.endsWith('.json') ? out.replace(/\.json$/, `-${project}.json`) : path.join(out, `import-measure-${project}.json`);
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, JSON.stringify(result, null, 2));
  }
});
