/**
 * Smoke tests for the pandoc.wasm loader and worker lifecycle (host phase H2), against the
 * production bundle built with VITE_E2E=1 (the `window.__quartoTest.pandoc` hook; see
 * src/test-hooks.ts). They need hub-client/public/pandoc/pandoc.wasm.gz in the build
 * (`node scripts/fetch-pandoc-wasm.mjs`), and no hub server.
 *
 * The page hook builds a tiny markdown-to-plain job (src/pandoc/smokeJob.ts) rather than
 * replaying an R0 recording: these tests exercise the loader and the worker, not any
 * format, and the recordings live on the Node side of the page boundary.
 */
import { expect, test, type Page, type Request } from '@playwright/test';
import type {} from './helpers/testHooks';

const ASSET = /\/pandoc\/pandoc\.wasm\.gz(\?|$)/;

/** Serialisable summary of a run outcome (the output bytes are decoded, the rest passes through). */
type Summary = { ok: boolean; kind?: string; text?: string; diagnostics: { code?: string; message?: string }[]; notices: string[] };

async function boot(page: Page) {
  await page.goto('/');
  // `__quartoTestReady` is assigned by main.tsx once it runs; goto's load event can precede it.
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
}

function trackAssetRequests(page: Page): Request[] {
  const seen: Request[] = [];
  page.on('request', (r) => {
    if (ASSET.test(r.url())) seen.push(r);
  });
  return seen;
}

test('no pandoc request before first use', async ({ page }) => {
  const seen = trackAssetRequests(page);
  await boot(page);
  await page.waitForTimeout(1500);
  expect(seen).toHaveLength(0);
});

test('exnref absent: a friendly error naming browser versions, and nothing is downloaded', async ({ page }) => {
  const seen = trackAssetRequests(page);
  await boot(page);
  const out = await page.evaluate(async (): Promise<Summary & { uiState: string }> => {
    WebAssembly.validate = () => false; // as a browser without exnref
    const { pandoc } = window.__quartoTest!;
    const { runner } = pandoc.createRunner();
    const { request, shareTree } = pandoc.smokeJob();
    const o = await runner.run(request, shareTree);
    return { ok: o.ok, kind: o.ok ? undefined : o.kind, diagnostics: o.diagnostics as never, notices: o.notices, uiState: pandoc.uiStateFor(o) };
  });
  expect(out).toMatchObject({ ok: false, kind: 'load-failed', uiState: 'unsupported' });
  expect(out.diagnostics[0].code).toBe('wasm-unsupported');
  expect(out.diagnostics[0].message).toContain('Chrome/Edge 137');
  expect(seen).toHaveLength(0);
});

test('abort mid-fetch cancels the render and the download', async ({ page }) => {
  let held: (() => void) | undefined;
  const requested = new Promise<void>((resolve) => {
    void page.route(ASSET, (route) => {
      held = () => void route.abort();
      resolve();
    });
  });
  await boot(page);
  await page.evaluate(() => {
    const { pandoc } = window.__quartoTest!;
    const { runner } = pandoc.createRunner();
    const { request, shareTree } = pandoc.smokeJob();
    const ac = new AbortController();
    const w = window as unknown as { __ac: AbortController; __run: Promise<{ kind?: string; ok: boolean }> };
    w.__ac = ac;
    w.__run = runner.run(request, shareTree, { signal: ac.signal }) as never;
  });
  await requested;
  const kind = await page.evaluate(async () => {
    const w = window as unknown as { __ac: AbortController; __run: Promise<{ kind?: string; ok: boolean }> };
    w.__ac.abort();
    return (await w.__run).kind;
  });
  expect(kind).toBe('aborted');
  held?.();
});

test('the second run is a cache hit: one network request for two loads', async ({ page }) => {
  const seen = trackAssetRequests(page);
  await boot(page);
  const outs = await page.evaluate(async () => {
    const { pandoc } = window.__quartoTest!;
    const dec = new TextDecoder();
    const results: { ok: boolean; text?: string; kind?: string }[] = [];
    // Two loaders: the second starts with no resident Module, so it must come from the Cache API.
    for (const md of ['First\n', 'Second\n']) {
      const { runner } = pandoc.createRunner();
      const { request, shareTree } = pandoc.smokeJob(md);
      const o = await runner.run(request, shareTree);
      results.push(o.ok ? { ok: true, text: dec.decode(o.output).trim() } : { ok: false, kind: o.kind });
    }
    return results;
  });
  expect(outs).toEqual([
    { ok: true, text: 'First' },
    { ok: true, text: 'Second' },
  ]);
  expect(seen).toHaveLength(1);
});

test('fault injection: oom, crash and a hang stopped by the wall timeout', async ({ page }) => {
  await boot(page);
  const outs = await page.evaluate(async () => {
    const { pandoc } = window.__quartoTest!;
    const { runner } = pandoc.createRunner({ wallTimeoutMs: 2000 });
    const run = async (fault: { kind: 'oom'; limit: string } | { kind: 'crash' } | { kind: 'hang' }) => {
      const { request, shareTree } = pandoc.smokeJob();
      const t0 = performance.now();
      const o = await runner.run(request, shareTree, { fault });
      return { kind: o.ok ? 'ok' : o.kind, ms: performance.now() - t0 };
    };
    const oom = await run({ kind: 'oom', limit: '256k' });
    const crash = await run({ kind: 'crash' });
    const hang = await run({ kind: 'hang' });
    // The resident Module survives the terminated worker: a clean run still works.
    const { request, shareTree } = pandoc.smokeJob();
    const after = await runner.run(request, shareTree);
    return { oom, crash, hang, after: after.ok };
  });
  expect(outs.oom.kind).toBe('oom');
  expect(outs.crash.kind).toBe('crash');
  expect(outs.hang.kind).toBe('timeout');
  expect(outs.hang.ms).toBeGreaterThan(1900);
  expect(outs.hang.ms).toBeLessThan(15_000);
  expect(outs.after).toBe(true);
});

test('the asset URL is absolute and honours a subpath deploy', async ({ page }) => {
  // Under a subpath deploy `document.baseURI` is `<origin>/sub/`. The page here is served at
  // `/`, so give the loader that base and rewrite the prefix back to the real asset.
  const seen = trackAssetRequests(page);
  await page.route('**/sub/pandoc/pandoc.wasm.gz', async (route) => {
    const real = new URL(route.request().url());
    real.pathname = real.pathname.replace('/sub/', '/');
    await route.fulfill({ response: await route.fetch({ url: real.href }) });
  });
  await boot(page);
  const out = await page.evaluate(async () => {
    const { pandoc } = window.__quartoTest!;
    const base = new URL('sub/', document.baseURI).href;
    const { runner, loader } = pandoc.createRunner({ loader: { env: { baseURI: base } } });
    const { request, shareTree } = pandoc.smokeJob();
    const o = await runner.run(request, shareTree);
    return { ok: o.ok, url: loader.assetUrl, base };
  });
  expect(out.ok).toBe(true);
  expect(out.url).toBe(`${out.base}pandoc/pandoc.wasm.gz`);
  expect(seen.map((r) => new URL(r.url()).pathname)).toEqual(['/sub/pandoc/pandoc.wasm.gz']);
});
