import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ExecuteResult } from '@quarto/pandoc-host';
import type { LoadResult, PandocLoader } from './pandocLoader';
import { smokeJob } from './smokeJob';
import { GRACE_CAP_MS, WARMUP_MAX_MS, WarmPandocRunner } from './warmPandocRunner';
import type { RunOptions, RunOutcome } from './pandocRunner';
import { FakeWorker, OK_RESULT, fakeLoader } from '../test-utils/fakePandocRunner';

const flush = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
  await new Promise((r) => setTimeout(r, 0));
};

function setup(opts: { load?: PandocLoader['load']; limits?: ConstructorParameters<typeof WarmPandocRunner>[0]['limits']; now?: () => number; idleMs?: number; lingerMs?: number; recycleAfter?: number } = {}) {
  const workers: FakeWorker[] = [];
  const clock = { t: 0 };
  const { loader, holds } = fakeLoader(opts.load);
  const runner = new WarmPandocRunner({
    loader,
    limits: opts.limits,
    idleMs: opts.idleMs,
    lingerMs: opts.lingerMs,
    recycleAfter: opts.recycleAfter,
    now: opts.now ?? (() => clock.t),
    createWorker: () => {
      const w = new FakeWorker();
      workers.push(w);
      return w;
    },
  });
  return { runner, workers, holds, clock, loader };
}

/** A fresh request for the same document (the runner takes ownership of the buffers it is given). */
const start = (runner: WarmPandocRunner, md = 'Hello\n', options: RunOptions = {}, sha?: string) => {
  const { request, shareTree } = smokeJob(md, sha);
  return runner.run(request, shareTree, options);
};
const types = (w: FakeWorker) => w.posts.map((m) => m.type);
const FAIL = (kind: Extract<ExecuteResult, { ok: false }>['kind']): ExecuteResult => ({ ok: false, kind, status: 1, stderr: '', stdout: '', diagnostics: [] });
const kind = (o: RunOutcome) => (o.ok ? 'ok' : o.kind);

describe('WarmPandocRunner: pool and dispatch', () => {
  it('reuses one worker across runs: one init, never terminated', async () => {
    const { runner, workers, holds } = setup();
    const a = start(runner);
    await flush();
    workers[0].respond(OK_RESULT());
    expect((await a).ok).toBe(true);
    const b = start(runner);
    await flush();
    expect(workers[0].runs()).toHaveLength(2);
    workers[0].respond(OK_RESULT());
    expect((await b).ok).toBe(true);
    expect(types(workers[0]).filter((t) => t === 'init')).toHaveLength(1);
    expect(workers[0].terminated).toBe(0);
    expect(holds.active).toBe(workers.length);
  });

  it('creates the spare when the first render completes', async () => {
    const { runner, workers } = setup();
    const a = start(runner);
    await flush();
    expect(workers).toHaveLength(1);
    workers[0].respond(OK_RESULT());
    await a;
    await flush();
    expect(workers).toHaveLength(2);
  });

  it('creates the spare at the first request that arrives while the only worker is busy, and runs both at once', async () => {
    const { runner, workers } = setup();
    const a = start(runner, 'one\n');
    await flush();
    expect(workers).toHaveLength(1);
    const b = start(runner, 'two\n');
    await flush();
    expect(workers).toHaveLength(2);
    expect(workers[0].runs()).toHaveLength(1);
    expect(workers[1].runs()).toHaveLength(1);
    // The older run was superseded but keeps running; both complete.
    workers[1].respond(OK_RESULT());
    workers[0].respond(OK_RESULT());
    expect([(await a).ok, (await b).ok]).toEqual([true, true]);
  });

  it('coalesces requests that wait for a worker: latest wins, replaced ones resolve superseded', async () => {
    const { runner, workers } = setup();
    const a = start(runner, 'a\n');
    await flush();
    const b = start(runner, 'b\n');
    await flush();
    // Both workers are busy now; c and d wait, d replaces c.
    const c = start(runner, 'c\n');
    const d = start(runner, 'd\n');
    expect(kind(await c)).toBe('superseded');
    workers[0].respond(OK_RESULT());
    await a;
    await flush();
    const run = workers[0].runs()[1];
    expect(new TextDecoder().decode(run.request.files[0].bytes)).toBe('d\n');
    workers[0].respond(OK_RESULT());
    workers[1].respond(OK_RESULT());
    expect([(await b).ok, (await d).ok]).toEqual([true, true]);
  });

  it('with both workers busy the newest request waits and nothing else is started', async () => {
    const { runner, workers } = setup();
    void start(runner, 'a\n');
    await flush();
    void start(runner, 'b\n');
    await flush();
    void start(runner, 'c\n');
    await flush();
    expect(workers).toHaveLength(2);
    expect(workers.map((w) => w.runs().length)).toEqual([1, 1]);
  });

  it('many requests during the module load produce one dispatch of the newest', async () => {
    let release!: (r: LoadResult) => void;
    const load = vi.fn(() => new Promise<LoadResult>((r) => (release = r)));
    const { runner, workers } = setup({ load: load as unknown as PandocLoader['load'] });
    const all = ['a', 'b', 'c', 'd'].map((x) => start(runner, `${x}\n`));
    await flush();
    expect(workers).toHaveLength(0);
    expect(load).toHaveBeenCalledTimes(1);
    release({ module: {} as WebAssembly.Module, source: 'network', notices: [] });
    await flush();
    expect(workers).toHaveLength(1);
    expect(workers[0].runs()).toHaveLength(1);
    expect(new TextDecoder().decode(workers[0].runs()[0].request.files[0].bytes)).toBe('d\n');
    const outs = await Promise.all(all.slice(0, 3));
    expect(outs.map(kind)).toEqual(['superseded', 'superseded', 'superseded']);
  });

  it('an abort or detach during the load does not cancel it', async () => {
    let signal: AbortSignal | undefined;
    let release!: (r: LoadResult) => void;
    const load = vi.fn((_sha: string, o?: { signal?: AbortSignal }) => {
      signal = o?.signal;
      return new Promise<LoadResult>((r) => (release = r));
    });
    const { runner, workers } = setup({ load: load as unknown as PandocLoader['load'] });
    const ac = new AbortController();
    const a = start(runner, 'a\n', { signal: ac.signal });
    await flush();
    ac.abort();
    expect(kind(await a)).toBe('aborted');
    expect(signal?.aborted).toBe(false);
    const b = start(runner, 'b\n');
    release({ module: {} as WebAssembly.Module, source: 'network', notices: ['note'] });
    await flush();
    workers[0].respond(OK_RESULT());
    const out = await b;
    expect(out.ok && out.notices).toEqual(['note']);
  });

  it('a load failure resolves the pending request and a later request retries', async () => {
    const load = vi.fn().mockRejectedValueOnce(new Error('boom')).mockResolvedValue({ module: {}, source: 'network', notices: [] });
    const { runner, workers } = setup({ load: load as unknown as PandocLoader['load'] });
    expect(kind(await start(runner))).toBe('load-failed');
    const b = start(runner);
    await flush();
    workers[0].respond(OK_RESULT());
    expect((await b).ok).toBe(true);
  });

  it('forwards load progress to the newest request', async () => {
    let onProgress: ((p: { phase: 'download'; loaded: number; total: null }) => void) | undefined;
    let release!: (r: LoadResult) => void;
    const load = (_sha: string, o?: { onProgress?: typeof onProgress }) => {
      onProgress = o?.onProgress;
      return new Promise<LoadResult>((r) => (release = r));
    };
    const { runner } = setup({ load: load as unknown as PandocLoader['load'] });
    const seenA: number[] = [];
    const seenB: number[] = [];
    void start(runner, 'a\n', { onLoadProgress: (p) => seenA.push(p.loaded) });
    void start(runner, 'b\n', { onLoadProgress: (p) => seenB.push(p.loaded) });
    await flush();
    onProgress?.({ phase: 'download', loaded: 5, total: null });
    expect([seenA, seenB]).toEqual([[], [5]]);
    release({ module: {} as WebAssembly.Module, source: 'network', notices: [] });
  });
});

describe('WarmPandocRunner: generation', () => {
  it('a changed generation key terminates every old worker and spawns a new one', async () => {
    const { runner, workers, holds } = setup();
    const a = start(runner, 'a\n');
    await flush();
    workers[0].respond(OK_RESULT());
    await a;
    await flush();
    expect(workers).toHaveLength(2);
    // A different share tree version is a different generation.
    const { request, shareTree } = smokeJob('b\n');
    request.share_tree_version = shareTree.share_tree_version = 'other';
    const b = runner.run(request, shareTree);
    await flush();
    expect(workers.slice(0, 2).map((w) => w.terminated)).toEqual([1, 1]);
    expect(workers).toHaveLength(3);
    expect(workers[2].runs()).toHaveLength(1);
    workers[2].respond(OK_RESULT());
    expect((await b).ok).toBe(true);
    expect(holds.active).toBe(runner.workerCount);
  });

  it('a run in flight on an old-generation worker resolves superseded at the reboot', async () => {
    const { runner, workers } = setup();
    const a = start(runner, 'a\n');
    await flush();
    const { request, shareTree } = smokeJob('b\n');
    request.share_tree_version = shareTree.share_tree_version = 'other';
    void runner.run(request, shareTree);
    expect(kind(await a)).toBe('superseded');
    expect(workers[0].terminated).toBe(1);
  });

  it('different limits are a different generation', async () => {
    const one = setup({ limits: { image_bytes: 1, reference_doc_bytes: 1, total_bytes: 1, collected_file_bytes: 1, collected_total_bytes: 1 } });
    const two = setup({ limits: { total_bytes: 1, reference_doc_bytes: 1, image_bytes: 1, collected_file_bytes: 1, collected_total_bytes: 1 } });
    // Key order does not matter.
    const key = (r: unknown) => (r as { generationKey: (q: unknown) => string }).generationKey(smokeJob().request);
    expect(key(one.runner)).toBe(key(two.runner));
    const three = setup({ limits: { image_bytes: 2, reference_doc_bytes: 1, total_bytes: 1, collected_file_bytes: 1, collected_total_bytes: 1 } });
    expect(key(three.runner)).not.toBe(key(one.runner));
  });
});

describe('WarmPandocRunner: warm-up', () => {
  /** Complete the first render (so a replay request exists when it is fast) and let the spare start. */
  async function afterFirstRender(clock: { t: number }, runner: WarmPandocRunner, workers: FakeWorker[], renderMs: number, result: ExecuteResult = OK_RESULT()) {
    const a = start(runner, 'first\n');
    await flush();
    clock.t += renderMs;
    workers[0].respond(result);
    await a;
    await flush();
  }

  it('warms the spare by replaying a render that took under WARMUP_MAX_MS, from the retained copy', async () => {
    const { runner, workers, clock } = setup();
    await afterFirstRender(clock, runner, workers, WARMUP_MAX_MS - 1);
    expect(workers).toHaveLength(2);
    const warm = workers[1].runs();
    expect(warm).toHaveLength(1);
    expect(new TextDecoder().decode(warm[0].request.files[0].bytes)).toBe('first\n');
  });

  it('does not warm after a render of WARMUP_MAX_MS or more', async () => {
    const { runner, workers, clock } = setup();
    await afterFirstRender(clock, runner, workers, WARMUP_MAX_MS);
    expect(workers).toHaveLength(2);
    expect(workers[1].runs()).toHaveLength(0);
  });

  it('does not use a failed render as the replay request', async () => {
    const { runner, workers, clock } = setup();
    await afterFirstRender(clock, runner, workers, 10, FAIL('pandoc-exit'));
    expect(workers[1].runs()).toHaveLength(0);
  });

  it('a request that arrives while a worker warms up waits for the warm-up; a failed warm-up is ignored', async () => {
    const { runner, workers, clock } = setup();
    await afterFirstRender(clock, runner, workers, 10);
    // Worker 0 is idle; the spare (1) is warming. Two overlapping requests: one each... the first goes to worker 0.
    const b = start(runner, 'b\n');
    await flush();
    expect(workers[0].runs()).toHaveLength(2);
    const c = start(runner, 'c\n');
    await flush();
    // c waits: worker 0 is busy and worker 1 is warming.
    expect(workers[1].runs()).toHaveLength(1);
    workers[1].respond(FAIL('crash'));
    await flush();
    expect(workers[1].runs()).toHaveLength(2);
    expect(new TextDecoder().decode(workers[1].runs()[1].request.files[0].bytes)).toBe('c\n');
    workers[0].respond(OK_RESULT());
    workers[1].respond(OK_RESULT());
    expect([(await b).ok, (await c).ok]).toEqual([true, true]);
  });

  it('the real request that was transferred is empty, so the replay needs the retained copy', async () => {
    const { runner, workers, clock } = setup();
    const { request, shareTree } = smokeJob('first\n');
    const a = runner.run(request, shareTree);
    await flush();
    expect(request.files[0].bytes.byteLength).toBe(0);
    clock.t += 5;
    workers[0].respond(OK_RESULT());
    await a;
    await flush();
    expect(workers[1].runs()[0].request.files[0].bytes.byteLength).toBeGreaterThan(0);
  });
});

describe('WarmPandocRunner: retire and failures', () => {
  it('stats.retire replaces the worker with a new one, which is warmed', async () => {
    const { runner, workers, clock, holds } = setup();
    const a = start(runner, 'first\n');
    await flush();
    clock.t += 5;
    workers[0].respond({ ...OK_RESULT(), stats: { ...(OK_RESULT() as Extract<ExecuteResult, { ok: true }>).stats, retire: true } } as ExecuteResult);
    await a;
    await flush();
    expect(workers[0].terminated).toBe(1);
    expect(workers).toHaveLength(3); // the spare and the replacement
    expect(workers.slice(1).every((w) => w.runs().length === 1)).toBe(true); // both warmed
    expect(holds.active).toBe(2);
  });

  it('a failure carries the worker stats', async () => {
    const { runner, workers } = setup();
    const a = start(runner);
    await flush();
    const stats = { mountMs: 1, instanceMs: 0, runMs: 2, memoryBytes: 3, mountedBytes: 4, retire: true as const };
    workers[0].respond({ ...FAIL('oom'), stats });
    const out = await a;
    expect(out.ok).toBe(false);
    expect(!out.ok && out.stats).toEqual(stats);
  });

  it('a worker error settles only the run that worker is serving', async () => {
    const { runner, workers } = setup();
    const a = start(runner, 'a\n');
    await flush();
    const b = start(runner, 'b\n');
    await flush();
    workers[0].onerror?.({ message: 'wasm trap' });
    expect(kind(await a)).toBe('crash');
    workers[1].respond(OK_RESULT());
    expect((await b).ok).toBe(true);
  });

  it('a worker that fails before ready blocks the pending request and is not respawned in a loop', async () => {
    const { runner, workers } = setup();
    // The first spawn never becomes ready.
    const original = FakeWorker.prototype.postMessage;
    const spy = vi.spyOn(FakeWorker.prototype, 'postMessage').mockImplementation(function (this: FakeWorker, m: unknown, t?: Transferable[]) {
      if ((m as { type: string }).type === 'init') {
        this.posts.push(m as never);
        queueMicrotask(() => this.onerror?.({ message: 'blocked by CSP' }));
        return;
      }
      original.call(this, m, t);
    });
    const a = start(runner);
    expect(kind(await a)).toBe('worker-blocked');
    await flush();
    expect(workers).toHaveLength(1);
    spy.mockRestore();
  });

  it('createWorker throwing is worker-blocked', async () => {
    const { loader } = fakeLoader();
    const runner = new WarmPandocRunner({
      loader,
      createWorker: () => {
        throw new Error('CSP');
      },
    });
    expect(kind(await start(runner))).toBe('worker-blocked');
  });
});

describe('WarmPandocRunner: abort is a detach', () => {
  it('a detached dispatched run resolves aborted at once, its result is dropped and the worker is not terminated', async () => {
    const { runner, workers } = setup();
    const ac = new AbortController();
    const a = start(runner, 'a\n', { signal: ac.signal });
    await flush();
    ac.abort();
    expect(kind(await a)).toBe('aborted');
    expect(workers[0].terminated).toBe(0);
    // A new request cannot use the busy worker: it goes to the spare.
    const b = start(runner, 'b\n');
    await flush();
    workers[0].respond(OK_RESULT()); // the detached run finishes; its result is dropped
    workers[1].respond(OK_RESULT());
    expect((await b).ok).toBe(true);
  });

  it('aborting a pending request removes it and resolves aborted', async () => {
    const { runner, workers } = setup();
    void start(runner, 'a\n');
    await flush();
    void start(runner, 'b\n');
    await flush();
    const ac = new AbortController();
    const c = start(runner, 'c\n', { signal: ac.signal });
    await flush();
    ac.abort();
    expect(kind(await c)).toBe('aborted');
    workers[0].respond(OK_RESULT());
    await flush();
    expect(workers[0].runs()).toHaveLength(1); // c was removed, nothing was dispatched
  });

  it('an already aborted signal resolves aborted without work', async () => {
    const { runner, workers } = setup();
    const ac = new AbortController();
    ac.abort();
    expect(kind(await start(runner, 'a\n', { signal: ac.signal }))).toBe('aborted');
    expect(workers).toHaveLength(0);
  });
});

describe('WarmPandocRunner: grace and wall timeout', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  const tick = (ms = 0) => vi.advanceTimersByTimeAsync(ms);

  /** A worker that answers each run after `renderMs` of fake time (never, for Infinity), unless it was terminated first. */
  class SimWorker extends FakeWorker {
    constructor(private readonly sim: { renderMs: number; dispatches: { at: number; text: string }[] }) {
      super();
    }
    postMessage(message: unknown, transfer?: Transferable[]) {
      super.postMessage(message, transfer);
      const m = this.posts[this.posts.length - 1];
      if (m.type !== 'run') return;
      this.sim.dispatches.push({ at: Date.now(), text: new TextDecoder().decode(m.request.files[0].bytes) });
      if (!Number.isFinite(this.sim.renderMs)) return;
      setTimeout(() => {
        if (!this.terminated) this.emit({ type: 'result', id: m.id, result: OK_RESULT() });
      }, this.sim.renderMs);
    }
  }

  function simSetup(renderMs: number, wallTimeoutMs?: number) {
    const sim = { renderMs, dispatches: [] as { at: number; text: string }[] };
    const workers: SimWorker[] = [];
    const { loader } = fakeLoader();
    const runner = new WarmPandocRunner({
      loader,
      now: () => Date.now(),
      wallTimeoutMs,
      createWorker: () => {
        const w = new SimWorker(sim);
        workers.push(w);
        return w;
      },
    });
    return { runner, sim, workers };
  }

  /** Edits every `everyMs`; returns when a result has been shown or `maxMs` has passed. Counts terminations before the first one. */
  async function type(runner: WarmPandocRunner, everyMs: number, maxMs: number) {
    let first: { atMs: number; terminations: number } | undefined;
    const t0 = Date.now();
    for (let n = 0; Date.now() - t0 < maxMs && !first; n++) {
      void start(runner, `v${n}\n`, { docKey: 'doc' }).then((o) => {
        if (o.ok && !first) first = { atMs: Date.now() - t0, terminations: runner.graceTerminations };
      });
      await tick(everyMs);
    }
    return first;
  }

  it('terminates a superseded run when its grace expires, re-creates the worker, and resolves it superseded', async () => {
    const { runner, workers } = setup({ now: () => Date.now() });
    const a = start(runner, 'a\n');
    await tick();
    void start(runner, 'b\n');
    await tick();
    expect(workers).toHaveLength(2);
    await tick(999);
    expect(workers[0].terminated).toBe(0);
    await tick(1);
    expect(workers[0].terminated).toBe(1);
    expect(kind(await a)).toBe('superseded');
    expect(runner.graceTerminations).toBe(1);
    expect(workers).toHaveLength(3); // the replacement
  });

  it('k doubles per termination, resets after a completion and stops growing at the cap', async () => {
    const { runner, workers } = setup({ now: () => Date.now() });
    const expectGrace = async (ms: number) => {
      const before = runner.graceTerminations;
      await tick(ms - 1);
      expect(runner.graceTerminations).toBe(before);
      await tick(1);
      expect(runner.graceTerminations).toBe(before + 1);
    };
    void start(runner, 'r0\n');
    await tick();
    const graces = [1000, 2000, 4000, 8000, GRACE_CAP_MS, GRACE_CAP_MS];
    for (const g of graces) {
      void start(runner, 'next\n'); // supersedes the run in flight
      await tick();
      await expectGrace(g);
    }
    // A completion resets k (and a quick one leaves a median of about 0): answer a fresh request at once.
    void start(runner, 'fast\n');
    await tick();
    const holder = workers.find((w) => !w.terminated && w.runs().some((r) => new TextDecoder().decode(r.request.files[0].bytes) === 'fast\n'))!;
    holder.respond(OK_RESULT());
    await tick();
    void start(runner, 'x\n');
    await tick();
    void start(runner, 'y\n');
    await tick();
    await expectGrace(1000);
  });

  it('the median is per document: three times the median of five completed renders, reset on a docKey change', async () => {
    const complete = async (runner: WarmPandocRunner, w: () => FakeWorker, ms: number, docKey: string) => {
      const p = start(runner, 'd\n', { docKey });
      await tick();
      await tick(ms);
      w().respond(OK_RESULT());
      await p;
      await tick();
    };
    const { runner, workers } = setup({ now: () => Date.now() });
    for (let i = 0; i < 5; i++) await complete(runner, () => workers[0], 2000, 'A');
    const before = runner.graceTerminations;
    void start(runner, 'x\n', { docKey: 'A' });
    await tick();
    void start(runner, 'y\n', { docKey: 'A' });
    await tick();
    await tick(5999);
    expect(runner.graceTerminations).toBe(before);
    await tick(1);
    expect(runner.graceTerminations).toBe(before + 1); // 3 * 2000 ms
    // A different document: the history is gone, so only the doubling term (k = 1 here) applies.
    const r2 = setup({ now: () => Date.now() });
    for (let i = 0; i < 5; i++) await complete(r2.runner, () => r2.workers[0], 2000, 'A');
    void start(r2.runner, 'x\n', { docKey: 'A' });
    await tick();
    void start(r2.runner, 'y\n', { docKey: 'B' });
    await tick();
    await tick(999);
    expect(r2.runner.graceTerminations).toBe(0);
    await tick(1);
    expect(r2.runner.graceTerminations).toBe(1);
  });

  it('a reboot resets k and the median', async () => {
    const { runner } = setup({ now: () => Date.now() });
    void start(runner, 'a\n');
    await tick();
    void start(runner, 'b\n');
    await tick(1000); // first termination: k = 1
    expect(runner.graceTerminations).toBe(1);
    const { request, shareTree } = smokeJob('c\n');
    request.share_tree_version = shareTree.share_tree_version = 'other';
    void runner.run(request, shareTree);
    await tick();
    void start(runner, 'd\n');
    await tick(); // new generation: the key differs again, so another reboot; either way k restarts at 0
    const before = runner.graceTerminations;
    const e = smokeJob('e\n');
    e.request.share_tree_version = e.shareTree.share_tree_version = 'other';
    void runner.run(e.request, e.shareTree);
    await tick();
    const f = smokeJob('f\n');
    f.request.share_tree_version = f.shareTree.share_tree_version = 'other';
    void runner.run(f.request, f.shareTree);
    await tick(1000);
    expect(runner.graceTerminations).toBe(before + 1);
  });

  it('simulation: a 2 s render with edits every 0.6 s eventually shows a result', async () => {
    const { runner } = simSetup(2000);
    const first = await type(runner, 600, 60_000);
    expect(first).toBeDefined();
    expect(first!.atMs).toBeLessThan(10_000);
  });

  it('simulation: a 10.9 s render under continuous edits completes (terminations before its first frame)', async () => {
    const { runner } = simSetup(10_900);
    const first = await type(runner, 600, 120_000);
    expect(first).toBeDefined();
    // Recorded for the docs (Task 6): the doubling needs this many grace terminations and this much typing.
    console.info(`10.9 s render under 0.6 s edits: first frame after ${first!.terminations} terminations, ${(first!.atMs / 1000).toFixed(1)} s of typing`);
    expect(first!.terminations).toBeLessThanOrEqual(5);
  });

  it('simulation: a runaway never holds the newest request for more than the grace cap', async () => {
    const { runner, sim } = simSetup(Infinity);
    const arrived: Record<string, number> = {};
    for (let n = 0; n < 400; n++) {
      arrived[`v${n}\n`] = Date.now();
      void start(runner, `v${n}\n`, { docKey: 'doc' });
      await tick(500);
    }
    expect(sim.dispatches.length).toBeGreaterThan(5);
    const waits = sim.dispatches.map((d) => d.at - arrived[d.text]);
    expect(Math.max(...waits)).toBeLessThanOrEqual(GRACE_CAP_MS);
  });

  it('a 1.5 s render with edits every 400 ms is not killed once the median is known; the spare takes the newest request', async () => {
    const { runner, sim } = simSetup(1500);
    // Learn the median: five undisturbed renders of the same document.
    for (let i = 0; i < 5; i++) {
      const p = start(runner, 'learn\n', { docKey: 'doc' });
      await tick(1500);
      await p;
    }
    const shown: number[] = [];
    for (let n = 0; n < 15; n++) {
      void start(runner, `v${n}\n`, { docKey: 'doc' }).then((o) => o.ok && shown.push(n));
      await tick(400);
    }
    await tick(5000);
    expect(runner.graceTerminations).toBe(0);
    expect(shown.length).toBeGreaterThan(0);
    expect(sim.dispatches.length).toBeGreaterThan(5);
  });

  it('the wall timeout resolves timeout, starts at dispatch, and a waiting request has none', async () => {
    const { runner, workers } = setup({ now: () => Date.now() });
    const a = start(runner, 'a\n', { wallTimeoutMs: 5000 });
    await tick();
    expect(workers).toHaveLength(1);
    await tick(4999);
    expect(workers[0].terminated).toBe(0);
    await tick(1);
    const out = await a;
    expect(kind(out)).toBe('timeout');
    expect(!out.ok && out.diagnostics[0]).toMatchObject({ code: 'pandoc-timeout' });
    expect(workers[0].terminated).toBe(1);

    // A request waiting for a worker is not subject to the wall timeout.
    const t = setup({ now: () => Date.now() });
    void start(t.runner, 'a\n');
    await tick();
    void start(t.runner, 'b\n');
    await tick();
    const c = start(t.runner, 'c\n', { wallTimeoutMs: 100 });
    let settled = false;
    void c.then(() => (settled = true));
    await tick(50);
    expect(settled).toBe(false);
  });
});

describe('WarmPandocRunner: lifetime and idle', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });
  const tick = (ms = 0) => vi.advanceTimersByTimeAsync(ms);
  const fresh = (idleMs?: number, lingerMs?: number) => setup({ now: () => Date.now(), idleMs, lingerMs });
  /** Every live worker holds the loader exactly once, and a terminated one holds it no more. */
  const holdsMatch = (t: ReturnType<typeof setup>) => {
    expect(t.holds.active).toBe(t.runner.workerCount);
    expect(t.workers.filter((w) => !w.terminated)).toHaveLength(t.runner.workerCount);
  };

  it('releases a worker hold exactly once on each termination path', async () => {
    // grace
    let t = fresh();
    void start(t.runner, 'a\n');
    await tick();
    void start(t.runner, 'b\n');
    await tick(1000);
    expect(t.runner.graceTerminations).toBe(1);
    holdsMatch(t);
    // wall timeout
    t = fresh();
    void start(t.runner, 'a\n', { wallTimeoutMs: 100 });
    await tick(100);
    expect(t.workers[0].terminated).toBe(1);
    holdsMatch(t);
    // reboot
    t = fresh();
    void start(t.runner, 'a\n');
    await tick();
    const other = smokeJob('b\n');
    other.request.share_tree_version = other.shareTree.share_tree_version = 'other';
    void t.runner.run(other.request, other.shareTree);
    await tick();
    expect(t.workers[0].terminated).toBe(1);
    holdsMatch(t);
    // memory retire
    t = fresh();
    const a = start(t.runner, 'a\n');
    await tick();
    t.workers[0].respond({ ...OK_RESULT(), stats: { mountMs: 0, instanceMs: 0, runMs: 0, memoryBytes: 1, mountedBytes: 0, retire: true } } as ExecuteResult);
    await a;
    await tick();
    expect(t.workers[0].terminated).toBe(1);
    holdsMatch(t);
    // a crashed worker
    t = fresh();
    const c = start(t.runner, 'a\n');
    await tick();
    t.workers[0].onerror?.({ message: 'trap' });
    expect(kind(await c)).toBe('crash');
    holdsMatch(t);
  });

  it('terminates the pool after the idle period, resets it at every run, and never fires while a render is in flight', async () => {
    const t = fresh(1000);
    const a = start(t.runner, 'a\n');
    await tick(5000); // longer than the idle period, but the render is in flight
    expect(t.workers[0].terminated).toBe(0);
    t.workers[0].respond(OK_RESULT());
    await a;
    await tick(900);
    expect(t.runner.workerCount).toBe(2);
    const b = start(t.runner, 'b\n'); // a run restarts the period
    await tick();
    t.workers[0].respond(OK_RESULT());
    await b;
    await tick(900);
    expect(t.runner.workerCount).toBe(2);
    await tick(100);
    expect(t.runner.workerCount).toBe(0);
    expect(t.holds.active).toBe(0);
    expect(t.workers.every((w) => w.terminated === 1)).toBe(true);
    // The next run starts a new pool (and loads the module again).
    const c = start(t.runner, 'c\n');
    await tick();
    expect(t.workers).toHaveLength(3);
    t.workers[2].respond(OK_RESULT());
    expect((await c).ok).toBe(true);
  });

  it('acquire/release: the workers linger 30 s after the last release, and a remount inside the linger keeps them', async () => {
    const t = fresh();
    t.runner.acquire();
    const a = start(t.runner, 'a\n');
    await tick();
    t.workers[0].respond(OK_RESULT());
    await a;
    await tick();
    // StrictMode: mount, cleanup, mount.
    t.runner.release();
    t.runner.acquire();
    await tick(60_000);
    expect(t.runner.workerCount).toBe(2);
    // An unmount, then a remount at 29.9 s.
    t.runner.release();
    await tick(29_900);
    expect(t.runner.workerCount).toBe(2);
    t.runner.acquire();
    await tick(60_000);
    expect(t.runner.workerCount).toBe(2);
    // An unmount that stays: after 30 s the workers go.
    t.runner.release();
    await tick(29_999);
    expect(t.runner.workerCount).toBe(2);
    await tick(1);
    expect(t.runner.workerCount).toBe(0);
    expect(t.holds.active).toBe(0);
  });

  it('after the linger a busy worker goes too: its detached run has no consumer', async () => {
    const t = fresh();
    t.runner.acquire();
    const ac = new AbortController();
    const a = start(t.runner, 'a\n', { signal: ac.signal });
    await tick();
    ac.abort();
    expect(kind(await a)).toBe('aborted');
    t.runner.release();
    await tick(30_000);
    expect(t.workers[0].terminated).toBe(1);
    expect(t.holds.active).toBe(0);
    // A pending request at that moment is aborted rather than left waiting for a consumer that is gone.
    const u = fresh();
    u.runner.acquire();
    void start(u.runner, 'a\n');
    await tick();
    void start(u.runner, 'b\n');
    await tick();
    const pending = start(u.runner, 'c\n');
    u.runner.release();
    await tick(30_000);
    expect(kind(await pending)).toBe('aborted');
    expect(u.runner.workerCount).toBe(0);
  });

  it('dropResident terminates the pool and a run in flight resolves aborted; a pending request reloads', async () => {
    const t = fresh();
    const a = start(t.runner, 'a\n');
    await tick();
    t.loader.dropResident();
    expect(kind(await a)).toBe('aborted');
    expect(t.workers[0].terminated).toBe(1);
    expect(t.holds.active).toBe(0);
    const b = start(t.runner, 'b\n');
    await tick();
    expect(t.workers).toHaveLength(2);
    t.workers[1].respond(OK_RESULT());
    expect((await b).ok).toBe(true);
  });
});

describe('WarmPandocRunner: module recycle', () => {
  /** Answer every run (jobs and warm-ups) that a live worker has not answered yet. */
  const drain = async (workers: FakeWorker[]) => {
    for (let round = 0; round < 3; round++) {
      for (const w of workers) {
        if (w.terminated) continue;
        for (;;) {
          try {
            w.respond(OK_RESULT());
          } catch {
            break;
          }
        }
      }
      await flush();
    }
  };

  it('recompiles the module after recycleAfter renders and keeps serving on new workers', async () => {
    let loads = 0;
    const { runner, workers, loader } = setup({
      recycleAfter: 3,
      load: async () => ({ module: { n: ++loads } as unknown as WebAssembly.Module, source: 'resident', notices: [] }),
    });
    const drops = vi.spyOn(loader, 'dropResident');
    for (let i = 0; i < 4; i++) {
      const r = start(runner);
      await flush();
      await drain(workers);
      expect((await r).ok).toBe(true);
    }
    expect(runner.recycles).toBeGreaterThanOrEqual(1);
    expect(drops).toHaveBeenCalledTimes(runner.recycles);
    // The last recycle may leave no request pending, so it reloads only when the next one arrives.
    expect(loads).toBeGreaterThanOrEqual(2);
    expect(loads).toBeLessThanOrEqual(runner.recycles + 1);
    expect(workers.filter((w) => w.terminated > 0).length).toBeGreaterThan(0);
  });

  it('never recycles by default', async () => {
    const { runner, workers, loader } = setup();
    const drops = vi.spyOn(loader, 'dropResident');
    for (let i = 0; i < 6; i++) {
      const r = start(runner);
      await flush();
      await drain(workers);
      await r;
    }
    expect(runner.recycles).toBe(0);
    expect(drops).not.toHaveBeenCalled();
  });
});
