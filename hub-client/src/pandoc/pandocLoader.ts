/**
 * Main-thread loader for pandoc.wasm (host phase H2; design D2, D4, D6).
 *
 * Fetches `pandoc.wasm.gz` (opaque gzip), caches the compressed bytes in the Cache API,
 * decompresses with `DecompressionStream`, checks the SHA-256 of the *decompressed* wasm
 * against the request's `expected_pandoc_wasm_sha256`, and only then compiles. The
 * compiled `WebAssembly.Module` stays resident here (so terminating a worker never
 * discards the compile) until an idle timeout drops it.
 *
 * It lives in hub-client, not in `@quarto/pandoc-host`, because it needs `document.baseURI`,
 * `caches` and `crypto.subtle`; the host package stays DOM-free. Every environment
 * dependency is injectable (`LoaderEnv`) so the unit tests use fakes.
 */

/** Minimum browsers with WebAssembly exception handling (exnref), per MDN's compat data (D4). */
export const MIN_BROWSERS = 'Chrome/Edge 137, Firefox 131 or Safari 18.4';

/** Where the asset is served, relative to the page (`hub-client/public/pandoc/`). */
export const DEFAULT_ASSET_PATH = 'pandoc/pandoc.wasm.gz';

export const DEFAULT_CACHE_NAME = 'q2-pandoc-wasm-v1';

/**
 * A response that gunzips to more than this many times its compressed size is rejected
 * before it is buffered whole (a gzip bomb, or a server answering with the wrong file).
 * The pinned asset expands 3.55x (59.2 MB from 16.7 MB; H3 measurement), so this leaves
 * room for a future pandoc that compresses worse while stopping gzip's ~1000x worst case.
 */
export const MAX_DECOMPRESSION_RATIO = 8;

/** The resident `Module` is dropped after this long with no load or render active. */
export const DEFAULT_IDLE_MS = 5 * 60 * 1000;

/**
 * A minimal module using `try_table` with `catch_all_ref`: it validates only where the
 * exnref exception-handling proposal is available, which pandoc.wasm needs.
 */
export const EXNREF_PROBE = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0, 10, 16, 1, 14, 0, 2, 0x69, 0x1f, 0x40, 1, 3, 0, 0x0b, 0xd0, 0x69, 0x0b, 0x1a,
  0x0b,
]);

export type LoadErrorCode =
  /** `WebAssembly` itself is missing (iOS Lockdown Mode, a browser policy). */
  | 'no-wasm'
  /** exnref exception handling is unavailable (D4). */
  | 'no-exnref'
  | 'no-decompression'
  | 'no-subtle-crypto'
  /** The download failed (network, HTTP status, or the body is neither gzip nor wasm). */
  | 'fetch-failed'
  /** The fetch failed while the browser is offline and nothing usable was cached. */
  | 'offline'
  | 'checksum-mismatch'
  /** `WebAssembly.compile` was blocked or failed (CSP, an extension, memory). */
  | 'compile-blocked'
  | 'aborted';

export class PandocLoadError extends Error {
  readonly code: LoadErrorCode;
  readonly url?: string;
  readonly bytes?: number;
  constructor(code: LoadErrorCode, message: string, detail: { url?: string; bytes?: number; cause?: unknown } = {}) {
    super(message, detail.cause === undefined ? undefined : { cause: detail.cause });
    this.name = 'PandocLoadError';
    this.code = code;
    this.url = detail.url;
    this.bytes = detail.bytes;
  }
}

export interface LoaderEnv {
  fetch: typeof fetch;
  caches: CacheStorage | undefined;
  DecompressionStream: typeof DecompressionStream | undefined;
  subtle: SubtleCrypto | undefined;
  /** False when the `WebAssembly` global does not exist at all. */
  hasWebAssembly: boolean;
  validate: (bytes: BufferSource) => boolean;
  compile: (bytes: BufferSource) => Promise<WebAssembly.Module>;
  baseURI: string;
  isOnline: () => boolean;
  /** `navigator.storage.persist()`: ask the browser not to evict the cached download. Best effort. */
  persist: () => Promise<boolean>;
}

/** The real environment, read lazily so a test can stub globals before the first load. */
export function browserEnv(): LoaderEnv {
  return {
    fetch: (...a) => fetch(...a),
    caches: typeof caches === 'undefined' ? undefined : caches,
    DecompressionStream: typeof DecompressionStream === 'undefined' ? undefined : DecompressionStream,
    subtle: typeof crypto === 'undefined' ? undefined : crypto.subtle,
    hasWebAssembly: typeof WebAssembly !== 'undefined',
    validate: (b) => WebAssembly.validate(b),
    compile: (b) => WebAssembly.compile(b),
    baseURI: typeof document === 'undefined' ? 'http://localhost/' : document.baseURI,
    isOnline: () => typeof navigator === 'undefined' || navigator.onLine !== false,
    persist: async () => (typeof navigator !== 'undefined' && navigator.storage?.persist ? navigator.storage.persist() : false),
  };
}

export type LoadPhase = 'download' | 'cached' | 'verify' | 'compile';

export interface LoadProgress {
  phase: LoadPhase;
  loaded: number;
  /** `Content-Length` of the download, when known. */
  total: number | null;
}

export interface LoadResult {
  module: WebAssembly.Module;
  source: 'resident' | 'cache' | 'network';
  /** Non-fatal conditions (cache unavailable, stale entry replaced). Shown in the status channel. */
  notices: string[];
}

export interface LoadOptions {
  signal?: AbortSignal;
  onProgress?: (p: LoadProgress) => void;
}

export interface LoaderConfig {
  assetPath?: string;
  cacheName?: string;
  idleMs?: number;
  env?: Partial<LoaderEnv>;
  /**
   * What the asset is called in messages (default `pandoc`). The typst loader (host phase H7)
   * reuses this class with `the Typst compiler` and `the Typst default fonts`.
   */
  label?: string;
  /** Whether the asset needs WebAssembly exnref (pandoc does; typst.ts and the fonts do not). Default true. */
  requireExnref?: boolean;
  /**
   * Recognises the asset's decompressed bytes, for a server that adds `Content-Encoding: gzip`
   * to `*.gz` (the browser then hands over the raw bytes). Default: the WebAssembly magic.
   */
  isRaw?: (bytes: Uint8Array) => boolean;
}

const hex = (buf: ArrayBuffer): string => Array.from(new Uint8Array(buf), (b) => b.toString(16).padStart(2, '0')).join('');

const isGzip = (b: Uint8Array) => b.length >= 2 && b[0] === 0x1f && b[1] === 0x8b;
const isWasm = (b: Uint8Array) => b.length >= 4 && b[0] === 0 && b[1] === 0x61 && b[2] === 0x73 && b[3] === 0x6d;

/** Read a stream whole, or cancel it and return null once it passes `cap` bytes. */
async function readCapped(stream: ReadableStream<Uint8Array>, cap: number): Promise<Uint8Array | null> {
  const reader = stream.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.length;
    if (total > cap) {
      await reader.cancel().catch(() => undefined);
      return null;
    }
    chunks.push(value);
  }
  return concat(chunks, total);
}

const concat = (chunks: Uint8Array[], total: number): Uint8Array => {
  const out = new Uint8Array(total);
  let k = 0;
  for (const c of chunks) {
    out.set(c, k);
    k += c.length;
  }
  return out;
};

const abortReason = (signal: AbortSignal): Error =>
  signal.reason instanceof Error ? signal.reason : new PandocLoadError('aborted', 'pandoc.wasm load cancelled');

interface InFlight {
  sha: string;
  promise: Promise<LoadResult>;
  controller: AbortController;
  waiters: number;
  listeners: Set<(p: LoadProgress) => void>;
}

export class PandocLoader {
  private readonly env: LoaderEnv;
  private readonly assetPath: string;
  private readonly cacheName: string;
  private readonly idleMs: number;
  private readonly label: string;
  private readonly requireExnref: boolean;
  private readonly isRaw: (bytes: Uint8Array) => boolean;

  private resident: { sha: string; module: WebAssembly.Module } | undefined;
  private inFlight: InFlight | undefined;
  private busy = 0;
  private rendered = 0;
  private idleTimer: ReturnType<typeof setTimeout> | undefined;
  private readonly dropListeners = new Set<() => void>();

  constructor(config: LoaderConfig = {}) {
    this.env = { ...browserEnv(), ...config.env };
    this.assetPath = config.assetPath ?? DEFAULT_ASSET_PATH;
    this.cacheName = config.cacheName ?? DEFAULT_CACHE_NAME;
    this.idleMs = config.idleMs ?? DEFAULT_IDLE_MS;
    this.label = config.label ?? 'pandoc';
    this.requireExnref = config.requireExnref ?? true;
    this.isRaw = config.isRaw ?? isWasm;
  }

  /**
   * Renders that every runner has finished on the resident module since it was compiled. WebKit's compiled code goes
   * bad after about 47 of them in total, warm or fresh (H10b 5b), so the warm runner reads this to recompile.
   */
  get renders(): number {
    return this.rendered;
  }

  /** A runner calls this when a render on the resident module has finished. */
  countRender(): void {
    this.rendered++;
  }

  get hasResidentModule(): boolean {
    return this.resident !== undefined;
  }

  /** Absolute asset URL: the app uses `base: './'`, so a relative path resolves against the page. */
  get assetUrl(): string {
    return new URL(this.assetPath, this.env.baseURI).href;
  }

  /**
   * Mark the loader busy (a render is active) so the idle timer does not run. Returns
   * the release function; calling it twice is harmless.
   */
  hold(): () => void {
    this.busy++;
    this.clearIdle();
    let released = false;
    return () => {
      if (released) return;
      released = true;
      this.busy--;
      this.armIdle();
    };
  }

  /**
   * Called whenever `dropResident()` runs, so an owner of workers that hold the module (the warm runner)
   * can let them go: dropping the loader's reference frees nothing while a worker holds the module.
   * Returns the unsubscribe function.
   */
  onDrop(listener: () => void): () => void {
    this.dropListeners.add(listener);
    return () => this.dropListeners.delete(listener);
  }

  /** Drop the resident `Module` now; the next load recompiles from the cache. */
  dropResident(): void {
    this.resident = undefined;
    this.rendered = 0;
    this.clearIdle();
    for (const l of [...this.dropListeners]) l();
  }

  /**
   * Resolve the compiled module for `expectedSha` (the SHA-256 of the decompressed wasm).
   * A load already in flight for the same SHA is shared. A waiter whose signal aborts
   * stops waiting; the underlying download is aborted only when no waiter is left, so a
   * superseding click that joined first keeps it alive.
   */
  load(expectedSha: string, options: LoadOptions = {}): Promise<LoadResult> {
    const { signal, onProgress } = options;
    if (signal?.aborted) return Promise.reject(abortReason(signal));
    const sha = expectedSha.toLowerCase();

    if (this.resident?.sha === sha) {
      this.armIdle();
      return Promise.resolve({ module: this.resident.module, source: 'resident', notices: [] });
    }

    let flight = this.inFlight;
    if (!flight || flight.sha !== sha || flight.controller.signal.aborted) {
      flight = this.startLoad(sha);
      this.inFlight = flight;
    }
    const f = flight;
    f.waiters++;
    if (onProgress) f.listeners.add(onProgress);

    return new Promise<LoadResult>((resolve, reject) => {
      let done = false;
      const leave = () => {
        if (done) return;
        done = true;
        if (onProgress) f.listeners.delete(onProgress);
        signal?.removeEventListener('abort', onAbort);
        if (--f.waiters === 0) f.controller.abort();
      };
      const onAbort = () => {
        const reason = abortReason(signal as AbortSignal);
        leave();
        reject(reason);
      };
      signal?.addEventListener('abort', onAbort, { once: true });
      f.promise.then(
        (r) => {
          if (done) return;
          done = true;
          signal?.removeEventListener('abort', onAbort);
          if (onProgress) f.listeners.delete(onProgress);
          f.waiters--;
          resolve(r);
        },
        (e) => {
          if (done) return;
          done = true;
          signal?.removeEventListener('abort', onAbort);
          if (onProgress) f.listeners.delete(onProgress);
          f.waiters--;
          reject(e);
        },
      );
    });
  }

  // ---- internals -----------------------------------------------------------

  private clearIdle() {
    if (this.idleTimer !== undefined) clearTimeout(this.idleTimer);
    this.idleTimer = undefined;
  }

  private armIdle() {
    this.clearIdle();
    if (this.busy > 0 || this.inFlight || !this.resident) return;
    this.idleTimer = setTimeout(() => {
      this.idleTimer = undefined;
      if (this.busy === 0 && !this.inFlight) this.resident = undefined;
    }, this.idleMs);
  }

  private startLoad(sha: string): InFlight {
    const controller = new AbortController();
    const listeners = new Set<(p: LoadProgress) => void>();
    const release = this.hold();
    const emit = (p: LoadProgress) => listeners.forEach((l) => l(p));
    const flight: InFlight = {
      sha,
      controller,
      waiters: 0,
      listeners,
      promise: this.loadModule(sha, controller.signal, emit).then(
        (r) => {
          this.resident = { sha, module: r.module };
          this.rendered = 0;
          return r;
        },
      ),
    };
    const settle = () => {
      if (this.inFlight === flight) this.inFlight = undefined;
      release();
    };
    flight.promise.then(settle, settle);
    return flight;
  }

  private checkFeatures() {
    const { env } = this;
    if (!env.hasWebAssembly)
      throw new PandocLoadError(
        'no-wasm',
        `This browser has WebAssembly turned off (iOS Lockdown Mode or a browser policy can do this), so ${this.label} cannot run. Use ${MIN_BROWSERS} or newer with WebAssembly enabled.`,
      );
    if (this.requireExnref && !env.validate(EXNREF_PROBE))
      throw new PandocLoadError(
        'no-exnref',
        `This browser cannot run ${this.label}: it lacks WebAssembly exception handling (exnref). Use ${MIN_BROWSERS} or newer.`,
      );
    if (!env.DecompressionStream)
      throw new PandocLoadError('no-decompression', `This browser lacks DecompressionStream, which is needed to unpack ${this.label}. Use ${MIN_BROWSERS} or newer.`);
    if (!env.subtle)
      throw new PandocLoadError(
        'no-subtle-crypto',
        `${this.label} cannot be verified here: crypto.subtle is only available in secure contexts (https or localhost). Open the app over https.`,
      );
  }

  private async loadModule(sha: string, signal: AbortSignal, emit: (p: LoadProgress) => void): Promise<LoadResult> {
    this.checkFeatures();
    const url = this.assetUrl;
    const key = `${url}?sha256=${sha}`;
    const notices: string[] = [];

    let cache: Cache | undefined;
    if (!this.env.caches) notices.push(`The Cache API is unavailable here, so ${this.label} is downloaded again on each visit.`);
    else {
      try {
        cache = await this.env.caches.open(this.cacheName);
      } catch (e) {
        notices.push(`The Cache API is unavailable (${describe(e)}), so ${this.label} is not kept between visits.`);
      }
    }
    if (signal.aborted) throw abortReason(signal);

    // 1. A cached copy, verified on every load (this also catches corruption and a stale entry).
    if (cache) {
      let hit: Response | undefined;
      try {
        hit = await cache.match(key);
      } catch (e) {
        notices.push(`The ${this.label} cache could not be read (${describe(e)}).`);
      }
      if (hit) {
        try {
          const stored = new Uint8Array(await hit.arrayBuffer());
          emit({ phase: 'cached', loaded: stored.length, total: stored.length });
          const wasm = await this.unpack(stored, sha, url, emit);
          if (signal.aborted) throw abortReason(signal);
          return { module: await this.compile(wasm, emit), source: 'cache', notices };
        } catch (e) {
          if (!(e instanceof PandocLoadError) || (e.code !== 'checksum-mismatch' && e.code !== 'fetch-failed')) throw e;
          notices.push(`The cached copy of ${this.label} failed verification and was downloaded again.`);
          await cache.delete(key).catch(() => false);
        }
      }
    }

    // 2. The network.
    const stored = await this.download(url, signal, emit);
    const wasm = await this.unpack(stored, sha, url, emit);

    // The SHA check has passed: only verified bytes are ever written. A cancel from here on
    // does not abort the write; waiters just stop waiting for it.
    if (cache) await this.store(cache, key, stored, notices);
    if (signal.aborted) throw abortReason(signal);
    return { module: await this.compile(wasm, emit), source: 'network', notices };
  }

  private async download(url: string, signal: AbortSignal, emit: (p: LoadProgress) => void): Promise<Uint8Array> {
    let res: Response;
    try {
      res = await this.env.fetch(url, { signal });
    } catch (e) {
      if (signal.aborted) throw abortReason(signal);
      if (!this.env.isOnline())
        throw new PandocLoadError(
          'offline',
          `You are offline and ${this.label} is not stored on this device (it has not been downloaded yet, or the browser evicted it). Connect once to download it; it then works offline.`,
          { url, cause: e },
        );
      throw new PandocLoadError('fetch-failed', `Could not download ${this.label} from ${url} (${describe(e)}).`, { url, cause: e });
    }
    if (!res.ok) throw new PandocLoadError('fetch-failed', `Could not download ${this.label} from ${url}: HTTP ${res.status}.`, { url });

    const lengthHeader = res.headers.get('content-length');
    const total = lengthHeader && Number(lengthHeader) > 0 ? Number(lengthHeader) : null;
    const chunks: Uint8Array[] = [];
    let loaded = 0;
    try {
      if (res.body) {
        const reader = res.body.getReader();
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          chunks.push(value);
          loaded += value.length;
          emit({ phase: 'download', loaded, total });
        }
      } else {
        const all = new Uint8Array(await res.arrayBuffer());
        chunks.push(all);
        loaded = all.length;
        emit({ phase: 'download', loaded, total: loaded });
      }
    } catch (e) {
      if (signal.aborted) throw abortReason(signal);
      throw new PandocLoadError('fetch-failed', `The download of ${this.label} from ${url} was interrupted after ${loaded} bytes (${describe(e)}).`, { url, bytes: loaded, cause: e });
    }
    return concat(chunks, loaded);
  }

  /** Sniff gzip vs raw wasm (some servers add `Content-Encoding: gzip` to `*.gz`), then SHA-check the wasm. */
  private async unpack(stored: Uint8Array, sha: string, url: string, emit: (p: LoadProgress) => void): Promise<Uint8Array> {
    emit({ phase: 'verify', loaded: 0, total: null });
    let wasm: Uint8Array;
    if (isGzip(stored)) {
      try {
        const DS = this.env.DecompressionStream as typeof DecompressionStream;
        const stream = new Blob([stored as BlobPart]).stream().pipeThrough(new DS('gzip'));
        const capped = await readCapped(stream, stored.length * MAX_DECOMPRESSION_RATIO);
        if (!capped)
          throw new PandocLoadError(
            'fetch-failed',
            `The ${this.label} download from ${url} (${stored.length} bytes) expands to more than ${MAX_DECOMPRESSION_RATIO} times its size, which the real asset does not; a proxy or server may have replaced it.`,
            { url, bytes: stored.length },
          );
        wasm = capped;
      } catch (e) {
        if (e instanceof PandocLoadError) throw e;
        throw new PandocLoadError('fetch-failed', `The ${this.label} download from ${url} (${stored.length} bytes) is corrupt: it could not be decompressed.`, { url, bytes: stored.length, cause: e });
      }
    } else if (this.isRaw(stored)) wasm = stored;
    else
      throw new PandocLoadError('fetch-failed', `The response from ${url} (${stored.length} bytes) is neither gzip nor the expected file; a proxy or server may have replaced it.`, {
        url,
        bytes: stored.length,
      });

    const actual = hex(await (this.env.subtle as SubtleCrypto).digest('SHA-256', wasm as BufferSource));
    if (actual !== sha)
      throw new PandocLoadError('checksum-mismatch', `${this.label} from ${url} (${stored.length} bytes) failed its checksum; a proxy may have corrupted it. Expected ${sha}, got ${actual}.`, {
        url,
        bytes: stored.length,
      });
    return wasm;
  }

  private async compile(wasm: Uint8Array, emit: (p: LoadProgress) => void): Promise<WebAssembly.Module> {
    emit({ phase: 'compile', loaded: 0, total: null });
    try {
      return await this.env.compile(wasm as BufferSource);
    } catch (e) {
      throw new PandocLoadError('compile-blocked', `The browser refused to compile ${this.label} (${describe(e)}). A content-security policy or an extension may be blocking WebAssembly.`, { cause: e });
    }
  }

  /** Write the verified bytes, then evict entries for other SHAs once the write has settled. */
  private async store(cache: Cache, key: string, stored: Uint8Array, notices: string[]) {
    try {
      await cache.put(key, new Response(stored as BodyInit, { headers: { 'content-type': 'application/octet-stream' } }));
    } catch (e) {
      notices.push(`${this.label} was not cached (${describe(e)}); it will be downloaded again next time.`);
      return;
    }
    try {
      for (const req of await cache.keys()) if (req.url !== key) await cache.delete(req);
    } catch {
      // Eviction is best-effort; an old entry is never used because the key carries the SHA.
    }
    // Offline use depends on the entry surviving; a refusal is normal and silent. Not awaited: Firefox
    // answers persist() with a permission prompt, which never settles under automation (and may sit
    // unanswered for a user), and the verified module must not wait on it.
    void this.env.persist().catch(() => false);
  }
}

function describe(e: unknown): string {
  if (e instanceof Error) return e.name === 'Error' ? e.message : `${e.name}: ${e.message}`;
  return String(e);
}
