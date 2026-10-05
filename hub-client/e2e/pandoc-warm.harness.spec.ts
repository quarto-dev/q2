/**
 * The warm preview path in a browser (pandoc-host H10b Task 5): warm pandoc workers against the fresh path on
 * the typst recordings and the argv matrix, the faults, the memory loop, overlapping renders under the real
 * preview pane, and (opt-in) timing. Drives the production runners through `window.__quartoTest.pandoc`
 * (`warmHooks` in `src/test-hooks.ts`).
 *
 * Needs `npm run build:wasm`, `npm run build -w ts-packages/typst-host` and a `VITE_E2E=1 npm run build`
 * (`npm run test:harness`). Run it with `--workers=1 --retries=0`:
 *
 *   npx playwright test --config playwright.harness.config.ts e2e/pandoc-warm.harness.spec.ts \
 *     --project=chromium|webkit --retries=0 --workers=1
 *
 * The timing specs are skipped unless `PANDOC_WARM_TIMING=1`: CI runs the harness with two workers, which
 * would load the machine unevenly, so they have their own `--workers=1` step (5c).
 */
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test, type Page } from '@playwright/test';
import type {} from './helpers/testHooks';
import { DOC_DIR, matrixDocs, recordedDocs } from '../src/test-utils/recordingFixtures';

// Serial, and no retry to mask a flaky median (the harness config sets `retries: 1` and `fullyParallel`).
test.describe.configure({ mode: 'serial', retries: 0 });

export interface SeedDoc {
  name: string;
  /** `.qmd` path relative to the document's directory. */
  qmd: string;
  files: { path: string; bytes: Uint8Array }[];
}

/** The six typst recordings. */
export const recordings = (): SeedDoc[] => recordedDocs().filter((d) => d.format === 'typst').map((d) => ({ name: d.name, qmd: d.qmd, files: d.files }));
/** The argv-matrix cases (each is `doc.qmd` plus companions). */
export const matrix = (): SeedDoc[] => matrixDocs().map((d) => ({ name: `matrix-${d.name}`, qmd: 'doc.qmd', files: d.files }));

/** Where a document's `.qmd` ends up in the VFS. */
export const qmdPath = (d: SeedDoc) => `${DOC_DIR}/${d.name}/${d.qmd}`;

export async function boot(page: Page) {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
    await window.__quartoTest!.wasmRenderer.initWasm();
  });
}

/** Seed documents into the VFS, each under `DOC_DIR/<name>/` (the VFS is cleared first). */
export async function seed(page: Page, docs: SeedDoc[]) {
  const files = docs.flatMap((d) => d.files.map((f) => ({ path: `${DOC_DIR}/${d.name}/${f.path}`, base64: Buffer.from(f.bytes).toString('base64') })));
  await page.evaluate(
    ({ files }) => {
      const { wasmRenderer } = window.__quartoTest!;
      wasmRenderer.vfsClear();
      for (const f of files) wasmRenderer.vfsAddBinaryFile(f.path, Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0)));
    },
    { files },
  );
}

test.describe('warm hooks (smoke)', () => {
  test.setTimeout(300_000);

  test('a warm run is byte-equal to a fresh run of the same document, and says it was warm', async ({ page }) => {
    const doc = recordings().find((d) => d.name === 'callouts-typst')!;
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const fresh = await page.evaluate((q) => window.__quartoTest!.pandoc.runRequest(q, 'fresh'), q);
    const warm = await page.evaluate((q) => window.__quartoTest!.pandoc.runRequest(q, 'warm'), q);
    const again = await page.evaluate((q) => window.__quartoTest!.pandoc.runRequest(q, 'warm'), q);
    expect(fresh.ok, JSON.stringify(fresh)).toBe(true);
    expect(warm.ok, JSON.stringify(warm)).toBe(true);
    expect(warm.outputSha).toBe(fresh.outputSha);
    expect(again.outputSha).toBe(fresh.outputSha);
    expect(warm.stats?.warm).toBe(true);
    expect(fresh.stats?.warm).toBeUndefined();
    expect(warm.stats?.fallback).toBeUndefined();
    const pool = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    expect(pool.workerCount).toBeGreaterThanOrEqual(1);
  });

  test('a sequence runs in one round trip and keeps the order it was given', async ({ page }) => {
    const docs = recordings().slice(0, 2);
    await boot(page);
    await seed(page, docs);
    const paths = docs.map(qmdPath);
    const out = await page.evaluate(({ paths, order }) => window.__quartoTest!.pandoc.runSequence(paths, order, 'warm'), { paths, order: [0, 1, 0] });
    expect(out.map((r) => r.doc)).toEqual([0, 1, 0]);
    expect(out.every((r) => r.ok)).toBe(true);
    expect(out[0].outputSha).toBe(out[2].outputSha);
  });

  test('the typing hook drives the real pane and logs frames, starts and edits on one clock', async ({ page }) => {
    const doc = recordings().find((d) => d.name === 'callouts-typst')!;
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const text = Buffer.from(doc.files.find((f) => f.path === doc.qmd)!.bytes).toString('utf8');
    const log = await page.evaluate(({ q, text }) => window.__quartoTest!.pandoc.typing(q, text, [0], { debounceMs: 100, warm: true }), { q, text });
    expect(log.edits).toHaveLength(1);
    expect(log.frames.length).toBeGreaterThanOrEqual(2);
    expect(log.starts.length).toBeGreaterThanOrEqual(2);
    expect(log.frames.map((f) => f.seq)).toEqual([...log.frames.map((f) => f.seq)].sort((a, b) => a - b));
  });
});

// ---- 5b: the correctness matrix -------------------------------------------------------------------------------

/** A seeded PRNG (mulberry32), so a failing order reproduces. */
function prng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** `n` renders drawn from `docs` documents: every document at least twice, shuffled, with some immediate repeats. */
function shuffledOrder(docs: number, seed: number): number[] {
  const rnd = prng(seed);
  const order: number[] = [];
  for (let round = 0; round < 2; round++) {
    const deck = Array.from({ length: docs }, (_, i) => i);
    for (let i = deck.length - 1; i > 0; i--) {
      const j = Math.floor(rnd() * (i + 1));
      [deck[i], deck[j]] = [deck[j], deck[i]];
    }
    order.push(...deck);
  }
  // The same document twice in a row, a few times.
  for (let k = 0; k < 4; k++) {
    const at = Math.floor(rnd() * order.length);
    order.splice(at, 0, order[at]);
  }
  return order;
}

const FONTS = ['Libertinus Serif', 'Inter'];
const CALLOUTS = () => recordings().find((d) => d.name === 'callouts-typst')!;
const textOf = (d: SeedDoc) => Buffer.from(d.files.find((f) => f.path === d.qmd)!.bytes).toString('utf8');

test.describe('warm path: correctness matrix', () => {
  test.setTimeout(900_000);

  test('A,B,A byte equality over a shuffled sequence of the typst recordings and the argv matrix, with no fallbacks', async ({ page }) => {
    const docs = [...recordings(), ...matrix()];
    await boot(page);
    await seed(page, docs);
    const paths = docs.map(qmdPath);
    const identity = docs.map((_, i) => i);
    // Both request shapes: the preview runs the `typst-pdf` request's pandoc leg, Download-as-typst the `typst` one.
    for (const format of ['typst', 'typst-pdf']) {
      const options = { format, fonts: format === 'typst-pdf' ? FONTS : undefined };
      const fresh = await page.evaluate(({ paths, order, options }) => window.__quartoTest!.pandoc.runSequence(paths, order, 'fresh', options), { paths, order: identity, options });
      for (const r of fresh) expect(r.ok, `fresh ${docs[r.doc].name} ${format}: ${r.stderr}`).toBe(true);
      const order = shuffledOrder(docs.length, 20261003);
      const warm = await page.evaluate(({ paths, order, options }) => window.__quartoTest!.pandoc.runSequence(paths, order, 'warm', options), { paths, order, options });
      expect(warm).toHaveLength(order.length);
      for (const [i, r] of warm.entries()) {
        const label = `${format} step ${i}: ${docs[r.doc].name}`;
        expect(r.ok, `${label}: ${r.kind} ${r.stderr}`).toBe(true);
        expect(r.outputSha, label).toBe(fresh[r.doc].outputSha);
        expect(r.stats?.warm, label).toBe(true);
        expect(r.stats?.fallback, label).toBeUndefined();
      }
    }
    // One warm worker did all of it (a second may exist as the spare; none was replaced).
    const pool = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    expect(pool.created).toBeLessThanOrEqual(2 + 2 * pool.recycles);
    expect(pool.graceTerminations).toBe(0);
  });

  test('the env between renders does not leak: each render sees its own request env', async ({ page }) => {
    const doc = CALLOUTS();
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const e1 = { SOURCE_DATE_EPOCH: '1700000000' };
    const e2 = { SOURCE_DATE_EPOCH: '1800000000' };
    const run = (mode: 'warm' | 'fresh', envs: (Record<string, string> | undefined)[]) =>
      page.evaluate(({ q, mode, envs }) => window.__quartoTest!.pandoc.runSequence([q], envs.map(() => 0), mode, { envs }), { q, mode, envs });
    const fresh = await run('fresh', [e1, e2, undefined]);
    const warm = await run('warm', [e1, e2, e1, undefined, e2]);
    expect(warm.every((r) => r.ok)).toBe(true);
    expect(warm.map((r) => r.outputSha)).toEqual([fresh[0].outputSha, fresh[1].outputSha, fresh[0].outputSha, fresh[2].outputSha, fresh[1].outputSha]);
  });

  test('an error, an out-of-memory run and a hang each leave the pool serving byte-equal renders', async ({ page }) => {
    const doc = CALLOUTS();
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const good = () => page.evaluate((q) => window.__quartoTest!.pandoc.runRequest(q, 'warm'), q);
    const fault = (fault: { kind: 'crash' | 'oom' | 'hang'; limit?: string }, wallTimeoutMs?: number) =>
      page.evaluate(({ q, fault, wallTimeoutMs }) => window.__quartoTest!.pandoc.runRequest(q, 'warm', { fault, wallTimeoutMs }), { q, fault, wallTimeoutMs });
    const reference = await page.evaluate((q) => window.__quartoTest!.pandoc.runRequest(q, 'fresh'), q);
    expect(reference.ok).toBe(true);
    expect((await good()).outputSha).toBe(reference.outputSha);

    const crashed = await fault({ kind: 'crash' });
    expect(crashed).toMatchObject({ ok: false, kind: 'crash' });
    const afterCrash = await good();
    expect(afterCrash.outputSha, 'after a crash').toBe(reference.outputSha);
    expect(afterCrash.stats?.warm).toBe(true);

    const oom = await fault({ kind: 'oom', limit: '5m' });
    expect(oom).toMatchObject({ ok: false, kind: 'oom' });
    expect((await good()).outputSha, 'after an out-of-memory run').toBe(reference.outputSha);

    // A hang: the wall timeout stops the worker, and the pool replaces it.
    const before = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    const hung = await fault({ kind: 'hang' }, 4000);
    expect(hung).toMatchObject({ ok: false, kind: 'timeout' });
    const afterHang = await good();
    expect(afterHang.outputSha, 'after a hang').toBe(reference.outputSha);
    const after = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    expect(after.created).toBeGreaterThan(before.created);
  });

  // WebKit 26.4 fails every render from the 47th `convert` on one compiled module (a Lua "module not found"), in warm and fresh
  // workers alike, until the module is compiled again; the warm runner recompiles every `WEBKIT_RECYCLE_AFTER` renders there.
  test('100 warm renders keep the wasm linear memory flat and every output byte-equal', async ({ page, browserName }) => {
    const doc = CALLOUTS();
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const fresh = await page.evaluate((q) => window.__quartoTest!.pandoc.runRequest(q, 'fresh'), q);
    const runs = await page.evaluate((q) => window.__quartoTest!.pandoc.runLoop(q, 100, 'warm'), q);
    const bad = runs.findIndex((r) => !r.ok || r.outputSha !== fresh.outputSha);
    expect(bad, `run ${bad} of 100: ${JSON.stringify(runs[bad])}; neighbours: ${JSON.stringify(runs.slice(Math.max(0, bad - 2), bad + 3).map((r) => [r.ok, r.elapsedMs, r.stats]))}`).toBe(-1);
    const mem = runs.map((r) => r.stats!.memoryBytes);
    // The linear memory never shrinks; flat means that it stops growing, not that it never grew.
    expect(mem[99], `memory after 100 renders (run 10: ${mem[9]})`).toBeLessThanOrEqual(mem[9] * 1.25);
    expect(runs.some((r) => r.stats?.retire)).toBe(false);
    const pool = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    // Each recycle replaces the pool (at most two workers) once; only WebKit recycles.
    expect(pool.created).toBeLessThanOrEqual(2 + 2 * pool.recycles);
    if (browserName === 'webkit') expect(pool.recycles).toBeGreaterThanOrEqual(2);
    else expect(pool.recycles).toBe(0);
    console.log(JSON.stringify({ memoryBytes: { run1: mem[0], run10: mem[9], run50: mem[49], run100: mem[99] } }));
  });

  test('overlapping renders under the real preview pane: frames never go backwards and the last edit is the last frame', async ({ page }) => {
    const doc = CALLOUTS();
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const text = textOf(doc);
    // Edits closer together than a render (about 0.3 s for the whole refresh), each after the debounce.
    const log = await page.evaluate(({ q, text }) => window.__quartoTest!.pandoc.typing(q, text, [150, 150, 150, 150, 150, 150], { debounceMs: 100, warm: true }), { q, text });
    const seqs = log.frames.map((f) => f.seq);
    expect(seqs, JSON.stringify(log)).toEqual([...seqs].sort((a, b) => a - b));
    expect(new Set(seqs).size).toBe(seqs.length);
    expect(log.starts.some((s) => s.live >= 2), JSON.stringify(log.starts)).toBe(true);
    expect(seqs[seqs.length - 1]).toBe(log.starts[log.starts.length - 1].seq);
  });
});

// ---- 5b: larger fixtures and scaling ------------------------------------------------------------------------------

const REPO = fileURLToPath(new URL('../../', import.meta.url));
const OUT = process.env.Q2_WARM_OUT ?? '/tmp/q2-warm';

/** `body` repeated `n` times with its heading ids made unique, so a repeated document has no duplicate labels. */
function repeated(text: string, n: number): string {
  const m = /^---\n[\s\S]*?\n---\n/.exec(text);
  const front = m ? m[0] : '';
  const body = text.slice(front.length);
  return front + Array.from({ length: n }, (_, i) => body.replace(/\{#([A-Za-z][\w-]*)/g, (_x, id) => `{#${id}-${i}`)).join('\n');
}

function walkDir(dir: string, skip: Set<string>, rel = ''): string[] {
  return readdirSync(dir).flatMap((n) => {
    if (skip.has(n)) return [];
    const p = path.join(dir, n);
    return statSync(p).isDirectory() ? walkDir(p, skip, `${rel}${n}/`) : [`${rel}${n}`];
  });
}

test.describe('warm path: larger fixtures', () => {
  test.setTimeout(900_000);

  async function compare(page: Page, label: string, q: string, loops = 3) {
    const fresh = await page.evaluate(({ q, loops }) => window.__quartoTest!.pandoc.runLoop(q, loops, 'fresh'), { q, loops });
    const warm = await page.evaluate(({ q, loops }) => window.__quartoTest!.pandoc.runLoop(q, loops + 1, 'warm'), { q, loops });
    expect(fresh.every((r) => r.ok), `${label} fresh: ${fresh[0].kind} ${fresh[0].stderr}`).toBe(true);
    expect(warm.every((r) => r.ok), `${label} warm: ${warm[0].kind} ${warm[0].stderr}`).toBe(true);
    for (const r of warm) expect(r.outputSha, label).toBe(fresh[0].outputSha);
    expect(warm.every((r) => r.stats?.warm && !r.stats.fallback), `${label}: warm, no fallback`).toBe(true);
    const med = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
    const row = { label, outputBytes: fresh[0].outputBytes, freshMs: Math.round(med(fresh.map((r) => r.elapsedMs))), warmMs: Math.round(med(warm.slice(1).map((r) => r.elapsedMs))), warmMemoryMb: Math.round(warm[warm.length - 1].stats!.memoryBytes / 2 ** 20) };
    console.log(JSON.stringify(row));
    return row;
  }

  test('an orange-book-class project page and brand.qmd: warm equals fresh, with the x10 and x100 scaling recorded', async ({ page }) => {
    await boot(page);
    const rows: unknown[] = [];

    // The orange-book project (a book with an extension template and Lua): one chapter as the active page.
    const ob = path.join(REPO, 'crates/quarto/tests/smoke-all/typst/orange-book');
    const skip = new Set(['_book', '.quarto', 'chapter1_files']);
    const obFiles = walkDir(ob, skip).map((r) => ({ path: r, bytes: readFileSync(path.join(ob, r)) }));
    await seed(page, [{ name: 'orange-book', qmd: 'chapter1.qmd', files: obFiles }]);
    rows.push(await compare(page, 'orange-book chapter1', `${DOC_DIR}/orange-book/chapter1.qmd`));

    // brand.qmd (37.7 KB). Its `{{< brand >}}` shortcodes, live and escaped (`{{{< brand >}}}` in inline code), stop the vendored
    // Lua with a nil concatenation (`brandCommand`, shortcodes-handlers.lua:111) on the fresh path too, so they are replaced by
    // plain text here. Its screenshots are absent, which only warns.
    const brand = Buffer.from(readFileSync(path.join(REPO, 'docs/guides/authoring/brand.qmd'), 'utf8').replace(/\{{2,3}< brand [^>]*>\}{2,3}/g, 'BRAND'));
    await seed(page, [{ name: 'brand', qmd: 'brand.qmd', files: [{ path: 'brand.qmd', bytes: brand }] }]);
    rows.push(await compare(page, 'brand.qmd', `${DOC_DIR}/brand/brand.qmd`));

    // Scaling: the superlinear cost is visible when the same document is repeated.
    const callouts = CALLOUTS();
    for (const n of [10, 100]) {
      const text = repeated(textOf(callouts), n);
      await seed(page, [{ name: `callouts-x${n}`, qmd: 'doc.qmd', files: [{ path: 'doc.qmd', bytes: Buffer.from(text) }] }]);
      rows.push(await compare(page, `callouts x${n}`, `${DOC_DIR}/callouts-x${n}/doc.qmd`, n === 100 ? 1 : 3));
    }
    mkdirSync(OUT, { recursive: true });
    writeFileSync(path.join(OUT, 'h10b-5b-larger-fixtures.json'), JSON.stringify({ date: new Date().toISOString(), rows }, null, 1));
  });
});

// ---- 5c: performance and typing -------------------------------------------------------------------------------
//
// Opt-in (`PANDOC_WARM_TIMING=1`, one worker): wall-clock medians need an otherwise idle machine. Locally the absolute
// figures are asserted and written to `Q2_WARM_OUT`; in CI (`CI` set) only same-run ratios, byte equality and frame order
// are. Each test boots its own page, so the WebKit module recycle (every 30 renders, fresh and warm counted together) is
// never reached by a timed sample: every test asserts that the pool did not recycle.

const med = (xs: number[]) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const IN_CI = !!process.env.CI;
/** The saving H10a Task 0(f) measured for the whole refresh, per engine; the warm median must beat the fresh one by half of it. */
const WHOLE_SAVING_MS: Record<string, number> = { chromium: 56, webkit: 200, firefox: 56 };
const PANDOC_LEG_LOCAL_BAR_MS = 125;
const PANDOC_LEG_CI_RATIO = 0.85;
const DEBOUNCE_MS = 500;

function record(name: string, value: unknown) {
  console.log(JSON.stringify({ [name]: value }));
  mkdirSync(OUT, { recursive: true });
  writeFileSync(path.join(OUT, `h10b-5c-${name}.json`), JSON.stringify({ date: new Date().toISOString(), ...(value as object) }, null, 1));
}

test.describe('warm path: timing', () => {
  test.skip(!process.env.PANDOC_WARM_TIMING, 'opt-in: set PANDOC_WARM_TIMING=1 (and run with --workers=1)');
  test.setTimeout(300_000);

  test('pandoc leg: warm callouts against fresh in the same run', async ({ page, browserName }) => {
    const doc = CALLOUTS();
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const N = 11; // the first is discarded
    // Interleave in two blocks (fresh then warm) in one page, so both see the same machine state.
    const fresh = await page.evaluate(({ q, N }) => window.__quartoTest!.pandoc.runLoop(q, N, 'fresh'), { q, N });
    const warm = await page.evaluate(({ q, N }) => window.__quartoTest!.pandoc.runLoop(q, N, 'warm'), { q, N });
    expect([...fresh, ...warm].every((r) => r.ok)).toBe(true);
    for (const r of warm) expect(r.outputSha).toBe(fresh[0].outputSha);
    expect(warm.slice(1).every((r) => r.stats?.warm && !r.stats.fallback)).toBe(true);
    const pool = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    expect(pool.recycles, 'the timed sample stays under the recycle count').toBe(0);
    const freshMed = med(fresh.slice(1).map((r) => r.elapsedMs));
    const warmMed = med(warm.slice(1).map((r) => r.elapsedMs));
    record(`pandoc-leg-${browserName}`, { browser: browserName, n: N - 1, freshMedMs: Math.round(freshMed), warmMedMs: Math.round(warmMed), ratio: +(warmMed / freshMed).toFixed(3), warmMs: warm.slice(1).map((r) => Math.round(r.elapsedMs)) });
    expect(warmMed, `warm ${warmMed} vs fresh ${freshMed}`).toBeLessThanOrEqual(PANDOC_LEG_CI_RATIO * freshMed);
    if (!IN_CI) expect(warmMed, 'local bar').toBeLessThanOrEqual(PANDOC_LEG_LOCAL_BAR_MS);
  });

  test('whole refresh: warm beats fresh by half of the saving Task 0(f) measured', async ({ page, browserName }) => {
    const doc = CALLOUTS();
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const text = textOf(doc);
    const RUNS = 9; // run 0 is the cold run; 10 renders per mode, 20 in all, plus the warm-ups
    const whole = (runs: { t0: number; tShown: number; phase: string }[]) => {
      expect(runs.every((r) => r.phase === 'done')).toBe(true);
      return runs.slice(1).map((r) => r.tShown - r.t0);
    };
    const fresh = whole(await page.evaluate(({ q, text, RUNS }) => window.__quartoTest!.pandoc.measurePdfRefresh(q, text, RUNS, { warm: false }), { q, text, RUNS }));
    const warm = whole(await page.evaluate(({ q, text, RUNS }) => window.__quartoTest!.pandoc.measurePdfRefresh(q, text, RUNS, { warm: true }), { q, text, RUNS }));
    const pool = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    expect(pool.recycles).toBe(0);
    const freshMed = med(fresh);
    const warmMed = med(warm);
    const bar = freshMed - (WHOLE_SAVING_MS[browserName] ?? 0) / 2;
    record(`whole-refresh-${browserName}`, { browser: browserName, n: RUNS, freshMedMs: Math.round(freshMed), warmMedMs: Math.round(warmMed), barMs: Math.round(bar), fresh: fresh.map(Math.round), warm: warm.map(Math.round) });
    expect(warmMed, `warm ${warmMed} vs the bar ${bar} (fresh ${freshMed})`).toBeLessThanOrEqual(bar);
  });

  test('typing: overlapping renders at three edit intervals, frames in order', async ({ page, browserName }) => {
    const doc = CALLOUTS();
    await boot(page);
    // callouts x10: a render longer than the edit interval (0.56 s for the pandoc leg alone).
    const text = repeated(textOf(doc), 10);
    await seed(page, [{ name: 'callouts-x10', qmd: 'doc.qmd', files: [{ path: 'doc.qmd', bytes: Buffer.from(text) }] }]);
    const q = `${DOC_DIR}/callouts-x10/doc.qmd`;
    const rows: unknown[] = [];
    let overlapped = false;
    for (const interval of [DEBOUNCE_MS + 50, 1.5 * DEBOUNCE_MS, 2.5 * DEBOUNCE_MS]) {
      const log = await page.evaluate(({ q, text, interval }) => window.__quartoTest!.pandoc.typing(q, text, [interval, interval, interval], { debounceMs: 500, warm: true }), { q, text, interval });
      const seqs = log.frames.map((f) => f.seq);
      expect(seqs, `interval ${interval}: ${JSON.stringify(log)}`).toEqual([...seqs].sort((a, b) => a - b));
      expect(new Set(seqs).size).toBe(seqs.length);
      expect(seqs[seqs.length - 1], 'the last edit is the last frame').toBe(log.starts[log.starts.length - 1].seq);
      const maxLive = Math.max(...log.starts.map((s) => s.live));
      overlapped ||= maxLive >= 2;
      rows.push({ interval, maxLive, frames: log.frames.length, starts: log.starts.length });
    }
    const pool = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    record(`typing-overlap-${browserName}`, { browser: browserName, rows, pool });
    // The shortest interval is below one render, so at least two runs must have been in flight at some start.
    expect((rows[0] as { maxLive: number }).maxLive, JSON.stringify(rows)).toBeGreaterThanOrEqual(2);
    expect(overlapped).toBe(true);
  });

  test('typing: one edit after a pause shows a frame within the measured warm whole refresh plus the debounce plus 50 ms', async ({ page, browserName }) => {
    const doc = CALLOUTS();
    await boot(page);
    await seed(page, [doc]);
    const q = qmdPath(doc);
    const text = textOf(doc);
    const runs = await page.evaluate(({ q, text }) => window.__quartoTest!.pandoc.measurePdfRefresh(q, text, 5, { warm: true }), { q, text });
    const wholeMed = med(runs.slice(1).map((r) => r.tShown - r.t0));
    const latencies: number[] = [];
    for (let i = 0; i < 3; i++) {
      const log = await page.evaluate(({ q, text }) => window.__quartoTest!.pandoc.typing(q, text, [1500], { debounceMs: 500, warm: true }), { q, text });
      const frame = log.frames.find((f) => f.tMs > log.edits[0].tMs);
      expect(frame, JSON.stringify(log)).toBeDefined();
      latencies.push(frame!.tMs - log.edits[0].tMs);
    }
    const pool = await page.evaluate(() => window.__quartoTest!.pandoc.warmRunnerStats());
    expect(pool.recycles).toBe(0);
    const latency = med(latencies);
    const bound = DEBOUNCE_MS + wholeMed + 50;
    record(`typing-latency-${browserName}`, { browser: browserName, wholeMedMs: Math.round(wholeMed), latenciesMs: latencies.map(Math.round), boundMs: Math.round(bound) });
    expect(latency, `latency ${latency} vs bound ${bound}`).toBeLessThanOrEqual(bound);
  });
});
