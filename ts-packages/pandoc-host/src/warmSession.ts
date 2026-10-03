import { WarmPandoc, type RunSignals, type WarmCreateOptions } from './warmPandoc.ts';
import type { Limits } from './limits.ts';
import type { ExecuteResult, Fault, PandocRequest, RunStats, ShareTree } from './types.ts';

const MB = 1024 * 1024;

export interface WarmSessionOptions {
  /** `memory.buffer.byteLength` above which a run reports `stats.retire` (default 512 MB; tests lower it). */
  retireBytes?: number;
  /** Consecutive errored renders after which the instance is recreated (default 50; tests lower it). */
  errorRecycleN?: number;
  limits?: Limits;
  shareRoot?: string;
  /** RTS options of the session's own instances (fault injection and tests only; the production path passes none). */
  rts?: string[];
  /** A seam for tests that need to force a health signal: how an instance is made. Defaults to `WarmPandoc.create`. */
  createInstance?: (module: WebAssembly.Module, options: WarmCreateOptions) => Promise<WarmInstance>;
}

export interface WarmRunOptions {
  /** Run this one request on a dedicated instance with the fault (an `oom` fault is an RTS option, fixed at init); the session's own instance is untouched. */
  fault?: Fault;
  onProgress?: (stage: 'mounting' | 'running') => void;
}

/** What `WarmSession` needs from an instance; `WarmPandoc` is one. */
export interface WarmInstance {
  run(request: PandocRequest, shareTree: ShareTree, hooks?: { onProgress?: (stage: 'mounting' | 'running') => void }): Promise<ExecuteResult>;
  canary(): boolean;
  memoryBytes(): number;
  readonly signals: RunSignals;
}

/**
 * One warm instance and the policy around it; worker-free, so the pool's worker handler only wraps it.
 *
 * Poisoned: a wasm trap, an fd-2 out-of-memory message, or a thrown exit after which the canary `convert` fails (heap
 * exhaustion kills an instance for good and looks like `os.exit(1)`; `os.exit(n)` does not poison). A poisoned
 * instance is dropped and a replacement is created at once, in the background, so the next request waits only for
 * what is left of it; no pool event is involved. `retire` is the other signal: after a render the linear memory
 * (which never shrinks) is over `retireBytes`, so the run's `stats.retire` is set and the worker owner replaces
 * the worker when it is idle. `errorRecycleN` consecutive errored renders recreate the instance too.
 */
export class WarmSession {
  private readonly module: WebAssembly.Module;
  private readonly options: WarmSessionOptions;
  private readonly retireBytes: number;
  private readonly errorRecycleN: number;
  private current!: Promise<WarmInstance>;
  private consecutiveErrors = 0;
  /** Instances created so far, the first included (a replacement is visible here). */
  created = 0;

  private constructor(module: WebAssembly.Module, options: WarmSessionOptions) {
    this.module = module;
    this.options = options;
    this.retireBytes = options.retireBytes ?? 512 * MB;
    this.errorRecycleN = options.errorRecycleN ?? 50;
  }

  /** Instantiates the first instance eagerly. */
  static async create(module: WebAssembly.Module, options: WarmSessionOptions = {}): Promise<WarmSession> {
    const session = new WarmSession(module, options);
    session.current = session.make(options.rts);
    await session.current;
    return session;
  }

  private make(rts: string[] | undefined, fault?: Fault): Promise<WarmInstance> {
    this.created++;
    const o: WarmCreateOptions = { rts, fault, limits: this.options.limits, shareRoot: this.options.shareRoot };
    return (this.options.createInstance ?? WarmPandoc.create)(this.module, o);
  }

  /** Run one request. Failures carry `stats` too, and `stats.retire` is decided after every render. */
  async run(request: PandocRequest, shareTree: ShareTree, runOptions: WarmRunOptions = {}): Promise<ExecuteResult> {
    if (runOptions.fault) {
      const dedicated = await this.make(undefined, runOptions.fault);
      return this.finish(await dedicated.run(request, shareTree, { onProgress: runOptions.onProgress }), dedicated, false);
    }
    let instance: WarmInstance;
    try {
      instance = await this.current;
    } catch {
      // The background replacement failed (for example no memory); try once more before giving up.
      this.current = this.make(this.options.rts);
      instance = await this.current;
    }
    const result = await instance.run(request, shareTree, { onProgress: runOptions.onProgress });
    return this.finish(result, instance, true);
  }

  private finish(result: ExecuteResult, instance: WarmInstance, own: boolean): ExecuteResult {
    const memory = instance.memoryBytes();
    const base: RunStats = result.stats ?? { mountMs: 0, instanceMs: 0, runMs: 0, memoryBytes: memory, mountedBytes: 0 };
    const retire = memory > this.retireBytes;
    const out = { ...result, stats: { ...base, ...(retire ? { retire: true as const } : {}) } } as ExecuteResult;
    if (!own) return out;

    this.consecutiveErrors = result.ok ? 0 : this.consecutiveErrors + 1;
    if (this.isPoisoned(instance) || this.consecutiveErrors >= this.errorRecycleN) {
      this.consecutiveErrors = 0;
      this.current = this.make(this.options.rts);
      // A failed replacement surfaces on the next run; do not leave the rejection unhandled meanwhile.
      this.current.catch(() => undefined);
    }
    return out;
  }

  private isPoisoned(instance: WarmInstance): boolean {
    const s = instance.signals;
    if (s.trapped || s.oomMessage) return true;
    return s.thrownExit !== null && !instance.canary();
  }
}
