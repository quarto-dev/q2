/**
 * The warm runner (H10b; design D2(b)): a pool of at most two persistent pandoc workers, each
 * holding a `WarmSession` (H10a), sharing the one `PandocLoader` with the fresh runner.
 *
 * Dispatch: a new request goes to an idle worker, otherwise it replaces the single pending
 * request (latest wins; the replaced request resolves `superseded`) and starts when a worker
 * frees. A render that a newer request has superseded keeps running; only the grace rule, the
 * wall timeout and a reboot terminate a worker (a signal never does: it detaches).
 */
import { ARGV_ALLOWLIST_VERSION, prepareForPost } from '@quarto/pandoc-host';
import type { ExecuteResult, Limits, PandocRequest, ShareTree, WorkerRequest } from '@quarto/pandoc-host';
import { DEFAULT_IDLE_MS, type LoadProgress, type PandocLoader } from './pandocLoader';
import { DEFAULT_WALL_TIMEOUT_MS, failure, host, loadFailure, type RunFailure, type RunOptions, type RunOutcome, type RunnerConfig, type WorkerLike } from './pandocRunner';

/** A render that took less than this (runner clock) leaves its request as the replay request that warms new workers. */
export const WARMUP_MAX_MS = 500;

/** The pool never holds more workers than this. */
export const MAX_WORKERS = 2;

/** No grace period exceeds this: a runaway filter holds a worker for at most this long once something newer exists. */
export const GRACE_CAP_MS = 16_000;

/** After the last `release()` the workers linger this long (a React remount re-`acquire()`s inside it). */
export const LINGER_MS = 30_000;

export interface WarmRunnerConfig extends RunnerConfig {
  /** Idle period after which the pool is dropped (default `DEFAULT_IDLE_MS`). */
  idleMs?: number;
  /** See `LINGER_MS`. */
  lingerMs?: number;
  /**
   * Recompile the module after this many renders on it (`loader.renders`: every runner, warm-ups included; default: never). WebKit 26.4 stops finding
   * files from about the 47th `convert` on a compiled module, in every worker that uses it, until the module is compiled
   * again (H10b 5b), so `pandocService` sets this in WebKit.
   */
  recycleAfter?: number;
}

/** The doubling term's exponent stops here, so that 1000 * 2^k reaches `GRACE_CAP_MS`. */
const MAX_K = 4;

type WorkerState = 'starting' | 'idle' | 'busy' | 'warming';

interface Job {
  seq: number;
  request: PandocRequest;
  shareTree: ShareTree;
  options: RunOptions;
  key: string;
  sha: string;
  docKey: string | undefined;
  resolve: (o: RunOutcome) => void;
  /** The promise is resolved (the caller no longer waits). A detached run stays on its worker until it finishes. */
  settled: boolean;
  worker?: PoolWorker;
  runId?: number;
  dispatchedAt?: number;
  /** A pre-transfer copy of the request's byte buffers, until the render completes. */
  copy?: PandocRequest;
  removeAbort?: () => void;
  /** The wall limit for this render, from dispatch. */
  wallMs: number;
  wallTimer?: ReturnType<typeof setTimeout>;
  /** Set when a newer request arrived while this run was in flight. */
  supersededAt?: number;
  graceTimer?: ReturnType<typeof setTimeout>;
}

interface PoolWorker {
  worker: WorkerLike;
  /** The generation key this worker was built with. */
  key: string;
  state: WorkerState;
  job?: Job;
  nextRunId: number;
  warmId?: number;
  releaseHold: () => void;
  ready: boolean;
  /** Bounds a warm-up run, which has no job. */
  warmTimer?: ReturnType<typeof setTimeout>;
}

interface Replay {
  key: string;
  request: PandocRequest;
  shareTree: ShareTree;
}

const stable = (v: unknown): string =>
  v && typeof v === 'object' && !Array.isArray(v)
    ? `{${Object.keys(v)
        .sort()
        .map((k) => `${JSON.stringify(k)}:${stable((v as Record<string, unknown>)[k])}`)
        .join(',')}}`
    : JSON.stringify(v) ?? 'null';

const median = (xs: number[]): number => {
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};

const cloneBytes = (r: PandocRequest): PandocRequest => ({
  ...r,
  files: r.files.map((f) => ({ path: f.path, bytes: f.bytes.slice() })),
  resource_refs: r.resource_refs.map((f) => ({ path: f.path, bytes: f.bytes.slice() })),
});

export class WarmPandocRunner {
  private readonly loader: PandocLoader;
  private readonly createWorker: () => WorkerLike;
  private readonly limits?: Limits;
  private readonly wallTimeoutMs: number;
  private readonly idleMs: number;
  private readonly lingerMs: number;
  private readonly now: () => number;
  private readonly recycleAfter: number;

  private workers: PoolWorker[] = [];
  private pending: Job | undefined;
  private latest: { key: string; sha: string } | undefined;
  private module: { sha: string; module: WebAssembly.Module } | undefined;
  private loading: { sha: string; controller: AbortController } | undefined;
  private notices: string[] = [];
  private replay: Replay | undefined;
  /** How many workers the pool wants: 1, and 2 from the first overlap or the first completed render. */
  private wanted = 1;
  /** A worker failed before it was ready: do not respawn on our own until the next request. */
  private startFailed = false;
  private nextSeq = 1;
  /** Consecutive grace terminations since the last completed render (capped at `MAX_K`). */
  private k = 0;
  /** Times of the last five completed renders of `histKey`, dispatch to result on the runner clock. */
  private history: number[] = [];
  private histKey: { docKey: string | undefined } | undefined;
  /** Workers terminated by the grace rule so far (tests and the harness read it). */
  graceTerminations = 0;
  /** Workers created so far, replacements included (tests and the harness read it). */
  created = 0;

  /** Times the module was recompiled by `recycleAfter` (tests and the harness read it). */
  recycles = 0;

  private idleTimer: ReturnType<typeof setTimeout> | undefined;
  private lingerTimer: ReturnType<typeof setTimeout> | undefined;
  private holders = 0;

  constructor(config: WarmRunnerConfig) {
    this.loader = config.loader;
    this.createWorker = config.createWorker;
    this.limits = config.limits;
    this.wallTimeoutMs = config.wallTimeoutMs ?? DEFAULT_WALL_TIMEOUT_MS;
    this.now = config.now ?? (() => performance.now());
    this.idleMs = config.idleMs ?? DEFAULT_IDLE_MS;
    this.lingerMs = config.lingerMs ?? LINGER_MS;
    this.recycleAfter = config.recycleAfter ?? Infinity;
    // Dropping the loader's reference frees nothing while our workers hold the module: let them go too.
    this.loader.onDrop(() => this.shutdown('aborted', false));
  }

  /** The pane takes the pool at mount (a refcount: StrictMode's mount, cleanup, mount is two acquires and one release). */
  acquire(): void {
    this.holders++;
    clearTimeout(this.lingerTimer);
    this.lingerTimer = undefined;
  }

  /** At zero the workers linger `lingerMs`, then go whether busy or idle (a detached run has no consumer). */
  release(): void {
    if (this.holders === 0) return;
    if (--this.holders > 0) return;
    clearTimeout(this.lingerTimer);
    this.lingerTimer = setTimeout(() => {
      this.lingerTimer = undefined;
      this.shutdown('aborted', true);
    }, this.lingerMs);
  }

  /** Workers alive now (any state). */
  get workerCount(): number {
    return this.workers.length;
  }

  /**
   * Run one request. Takes ownership of the request's byte buffers (they are transferred to
   * the worker); a pre-transfer copy is kept until the render completes, to warm new workers.
   * An abort signal detaches: it never terminates a worker.
   */
  run(request: PandocRequest, shareTree: ShareTree, options: RunOptions = {}): Promise<RunOutcome> {
    if (options.signal?.aborted) return Promise.resolve(failure('aborted', [], []));
    return new Promise<RunOutcome>((resolve) => {
      const job: Job = {
        seq: this.nextSeq++,
        request,
        shareTree,
        options,
        key: this.generationKey(request),
        sha: request.expected_pandoc_wasm_sha256.toLowerCase(),
        docKey: options.docKey,
        resolve,
        settled: false,
        wallMs: options.wallTimeoutMs ?? this.wallTimeoutMs,
      };
      const onAbort = () => this.detach(job);
      options.signal?.addEventListener('abort', onAbort, { once: true });
      job.removeAbort = () => options.signal?.removeEventListener('abort', onAbort);
      this.arrive(job);
    });
  }

  // ---- arrival, abort -------------------------------------------------------

  private generationKey(r: PandocRequest): string {
    return [r.expected_pandoc_wasm_sha256.toLowerCase(), stable(this.limits ?? null), r.share_tree_version, ARGV_ALLOWLIST_VERSION].join('|');
  }

  private arrive(job: Job): void {
    this.startFailed = false;
    this.latest = { key: job.key, sha: job.sha };
    if (this.histKey && this.histKey.docKey !== job.docKey) this.resetGrace();
    this.histKey = { docKey: job.docKey };
    for (const w of this.workers) if (w.state === 'busy' && w.job && w.job.supersededAt === undefined) this.supersede(w.job);
    const replaced = this.pending;
    this.pending = job;
    if (replaced) this.settle(replaced, failure('superseded', [], this.takeNotices()));
    if (this.workers.length > 0 && !this.workers.some((w) => w.state === 'idle' || w.state === 'starting')) this.wanted = MAX_WORKERS;
    job.options.onStage?.('loading');
    this.pump();
  }


  private detach(job: Job): void {
    if (job.settled) return;
    if (this.pending === job) this.pending = undefined;
    this.settle(job, failure('aborted', [], []));
  }

  private settle(job: Job, outcome: RunOutcome): void {
    if (job.settled) return;
    job.settled = true;
    job.removeAbort?.();
    job.resolve(outcome);
  }

  private takeNotices(): string[] {
    const n = this.notices;
    this.notices = [];
    return n;
  }

  // ---- the load -------------------------------------------------------------

  private ensureLoad(sha: string): void {
    if (this.loading?.sha === sha) return;
    this.loading?.controller.abort();
    const entry = { sha, controller: new AbortController() };
    this.loading = entry;
    this.loader
      .load(sha, { signal: entry.controller.signal, onProgress: (p) => this.forwardProgress(p) })
      .then(
        (r) => {
          if (this.loading !== entry) return;
          this.loading = undefined;
          this.module = { sha, module: r.module };
          this.notices.push(...r.notices);
          this.pump();
        },
        (e) => {
          if (this.loading !== entry) return;
          this.loading = undefined;
          const j = this.pending;
          if (!j) return;
          this.pending = undefined;
          this.settle(j, loadFailure(e, this.takeNotices()));
        },
      );
  }

  /** Load progress goes to the newest request: the pending one, else the newest running one. */
  private forwardProgress(p: LoadProgress): void {
    const target = this.pending ?? this.workers.map((w) => w.job).filter((j): j is Job => !!j && !j.settled).sort((a, b) => b.seq - a.seq)[0];
    target?.options.onLoadProgress?.(p);
  }

  // ---- the pool -------------------------------------------------------------

  private pump(): void {
    this.pumpOnce();
    this.touchIdle();
  }

  private pumpOnce(): void {
    if (!this.latest) return;
    const { key, sha } = this.latest;
    if (this.workers.some((w) => w.key !== key)) this.reboot(key);
    if (this.module?.sha !== sha) {
      this.ensureLoad(sha);
      return;
    }
    if (this.loader.renders >= this.recycleAfter && !this.workers.some((w) => w.state === 'busy' || w.state === 'warming')) {
      this.recycle();
      return;
    }
    const job = this.pending;
    if (job) {
      const w = this.workers.find((x) => x.state === 'idle' && x.key === job.key);
      if (w) {
        this.pending = undefined;
        this.dispatch(job, w);
      }
    }
    while (!this.startFailed && this.workers.length < this.wanted) {
      if (!this.spawn(key)) break;
    }
  }

  /**
   * The runner-wide idle timer: reset at every state change, never armed while a render is in flight
   * (the loader's own timer cannot arm while a worker holds it).
   */
  private touchIdle(): void {
    clearTimeout(this.idleTimer);
    this.idleTimer = undefined;
    if (this.workers.length === 0 || this.pending || this.workers.some((w) => w.state === 'busy' || w.state === 'warming')) return;
    this.idleTimer = setTimeout(() => {
      this.idleTimer = undefined;
      this.shutdown('aborted', false);
    }, this.idleMs);
  }

  /**
   * Terminate every worker, release their holds, and forget the module and the replay request. Runs still on
   * a worker resolve `failure(kind)`; the pending request is kept (it reloads) unless `abortPending`.
   */
  private shutdown(kind: 'aborted', abortPending: boolean): void {
    clearTimeout(this.idleTimer);
    this.idleTimer = undefined;
    for (const w of [...this.workers]) {
      const job = w.job;
      this.terminate(w);
      if (job) this.settle(job, failure(kind, [], this.takeNotices()));
    }
    this.replay = undefined;
    this.module = undefined;
    this.loading?.controller.abort();
    this.loading = undefined;
    this.wanted = 1;
    this.startFailed = false;
    this.resetGrace();
    if (abortPending && this.pending) {
      const j = this.pending;
      this.pending = undefined;
      this.settle(j, failure(kind, [], []));
    }
    if (this.pending) this.pump();
  }

  /**
   * Drop the compiled module and every worker (all idle), keep the replay request and the pending one: the pending
   * request reloads, which compiles again from the cache, and the new workers are warmed from the replay.
   */
  private recycle(): void {
    const replay = this.replay;
    this.recycles++;
    this.loader.dropResident();
    this.replay = replay;
  }

  /** The generation changed: every worker of the old generation goes at once, whatever it is doing. */
  private reboot(key: string): void {
    for (const w of [...this.workers]) {
      if (w.key === key) continue;
      const job = w.job;
      this.terminate(w);
      if (job) this.settle(job, failure('superseded', [], this.takeNotices()));
    }
    if (this.replay && this.replay.key !== key) this.replay = undefined;
    this.wanted = 1;
    this.resetGrace();
  }


  private spawn(key: string): boolean {
    const mod = this.module;
    if (!mod) return false;
    let worker: WorkerLike;
    try {
      worker = this.createWorker();
    } catch (e) {
      this.startFailed = true;
      this.blocked(`The pandoc worker could not be started (${String(e)}). An extension or content-security policy may be blocking workers.`);
      return false;
    }
    const w: PoolWorker = { worker, key, state: 'starting', nextRunId: 1, releaseHold: this.loader.hold(), ready: false };
    this.workers.push(w);
    this.created++;
    worker.onmessage = (ev) => this.onMessage(w, ev.data);
    worker.onerror = (ev) => {
      const detail = ev && typeof ev === 'object' && 'message' in ev ? String((ev as { message: unknown }).message) : 'unknown error';
      this.workerFailed(w, w.ready ? 'crash' : 'worker-blocked', w.ready ? `The pandoc worker crashed (${detail}).` : `The pandoc worker failed to start (${detail}). An extension or content-security policy may be blocking it.`);
    };
    worker.onmessageerror = () => this.workerFailed(w, 'crash', 'The pandoc worker sent a message that could not be read.');
    const init: WorkerRequest = { type: 'init', module: mod.module, limits: this.limits, warm: true };
    try {
      worker.postMessage(init);
    } catch (e) {
      this.workerFailed(w, 'worker-blocked', `The pandoc worker could not be started (${String(e)}).`);
      return false;
    }
    return true;
  }

  /** No worker can serve the pending request: say so. */
  private blocked(message: string): void {
    const j = this.pending;
    if (!j || this.workers.length > 0) return;
    this.pending = undefined;
    this.settle(j, failure('worker-blocked', [host('worker-blocked', message)], this.takeNotices()));
  }

  private terminate(w: PoolWorker): void {
    const i = this.workers.indexOf(w);
    if (i < 0) return;
    this.workers.splice(i, 1);
    if (w.job) this.clearTimers(w.job);
    clearTimeout(w.warmTimer);
    w.worker.onmessage = w.worker.onerror = w.worker.onmessageerror = null;
    w.worker.terminate();
    w.releaseHold();
  }


  /** A worker error settles only the run that worker is serving. */
  private workerFailed(w: PoolWorker, kind: 'crash' | 'worker-blocked', message: string): void {
    if (!this.workers.includes(w)) return;
    const job = w.job;
    const before = !w.ready;
    this.terminate(w);
    if (before) this.startFailed = true;
    if (job) this.settle(job, failure(kind, [host(kind === 'crash' ? 'pandoc-crash' : 'worker-blocked', message)], this.takeNotices()));
    else if (before) this.blocked(message);
    this.pump();
  }

  private onMessage(w: PoolWorker, msg: import('@quarto/pandoc-host').WorkerResponse): void {
    if (!this.workers.includes(w)) return;
    if (msg.type === 'ready') {
      w.ready = true;
      w.state = 'idle';
      const r = this.replay;
      if (r && r.key === w.key) this.warmUp(w, r);
      else this.pump();
    } else if (msg.type === 'progress') {
      const j = w.job;
      if (j && !j.settled && msg.id === j.runId) j.options.onStage?.(msg.stage);
    } else if (msg.id === w.warmId && w.state === 'warming') {
      this.warmed(w, msg.result);
    } else if (w.job && msg.id === w.job.runId) {
      this.complete(w, w.job, msg.result);
    }
  }

  // ---- dispatch and completion ---------------------------------------------

  private dispatch(job: Job, w: PoolWorker): void {
    w.state = 'busy';
    w.job = job;
    job.worker = w;
    job.runId = w.nextRunId++;
    job.dispatchedAt = this.now();
    job.copy = cloneBytes(job.request);
    job.wallTimer = setTimeout(() => this.wallExpired(job), job.wallMs);
    if (!job.settled) job.options.onStage?.('starting');
    const prepared = prepareForPost(job.request);
    const run: WorkerRequest = { type: 'run', id: job.runId, request: prepared.value, shareTree: job.shareTree, fault: job.options.fault };
    w.worker.postMessage(run, prepared.transfer);
  }


  private complete(w: PoolWorker, job: Job, result: ExecuteResult): void {
    const elapsed = this.now() - (job.dispatchedAt ?? 0);
    w.state = 'idle';
    w.job = undefined;
    job.worker = undefined;
    this.clearTimers(job);
    this.k = 0;
    this.loader.countRender();
    if (result.ok && this.histKey?.docKey === job.docKey) this.history = [...this.history, elapsed].slice(-5);
    if (result.ok && elapsed < WARMUP_MAX_MS && job.copy) this.replay = { key: job.key, request: job.copy, shareTree: job.shareTree };
    job.copy = undefined;
    if (!job.settled) {
      const notices = this.takeNotices();
      this.settle(job, result.ok ? { ...result, notices } : failure(result.kind, result.diagnostics, notices, { status: result.status, stderr: result.stderr, stdout: result.stdout, ...(result.stats ? { stats: result.stats } : {}) }));
    }
    this.wanted = MAX_WORKERS;
    if (result.stats?.retire) this.terminate(w);
    this.pump();
  }


  // ---- grace and wall timeout ----------------------------------------------

  private resetGrace(): void {
    this.k = 0;
    this.history = [];
  }

  private clearTimers(job: Job): void {
    clearTimeout(job.wallTimer);
    clearTimeout(job.graceTimer);
    job.wallTimer = job.graceTimer = undefined;
  }

  /**
   * A newer request arrived while `job` is in flight: it keeps running, for at most
   * `min(wall, GRACE_CAP_MS, max(1000 * 2^k, 3 * median))` ms from now. The value is fixed here, once.
   */
  private supersede(job: Job): void {
    job.supersededAt = this.now();
    const doubling = 1000 * 2 ** Math.min(this.k, MAX_K);
    const typical = this.history.length ? 3 * median(this.history) : 0;
    const graceMs = Math.min(job.wallMs, GRACE_CAP_MS, Math.max(doubling, typical));
    job.graceTimer = setTimeout(() => this.graceExpired(job), graceMs);
  }

  private graceExpired(job: Job): void {
    const w = job.worker;
    if (!w) return;
    this.k = Math.min(this.k + 1, MAX_K);
    this.graceTerminations++;
    this.terminate(w);
    this.settle(job, failure('superseded', [], this.takeNotices()));
    this.pump();
  }

  private wallExpired(job: Job): void {
    const w = job.worker;
    if (!w) return;
    this.terminate(w);
    this.settle(job, failure('timeout', [host('pandoc-timeout', `pandoc did not finish within ${Math.round(job.wallMs / 1000)} s and was stopped. A filter may be stuck in a loop.`)], this.takeNotices()));
    this.pump();
  }

  /** Run the replay request once with its output discarded, so the worker's first real render is warm. */
  private warmUp(w: PoolWorker, replay: Replay): void {
    w.state = 'warming';
    const id = w.nextRunId++;
    w.warmId = id;
    w.warmTimer = setTimeout(() => {
      this.terminate(w);
      this.pump();
    }, this.wallTimeoutMs);
    const prepared = prepareForPost(cloneBytes(replay.request));
    const run: WorkerRequest = { type: 'run', id, request: prepared.value, shareTree: replay.shareTree };
    w.worker.postMessage(run, prepared.transfer);
  }

  private warmed(w: PoolWorker, result: ExecuteResult): void {
    w.state = 'idle';
    w.warmId = undefined;
    clearTimeout(w.warmTimer);
    this.loader.countRender();
    if (result.stats?.retire) this.terminate(w);
    this.pump();
  }
}

export type { RunFailure };
