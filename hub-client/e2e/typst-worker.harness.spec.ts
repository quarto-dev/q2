/**
 * Browser tests for the typst compiler worker (host phase H7), against the production bundle
 * built with VITE_E2E=1 (the `window.__quartoTest.typst` hook; see src/test-hooks.ts). They need
 * the served assets in the build (`node scripts/fetch-pandoc-wasm.mjs`), the Rust wasm (for the
 * vendored packages) and no hub server.
 *
 * The package registry is faked with Playwright routes, so nothing here touches
 * packages.typst.org; the fetch, the Cache API and the 404/offline diagnostics are the real
 * worker code.
 */
import { expect, test, type BrowserContext, type Page, type Request } from '@playwright/test';
import type {} from './helpers/testHooks';
import { pkg, tarGz } from '../../ts-packages/typst-host/src/fixtures.test-util';

const WASM = /\/typst\/typst\.wasm\.gz(\?|$)/;
const FONTS = /\/typst\/fonts\.bin\.gz(\?|$)/;

type Summary = {
  ok: boolean;
  kind?: string;
  pages?: number;
  header?: string;
  families?: string[];
  diagnostics: { origin?: string; code?: string; message?: string; package?: string }[];
  notices: string[];
  uiState: string;
  attempts?: number;
  packagesFetched?: number;
};

async function boot(page: Page) {
  await page.goto('/');
  await page.waitForFunction(() => !!window.__quartoTestReady);
  await page.evaluate(async () => {
    await window.__quartoTestReady;
  });
}

function track(page: Page, re: RegExp): Request[] {
  const seen: Request[] = [];
  page.on('request', (r) => {
    if (re.test(r.url())) seen.push(r);
  });
  return seen;
}

/** Compile `body` (and optional extra files) with a fresh runner; the whole summary is serialisable. */
async function compile(page: Page, body: string, options: { vendored?: boolean; files?: { path: string; text: string }[] } = {}): Promise<Summary> {
  return page.evaluate(
    async ({ body, options }) => {
      const { typst } = window.__quartoTest!;
      const w = window as unknown as { __typstRunner?: ReturnType<typeof typst.createRunner> };
      const { runner } = (w.__typstRunner ??= typst.createRunner());
      const enc = new TextEncoder();
      const vendored = options.vendored ? await typst.vendoredAssets() : undefined;
      const files = [{ path: '/doc/main.typ', bytes: enc.encode(body) }, ...(options.files ?? []).map((f) => ({ path: f.path, bytes: enc.encode(f.text) }))];
      const o = await runner.run({ input: { main: '/doc/main.typ', files }, fonts: vendored?.fonts, vendoredPackages: vendored?.vendoredPackages });
      const stats = o.ok ? o.stats : undefined;
      return {
        ok: o.ok,
        kind: o.ok ? undefined : o.kind,
        pages: o.ok ? o.pages : undefined,
        header: o.ok ? new TextDecoder('latin1').decode(o.pdf.subarray(0, 8)) : undefined,
        families: o.ok ? o.fontFamilies : undefined,
        diagnostics: o.diagnostics as never,
        notices: o.notices,
        uiState: typst.uiStateFor(o),
        attempts: stats?.attempts,
        packagesFetched: stats?.packagesFetched,
      };
    },
    { body, options },
  );
}

async function fakeRegistry(context: BrowserContext, tarballs: Record<string, Uint8Array | 'offline' | 404>) {
  const hits: string[] = [];
  await context.route('https://packages.typst.org/**', async (route) => {
    const name = new URL(route.request().url()).pathname.split('/').pop()!.replace(/\.tar\.gz$/, '');
    // A CORS preflight (WebKit sends one for a cross-origin worker fetch) is not a registry hit.
    if (route.request().method() === 'OPTIONS') return route.fulfill({ status: 204, headers: { 'access-control-allow-origin': '*', 'access-control-allow-methods': 'GET' } });
    hits.push(name);
    const t = tarballs[name];
    if (t === 'offline') return route.abort('internetdisconnected');
    if (t === undefined || t === 404) return route.fulfill({ status: 404, body: 'not found' });
    return route.fulfill({ status: 200, contentType: 'application/gzip', headers: { 'access-control-allow-origin': '*' }, body: Buffer.from(t) });
  });
  return hits;
}

test('no typst request before first use', async ({ page }) => {
  const wasm = track(page, WASM);
  const fonts = track(page, FONTS);
  await boot(page);
  await page.waitForTimeout(1500);
  expect(wasm).toHaveLength(0);
  expect(fonts).toHaveLength(0);
});

test('compiles a two-page document in a worker; the assets are fetched once and reused', async ({ page }) => {
  const wasm = track(page, WASM);
  const fonts = track(page, FONTS);
  await boot(page);
  const a = await compile(page, '= One\nHello\n#pagebreak()\nTwo');
  expect(a, JSON.stringify(a)).toMatchObject({ ok: true, pages: 2, uiState: 'done' });
  expect(a.header).toMatch(/^%PDF-1\./);
  expect(a.families).toEqual(expect.arrayContaining(['Libertinus Serif', 'New Computer Modern', 'DejaVu Sans Mono']));
  const b = await compile(page, 'One page');
  expect(b).toMatchObject({ ok: true, pages: 1 });
  expect(wasm).toHaveLength(1);
  expect(fonts).toHaveLength(1);
});

test('a vendored package and the Font Awesome fonts come from the Rust assets with no registry request', async ({ page, context }) => {
  const hits = await fakeRegistry(context, {});
  await boot(page);
  const r = await compile(page, '#import "@preview/fontawesome:0.5.0": fa-icon\n#fa-icon("heart")', { vendored: true });
  expect(r, JSON.stringify(r)).toMatchObject({ ok: true, pages: 1 });
  expect(r.families?.some((f) => f.startsWith('Font Awesome 6'))).toBe(true);
  expect(hits).toEqual([]);
});

test('a registry package is fetched once, then served from the Cache API by the next worker', async ({ page, context }) => {
  const hits = await fakeRegistry(context, { 'mini-0.1.0': await tarGz(pkg('mini', '0.1.0', '#let hello = "from mini"')) });
  await boot(page);
  const src = '#import "@preview/mini:0.1.0": hello\n#hello';
  const a = await compile(page, src);
  expect(a, JSON.stringify(a)).toMatchObject({ ok: true, packagesFetched: 1 });
  const b = await compile(page, src);
  expect(b).toMatchObject({ ok: true });
  expect(hits).toEqual(['mini-0.1.0']);
});

test('a package that imports a package is found by the retry', async ({ page, context }) => {
  const hits = await fakeRegistry(context, {
    'outer-0.1.0': await tarGz(pkg('outer', '0.1.0', '#import "@preview/inner:0.2.0": value\n#let greeting = value')),
    'inner-0.2.0': await tarGz(pkg('inner', '0.2.0', '#let value = "nested"')),
  });
  await boot(page);
  const r = await compile(page, '#import "@preview/outer:0.1.0": greeting\n#greeting');
  expect(r, JSON.stringify(r)).toMatchObject({ ok: true, attempts: 2, packagesFetched: 2 });
  expect(hits.sort()).toEqual(['inner-0.2.0', 'outer-0.1.0']);
});

test('a missing package yields a diagnostic naming it', async ({ page, context }) => {
  await fakeRegistry(context, {});
  await boot(page);
  const r = await compile(page, '#import "@preview/nope:1.0.0": x\n#x');
  expect(r).toMatchObject({ ok: false, kind: 'typst-error', uiState: 'typst-error' });
  expect(r.diagnostics.some((d) => d.origin === 'host' && d.package === '@preview/nope:1.0.0')).toBe(true);
});

test('a package fetch failure (offline) names the package and the cause', async ({ page, context }) => {
  await fakeRegistry(context, { 'down-1.0.0': 'offline' });
  await boot(page);
  const r = await compile(page, '#import "@preview/down:1.0.0": x\n#x');
  expect(r).toMatchObject({ ok: false, kind: 'package-fetch', uiState: 'package-error' });
  const d = r.diagnostics[0];
  expect(d).toMatchObject({ origin: 'host', code: 'package-fetch-failed', package: '@preview/down:1.0.0' });
  expect(d.message).toContain('@preview/down:1.0.0');
});

test('a typst error carries its position and blocks the PDF', async ({ page }) => {
  await boot(page);
  const r = await compile(page, '= Title\n#let x = (1 + "a")\n#x');
  expect(r).toMatchObject({ ok: false, kind: 'typst-error' });
  expect(r.diagnostics[0]).toMatchObject({ origin: 'typst' });
});

test('abort mid-start terminates the worker, reports one cancel, and the module stays for the next compile', async ({ page }) => {
  await boot(page);
  const out = await page.evaluate(async () => {
    const { typst } = window.__quartoTest!;
    const { runner, loader } = typst.createRunner();
    const job = { input: { main: '/m.typ', files: [{ path: '/m.typ', bytes: new TextEncoder().encode('x') }] } };
    const ac = new AbortController();
    const first = await runner.run(job, { signal: ac.signal, onStage: (s) => s === 'starting' && queueMicrotask(() => ac.abort()) });
    const second = await runner.run({ input: { main: '/m.typ', files: [{ path: '/m.typ', bytes: new TextEncoder().encode('x') }] } });
    return { first: { ok: first.ok, kind: first.ok ? undefined : first.kind, diagnostics: first.diagnostics.length }, secondOk: second.ok, resident: loader.hasResidentModule };
  });
  expect(out).toEqual({ first: { ok: false, kind: 'aborted', diagnostics: 0 }, secondOk: true, resident: true });
});

test('the wall timeout stops a worker and says so', async ({ page }) => {
  await boot(page);
  const out = await page.evaluate(async () => {
    const { typst } = window.__quartoTest!;
    const { runner } = typst.createRunner({ wallTimeoutMs: 1 });
    const o = await runner.run({ input: { main: '/m.typ', files: [{ path: '/m.typ', bytes: new TextEncoder().encode('x') }] } });
    return { ok: o.ok, kind: o.ok ? undefined : o.kind, code: (o.diagnostics[0] as { code?: string } | undefined)?.code, uiState: typst.uiStateFor(o) };
  });
  expect(out).toEqual({ ok: false, kind: 'timeout', code: 'typst-timeout', uiState: 'timeout' });
});

test('an offline load with nothing cached is "offline", not a hang', async ({ page, context }) => {
  await boot(page);
  await context.setOffline(true);
  const out = await page.evaluate(async () => {
    const { typst } = window.__quartoTest!;
    const { runner } = typst.createRunner();
    const o = await runner.run({ input: { main: '/m.typ', files: [{ path: '/m.typ', bytes: new TextEncoder().encode('x') }] } });
    return { ok: o.ok, uiState: typst.uiStateFor(o) };
  });
  await context.setOffline(false);
  expect(out).toMatchObject({ ok: false });
  expect(['offline', 'download-failed']).toContain(out.uiState);
});
