/**
 * Real-wasm tests for the typst loader, runner and worker (host phase H7): the compressed
 * assets through the loaders (the same verified, cached path as pandoc.wasm), a compile through
 * a real worker thread, and the abort/terminate behaviour against a real instance.
 * Needs `node scripts/fetch-pandoc-wasm.mjs` plus `npm run build -w ts-packages/typst-host`-free
 * sources (the worker thread imports the TS source directly).
 */
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync } from 'node:zlib';
import { beforeAll, describe, expect, it } from 'vitest';
import { countPdfPages } from '@quarto/typst-host';
import { FakeCache } from '../test-utils/fakeCache';
import { nodeTypstWorker } from '../test-utils/nodeTypstWorker';
import { createTypstLoader, TypstFontsLoader, TYPST_FONTS_PATH, TYPST_FONTS_SHA256, TYPST_WASM_PATH, TYPST_WASM_SHA256 } from './typstAssets';
import { TypstRunner } from './typstRunner';

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, '../../..');
const PUBLIC = path.join(repo, 'hub-client/public');
const CI = !!process.env.CI;
const haveAssets = existsSync(path.join(PUBLIC, TYPST_WASM_PATH)) && existsSync(path.join(PUBLIC, TYPST_FONTS_PATH));
const BASE = 'https://app.test/';

it('has the typst assets (required in CI)', () => {
  if (!haveAssets && CI) throw new Error(`typst assets missing under ${PUBLIC}/typst: run node scripts/fetch-pandoc-wasm.mjs --require`);
  if (!haveAssets) console.warn('SKIPPING real-wasm typst tests: run node scripts/fetch-pandoc-wasm.mjs');
});

const text = (s: string) => new TextEncoder().encode(s);

describe.skipIf(!haveAssets)('typst loader and runner against the real assets', () => {
  const served: Record<string, Uint8Array> = {};
  beforeAll(() => {
    for (const p of [TYPST_WASM_PATH, TYPST_FONTS_PATH]) served[`${BASE}${p}`] = new Uint8Array(readFileSync(path.join(PUBLIC, p)));
  });

  const make = (opts: { wallTimeoutMs?: number; raw?: boolean } = {}) => {
    const urls: string[] = [];
    // The loaders use different cache names, as in the browser; a shared FakeCache would evict
    // one asset when the other is stored (the loader drops entries for other SHAs).
    const caches = new Map<string, FakeCache>();
    const storage = { open: async (name: string) => (caches.get(name) ?? caches.set(name, new FakeCache()).get(name)) } as unknown as CacheStorage;
    const env = {
      caches: storage,
      baseURI: BASE,
      fetch: (async (url: string) => {
        urls.push(url);
        const body = served[url];
        // `raw`: a server that adds Content-Encoding: gzip to *.gz, so the browser hands over the decoded bytes.
        return body ? new Response((opts.raw ? new Uint8Array(gunzipSync(body)) : body).slice()) : new Response('', { status: 404 });
      }) as unknown as typeof fetch,
    };
    const loader = createTypstLoader({ env });
    const fonts = new TypstFontsLoader({ env });
    const runner = new TypstRunner({ loader, fonts, createWorker: nodeTypstWorker, wallTimeoutMs: opts.wallTimeoutMs });
    return { loader, fonts, runner, caches, urls };
  };

  it('the served assets match the pinned checksums (the loaders verify them)', async () => {
    const { loader, fonts, caches } = make();
    const wasm = await loader.load(TYPST_WASM_SHA256);
    expect(wasm.module).toBeInstanceOf(WebAssembly.Module);
    expect(wasm.source).toBe('network');
    const f = await fonts.load();
    expect(f.fonts).toHaveLength(17);
    expect([...(caches.get('q2-typst-wasm-v1')?.entries.keys() ?? [])]).toEqual([`${BASE}${TYPST_WASM_PATH}?sha256=${TYPST_WASM_SHA256}`]);
    expect([...(caches.get('q2-typst-fonts-v1')?.entries.keys() ?? [])]).toEqual([`${BASE}${TYPST_FONTS_PATH}?sha256=${TYPST_FONTS_SHA256}`]);
  }, 60_000);

  it('loads both assets when the server has already decoded them (Content-Encoding: gzip on *.gz)', async () => {
    const { loader, fonts } = make({ raw: true });
    expect((await loader.load(TYPST_WASM_SHA256)).module).toBeInstanceOf(WebAssembly.Module);
    expect((await fonts.load()).fonts).toHaveLength(17);
  }, 60_000);

  it('compiles a document in a real worker, and a second compile reuses the resident assets', async () => {
    const { runner, urls } = make();
    const input = (body: string) => ({ main: '/doc/a.typ', files: [{ path: '/doc/a.typ', bytes: text(body) }] });
    const a = await runner.run({ input: input('= One\nHello\n#pagebreak()\nTwo') });
    if (!a.ok) throw new Error(JSON.stringify(a.diagnostics));
    expect(a.pages).toBe(2);
    expect(countPdfPages(a.pdf)).toBe(2);
    expect(a.fontFamilies).toEqual(expect.arrayContaining(['Libertinus Serif', 'New Computer Modern', 'DejaVu Sans Mono']));
    const fetched = urls.length;
    expect(fetched).toBe(2);
    const b = await runner.run({ input: input('Just one page') });
    expect(b).toMatchObject({ ok: true, pages: 1 });
    expect(urls).toHaveLength(fetched);
  }, 60_000);

  it('a missing package is named in the diagnostics (no fetcher in this worker)', async () => {
    const { runner } = make();
    const o = await runner.run({ input: { main: '/m.typ', files: [{ path: '/m.typ', bytes: text('#import "@preview/not-there:1.0.0": x\n#x') }] } });
    expect(o).toMatchObject({ ok: false, kind: 'typst-error' });
    expect(o.diagnostics.some((d) => d.origin === 'host' && 'package' in d && d.package === '@preview/not-there:1.0.0')).toBe(true);
  }, 60_000);

  it('listFonts reports the default families plus the job’s own fonts', async () => {
    const { runner } = make();
    const o = await runner.listFonts();
    if (!o.ok) throw new Error(JSON.stringify(o.diagnostics));
    expect(o.families).toEqual(expect.arrayContaining(['Libertinus Serif', 'New Computer Modern', 'DejaVu Sans Mono']));
  }, 60_000);

  it('the wall timeout terminates a real worker; the Module survives for the next compile', async () => {
    const { runner, loader } = make();
    // A zero-ish wall limit fires while the worker is still instantiating.
    const slow = await runner.run({ input: { main: '/m.typ', files: [{ path: '/m.typ', bytes: text('x') }] } }, { wallTimeoutMs: 1 });
    expect(slow).toMatchObject({ ok: false, kind: 'timeout' });
    expect(loader.hasResidentModule).toBe(true);
    const ok = await runner.run({ input: { main: '/m.typ', files: [{ path: '/m.typ', bytes: text('x') }] } });
    expect(ok.ok).toBe(true);
  }, 60_000);

  it('an abort during a compile terminates the worker and reports one cancel', async () => {
    const { runner } = make();
    const ac = new AbortController();
    const p = runner.run({ input: { main: '/m.typ', files: [{ path: '/m.typ', bytes: text('x') }] } }, { signal: ac.signal, onStage: (s) => s === 'starting' && ac.abort() });
    expect(await p).toMatchObject({ ok: false, kind: 'aborted', diagnostics: [] });
  }, 60_000);
});
