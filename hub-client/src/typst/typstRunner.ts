/**
 * Worker lifecycle for typst compiles (host phase H7; design D2, D8.5), the sibling of
 * `PandocRunner`: one short-lived worker per compile, the compiled typst.ts `Module` (and the
 * default fonts) held on the main thread and posted into each worker, so terminating a worker
 * (abort, timeout) never discards the compile. At most one job is live: a new `run()` or
 * `listFonts()` supersedes the previous one.
 *
 * The runner returns a typed outcome rather than throwing for expected failures. Every failure
 * class carries at least one diagnostic (none for a user cancel).
 */
import type { CompileFailureKind, CompileInput, CompileSuccess, Diagnostic, HostDiagnostic, Limits, TarballCache, TypstFile, WorkerRequest, WorkerResponse } from '@quarto/typst-host';
import { PandocLoadError, type LoadProgress, type PandocLoader } from '../pandoc/pandocLoader';
import { LOAD_DIAGNOSTIC, uiStateForLoadError, type UiState } from '../pandoc/pandocRunner';
import { TYPST_WASM_SHA256, type TypstFontsLoader } from './typstAssets';

/** Wall limit for one compile (including worker start): a runaway document is terminated. */
export const DEFAULT_WALL_TIMEOUT_MS = 120_000;

/** The slice of `Worker` the runner uses; tests supply a fake. */
export interface WorkerLike {
  postMessage(message: unknown, transfer?: Transferable[]): void;
  terminate(): void;
  onmessage: ((event: { data: WorkerResponse }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onmessageerror: ((event: unknown) => void) | null;
}

export type TypstRunStage = 'loading' | 'starting' | 'compiling';

export interface TypstJob {
  input: CompileInput;
  /** Fonts beyond the defaults: the vendored Font Awesome fonts and brand/project fonts. */
  fonts?: Uint8Array[];
  /** Vendored packages, paths relative to the cache root (`preview/<name>/<version>/...`). */
  vendoredPackages?: TypstFile[];
}

export type TypstRunFailureKind =
  /** From the compile: `invalid-input`, `typst-error`, `package-fetch`, `oom`, `crash`. */
  | CompileFailureKind
  /** The loader could not provide the wasm or the fonts; see `loadError`. */
  | 'load-failed'
  /** `new Worker` was blocked or the worker script failed to load. */
  | 'worker-blocked'
  | 'timeout'
  | 'aborted'
  | 'superseded';

export interface TypstRunFailure {
  ok: false;
  kind: TypstRunFailureKind;
  diagnostics: Diagnostic[];
  loadError?: PandocLoadError;
  notices: string[];
}

export type TypstRunSuccess = CompileSuccess & { notices: string[]; fontFamilies: string[] };
export type TypstRunOutcome = TypstRunSuccess | TypstRunFailure;

export interface FontListOutcome {
  ok: true;
  /** Family names of every loaded font, for `typst-available-fonts`. */
  families: string[];
  notices: string[];
}

/**
 * One UI state per failure class. The typst-specific classes (`typst-error`, `package-error`)
 * have no copy yet: the download UI gains them with the PDF chain (host phase H8).
 */
export type TypstUiState = UiState | 'typst-error' | 'package-error';

export function typstUiStateFor(o: TypstRunOutcome | (TypstRunFailure | FontListOutcome)): TypstUiState {
  if (o.ok) return 'done';
  switch (o.kind) {
    case 'aborted':
    case 'superseded':
      return 'cancelled';
    case 'load-failed':
      return uiStateForLoadError(o.loadError);
    case 'worker-blocked':
      return 'blocked';
    case 'timeout':
      return 'timeout';
    case 'oom':
      return 'out-of-memory';
    case 'crash':
      return 'crashed';
    case 'typst-error':
      return 'typst-error';
    case 'package-fetch':
      return 'package-error';
    case 'invalid-input':
      return 'invalid-request';
  }
}

export interface TypstRunOptions {
  signal?: AbortSignal;
  onStage?: (stage: TypstRunStage) => void;
  onLoadProgress?: (p: LoadProgress) => void;
  wallTimeoutMs?: number;
}

export interface TypstRunnerConfig {
  loader: PandocLoader;
  fonts: TypstFontsLoader;
  createWorker: () => WorkerLike;
  wallTimeoutMs?: number;
  limits?: Partial<Limits>;
  /** Where package tarballs are kept between compiles (the Cache API); the worker asks for it by message. */
  cache?: TarballCache;
}

class Superseded extends Error {
  constructor() {
    super('superseded by a newer compile');
    this.name = 'Superseded';
  }
}

const host = (code: HostDiagnostic['code'], message: string): HostDiagnostic => ({ origin: 'host', kind: 'error', code, message, stage: 'typst' });

const failure = (kind: TypstRunFailureKind, diagnostics: Diagnostic[], notices: string[], extra: Partial<TypstRunFailure> = {}): TypstRunFailure => ({
  ok: false,
  kind,
  diagnostics,
  notices,
  ...extra,
});

/**
 * Collects the buffers to transfer to a worker: a view that does not span its buffer is
 * copied (structured clone would otherwise copy the whole backing buffer), and each distinct
 * buffer is listed once (a duplicate transfer entry throws).
 */
function transfers() {
  const seen = new Set<ArrayBuffer>();
  const list: ArrayBuffer[] = [];
  const own = (b: Uint8Array): Uint8Array => {
    const buf = b.buffer;
    const v = !(buf instanceof ArrayBuffer) || b.byteOffset !== 0 || b.byteLength !== buf.byteLength ? b.slice() : b;
    if (!seen.has(v.buffer as ArrayBuffer)) {
      seen.add(v.buffer as ArrayBuffer);
      list.push(v.buffer as ArrayBuffer);
    }
    return v;
  };
  return { list, own };
}

export class TypstRunner {
  private readonly loader: PandocLoader;
  private readonly fonts: TypstFontsLoader;
  private readonly createWorker: () => WorkerLike;
  private readonly wallTimeoutMs: number;
  private readonly limits?: Partial<Limits>;
  private readonly cache?: TarballCache;
  private current: { abort: (reason: Error) => void } | undefined;
  private nextId = 1;

  constructor(config: TypstRunnerConfig) {
    this.loader = config.loader;
    this.fonts = config.fonts;
    this.createWorker = config.createWorker;
    this.wallTimeoutMs = config.wallTimeoutMs ?? DEFAULT_WALL_TIMEOUT_MS;
    this.limits = config.limits;
    this.cache = config.cache;
  }

  /**
   * Compile one job. Takes ownership of the job's byte buffers (they are transferred to the
   * worker).
   */
  run(job: TypstJob, options: TypstRunOptions = {}): Promise<TypstRunOutcome> {
    return this.execute(job, options, 'run') as Promise<TypstRunOutcome>;
  }

  /**
   * The family names of the default fonts plus `fonts`, for `typst-available-fonts`: the PDF
   * chain needs them before pandoc runs. Starts a worker, initialises it and discards it.
   */
  listFonts(fonts: Uint8Array[] = [], options: TypstRunOptions = {}): Promise<FontListOutcome | TypstRunFailure> {
    return this.execute({ input: { main: '', files: [] }, fonts }, options, 'fonts') as Promise<FontListOutcome | TypstRunFailure>;
  }

  private async execute(job: TypstJob, options: TypstRunOptions, mode: 'run' | 'fonts'): Promise<TypstRunOutcome | FontListOutcome | TypstRunFailure> {
    const own = new AbortController();
    const abort = (reason: Error) => {
      if (!own.signal.aborted) own.abort(reason);
    };
    const onUserAbort = () => abort(options.signal?.reason instanceof Error ? options.signal.reason : new Error('cancelled'));
    if (options.signal?.aborted) return failure('aborted', [], []);
    options.signal?.addEventListener('abort', onUserAbort, { once: true });

    const loading = Promise.all([
      this.loader.load(TYPST_WASM_SHA256, { signal: own.signal, onProgress: options.onLoadProgress }),
      this.fonts.load({ signal: own.signal }),
    ]);
    loading.catch(() => undefined);
    const previous = this.current;
    const me = { abort };
    this.current = me;
    previous?.abort(new Superseded());

    const releaseLoader = this.loader.hold();
    const releaseFonts = this.fonts.hold();
    options.onStage?.('loading');
    let worker: WorkerLike | undefined;
    let terminated = false;
    const terminate = () => {
      if (worker && !terminated) {
        terminated = true;
        worker.onmessage = worker.onerror = worker.onmessageerror = null;
        worker.terminate();
      }
    };
    try {
      let module: WebAssembly.Module;
      let defaultFonts: Uint8Array[];
      let notices: string[];
      try {
        const [wasm, fonts] = await loading;
        module = wasm.module;
        defaultFonts = fonts.fonts;
        notices = [...wasm.notices, ...fonts.notices];
      } catch (e) {
        return this.interrupted(own.signal, [], e);
      }
      if (own.signal.aborted) return this.interrupted(own.signal, notices);

      options.onStage?.('starting');
      return await new Promise<TypstRunOutcome | FontListOutcome | TypstRunFailure>((resolve) => {
        let settled = false;
        const timeoutMs = options.wallTimeoutMs ?? this.wallTimeoutMs;
        // Started before the abort check below, so `settle` always has a timer to clear.
        const timer = setTimeout(() => {
          settle(failure('timeout', [host('typst-timeout', `The Typst compiler did not finish within ${Math.round(timeoutMs / 1000)} s and was stopped.`)], notices));
        }, timeoutMs);
        const settle = (o: TypstRunOutcome | FontListOutcome | TypstRunFailure) => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          own.signal.removeEventListener('abort', onAbort);
          terminate();
          resolve(o);
        };
        const onAbort = () => settle(this.interrupted(own.signal, notices));
        own.signal.addEventListener('abort', onAbort, { once: true });
        // An abort raised by the 'starting' stage callback fired before the listener existed.
        if (own.signal.aborted) {
          onAbort();
          return;
        }

        try {
          worker = this.createWorker();
        } catch (e) {
          settle(failure('worker-blocked', [host('worker-blocked', `The Typst worker could not be started (${String(e)}). An extension or content-security policy may be blocking workers.`)], notices));
          return;
        }
        const id = this.nextId++;
        let ready = false;
        let fontFamilies: string[] = [];
        worker.onmessage = (ev) => {
          const msg = ev.data;
          if (settled) return;
          if (msg.type === 'ready') {
            ready = true;
            if (mode === 'fonts') {
              settle({ ok: true, families: msg.fontFamilies, notices });
              return;
            }
            fontFamilies = msg.fontFamilies;
            const t = transfers();
            const input: CompileInput = { ...job.input, files: job.input.files.map((f) => ({ path: f.path, bytes: t.own(f.bytes) })) };
            const run: WorkerRequest = { type: 'run', id, input };
            worker?.postMessage(run, t.list);
          } else if (msg.type === 'cache-get') {
            const w = worker;
            void (this.cache?.get(msg.url) ?? Promise.resolve(undefined))
              .catch(() => undefined)
              .then((bytes) => {
                if (settled || !w) return;
                w.postMessage({ type: 'cache-reply', reqId: msg.reqId, bytes } satisfies WorkerRequest, bytes ? [bytes.buffer as ArrayBuffer] : []);
              });
          } else if (msg.type === 'cache-put') {
            // Best-effort and not tied to this compile: it may finish after the worker is gone.
            void this.cache?.put(msg.url, msg.bytes).catch(() => undefined);
          } else if (msg.type === 'init-failed') {
            settle(failure('crash', [host('typst-crash', `The Typst compiler could not start: ${msg.message}`)], notices));
          } else if (msg.type === 'progress') {
            if (msg.id === id) options.onStage?.('compiling');
          } else if (msg.type === 'result' && msg.id === id) {
            const r = msg.result;
            settle(r.ok ? { ...r, notices, fontFamilies } : failure(r.kind, r.diagnostics, notices));
          }
        };
        worker.onerror = (ev) => {
          const detail = ev && typeof ev === 'object' && 'message' in ev ? String((ev as { message: unknown }).message) : 'unknown error';
          if (!ready) settle(failure('worker-blocked', [host('worker-blocked', `The Typst worker failed to start (${detail}). An extension or content-security policy may be blocking it.`)], notices));
          else settle(failure('crash', [host('typst-crash', `The Typst worker crashed (${detail}).`)], notices));
        };
        worker.onmessageerror = () => settle(failure('crash', [host('typst-crash', 'The Typst worker sent a message that could not be read.')], notices));

        const all = [...defaultFonts, ...(job.fonts ?? [])];
        const t = transfers();
        const init: WorkerRequest = {
          type: 'init',
          init: {
            module,
            // The defaults stay with the loader (they are cloned); the job's own buffers are transferred.
            fonts: all.map((f, i) => (i < defaultFonts.length ? f : t.own(f))),
            vendoredPackages: job.vendoredPackages?.map((f) => ({ path: f.path, bytes: t.own(f.bytes) })),
            limits: this.limits,
          },
        };
        try {
          worker.postMessage(init, t.list);
        } catch (e) {
          settle(failure('worker-blocked', [host('worker-blocked', `The Typst worker could not be started (${String(e)}).`)], notices));
        }
      });
    } finally {
      terminate();
      options.signal?.removeEventListener('abort', onUserAbort);
      if (this.current === me) this.current = undefined;
      releaseLoader();
      releaseFonts();
    }
  }

  private interrupted(signal: AbortSignal, notices: string[], loadError?: unknown): TypstRunFailure {
    if (signal.aborted) return failure(signal.reason instanceof Superseded ? 'superseded' : 'aborted', [], notices);
    if (loadError instanceof PandocLoadError) {
      if (loadError.code === 'aborted') return failure('aborted', [], notices);
      return failure('load-failed', [host(LOAD_DIAGNOSTIC[loadError.code] as HostDiagnostic['code'], loadError.message)], notices, { loadError });
    }
    const message = loadError instanceof Error ? loadError.message : String(loadError);
    return failure('load-failed', [host('download-failed', message)], notices, { loadError: new PandocLoadError('fetch-failed', message, { cause: loadError }) });
  }
}
