import { describe, expect, it, vi } from 'vitest';
import type { ExecuteResult } from '@quarto/pandoc-host';
import { PandocLoadError, type LoadResult, type PandocLoader } from './pandocLoader';
import { PandocRunner, uiStateFor, type RunOutcome } from './pandocRunner';
import { smokeJob } from './smokeJob';
import { FakeWorker, OK_RESULT, fakeLoader } from '../test-utils/fakePandocRunner';

function setup(opts: { load?: PandocLoader['load']; wallTimeoutMs?: number; autoReady?: boolean } = {}) {
  const workers: FakeWorker[] = [];
  const { loader, holds } = fakeLoader(opts.load);
  const runner = new PandocRunner({
    loader,
    wallTimeoutMs: opts.wallTimeoutMs,
    createWorker: () => {
      const w = new FakeWorker();
      w.autoReady = opts.autoReady ?? true;
      workers.push(w);
      return w;
    },
  });
  return { runner, workers, holds };
}

const job = () => smokeJob();
const FAIL = (kind: Extract<ExecuteResult, { ok: false }>['kind'], status: number | null = 1, stderr = ''): ExecuteResult => ({
  ok: false,
  kind,
  status,
  stderr,
  stdout: '',
  diagnostics: [],
});
/** Wait until the worker has been sent `run` (init and run). */
const ran = (w: () => FakeWorker | undefined) => vi.waitFor(() => expect(w()?.posts.map((m) => m.type)).toEqual(['init', 'run']));

describe('PandocRunner: normal run', () => {
  it('posts init then run, returns the result, and terminates its one worker', async () => {
    const { runner, workers, holds } = setup();
    const stages: string[] = [];
    const { request, shareTree } = job();
    const p = runner.run(request, shareTree, { onStage: (s) => stages.push(s) });
    await ran(() => workers[0]);
    workers[0].respond(OK_RESULT());
    const out = await p;
    expect(out.ok).toBe(true);
    expect(workers[0].posts.map((m) => m.type)).toEqual(['init', 'run']);
    expect(workers[0].terminated).toBe(1);
    expect(holds.active).toBe(0);
    expect(stages).toEqual(['loading', 'starting']);
    expect(uiStateFor(out)).toBe('done');
  });

  it('maps host failure kinds through with stderr and status', async () => {
    const { runner, workers } = setup();
    const { request, shareTree } = job();
    const p = runner.run(request, shareTree);
    await ran(() => workers[0]);
    workers[0].respond(FAIL('pandoc-exit', 64, 'boom'));
    const out = await p;
    expect(out).toMatchObject({ ok: false, kind: 'pandoc-exit', status: 64, stderr: 'boom' });
    expect(uiStateFor(out)).toBe('pandoc-error');
  });
});

describe('PandocRunner: host inputs and collected files (interface 1)', () => {
  it('sends inputs in the run message, transferring their buffers, and hands collected files back', async () => {
    const { runner, workers } = setup();
    const { request, shareTree } = job();
    const source = new Uint8Array([9, 8, 7]);
    const p = runner.run(request, shareTree, { inputs: { '/__q2_share__/import/source.docx': source } });
    await ran(() => workers[0]);
    const run = workers[0].runs()[0];
    expect([...run.inputs!['/__q2_share__/import/source.docx']]).toEqual([9, 8, 7]);
    expect(source.byteLength).toBe(0); // transferred: the caller's view is detached
    const collected = [{ path: '/__q2_share__/import/media/a.png', bytes: new Uint8Array([1]) }];
    workers[0].respond({ ...OK_RESULT(), collected } as ExecuteResult);
    const out = await p;
    expect(out.ok && out.collected).toEqual(collected);
  });

  it('a run without inputs posts none, and a sub-view input is copied rather than transferred', async () => {
    const { runner, workers } = setup();
    const { request, shareTree } = job();
    const p = runner.run(request, shareTree);
    await ran(() => workers[0]);
    expect(workers[0].runs()[0]).not.toHaveProperty('inputs');
    workers[0].respond(OK_RESULT());
    await p;

    const two = setup();
    const again = job(); // the first run transferred `request`'s buffers
    const big = new Uint8Array([0, 1, 2, 3, 4, 5]);
    const q = two.runner.run(again.request, again.shareTree, { inputs: { '/__q2_share__/import/s.docx': big.subarray(1, 4) } });
    await ran(() => two.workers[0]);
    expect([...two.workers[0].runs()[0].inputs!['/__q2_share__/import/s.docx']]).toEqual([1, 2, 3]);
    expect(big.byteLength).toBe(6); // the original buffer is untouched
    two.workers[0].respond(OK_RESULT());
    await q;
  });
});

describe('PandocRunner: abort, timeout, supersede', () => {
  it('abort terminates the worker exactly once', async () => {
    const { runner, workers } = setup();
    const ac = new AbortController();
    const { request, shareTree } = job();
    const p = runner.run(request, shareTree, { signal: ac.signal });
    await ran(() => workers[0]);
    ac.abort();
    const out = await p;
    expect(out).toMatchObject({ ok: false, kind: 'aborted', diagnostics: [] });
    expect(workers[0].terminated).toBe(1);
    expect(uiStateFor(out)).toBe('cancelled');
  });

  it('wall timeout terminates the worker exactly once with a timeout diagnostic', async () => {
    vi.useFakeTimers();
    try {
      const { runner, workers, holds } = setup({ wallTimeoutMs: 120_000 });
      const { request, shareTree } = job();
      const p = runner.run(request, shareTree);
      await vi.advanceTimersByTimeAsync(0);
      expect(workers[0].posts.map((m) => m.type)).toEqual(['init', 'run']);
      await vi.advanceTimersByTimeAsync(119_000);
      expect(workers[0].terminated).toBe(0);
      await vi.advanceTimersByTimeAsync(2_000);
      const out = await p;
      expect(out).toMatchObject({ ok: false, kind: 'timeout' });
      expect(out.diagnostics[0]).toMatchObject({ origin: 'host', code: 'pandoc-timeout' });
      expect(workers[0].terminated).toBe(1);
      expect(holds.active).toBe(0);
      expect(uiStateFor(out)).toBe('timeout');
    } finally {
      vi.useRealTimers();
    }
  });

  it('the wall timeout is overridable per run', async () => {
    const { runner, workers } = setup();
    const { request, shareTree } = job();
    const out = await runner.run(request, shareTree, { wallTimeoutMs: 30 });
    expect(out).toMatchObject({ kind: 'timeout' });
    expect(workers[0].terminated).toBe(1);
  });

  it('two quick clicks leave one live worker and reject the first as superseded', async () => {
    const { runner, workers } = setup();
    const a = job();
    const b = job();
    const first = runner.run(a.request, a.shareTree);
    await ran(() => workers[0]);
    const second = runner.run(b.request, b.shareTree);
    const firstOut = await first;
    expect(firstOut).toMatchObject({ ok: false, kind: 'superseded' });
    expect(uiStateFor(firstOut)).toBe('cancelled');
    expect(workers[0].terminated).toBe(1);
    await ran(() => workers[1]);
    expect(workers.filter((w) => w.terminated === 0)).toHaveLength(1); // one live worker
    workers[1].respond(OK_RESULT());
    expect((await second).ok).toBe(true);
    expect(workers[1].terminated).toBe(1);
  });

  it('a late result from a terminated worker is ignored (no second outcome, no side effects)', async () => {
    const { runner, workers } = setup();
    const ac = new AbortController();
    const { request, shareTree } = job();
    const p = runner.run(request, shareTree, { signal: ac.signal });
    await ran(() => workers[0]);
    ac.abort();
    const out = await p;
    expect(out).toMatchObject({ kind: 'aborted' });
    expect(() => workers[0].respond(OK_RESULT())).not.toThrow();
    expect(workers[0].terminated).toBe(1);
    expect(workers).toHaveLength(1);
  });

  it('a superseded render does not abort a shared load that the new click joined', async () => {
    let aborted = 0;
    let resolveLoad!: (r: LoadResult) => void;
    const shared = new Promise<LoadResult>((r) => (resolveLoad = r));
    const waiters = new Set<AbortSignal>();
    const load = ((_sha: string, o?: { signal?: AbortSignal }) => {
      const s = o?.signal as AbortSignal;
      waiters.add(s);
      s.addEventListener('abort', () => {
        waiters.delete(s);
        if (waiters.size === 0) aborted++;
      });
      return new Promise<LoadResult>((resolve, reject) => {
        shared.then(resolve);
        s.addEventListener('abort', () => reject(s.reason));
      });
    }) as unknown as PandocLoader['load'];
    const { runner, workers } = setup({ load });
    const a = job();
    const b = job();
    const first = runner.run(a.request, a.shareTree);
    const second = runner.run(b.request, b.shareTree);
    expect(await first).toMatchObject({ kind: 'superseded' });
    expect(aborted).toBe(0); // the second click still waits on the load
    resolveLoad({ module: {} as WebAssembly.Module, source: 'network', notices: [] });
    await ran(() => workers[0]);
    workers[0].respond(OK_RESULT());
    expect((await second).ok).toBe(true);
    expect(workers).toHaveLength(1); // the superseded click never created a worker
  });
});

describe('PandocRunner: failure taxonomy (one diagnostic and one UI state each)', () => {
  const loadFails = (code: PandocLoadError['code']) =>
    setup({
      load: (async () => {
        throw new PandocLoadError(code, `load failed: ${code}`, { url: 'https://x/pandoc.wasm.gz' });
      }) as unknown as PandocLoader['load'],
    });

  const cases: [PandocLoadError['code'], string, string][] = [
    ['no-wasm', 'wasm-unsupported', 'unsupported'],
    ['no-exnref', 'wasm-unsupported', 'unsupported'],
    ['no-decompression', 'wasm-unsupported', 'unsupported'],
    ['no-subtle-crypto', 'wasm-unsupported', 'unsupported'],
    ['compile-blocked', 'compile-blocked', 'blocked'],
    ['fetch-failed', 'download-failed', 'download-failed'],
    ['checksum-mismatch', 'checksum-mismatch', 'download-failed'],
    ['offline', 'offline', 'offline'],
  ];
  for (const [code, diag, ui] of cases)
    it(`load error ${code} -> ${diag} / ${ui}`, async () => {
      const { runner, workers, holds } = loadFails(code);
      const { request, shareTree } = job();
      const out = await runner.run(request, shareTree);
      expect(out).toMatchObject({ ok: false, kind: 'load-failed' });
      expect(out.diagnostics).toHaveLength(1);
      expect(out.diagnostics[0]).toMatchObject({ origin: 'host', code: diag });
      expect(uiStateFor(out)).toBe(ui);
      expect(workers).toHaveLength(0); // no worker for a failed load
      expect(holds.active).toBe(0);
    });

  it('new Worker throwing (CSP, extension) -> worker-blocked / blocked', async () => {
    const { loader } = fakeLoader();
    const runner = new PandocRunner({
      loader,
      createWorker: () => {
        throw new Error('SecurityError: blocked by CSP');
      },
    });
    const { request, shareTree } = job();
    const out = await runner.run(request, shareTree);
    expect(out).toMatchObject({ ok: false, kind: 'worker-blocked' });
    expect(out.diagnostics[0]).toMatchObject({ code: 'worker-blocked' });
    expect(uiStateFor(out)).toBe('blocked');
  });

  it('the worker script failing to load (error before ready) -> worker-blocked', async () => {
    const { runner, workers } = setup({ autoReady: false });
    const { request, shareTree } = job();
    const p = runner.run(request, shareTree);
    await vi.waitFor(() => expect(workers).toHaveLength(1));
    workers[0].onerror?.({ message: 'Failed to load module script' });
    const out = await p;
    expect(out).toMatchObject({ kind: 'worker-blocked' });
    expect(workers[0].terminated).toBe(1);
  });

  it('a worker error after ready -> crash / crashed, one diagnostic', async () => {
    const { runner, workers } = setup();
    const { request, shareTree } = job();
    const p = runner.run(request, shareTree);
    await ran(() => workers[0]);
    workers[0].onerror?.({ message: 'wasm trap' });
    const out = await p;
    expect(out).toMatchObject({ ok: false, kind: 'crash' });
    expect(out.diagnostics).toHaveLength(1);
    expect(uiStateFor(out)).toBe('crashed');
  });

  const hostKinds: [Extract<ExecuteResult, { ok: false }>['kind'], string][] = [
    ['oom', 'out-of-memory'],
    ['crash', 'crashed'],
    ['no-output', 'crashed'],
    ['pandoc-exit', 'pandoc-error'],
    ['invalid-request', 'invalid-request'],
  ];
  for (const [kind, ui] of hostKinds)
    it(`host failure ${kind} -> ${ui}`, async () => {
      const { runner, workers } = setup();
      const { request, shareTree } = job();
      const p = runner.run(request, shareTree);
      await ran(() => workers[0]);
      workers[0].respond(FAIL(kind));
      const out: RunOutcome = await p;
      expect(out).toMatchObject({ ok: false, kind });
      expect(uiStateFor(out)).toBe(ui);
    });
});
