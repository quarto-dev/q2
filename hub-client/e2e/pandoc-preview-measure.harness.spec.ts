/**
 * The whole PDF preview refresh, split by stage (host phase H10a, Task 0(f)). Opt-in: timing numbers are
 * not assertions.
 *
 *   VITE_E2E=1 npm run build
 *   Q2_MEASURE_F=1 Q2_MEASURE_OUT=<dir> npx playwright test --config playwright.harness.config.ts \
 *     e2e/pandoc-preview-measure.harness.spec.ts --project=chromium|webkit --retries=0 --workers=1
 *
 * It drives the production preview controller and viewer (`pandoc.measurePdfRefresh` in `src/test-hooks.ts`) over
 * `callouts.qmd` (the R0 typst recording's document) and writes `<dir>/h10a-0b-app-<browser>.json`: the raw
 * stage marks of every run and the split computed from them. The warm whole refresh is derived from it in the
 * evidence note (the fresh whole refresh minus the d2b harness's fresh pandoc leg plus its warm leg).
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import type {} from './helpers/testHooks';

test.skip(!process.env.Q2_MEASURE_F, 'opt-in: set Q2_MEASURE_F=1');

const OUT = process.env.Q2_MEASURE_OUT ?? '/tmp/q2-measure';
const RUNS = Number(process.env.Q2_MEASURE_RUNS ?? 12);
const QMD = fileURLToPath(new URL('../../crates/quarto-core/tests/fixtures/pandoc-recordings/recordings/callouts-typst/fs/__q2_doc__/callouts.qmd', import.meta.url));

type Mark = { stage: string; t: number };
type Run = { t0: number; marks: Mark[]; tPdf: number; tShown: number; phase: string };

/** The stages in the order the PDF chain sets them: listFonts (`typst-*`), `preparing`, pandoc, `typst-*`, done. */
function split(r: Run) {
  const at = (pred: (m: Mark, i: number) => boolean, from = 0) => r.marks.findIndex((m, i) => i >= from && pred(m, i));
  const pandocStages = ['loading', 'starting', 'mounting', 'running'];
  const iTypst1 = at((m) => m.stage.startsWith('typst-'));
  const iPrep = at((m) => m.stage === 'preparing', iTypst1 + 1);
  const iPandoc = at((m) => pandocStages.includes(m.stage), iPrep + 1);
  const iRunning = at((m) => m.stage === 'running', iPandoc);
  const iTypst2 = at((m) => m.stage.startsWith('typst-'), iPandoc + 1);
  const iCompiling = at((m) => m.stage === 'typst-compiling', iTypst2);
  const t = (i: number) => r.marks[i].t;
  const ok = [iTypst1, iPrep, iPandoc, iRunning, iTypst2, iCompiling].every((i) => i >= 0);
  if (!ok) return null;
  return {
    listFonts: t(iPrep) - r.t0, // typst worker spawn, `TypstSession.create`, font families (the per-run `listFonts` spawn)
    rustRequest: t(iPandoc) - t(iPrep), // `render_pandoc_request` on the main thread
    pandocLeg: t(iTypst2) - t(iPandoc), // runner: worker spawn, mount, pandoc run, read, result message
    pandocSpawnMount: t(iRunning) - t(iPandoc),
    typstLeg: r.tPdf - t(iTypst2), // second typst worker spawn, init, compile
    typstCompile: r.tPdf - t(iCompiling),
    viewer: r.tShown - r.tPdf, // pdf.js iframe load, pages laid out and the first painted
    whole: r.tShown - r.t0,
  };
}

const med = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];

test('the whole PDF preview refresh, split by stage', async ({ page, browserName }) => {
  test.setTimeout(600_000);
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
    await window.__quartoTest!.wasmRenderer.initWasm();
    window.__quartoTest!.wasmRenderer.vfsClear();
  });
  const text = readFileSync(QMD, 'utf8');
  const runs = await page.evaluate(([p, t, n]) => window.__quartoTest!.pandoc.measurePdfRefresh(p as string, t as string, n as number), ['/__q2_doc__/callouts.qmd', text, RUNS] as const);
  const rows = runs.map((r) => ({ phase: r.phase, split: split(r) }));
  expect(rows.every((r) => r.phase === 'done' && r.split), JSON.stringify(rows.map((r) => r.phase))).toBe(true);
  const measured = rows.slice(1).map((r) => r.split!); // run 0 is the cold run
  const keys = Object.keys(measured[0]) as (keyof (typeof measured)[0])[];
  const summary = Object.fromEntries(keys.map((k) => [k, { med: med(measured.map((m) => m[k])), min: Math.min(...measured.map((m) => m[k])), max: Math.max(...measured.map((m) => m[k])) }]));
  mkdirSync(OUT, { recursive: true });
  const ua = await page.evaluate(() => navigator.userAgent);
  writeFileSync(path.join(OUT, `h10a-0b-app-${browserName}.json`), JSON.stringify({ browser: browserName, ua, runs: RUNS, date: new Date().toISOString(), summary, cold: rows[0].split, raw: runs }, null, 1));
  console.log(JSON.stringify(summary));
});
