import { createHash, randomBytes } from 'node:crypto';
import { gzipSync } from 'node:zlib';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { FakeCache, fakeStorage } from '../test-utils/fakeCache';
import { MAX_DECOMPRESSION_RATIO, MIN_BROWSERS, browserEnv, PandocLoadError, PandocLoader, type LoaderEnv } from './pandocLoader';

const sha = (b: Uint8Array) => createHash('sha256').update(b).digest('hex');
const wasmBytes = (tag: string) => new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0, ...new TextEncoder().encode(tag)]);

const WASM = wasmBytes('good');
const WASM_SHA = sha(WASM);
const GZ = new Uint8Array(gzipSync(WASM));
const BASE = 'https://app.test/sub/';
const URL_ = `${BASE}pandoc/pandoc.wasm.gz`;

function harness(over: Partial<LoaderEnv> = {}, config: { idleMs?: number } = {}) {
  const compile = vi.fn(async (b: BufferSource) => ({ bytes: b }) as unknown as WebAssembly.Module);
  const fetchMock = vi.fn(async (_u: unknown, _i?: unknown) => new Response(GZ.slice(), { headers: { 'content-length': String(GZ.length) } }));
  const cache = new FakeCache();
  const env: Partial<LoaderEnv> = {
    fetch: fetchMock as unknown as typeof fetch,
    caches: fakeStorage(cache),
    hasWebAssembly: true,
    validate: () => true,
    compile,
    baseURI: BASE,
    isOnline: () => true,
    persist: async () => true,
    ...over,
  };
  return { loader: new PandocLoader({ env, ...config }), compile, fetchMock, cache };
}

const keyFor = (s: string) => `${URL_}?sha256=${s}`;

afterEach(() => vi.useRealTimers());

describe('PandocLoader: input cases (gzip, raw wasm, neither)', () => {
  it('gzip: decompresses, verifies, compiles, caches the compressed bytes', async () => {
    const { loader, compile, cache, fetchMock } = harness();
    const r = await loader.load(WASM_SHA);
    expect(r.source).toBe('network');
    expect(fetchMock.mock.calls[0][0]).toBe(URL_); // absolute, resolved against baseURI
    expect(new Uint8Array(compile.mock.calls[0][0] as ArrayBuffer)).toEqual(WASM);
    expect(cache.entries.get(keyFor(WASM_SHA))).toEqual(GZ);
  });

  it('raw wasm (a server decoded Content-Encoding: gzip): accepted', async () => {
    const { loader, compile } = harness({ fetch: (async () => new Response(WASM.slice())) as typeof fetch });
    await loader.load(WASM_SHA);
    expect(compile).toHaveBeenCalledOnce();
  });

  it('neither gzip nor wasm: rejected, naming the URL and size, never compiled', async () => {
    const junk = new TextEncoder().encode('<html>proxy login</html>');
    const { loader, compile, cache } = harness({ fetch: (async () => new Response(junk.slice())) as typeof fetch });
    const err = await loader.load(WASM_SHA).catch((e) => e);
    expect(err).toBeInstanceOf(PandocLoadError);
    expect(err.code).toBe('fetch-failed');
    expect(err.message).toContain(URL_);
    expect(err.message).toContain(String(junk.length));
    expect(compile).not.toHaveBeenCalled();
    expect(cache.entries.size).toBe(0);
  });

  it('a gzip bomb is rejected before it is buffered whole, and never compiled or cached', async () => {
    // Zeros: ~1000x. Starts with the wasm magic so only the ratio can reject it.
    const bomb = new Uint8Array(1 << 22);
    bomb.set(WASM);
    const gz = new Uint8Array(gzipSync(bomb));
    expect(bomb.length / gz.length).toBeGreaterThan(MAX_DECOMPRESSION_RATIO);
    const { loader, compile, cache } = harness({ fetch: (async () => new Response(gz.slice())) as typeof fetch });
    const err = await loader.load(sha(bomb)).catch((e) => e);
    expect(err).toBeInstanceOf(PandocLoadError);
    expect(err.code).toBe('fetch-failed');
    expect(err.message).toContain(URL_);
    expect(err.message).toContain(`${MAX_DECOMPRESSION_RATIO} times`);
    expect(compile).not.toHaveBeenCalled();
    expect(cache.entries.size).toBe(0);
  });

  it('a build that compresses just under the ratio is accepted', async () => {
    // Half random, half zeros: about 2x, inside the guard (the real asset is 3.55x).
    const payload = new Uint8Array(64 * 1024);
    payload.set(randomBytes(32 * 1024), 0);
    payload.set(WASM);
    const gz = new Uint8Array(gzipSync(payload));
    expect(payload.length / gz.length).toBeLessThan(MAX_DECOMPRESSION_RATIO);
    const { loader, compile } = harness({ fetch: (async () => new Response(gz.slice())) as typeof fetch });
    await loader.load(sha(payload));
    expect(compile).toHaveBeenCalledOnce();
  });

  it('a truncated gzip is reported as corrupt, not compiled', async () => {
    const { loader, compile } = harness({ fetch: (async () => new Response(GZ.slice(0, 12))) as typeof fetch });
    await expect(loader.load(WASM_SHA)).rejects.toMatchObject({ code: 'fetch-failed' });
    expect(compile).not.toHaveBeenCalled();
  });
});

describe('PandocLoader: SHA check and cache', () => {
  it('compares the SHA of the decompressed bytes: a .gz whose own hash matches is still rejected', async () => {
    const other = wasmBytes('evil');
    const gz = new Uint8Array(gzipSync(other));
    const { loader, compile, cache } = harness({ fetch: (async () => new Response(gz.slice())) as typeof fetch });
    // The expected value equals the *compressed* bytes' hash: not what is checked.
    await expect(loader.load(sha(gz))).rejects.toMatchObject({ code: 'checksum-mismatch' });
    // And a wrong decompressed hash is a mismatch naming URL and size.
    const err = await loader.load(WASM_SHA).catch((e) => e);
    expect(err.code).toBe('checksum-mismatch');
    expect(err.message).toContain(URL_);
    expect(compile).not.toHaveBeenCalled();
    expect(cache.entries.size).toBe(0);
  });

  it('a corrupted cache entry is dropped, refetched, and never compiled', async () => {
    const { loader, compile, cache, fetchMock } = harness();
    cache.entries.set(keyFor(WASM_SHA), new Uint8Array(gzipSync(wasmBytes('stale'))));
    const r = await loader.load(WASM_SHA);
    expect(r.source).toBe('network');
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(compile).toHaveBeenCalledOnce();
    expect(new Uint8Array(compile.mock.calls[0][0] as ArrayBuffer)).toEqual(WASM); // only verified bytes
    expect(r.notices.join(' ')).toMatch(/failed verification/);
    expect(cache.entries.get(keyFor(WASM_SHA))).toEqual(GZ); // replaced with the good copy
  });

  it('a cache hit skips the network', async () => {
    const { loader, cache, fetchMock } = harness();
    cache.entries.set(keyFor(WASM_SHA), GZ);
    const r = await loader.load(WASM_SHA);
    expect(r.source).toBe('cache');
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('evicts old-SHA entries after the new entry is written', async () => {
    const { loader, cache } = harness();
    cache.entries.set(keyFor('0'.repeat(64)), GZ);
    await loader.load(WASM_SHA);
    expect([...cache.entries.keys()]).toEqual([keyFor(WASM_SHA)]);
  });

  it('caches.open throwing SecurityError proceeds uncached', async () => {
    const caches = { open: async () => Promise.reject(new DOMException('denied', 'SecurityError')) } as unknown as CacheStorage;
    const { loader } = harness({ caches });
    const r = await loader.load(WASM_SHA);
    expect(r.source).toBe('network');
    expect(r.notices.join(' ')).toMatch(/SecurityError/);
  });

  it('QuotaExceededError on put proceeds uncached', async () => {
    const { loader, cache } = harness();
    cache.putError = new DOMException('full', 'QuotaExceededError');
    const r = await loader.load(WASM_SHA);
    expect(r.module).toBeDefined();
    expect(r.notices.join(' ')).toMatch(/not cached/);
    expect(cache.entries.size).toBe(0);
  });

  it('a cancel during the cache write leaves no unverified entry in use; the next load succeeds', async () => {
    const { loader, cache, compile } = harness();
    let open!: () => void;
    cache.putGate = new Promise<void>((r) => (open = r));
    const ac = new AbortController();
    const first = loader.load(WASM_SHA, { signal: ac.signal });
    first.catch(() => undefined);
    await vi.waitFor(() => expect(cache.putGate).not.toBeNull());
    await new Promise((r) => setTimeout(r, 20)); // let the put start and block
    ac.abort();
    await expect(first).rejects.toBeDefined();
    open(); // the write finishes after the cancel: what lands is the verified entry
    cache.putGate = null;
    await vi.waitFor(() => expect(cache.entries.size).toBe(1));
    expect(cache.entries.get(keyFor(WASM_SHA))).toEqual(GZ);
    const second = await loader.load(WASM_SHA);
    expect(second.module).toBeDefined();
    expect(compile.mock.calls.length).toBeGreaterThanOrEqual(1);
  });

  it('a half-written entry from an earlier cancelled write is caught by the per-load check', async () => {
    const { loader, cache } = harness();
    cache.entries.set(keyFor(WASM_SHA), GZ.slice(0, GZ.length - 5)); // truncated
    const r = await loader.load(WASM_SHA);
    expect(r.source).toBe('network');
  });
});

describe('PandocLoader: storage persistence', () => {
  it('asks for persistent storage once the verified entry is written', async () => {
    const persist = vi.fn(async () => true);
    const { loader } = harness({ persist });
    await loader.load(WASM_SHA);
    expect(persist).toHaveBeenCalledOnce();
  });

  it('a refused or throwing persist() does not fail the load', async () => {
    const { loader } = harness({ persist: async () => Promise.reject(new Error('nope')) });
    expect((await loader.load(WASM_SHA)).source).toBe('network');
  });

  it('does not ask when nothing was cached', async () => {
    const persist = vi.fn(async () => true);
    const { loader } = harness({ persist, caches: undefined });
    await loader.load(WASM_SHA);
    expect(persist).not.toHaveBeenCalled();
  });
});

describe('PandocLoader: feature detection', () => {
  it('WebAssembly missing altogether (Lockdown Mode, policy): its own message, not a download failure', async () => {
    const { loader, fetchMock } = harness({ hasWebAssembly: false, validate: () => false });
    await expect(loader.load(WASM_SHA)).rejects.toMatchObject({ code: 'no-wasm', message: expect.stringContaining('WebAssembly turned off') });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('the real environment reports a missing WebAssembly global instead of throwing', () => {
    vi.stubGlobal('WebAssembly', undefined);
    try {
      expect(browserEnv().hasWebAssembly).toBe(false);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it('exnref unavailable: friendly message naming browser versions, nothing downloaded', async () => {
    const { loader, fetchMock } = harness({ validate: () => false });
    const err = await loader.load(WASM_SHA).catch((e) => e);
    expect(err.code).toBe('no-exnref');
    expect(err.message).toContain(MIN_BROWSERS);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('DecompressionStream unavailable: its own message', async () => {
    const { loader, fetchMock } = harness({ DecompressionStream: undefined });
    await expect(loader.load(WASM_SHA)).rejects.toMatchObject({ code: 'no-decompression', message: expect.stringContaining('DecompressionStream') });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('crypto.subtle unavailable (insecure context): its own message', async () => {
    const { loader, fetchMock } = harness({ subtle: undefined });
    await expect(loader.load(WASM_SHA)).rejects.toMatchObject({ code: 'no-subtle-crypto', message: expect.stringContaining('secure contexts') });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('Cache API unavailable: proceeds uncached with a notice', async () => {
    const { loader } = harness({ caches: undefined });
    const r = await loader.load(WASM_SHA);
    expect(r.source).toBe('network');
    expect(r.notices.join(' ')).toMatch(/Cache API is unavailable/);
  });
});

describe('PandocLoader: failures', () => {
  it('HTTP error names the URL and status', async () => {
    const { loader } = harness({ fetch: (async () => new Response('nope', { status: 404 })) as typeof fetch });
    const err = await loader.load(WASM_SHA).catch((e) => e);
    expect(err).toMatchObject({ code: 'fetch-failed' });
    expect(err.message).toContain('404');
    expect(err.message).toContain(URL_);
  });

  it('offline with nothing cached: says the file may have been evicted', async () => {
    const { loader } = harness({ fetch: (async () => Promise.reject(new TypeError('Failed to fetch'))) as typeof fetch, isOnline: () => false });
    await expect(loader.load(WASM_SHA)).rejects.toMatchObject({ code: 'offline', message: expect.stringContaining('evicted') });
  });

  it('offline with a cached copy works', async () => {
    const { loader, cache } = harness({ fetch: (async () => Promise.reject(new TypeError('Failed to fetch'))) as typeof fetch, isOnline: () => false });
    cache.entries.set(keyFor(WASM_SHA), GZ);
    expect((await loader.load(WASM_SHA)).source).toBe('cache');
  });

  it('compile blocked (CSP/extension)', async () => {
    const { loader } = harness({ compile: async () => Promise.reject(new Error('CompileError: blocked by CSP')) });
    await expect(loader.load(WASM_SHA)).rejects.toMatchObject({ code: 'compile-blocked', message: expect.stringMatching(/content-security policy/) });
  });

  it('reports download progress with the content length', async () => {
    const { loader } = harness();
    const seen: string[] = [];
    await loader.load(WASM_SHA, { onProgress: (p) => seen.push(`${p.phase}:${p.loaded}/${p.total}`) });
    expect(seen).toContain(`download:${GZ.length}/${GZ.length}`);
    expect(seen.at(-1)).toBe('compile:0/null');
  });
});

describe('PandocLoader: cancel and sharing', () => {
  const slowFetch = () => {
    let calls = 0;
    let signalSeen: AbortSignal | undefined;
    const f = ((_u: unknown, init?: { signal?: AbortSignal }) => {
      calls++;
      signalSeen = init?.signal;
      return new Promise<Response>((resolve, reject) => {
        init?.signal?.addEventListener('abort', () => reject(init.signal?.reason ?? new Error('aborted')));
        (f as unknown as { resolve: () => void }).resolve = () => resolve(new Response(GZ.slice()));
      });
    }) as unknown as typeof fetch & { resolve: () => void };
    return { f, calls: () => calls, signal: () => signalSeen };
  };

  it('aborting the only waiter aborts the download', async () => {
    const s = slowFetch();
    const { loader } = harness({ fetch: s.f });
    const ac = new AbortController();
    const p = loader.load(WASM_SHA, { signal: ac.signal });
    await vi.waitFor(() => expect(s.calls()).toBe(1));
    ac.abort();
    await expect(p).rejects.toBeDefined();
    expect(s.signal()?.aborted).toBe(true);
  });

  it('a second click shares the in-flight load; the first one cancelling does not abort it', async () => {
    const s = slowFetch();
    const { loader } = harness({ fetch: s.f });
    const first = new AbortController();
    const a = loader.load(WASM_SHA, { signal: first.signal });
    a.catch(() => undefined);
    await vi.waitFor(() => expect(s.calls()).toBe(1));
    const b = loader.load(WASM_SHA);
    first.abort();
    await expect(a).rejects.toBeDefined();
    expect(s.signal()?.aborted).toBe(false);
    s.f.resolve();
    expect((await b).source).toBe('network');
    expect(s.calls()).toBe(1);
  });

  it('a load after the only waiter cancelled starts a fresh download', async () => {
    const s = slowFetch();
    const { loader } = harness({ fetch: s.f });
    const ac = new AbortController();
    const a = loader.load(WASM_SHA, { signal: ac.signal });
    a.catch(() => undefined);
    await vi.waitFor(() => expect(s.calls()).toBe(1));
    ac.abort();
    const b = loader.load(WASM_SHA);
    await vi.waitFor(() => expect(s.calls()).toBe(2));
    s.f.resolve();
    await b;
  });
});

describe('PandocLoader: resident module and idle drop', () => {
  it('keeps the module resident: a second load does not compile again', async () => {
    const { loader, compile } = harness();
    const a = await loader.load(WASM_SHA);
    const b = await loader.load(WASM_SHA);
    expect(b.source).toBe('resident');
    expect(b.module).toBe(a.module);
    expect(compile).toHaveBeenCalledOnce();
  });

  it('drops the module after the idle period and recompiles from the cache on the next load', async () => {
    vi.useFakeTimers();
    const { loader, compile, fetchMock } = harness({}, { idleMs: 5 * 60_000 });
    await loader.load(WASM_SHA);
    expect(loader.hasResidentModule).toBe(true);
    await vi.advanceTimersByTimeAsync(4 * 60_000);
    expect(loader.hasResidentModule).toBe(true);
    await vi.advanceTimersByTimeAsync(61_000);
    expect(loader.hasResidentModule).toBe(false);
    const r = await loader.load(WASM_SHA);
    expect(r.source).toBe('cache');
    expect(fetchMock).toHaveBeenCalledOnce(); // no second download
    expect(compile).toHaveBeenCalledTimes(2);
  });

  it('the idle timer does not run while a render holds the loader', async () => {
    vi.useFakeTimers();
    const { loader } = harness({}, { idleMs: 1000 });
    await loader.load(WASM_SHA);
    const release = loader.hold();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(loader.hasResidentModule).toBe(true);
    release();
    release(); // idempotent
    await vi.advanceTimersByTimeAsync(1001);
    expect(loader.hasResidentModule).toBe(false);
  });
});
