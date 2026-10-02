/**
 * Worker lifecycle for pandoc renders (host phase H2; design D2, D8.5, failure taxonomy).
 *
 * One short-lived worker per render. The compiled `Module` is held by the `PandocLoader`
 * and posted into each worker, so terminating a worker (abort, timeout) never discards
 * the compile. At most one render is live: a new `run()` supersedes the previous one.
 *
 * The runner returns a typed outcome rather than throwing for expected failures; every
 * failure class carries one diagnostic (none for a user cancel) and maps to one UI state
 * through `uiStateFor`.
 */
import { prepareForPost } from '@quarto/pandoc-host';
import type { Diagnostic, ExecuteResult, ExecuteSuccess, Fault, HostDiagnostic, Limits, PandocRequest, ShareTree, WorkerRequest, WorkerResponse } from '@quarto/pandoc-host';
import { PandocLoadError, type LoadProgress, type PandocLoader } from './pandocLoader';

/** Wall limit for one render: wasm cannot be interrupted, so a runaway filter is terminated. */
export const DEFAULT_WALL_TIMEOUT_MS = 120_000;

/** The slice of `Worker` the runner uses; tests supply a fake. */
export interface WorkerLike {
  postMessage(message: unknown, transfer?: Transferable[]): void;
  terminate(): void;
  onmessage: ((event: { data: WorkerResponse }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onmessageerror: ((event: unknown) => void) | null;
}

export type RunStage = 'loading' | 'starting' | 'mounting' | 'running';

export type RunFailureKind =
  /** From the host core: `invalid-request`, `pandoc-exit`, `oom`, `crash`, `no-output`. */
  | ExecuteFailureKind
  /** The loader could not provide a module; see `loadError`. */
  | 'load-failed'
  /** `new Worker` was blocked or the worker script failed to load. */
  | 'worker-blocked'
  /** The wall timeout elapsed and the worker was terminated. */
  | 'timeout'
  /** The caller's signal aborted the render. */
  | 'aborted'
  /** A later `run()` replaced this one. */
  | 'superseded';

type ExecuteFailureKind = Exclude<ExecuteResult, ExecuteSuccess>['kind'];

export interface RunFailure {
  ok: false;
  kind: RunFailureKind;
  status: number | null;
  stderr: string;
  stdout: string;
  diagnostics: Diagnostic[];
  /** Set when `kind === 'load-failed'`. */
  loadError?: PandocLoadError;
  /** Non-fatal loader notices (cache unavailable, ...). */
  notices: string[];
}

export type RunSuccess = ExecuteSuccess & { notices: string[] };
export type RunOutcome = RunSuccess | RunFailure;

/** One UI state per failure class (consumed by the H5 download UI). */
export type UiState =
  | 'done'
  | 'cancelled'
  | 'unsupported'
  | 'blocked'
  | 'download-failed'
  | 'offline'
  | 'timeout'
  | 'out-of-memory'
  | 'crashed'
  | 'pandoc-error'
  | 'invalid-request';

export function uiStateFor(o: RunOutcome): UiState {
  if (o.ok) return 'done';
  switch (o.kind) {
    case 'aborted':
    case 'superseded':
      return 'cancelled';
    case 'load-failed':
      switch (o.loadError?.code) {
        case 'no-wasm':
        case 'no-exnref':
        case 'no-decompression':
        case 'no-subtle-crypto':
          return 'unsupported';
        case 'compile-blocked':
          return 'blocked';
        case 'offline':
          return 'offline';
        default:
          return 'download-failed';
      }
    case 'worker-blocked':
      return 'blocked';
    case 'timeout':
      return 'timeout';
    case 'oom':
      return 'out-of-memory';
    case 'crash':
    case 'no-output':
      return 'crashed';
    case 'pandoc-exit':
      return 'pandoc-error';
    case 'invalid-request':
      return 'invalid-request';
  }
}

export interface RunOptions {
  /** The user's cancel. Aborts the render; a wasm load shared with a newer click continues. */
  signal?: AbortSignal;
  fault?: Fault;
  onStage?: (stage: RunStage) => void;
  onLoadProgress?: (p: LoadProgress) => void;
  /** Overrides the runner's wall timeout (the test hook shortens it for the hang case). */
  wallTimeoutMs?: number;
}

export interface RunnerConfig {
  loader: PandocLoader;
  createWorker: () => WorkerLike;
  wallTimeoutMs?: number;
  limits?: Limits;
}

class Superseded extends Error {
  constructor() {
    super('superseded by a newer render');
    this.name = 'Superseded';
  }
}

const host = (code: HostDiagnostic['code'], message: string): HostDiagnostic => ({ origin: 'host', kind: 'error', code, message });

const failure = (kind: RunFailureKind, diagnostics: Diagnostic[], notices: string[], extra: Partial<RunFailure> = {}): RunFailure => ({
  ok: false,
  kind,
  status: null,
  stderr: '',
  stdout: '',
  diagnostics,
  notices,
  ...extra,
});

const LOAD_DIAGNOSTIC: Record<PandocLoadError['code'], HostDiagnostic['code']> = {
  'no-wasm': 'wasm-unsupported',
  'no-exnref': 'wasm-unsupported',
  'no-decompression': 'wasm-unsupported',
  'no-subtle-crypto': 'wasm-unsupported',
  'fetch-failed': 'download-failed',
  offline: 'offline',
  'checksum-mismatch': 'checksum-mismatch',
  'compile-blocked': 'compile-blocked',
  aborted: 'download-failed',
};

export class PandocRunner {
  private readonly loader: PandocLoader;
  private readonly createWorker: () => WorkerLike;
  private readonly wallTimeoutMs: number;
  private readonly limits?: Limits;
  private current: { abort: (reason: Error) => void } | undefined;
  private nextId = 1;

  constructor(config: RunnerConfig) {
    this.loader = config.loader;
    this.createWorker = config.createWorker;
    this.wallTimeoutMs = config.wallTimeoutMs ?? DEFAULT_WALL_TIMEOUT_MS;
    this.limits = config.limits;
  }

  /**
   * Run one request. Takes ownership of the request's byte buffers (they are transferred
   * to the worker); the share tree is structured-cloned, since the caller reuses it
   * across renders.
   */
  async run(request: PandocRequest, shareTree: ShareTree, options: RunOptions = {}): Promise<RunOutcome> {
    const own = new AbortController();
    const abort = (reason: Error) => {
      if (!own.signal.aborted) own.abort(reason);
    };
    const onUserAbort = () => abort(options.signal?.reason instanceof Error ? options.signal.reason : new Error('cancelled'));
    if (options.signal?.aborted) return failure('aborted', [], []);
    options.signal?.addEventListener('abort', onUserAbort, { once: true });

    // Join the load before superseding the previous render, so a shared in-flight load keeps its waiter.
    const loading = this.loader.load(request.expected_pandoc_wasm_sha256, { signal: own.signal, onProgress: options.onLoadProgress });
    loading.catch(() => undefined);
    const previous = this.current;
    const me = { abort };
    this.current = me;
    previous?.abort(new Superseded());

    const releaseLoader = this.loader.hold();
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
      let loaded;
      try {
        loaded = await loading;
      } catch (e) {
        return this.interrupted(own.signal, [], e);
      }
      const notices = loaded.notices;
      if (own.signal.aborted) return this.interrupted(own.signal, notices);

      options.onStage?.('starting');
      const outcome = await new Promise<RunOutcome>((resolve) => {
        let settled = false;
        let timer: ReturnType<typeof setTimeout> | undefined;
        const settle = (o: RunOutcome) => {
          if (settled) return;
          settled = true;
          if (timer !== undefined) clearTimeout(timer);
          own.signal.removeEventListener('abort', onAbort);
          terminate();
          resolve(o);
        };
        const onAbort = () => settle(this.interrupted(own.signal, notices));
        own.signal.addEventListener('abort', onAbort, { once: true });

        try {
          worker = this.createWorker();
        } catch (e) {
          settle(failure('worker-blocked', [host('worker-blocked', `The pandoc worker could not be started (${String(e)}). An extension or content-security policy may be blocking workers.`)], notices));
          return;
        }
        const id = this.nextId++;
        let ready = false;
        worker.onmessage = (ev) => {
          const msg = ev.data;
          if (settled) return;
          if (msg.type === 'ready') {
            ready = true;
            const prepared = prepareForPost(request);
            const run: WorkerRequest = { type: 'run', id, request: prepared.value, shareTree, fault: options.fault };
            const timeoutMs = options.wallTimeoutMs ?? this.wallTimeoutMs;
            timer = setTimeout(() => {
              settle(
                failure('timeout', [host('pandoc-timeout', `pandoc did not finish within ${Math.round(timeoutMs / 1000)} s and was stopped. A filter may be stuck in a loop.`)], notices),
              );
            }, timeoutMs);
            worker?.postMessage(run, prepared.transfer);
          } else if (msg.type === 'progress') {
            if (msg.id === id) options.onStage?.(msg.stage);
          } else if (msg.type === 'result' && msg.id === id) {
            const r = msg.result;
            settle(r.ok ? { ...r, notices } : failure(r.kind, r.diagnostics, notices, { status: r.status, stderr: r.stderr, stdout: r.stdout }));
          }
        };
        worker.onerror = (ev) => {
          const detail = ev && typeof ev === 'object' && 'message' in ev ? String((ev as { message: unknown }).message) : 'unknown error';
          if (!ready)
            settle(failure('worker-blocked', [host('worker-blocked', `The pandoc worker failed to start (${detail}). An extension or content-security policy may be blocking it.`)], notices));
          else settle(failure('crash', [host('pandoc-crash', `The pandoc worker crashed (${detail}).`)], notices));
        };
        worker.onmessageerror = () => settle(failure('crash', [host('pandoc-crash', 'The pandoc worker sent a message that could not be read.')], notices));
        const init: WorkerRequest = { type: 'init', module: loaded.module, limits: this.limits };
        try {
          worker.postMessage(init);
        } catch (e) {
          settle(failure('worker-blocked', [host('worker-blocked', `The pandoc worker could not be started (${String(e)}).`)], notices));
        }
      });
      return outcome;
    } finally {
      terminate();
      options.signal?.removeEventListener('abort', onUserAbort);
      if (this.current === me) this.current = undefined;
      releaseLoader();
    }
  }

  /** Classify a stop that came from an abort signal or a load rejection. */
  private interrupted(signal: AbortSignal, notices: string[], loadError?: unknown): RunFailure {
    if (signal.aborted) return failure(signal.reason instanceof Superseded ? 'superseded' : 'aborted', [], notices);
    if (loadError instanceof PandocLoadError) {
      if (loadError.code === 'aborted') return failure('aborted', [], notices);
      return failure('load-failed', [host(LOAD_DIAGNOSTIC[loadError.code], loadError.message)], notices, { loadError });
    }
    const message = loadError instanceof Error ? loadError.message : String(loadError);
    return failure('load-failed', [host('download-failed', message)], notices, { loadError: new PandocLoadError('fetch-failed', message, { cause: loadError }) });
  }
}
