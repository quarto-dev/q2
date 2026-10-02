/**
 * Real-wasm tests for the pandoc loader and runner (host phase H2): the compressed asset
 * through the loader to a compiled `Module`, the corrupted-cache and sniffing cases against
 * the real bytes, a render through a real worker thread, and memory over many renders.
 * Needs `node scripts/fetch-pandoc-wasm.mjs` (a missing wasm skips locally and fails under CI).
 */
import { readFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { beforeAll, describe, expect, it } from 'vitest';
import { FakeCache, fakeStorage } from '../test-utils/fakeCache';
import { nodePandocWorker } from '../test-utils/nodeWorker';
import { CONSTANTS, WASM_PATH, loadRecording, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { EXNREF_PROBE, PandocLoader } from './pandocLoader';
import { PandocRunner } from './pandocRunner';
import { smokeJob } from './smokeJob';

const CI = !!process.env.CI;
const haveWasm = pandocWasmAvailable();
const SHA: string = CONSTANTS.wasm_sha256;
const BASE = 'https://app.test/';
const KEY = `${BASE}pandoc/pandoc.wasm.gz?sha256=${SHA}`;

it('has pandoc.wasm available (required in CI)', () => {
  if (!haveWasm && CI) throw new Error(`pandoc.wasm missing at ${WASM_PATH}: run node scripts/fetch-pandoc-wasm.mjs --require`);
  if (!haveWasm) console.warn(`SKIPPING real-wasm loader tests: ${WASM_PATH} missing`);
});

it('the exnref probe validates only where the proposal exists (this process has the flag)', () => {
  expect(WebAssembly.validate(EXNREF_PROBE)).toBe(true);
});

describe.skipIf(!haveWasm)('loader and runner against the real pandoc.wasm', () => {
  let wasm: Uint8Array;
  let gz: Uint8Array;
  beforeAll(() => {
    wasm = new Uint8Array(readFileSync(WASM_PATH));
    gz = new Uint8Array(gzipSync(wasm, { level: 1 }));
  });

  const newLoader = (body: () => Uint8Array, cache = new FakeCache()) => {
    let fetches = 0;
    const loader = new PandocLoader({
      env: {
        caches: fakeStorage(cache),
        baseURI: BASE,
        fetch: (async () => {
          fetches++;
          return new Response(body().slice());
        }) as typeof fetch,
      },
    });
    return { loader, cache, fetches: () => fetches };
  };

  it('compressed fixture -> verified -> compiled Module, cached; the second load is a cache hit', async () => {
    const { loader, cache, fetches } = newLoader(() => gz);
    const r = await loader.load(SHA);
    expect(r.module).toBeInstanceOf(WebAssembly.Module);
    expect(r.source).toBe('network');
    expect(cache.entries.has(KEY)).toBe(true);
    loader.dropResident();
    const again = await loader.load(SHA);
    expect(again.source).toBe('cache');
    expect(fetches()).toBe(1);
  }, 60_000);

  it('a corrupted cache entry is refetched and never compiled', async () => {
    const bad = gz.slice();
    bad[Math.floor(bad.length / 2)] ^= 0xff;
    const cache = new FakeCache();
    cache.entries.set(KEY, bad);
    const { loader, fetches } = newLoader(() => gz, cache);
    const r = await loader.load(SHA);
    expect(r.source).toBe('network');
    expect(fetches()).toBe(1);
    expect(r.notices.join(' ')).toMatch(/failed verification/);
    // (not toEqual: vitest's deep equality over 16 MB typed arrays exhausts the heap)
    expect(Buffer.from(cache.entries.get(KEY) as Uint8Array).equals(gz)).toBe(true);
  }, 60_000);

  it('raw wasm from a server that decoded Content-Encoding is accepted', async () => {
    const { loader } = newLoader(() => wasm);
    expect((await loader.load(SHA)).module).toBeInstanceOf(WebAssembly.Module);
  }, 60_000);

  it('a wasm that decompresses to the wrong bytes is rejected before compiling', async () => {
    const tampered = wasm.slice();
    tampered[tampered.length - 1] ^= 1;
    const { loader } = newLoader(() => new Uint8Array(gzipSync(tampered, { level: 1 })));
    await expect(loader.load(SHA)).rejects.toMatchObject({ code: 'checksum-mismatch' });
  }, 60_000);

  describe('runner through a real worker thread', () => {
    it('renders, and about 20 consecutive renders keep memory flat within a bound', async () => {
      const { loader } = newLoader(() => gz);
      const runner = new PandocRunner({ loader, createWorker: nodePandocWorker });
      const run = async (i: number) => {
        const { request, shareTree } = smokeJob(`Render ${i}\n`, SHA);
        const out = await runner.run(request, shareTree);
        if (!out.ok) throw new Error(`${out.kind}: ${out.stderr} ${JSON.stringify(out.diagnostics)}`);
        expect(new TextDecoder().decode(out.output).trim()).toBe(`Render ${i}`);
      };
      await run(0);
      await run(1); // warm: the Module is resident, allocator pools are primed
      globalThis.gc?.();
      const before = process.memoryUsage().rss;
      for (let i = 2; i < 22; i++) await run(i);
      globalThis.gc?.();
      const growth = process.memoryUsage().rss - before;
      // Measured ~8 MB over 20 renders. A leaked instance is ~60 MB of linear memory or more, so 20 leaks would be >1 GB.
      expect(growth, `rss growth over 20 renders: ${(growth / 1024 / 1024).toFixed(1)} MB`).toBeLessThan(100 * 1024 * 1024);
    }, 300_000);

    it('a hang fault is stopped by the wall timeout; the Module survives for the next render', async () => {
      const { loader } = newLoader(() => gz);
      const runner = new PandocRunner({ loader, createWorker: nodePandocWorker, wallTimeoutMs: 1500 });
      const a = smokeJob('x\n', SHA);
      const hung = await runner.run(a.request, a.shareTree, { fault: { kind: 'hang' } });
      expect(hung).toMatchObject({ ok: false, kind: 'timeout' });
      expect(loader.hasResidentModule).toBe(true);
      const b = smokeJob('after\n', SHA);
      const out = await runner.run(b.request, b.shareTree);
      expect(out.ok).toBe(true);
    }, 120_000);

    it('oom and crash faults come back classified', async () => {
      const { loader } = newLoader(() => gz);
      const runner = new PandocRunner({ loader, createWorker: nodePandocWorker });
      // A trivial document fits in any heap; the H1 test uses this recording for the same reason.
      const rec = loadRecording('callouts-docx');
      expect(await runner.run(rec.request, rec.shareTree, { fault: { kind: 'oom', limit: '5m' } })).toMatchObject({ ok: false, kind: 'oom', status: 251 });
      const b = smokeJob('x\n', SHA);
      expect(await runner.run(b.request, b.shareTree, { fault: { kind: 'crash' } })).toMatchObject({ ok: false, kind: 'crash' });
    }, 120_000);
  });
});
