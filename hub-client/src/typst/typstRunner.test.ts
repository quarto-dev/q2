import { describe, expect, it } from 'vitest';
import type { CompileResult, WorkerRequest, WorkerResponse } from '@quarto/typst-host';
import { PandocLoadError, type LoadResult, type PandocLoader } from '../pandoc/pandocLoader';
import type { TypstFontsLoader } from './typstAssets';
import { TypstRunner, typstUiStateFor, type TypstRunOutcome, type WorkerLike } from './typstRunner';

/** A scriptable Worker double: answers init with `ready`, and records posts and terminations. */
class FakeWorker implements WorkerLike {
  onmessage: WorkerLike['onmessage'] = null;
  onerror: WorkerLike['onerror'] = null;
  onmessageerror: WorkerLike['onmessageerror'] = null;
  terminated = 0;
  posts: { msg: WorkerRequest; transfer?: Transferable[] }[] = [];
  autoReady = true;
  postMessage(message: unknown, transfer?: Transferable[]) {
    const msg = message as WorkerRequest;
    this.posts.push({ msg, transfer });
    if (msg.type === 'init' && this.autoReady) queueMicrotask(() => this.emit({ type: 'ready', fontFamilies: ['Libertinus Serif', 'Brand Sans'] }));
  }
  respond(result: CompileResult) {
    const run = this.posts.map((p) => p.msg).find((m): m is Extract<WorkerRequest, { type: 'run' }> => m.type === 'run');
    if (!run) throw new Error('no run message posted yet');
    this.emit({ type: 'result', id: run.id, result });
  }
  terminate() {
    this.terminated++;
  }
  emit(m: WorkerResponse) {
    this.onmessage?.({ data: m });
  }
}

const OK = (): CompileResult => ({
  ok: true,
  pdf: new Uint8Array([37, 80, 68, 70]),
  pages: 2,
  diagnostics: [],
  stats: { prepareMs: 0, attempts: 1, compileMs: 1, packagesFetched: 0, packageBytes: 0 },
});

const DEFAULT_FONT = new Uint8Array([1, 2, 3]);

function fakeLoaders(opts: { load?: PandocLoader['load']; fontsLoad?: TypstFontsLoader['load'] } = {}) {
  const holds = { active: 0, total: 0 };
  const hold = () => {
    holds.active++;
    holds.total++;
    let done = false;
    return () => {
      if (!done) {
        done = true;
        holds.active--;
      }
    };
  };
  const loader = {
    load: opts.load ?? (async () => ({ module: {} as WebAssembly.Module, source: 'resident', notices: [] }) satisfies LoadResult),
    hold,
  } as unknown as PandocLoader;
  const fonts = {
    load: opts.fontsLoad ?? (async () => ({ fonts: [DEFAULT_FONT], notices: ['fonts notice'] })),
    hold,
  } as unknown as TypstFontsLoader;
  return { loader, fonts, holds };
}

function setup(opts: { load?: PandocLoader['load']; fontsLoad?: TypstFontsLoader['load']; wallTimeoutMs?: number; autoReady?: boolean; serialized?: boolean } = {}) {
  const workers: FakeWorker[] = [];
  const { loader, fonts, holds } = fakeLoaders(opts);
  const runner = new TypstRunner({
    loader,
    fonts,
    wallTimeoutMs: opts.wallTimeoutMs,
    serialized: opts.serialized,
    createWorker: () => {
      const w = new FakeWorker();
      if (opts.autoReady === false) w.autoReady = false;
      workers.push(w);
      return w;
    },
  });
  return { runner, workers, holds };
}

const job = () => ({
  input: { main: '/doc/a.typ', files: [{ path: '/doc/a.typ', bytes: new Uint8Array([1, 2]) }] },
  fonts: [new Uint8Array([9, 9])],
  vendoredPackages: [{ path: 'preview/p/1.0.0/typst.toml', bytes: new Uint8Array([5]) }],
});
const tick = () => new Promise((r) => setTimeout(r, 0));

describe('TypstRunner', () => {
  it('compiles: init with defaults then the job’s fonts, then run; the worker is terminated', async () => {
    const { runner, workers, holds } = setup();
    const stages: string[] = [];
    const p = runner.run(job(), { onStage: (s) => stages.push(s) });
    await tick();
    const w = workers[0];
    expect(w.posts[0].msg).toMatchObject({ type: 'init' });
    const init = w.posts[0].msg as Extract<WorkerRequest, { type: 'init' }>;
    expect(init.init.fonts.map((f) => [...f])).toEqual([[1, 2, 3], [9, 9]]);
    expect(init.init.vendoredPackages?.[0].path).toBe('preview/p/1.0.0/typst.toml');
    expect(w.posts[1].msg).toMatchObject({ type: 'run', input: { main: '/doc/a.typ' } });
    w.emit({ type: 'progress', id: 1, stage: 'compiling' });
    w.respond(OK());
    const o = await p;
    expect(o).toMatchObject({ ok: true, pages: 2, fontFamilies: ['Libertinus Serif', 'Brand Sans'], notices: ['fonts notice'] });
    expect(stages).toEqual(['loading', 'starting', 'compiling']);
    expect(w.terminated).toBe(1);
    expect(holds.active).toBe(0);
    expect(typstUiStateFor(o)).toBe('done');
  });

  it('transfers the job’s own buffers (not the default fonts)', async () => {
    const { runner, workers } = setup();
    const j = job();
    const p = runner.run(j);
    await tick();
    const w = workers[0];
    const initTransfer = w.posts[0].transfer as ArrayBuffer[];
    expect(initTransfer).toContain(j.fonts[0].buffer);
    expect(initTransfer).not.toContain(DEFAULT_FONT.buffer);
    expect(w.posts[1].transfer).toContain(j.input.files[0].bytes.buffer);
    w.respond(OK());
    await p;
  });

  it('classifies compile failures and maps each to one UI state', async () => {
    const cases: [CompileResult & { ok: false }, string][] = [
      [{ ok: false, kind: 'typst-error', diagnostics: [{ origin: 'typst', kind: 'error', message: 'x', path: '/a.typ', range: '0:0-0:1', stage: 'typst' }] }, 'typst-error'],
      [{ ok: false, kind: 'package-fetch', diagnostics: [{ origin: 'host', kind: 'error', code: 'package-fetch-failed', message: 'offline', stage: 'typst' }] }, 'package-error'],
      [{ ok: false, kind: 'invalid-input', diagnostics: [] }, 'invalid-request'],
      [{ ok: false, kind: 'oom', diagnostics: [] }, 'out-of-memory'],
      [{ ok: false, kind: 'crash', diagnostics: [] }, 'crashed'],
    ];
    for (const [result, state] of cases) {
      const { runner, workers } = setup();
      const p = runner.run(job());
      await tick();
      workers[0].respond(result);
      const o = await p;
      expect(o).toMatchObject({ ok: false, kind: result.kind, diagnostics: result.diagnostics });
      expect(typstUiStateFor(o)).toBe(state);
    }
  });

  it('abort terminates the worker once and reports one cancel with no diagnostic', async () => {
    const { runner, workers, holds } = setup();
    const ac = new AbortController();
    const p = runner.run(job(), { signal: ac.signal });
    await tick();
    ac.abort();
    ac.abort();
    const o = await p;
    expect(o).toMatchObject({ ok: false, kind: 'aborted', diagnostics: [] });
    expect(workers[0].terminated).toBe(1);
    expect(holds.active).toBe(0);
    expect(typstUiStateFor(o)).toBe('cancelled');
    // A late result from the terminated worker is ignored.
    workers[0].respond(OK());
  });

  it('an abort before the call never starts a worker', async () => {
    const { runner, workers } = setup();
    const ac = new AbortController();
    ac.abort();
    expect(await runner.run(job(), { signal: ac.signal })).toMatchObject({ ok: false, kind: 'aborted' });
    expect(workers).toHaveLength(0);
  });

  it('a newer run supersedes the previous one, terminating its worker', async () => {
    const { runner, workers } = setup();
    const first = runner.run(job());
    await tick();
    const second = runner.run(job());
    expect(await first).toMatchObject({ ok: false, kind: 'superseded' });
    await tick();
    expect(workers[0].terminated).toBe(1);
    workers[1].respond(OK());
    expect(await second).toMatchObject({ ok: true });
  });

  it('the wall timeout terminates the worker and says so', async () => {
    const { runner, workers } = setup({ wallTimeoutMs: 20 });
    const p = runner.run(job());
    const o = await p;
    expect(o).toMatchObject({ ok: false, kind: 'timeout', diagnostics: [{ code: 'typst-timeout' }] });
    expect(workers[0].terminated).toBe(1);
    expect(typstUiStateFor(o)).toBe('timeout');
  });

  it('a worker that cannot be constructed is "blocked"', async () => {
    const { loader, fonts } = fakeLoaders();
    const runner = new TypstRunner({
      loader,
      fonts,
      createWorker: () => {
        throw new Error('blocked by CSP');
      },
    });
    const o = await runner.run(job());
    expect(o).toMatchObject({ ok: false, kind: 'worker-blocked', diagnostics: [{ code: 'worker-blocked' }] });
    expect(typstUiStateFor(o)).toBe('blocked');
  });

  it('a worker error before ready is "blocked"; after ready it is a crash', async () => {
    const a = setup({ autoReady: false });
    const pa = a.runner.run(job());
    await tick();
    a.workers[0].onerror?.({ message: 'script failed' });
    expect(await pa).toMatchObject({ ok: false, kind: 'worker-blocked' });

    const b = setup();
    const pb = b.runner.run(job());
    await tick();
    b.workers[0].onerror?.({ message: 'wasm trap' });
    expect(await pb).toMatchObject({ ok: false, kind: 'crash', diagnostics: [{ code: 'typst-crash' }] });
  });

  it('a failed worker init is a crash naming the reason', async () => {
    const { runner, workers } = setup({ autoReady: false });
    const p = runner.run(job());
    await tick();
    workers[0].emit({ type: 'init-failed', message: 'out of memory' });
    expect(await p).toMatchObject({ ok: false, kind: 'crash', diagnostics: [{ message: expect.stringContaining('out of memory') }] });
  });

  it('a load failure maps to the same states as pandoc’s', async () => {
    const codes: [PandocLoadError['code'], string][] = [
      ['offline', 'offline'],
      ['fetch-failed', 'download-failed'],
      ['checksum-mismatch', 'download-failed'],
      ['compile-blocked', 'blocked'],
      ['no-decompression', 'unsupported'],
    ];
    for (const [code, state] of codes) {
      const { runner, workers } = setup({
        load: async () => {
          throw new PandocLoadError(code, `the Typst compiler: ${code}`);
        },
      });
      const o = await runner.run(job());
      expect(o).toMatchObject({ ok: false, kind: 'load-failed' });
      expect(typstUiStateFor(o)).toBe(state);
      expect(workers).toHaveLength(0);
    }
  });

  it('a fonts-load failure is a load failure too', async () => {
    const { runner } = setup({
      fontsLoad: async () => {
        throw new PandocLoadError('offline', 'offline');
      },
    });
    expect(typstUiStateFor(await runner.run(job()))).toBe('offline');
  });

  it('listFonts initialises a worker, returns the families and discards it', async () => {
    const { runner, workers } = setup();
    const o = await runner.listFonts([new Uint8Array([7])]);
    expect(o).toEqual({ ok: true, families: ['Libertinus Serif', 'Brand Sans'], notices: ['fonts notice'] });
    expect(workers).toHaveLength(1);
    expect(workers[0].terminated).toBe(1);
    expect(workers[0].posts.map((p) => p.msg.type)).toEqual(['init']);
  });

  it('keeps the loaders busy for the whole run, so the idle drop cannot fire mid-compile', async () => {
    const { runner, workers, holds } = setup();
    const p = runner.run(job());
    await tick();
    expect(holds.active).toBe(2);
    workers[0].respond(OK());
    await p;
    expect(holds.active).toBe(0);
  });
});

it('typstUiStateFor on a hand-built outcome', () => {
  const o: TypstRunOutcome = { ok: false, kind: 'aborted', diagnostics: [], notices: [] };
  expect(typstUiStateFor(o)).toBe('cancelled');
});

describe('TypstRunner: serialized mode (the preview\'s runner)', () => {
  const kindOf = (o: { ok: boolean } & Partial<{ kind: string }>) => (o.ok ? 'ok' : o.kind);

  it('never aborts a running job: a newer job waits, and the older one completes', async () => {
    const { runner, workers } = setup({ serialized: true });
    const a = runner.run(job());
    await tick();
    const b = runner.run(job());
    await tick();
    expect(workers).toHaveLength(1); // b has not started
    expect(workers[0].terminated).toBe(0);
    workers[0].respond(OK());
    expect(kindOf(await a)).toBe('ok');
    await tick();
    expect(workers).toHaveLength(2);
    workers[1].respond(OK());
    expect(kindOf(await b)).toBe('ok');
  });

  it('keeps one running and one waiting job: a newer job replaces the waiting one, which resolves superseded; run and listFonts share the slot', async () => {
    const { runner, workers } = setup({ serialized: true });
    const a = runner.run(job());
    await tick();
    const b = runner.listFonts([]);
    const c = runner.run(job());
    expect(kindOf(await b)).toBe('superseded');
    workers[0].respond(OK());
    await a;
    await tick();
    expect(workers).toHaveLength(2);
    expect(workers[1].posts[1].msg).toMatchObject({ type: 'run' }); // c, not b's font list
    workers[1].respond(OK());
    expect(kindOf(await c)).toBe('ok');
  });

  it('an abort signal detaches a running job: it resolves aborted at once, but keeps the slot until it finishes', async () => {
    const { runner, workers } = setup({ serialized: true });
    const ac = new AbortController();
    const a = runner.run(job(), { signal: ac.signal });
    await tick();
    ac.abort();
    expect(kindOf(await a)).toBe('aborted');
    expect(workers[0].terminated).toBe(0);
    const b = runner.run(job());
    await tick();
    expect(workers).toHaveLength(1); // b still waits for the detached job
    workers[0].respond(OK()); // its result is dropped
    await tick();
    expect(workers).toHaveLength(2);
    workers[1].respond(OK());
    expect(kindOf(await b)).toBe('ok');
  });

  it('a detached job stops reporting stages', async () => {
    const { runner, workers } = setup({ serialized: true });
    const ac = new AbortController();
    const stages: string[] = [];
    void runner.run(job(), { signal: ac.signal, onStage: (s) => stages.push(s) });
    await tick();
    ac.abort();
    workers[0].emit({ type: 'progress', id: 1, stage: 'compiling' });
    expect(stages).not.toContain('compiling');
  });

  it('an abort of the waiting job removes it', async () => {
    const { runner, workers } = setup({ serialized: true });
    const a = runner.run(job());
    await tick();
    const ac = new AbortController();
    const b = runner.run(job(), { signal: ac.signal });
    ac.abort();
    expect(kindOf(await b)).toBe('aborted');
    workers[0].respond(OK());
    await a;
    await tick();
    expect(workers).toHaveLength(1); // b never started
  });

  it('the wall timeout still stops a hung compile and frees the slot', async () => {
    const { runner, workers } = setup({ serialized: true, wallTimeoutMs: 20 });
    const a = runner.run(job());
    const b = runner.run(job());
    expect(kindOf(await a)).toBe('timeout');
    expect(workers[0].terminated).toBeGreaterThan(0);
    await tick();
    expect(workers).toHaveLength(2);
    workers[1].respond(OK());
    expect(kindOf(await b)).toBe('ok');
  });

  it('an already aborted signal resolves aborted without starting anything', async () => {
    const { runner, workers } = setup({ serialized: true });
    const ac = new AbortController();
    ac.abort();
    expect(kindOf(await runner.run(job(), { signal: ac.signal }))).toBe('aborted');
    expect(workers).toHaveLength(0);
  });

  it('without the option a newer job still supersedes the running one (the app-wide runner)', async () => {
    const { runner } = setup();
    const a = runner.run(job());
    await tick();
    void runner.run(job());
    expect(kindOf(await a)).toBe('superseded');
  });

  it("a compile on the app-wide runner (Download as PDF) and one on the serialized preview runner never cancel each other", async () => {
    const preview = setup({ serialized: true });
    const download = setup();
    const p = preview.runner.run(job());
    await tick();
    const d = download.runner.run(job());
    await tick();
    expect(preview.workers[0].terminated).toBe(0);
    expect(download.workers[0].terminated).toBe(0);
    preview.workers[0].respond(OK());
    download.workers[0].respond(OK());
    expect([kindOf(await p), kindOf(await d)]).toEqual(['ok', 'ok']);
  });
});
