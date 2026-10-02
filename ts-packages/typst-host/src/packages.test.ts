import { beforeAll, describe, expect, it } from 'vitest';
import { packageImports, missingPackage, registryFetcher, tarballUrl } from './packages.ts';
import { TypstSession } from './session.ts';
import type { PackageFetcher } from './types.ts';
import { defaultFonts, pkg, tarGz, text, typstModule, vendoredPackages } from './fixtures.test-util.ts';

describe('package import scanning', () => {
  it('finds exact-version imports in source, once each', () => {
    const src = '#import "@preview/a-b:1.2.3": x\n#import "@preview/a-b:1.2.3"\n#import "@local/z:0.1.0"\n#import "lib.typ"\n#let s = "@preview/not-a-spec"';
    expect(packageImports(src).map((s) => `${s.namespace}/${s.name}:${s.version}`)).toEqual(['preview/a-b:1.2.3', 'local/z:0.1.0']);
  });

  it('reads the package out of typst\'s not-found message', () => {
    expect(missingPackage('package not found (searched for @preview/nope:1.0.0)')).toEqual({ namespace: 'preview', name: 'nope', version: '1.0.0' });
    expect(missingPackage('file not found')).toBeUndefined();
  });

  it('builds registry URLs for the preview namespace only', () => {
    expect(tarballUrl({ namespace: 'preview', name: 'cetz', version: '0.3.0' })).toBe('https://packages.typst.org/preview/cetz-0.3.0.tar.gz');
    expect(tarballUrl({ namespace: 'local', name: 'cetz', version: '0.3.0' })).toBeUndefined();
  });
});

describe('registryFetcher', () => {
  const spec = { namespace: 'preview', name: 'p', version: '1.0.0' };
  it('serves from the cache before the network, and caches what it fetches', async () => {
    const store = new Map<string, Uint8Array>();
    let calls = 0;
    const fetcher = registryFetcher({
      fetch: (async () => (calls++, new Response(new Uint8Array([1, 2, 3])))) as typeof fetch,
      cache: { get: async (u) => store.get(u), put: async (u, b) => void store.set(u, b) },
    });
    expect(await fetcher(spec)).toEqual(new Uint8Array([1, 2, 3]));
    expect(await fetcher(spec)).toEqual(new Uint8Array([1, 2, 3]));
    expect(calls).toBe(1);
  });

  it('maps 404 to undefined and other statuses to an error', async () => {
    expect(await registryFetcher({ fetch: (async () => new Response('', { status: 404 })) as typeof fetch })(spec)).toBeUndefined();
    await expect(registryFetcher({ fetch: (async () => new Response('', { status: 503 })) as typeof fetch })(spec)).rejects.toThrow(/503/);
  });
});

let module: WebAssembly.Module;
let fonts: Uint8Array[];
beforeAll(async () => {
  module = await typstModule();
  fonts = defaultFonts();
}, 60_000);

const doc = (src: string) => ({ main: '/main.typ', files: [{ path: '/main.typ', bytes: text(src) }] });
const make = (fetchPackage?: PackageFetcher, limits = {}) => TypstSession.create({ module, fonts, vendoredPackages: vendoredPackages(), fetchPackage, limits });

describe('packages during a compile', () => {
  it('uses a vendored package without any fetch', async () => {
    let fetched = 0;
    const s = await make(async () => (fetched++, undefined));
    const r = await s.compile(doc('#import "@preview/fontawesome:0.5.0": fa-icon\n#fa-icon("heart")'));
    if (!r.ok) throw new Error(JSON.stringify(r.diagnostics));
    expect(fetched).toBe(0);
  });

  it('fetches a named package and strips ./ from its tarball paths', async () => {
    const tarball = await tarGz(pkg('mini', '0.1.0', '#let hello = "from mini"'));
    const asked: string[] = [];
    const s = await make(async (spec) => (asked.push(spec.name), tarball));
    const r = await s.compile(doc('#import "@preview/mini:0.1.0": hello\n#hello'));
    if (!r.ok) throw new Error(JSON.stringify(r.diagnostics));
    expect(asked).toEqual(['mini']);
    expect(r.stats).toMatchObject({ packagesFetched: 1, attempts: 1 });
    expect(r.stats.packageBytes).toBe(tarball.length);
  });

  it('finds a package that a package imports by retrying after the not-found diagnostic', async () => {
    const tarballs: Record<string, Uint8Array> = {
      outer: await tarGz(pkg('outer', '0.1.0', '#import "@preview/inner:0.2.0": value\n#let greeting = value')),
      inner: await tarGz(pkg('inner', '0.2.0', '#let value = "nested"')),
    };
    const asked: string[] = [];
    const s = await make(async (spec) => (asked.push(spec.name), tarballs[spec.name]));
    const r = await s.compile(doc('#import "@preview/outer:0.1.0": greeting\n#greeting'));
    if (!r.ok) throw new Error(JSON.stringify(r.diagnostics));
    expect(asked).toEqual(['outer', 'inner']);
    expect(r.stats).toMatchObject({ packagesFetched: 2, attempts: 2 });
  });

  it('names a package the registry does not have (404)', async () => {
    const s = await make(async () => undefined);
    const r = await s.compile(doc('#import "@preview/nope:1.0.0": x\n#x'));
    expect(r).toMatchObject({ ok: false, kind: 'typst-error' });
    expect(r.diagnostics.some((d) => d.origin === 'host' && d.code === 'package-not-found' && d.package === '@preview/nope:1.0.0')).toBe(true);
    expect(r.diagnostics.some((d) => d.origin === 'typst' && d.message.includes('@preview/nope:1.0.0'))).toBe(true);
  });

  it('names a package it cannot fetch (offline)', async () => {
    const s = await make(async () => {
      throw new TypeError('Failed to fetch');
    });
    const r = await s.compile(doc('#import "@preview/offline-pkg:2.0.0": x\n#x'));
    expect(r).toMatchObject({ ok: false, kind: 'package-fetch' });
    const d = r.diagnostics[0];
    expect(d).toMatchObject({ origin: 'host', code: 'package-fetch-failed', package: '@preview/offline-pkg:2.0.0' });
    expect(d.message).toContain('@preview/offline-pkg:2.0.0');
    expect(d.message).toContain('Failed to fetch');
  });

  it('without a fetcher, a package outside the vendored set is reported as not found', async () => {
    const s = await make();
    const r = await s.compile(doc('#import "@preview/cetz:0.3.0": canvas\n#canvas'));
    expect(r).toMatchObject({ ok: false, kind: 'typst-error' });
    expect(r.diagnostics.some((d) => d.origin === 'host' && d.package === '@preview/cetz:0.3.0')).toBe(true);
  });

  it('stops at the package-count limit', async () => {
    const s = await make(async () => undefined, { max_packages: 1 });
    const r = await s.compile(doc('#import "@preview/a:1.0.0"\n#import "@preview/b:1.0.0"'));
    expect(r).toMatchObject({ ok: false, kind: 'package-fetch' });
    expect(r.diagnostics[0]).toMatchObject({ code: 'limit-exceeded' });
  });

  it('stops at the package-bytes limit', async () => {
    const tarball = await tarGz(pkg('big', '1.0.0', '#let x = 1'));
    const s = await make(async () => tarball, { package_bytes: 10 });
    const r = await s.compile(doc('#import "@preview/big:1.0.0": x\n#x'));
    expect(r).toMatchObject({ ok: false, kind: 'package-fetch' });
    expect(r.diagnostics[0]).toMatchObject({ code: 'limit-exceeded' });
  });

  it('stops at the package time limit', async () => {
    const s = await make((_spec, signal) => new Promise((_res, rej) => signal?.addEventListener('abort', () => rej(new Error('aborted')))), { package_ms: 50 });
    const r = await s.compile(doc('#import "@preview/slow:1.0.0": x\n#x'));
    expect(r).toMatchObject({ ok: false, kind: 'package-fetch' });
    expect(r.diagnostics[0].message).toMatch(/time limit/);
  });
});
