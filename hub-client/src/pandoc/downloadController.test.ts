import { describe, expect, it, vi } from 'vitest';
import type { PandocRequest, ShareTree } from '@quarto/pandoc-host';
import { DownloadController, PROGRESS_THROTTLE_MS, type DownloadDeps, type DownloadFormat, type RequestEnvelope } from './downloadController';
import type { RunOptions, RunOutcome } from './pandocRunner';

const DOCX: DownloadFormat = { key: 'docx', label: 'Word', extension: 'docx', mime: 'application/docx' };
const tree: ShareTree = { share_tree_version: 'v1', files: [] };
const request = { stage_name: 'pandoc', json_path: '/x.json' } as unknown as PandocRequest;

const okEnvelope = (extra: Partial<RequestEnvelope> = {}): RequestEnvelope => ({
  success: true,
  diagnostics: [],
  stats: { unexecuted_cells: 0 },
  request,
  ...extra,
});

const success = (extra: Record<string, unknown> = {}): RunOutcome =>
  ({ ok: true, status: 0, output: new Uint8Array([1, 2, 3]), outputPath: '/o.docx', stderr: '', stdout: '', diagnostics: [], stats: {}, notices: [], ...extra }) as unknown as RunOutcome;

const failure = (kind: string, extra: Record<string, unknown> = {}): RunOutcome =>
  ({ ok: false, kind, status: null, stderr: '', stdout: '', diagnostics: [], notices: [], ...extra }) as unknown as RunOutcome;

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

function setup(overrides: Partial<DownloadDeps> = {}) {
  const save = vi.fn();
  const run = vi.fn<(r: PandocRequest, t: ShareTree, o?: RunOptions) => Promise<RunOutcome>>(async () => success());
  const classify = vi.fn(() => ({ success: true, diagnostics: [] as unknown[] }));
  const buildRequest = vi.fn(async () => okEnvelope());
  const controller = new DownloadController({
    buildRequest,
    getShareTree: () => tree,
    runner: { run },
    classify,
    save,
    nowSeconds: () => 1700000000,
    ...overrides,
  });
  return { controller, save, run, classify, buildRequest };
}

describe('DownloadController', () => {
  it('downloads a successful render under the sanitized name', async () => {
    const { controller, save, buildRequest } = setup();
    await controller.start({ path: 'dir/My Doc.qmd', format: DOCX });
    expect(buildRequest).toHaveBeenCalledWith('dir/My Doc.qmd', 'docx', 1700000000, expect.any(AbortSignal));
    expect(save).toHaveBeenCalledTimes(1);
    const [blob, name] = save.mock.calls[0];
    expect(name).toBe('My Doc.docx');
    expect((blob as Blob).type).toBe('application/docx');
    expect(controller.getSnapshot()).toMatchObject({ phase: 'done', fileName: 'My Doc.docx' });
  });

  it('downloads a warnings-only run and reports the warnings and the unexecuted-cell count', async () => {
    const warning = { kind: 'warning', title: 'w', code: 'Q-11-1' };
    const { controller, save } = setup({
      buildRequest: async () => okEnvelope({ stats: { unexecuted_cells: 3 }, diagnostics: [{ kind: 'warning', title: 'req', code: 'Q-20-9' }] }),
      classify: () => ({ success: true, diagnostics: [warning] }),
    });
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(save).toHaveBeenCalledTimes(1);
    const s = controller.getSnapshot();
    expect(s.phase).toBe('done');
    if (s.phase === 'done') {
      expect(s.unexecutedCells).toBe(3);
      expect(s.warnings.map((w) => w.kind)).toEqual(['warning', 'warning']);
    }
  });

  it('produces no Blob when the request build reports an error', async () => {
    const err = { kind: 'error', title: 'bad document' };
    const { controller, save, run } = setup({ buildRequest: async () => ({ success: false, diagnostics: [err], error: 'nope' }) });
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(save).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
    expect(controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'request-failed', message: 'nope' });
  });

  it('produces no Blob when a diagnostic is an error even though a request came back', async () => {
    const { controller, save, run } = setup({ buildRequest: async () => okEnvelope({ diagnostics: [{ kind: 'error', title: 'x' }] }) });
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(save).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
  });

  it('produces no Blob on a non-zero exit and shows the classified Q-20-3', async () => {
    const q203 = { kind: 'error', title: 'pandoc failed', code: 'Q-20-3' };
    const classify = vi.fn(() => ({ success: false, diagnostics: [q203] as unknown[] }));
    const { controller, save } = setup({
      classify,
      runner: { run: async () => failure('pandoc-exit', { status: 64, stderr: 'boom' }) },
    });
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(save).not.toHaveBeenCalled();
    expect(classify).toHaveBeenCalledWith('pandoc', false, 'exit status: 64', 'boom', '/x.json');
    expect(controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'pandoc-error', diagnostics: [q203] });
  });

  it('produces no Blob when classification of a zero exit finds an error', async () => {
    const { controller, save } = setup({ classify: () => ({ success: true, diagnostics: [{ kind: 'error', title: 'late' }] }) });
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(save).not.toHaveBeenCalled();
    expect(controller.getSnapshot().phase).toBe('failed');
  });

  it('maps host failures to their UI state', async () => {
    const { controller } = setup({ runner: { run: async () => failure('timeout', { diagnostics: [{ origin: 'host', kind: 'error', code: 'pandoc-timeout', message: 'slow' }] }) } });
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'timeout' });
  });

  it('an unexpected throw is a crash, and a memory failure on this thread is out-of-memory', async () => {
    for (const [error, state] of [
      [new Error('boom'), 'crashed'],
      [new RangeError('WebAssembly.Memory.grow(): Maximum memory size exceeded'), 'out-of-memory'],
      [new RangeError('Array buffer allocation failed'), 'out-of-memory'],
    ] as const) {
      const { controller, save } = setup({
        buildRequest: async () => {
          throw error;
        },
      });
      await controller.start({ path: 'a.qmd', format: DOCX });
      expect(controller.getSnapshot()).toMatchObject({ phase: 'failed', state, message: error.message });
      expect(save).not.toHaveBeenCalled();
    }
  });

  it('two quick clicks: the first is superseded, its late response is dropped, one download happens', async () => {
    const first = deferred<RequestEnvelope>();
    const second = deferred<RequestEnvelope>();
    let n = 0;
    const buildRequest = vi.fn(() => (++n === 1 ? first.promise : second.promise));
    const run = vi.fn(async () => success());
    const { controller, save } = setup({ buildRequest, runner: { run } });

    const p1 = controller.start({ path: 'one.qmd', format: DOCX });
    const p2 = controller.start({ path: 'two.qmd', format: DOCX });
    second.resolve(okEnvelope());
    await p2;
    // The first click's stale response arrives after the second finished.
    first.resolve(okEnvelope());
    await p1;

    expect(run).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0][1]).toBe('two.docx');
    expect(controller.getSnapshot()).toMatchObject({ phase: 'done', fileName: 'two.docx', clickId: 2 });
  });

  it('a runner outcome of "superseded" never overwrites the newer click', async () => {
    const firstRun = deferred<RunOutcome>();
    let n = 0;
    const run = vi.fn((_r: PandocRequest, _t: ShareTree) => {
      if (++n === 1) return firstRun.promise;
      // The real runner aborts the older render when a newer run starts.
      return Promise.resolve(success());
    });
    const { controller, save } = setup({ runner: { run } });
    const p1 = controller.start({ path: 'one.qmd', format: DOCX });
    await vi.waitFor(() => expect(run).toHaveBeenCalledTimes(1));
    const p2 = controller.start({ path: 'two.qmd', format: DOCX });
    await p2;
    firstRun.resolve(failure('superseded'));
    await p1;
    expect(save).toHaveBeenCalledTimes(1);
    expect(controller.getSnapshot()).toMatchObject({ phase: 'done', fileName: 'two.docx' });
  });

  it('the click\'s signal reaches the request build and aborts on a newer click', async () => {
    const signals: AbortSignal[] = [];
    const first = deferred<RequestEnvelope>();
    let n = 0;
    const buildRequest = vi.fn((_p: string, _f: string, _s: number, signal: AbortSignal) => {
      signals.push(signal);
      return ++n === 1 ? first.promise : Promise.resolve(okEnvelope());
    });
    const { controller } = setup({ buildRequest });
    const p1 = controller.start({ path: 'one.qmd', format: DOCX });
    const p2 = controller.start({ path: 'two.qmd', format: DOCX });
    expect(signals[0].aborted).toBe(true);
    expect(signals[1].aborted).toBe(false);
    first.resolve(okEnvelope());
    await Promise.all([p1, p2]);
  });

  it('cancel aborts the live render and nothing downloads', async () => {
    const pending = deferred<RunOutcome>();
    let signal: AbortSignal | undefined;
    const run = vi.fn((_r: PandocRequest, _t: ShareTree, o?: RunOptions) => {
      signal = o?.signal;
      return pending.promise;
    });
    const { controller, save } = setup({ runner: { run } });
    const p = controller.start({ path: 'a.qmd', format: DOCX });
    await vi.waitFor(() => expect(run).toHaveBeenCalled());
    controller.cancel();
    expect(signal?.aborted).toBe(true);
    expect(controller.getSnapshot().phase).toBe('cancelled');
    pending.resolve(success());
    await p;
    expect(save).not.toHaveBeenCalled();
    expect(controller.getSnapshot().phase).toBe('cancelled');
  });

  it('throttles load progress but always passes a phase change', async () => {
    let t = 0;
    const seen: string[] = [];
    const run = vi.fn(async (_r: PandocRequest, _t: ShareTree, o?: RunOptions) => {
      o?.onStage?.('loading');
      o?.onLoadProgress?.({ phase: 'download', loaded: 1, total: 10 });
      t += PROGRESS_THROTTLE_MS / 4;
      o?.onLoadProgress?.({ phase: 'download', loaded: 2, total: 10 });
      t += PROGRESS_THROTTLE_MS;
      o?.onLoadProgress?.({ phase: 'download', loaded: 3, total: 10 });
      o?.onLoadProgress?.({ phase: 'verify', loaded: 10, total: 10 });
      return success();
    });
    const { controller } = setup({ runner: { run }, nowMs: () => t });
    controller.subscribe(() => {
      const s = controller.getSnapshot();
      if (s.phase === 'working' && s.load) seen.push(`${s.load.phase}:${s.load.loaded}`);
    });
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(seen).toEqual(['download:1', 'download:3', 'verify:10']);
  });

  it('uses the native executor when one is given, with the editor text', async () => {
    const native = vi.fn(async () => ({ kind: 'ok' as const, blob: new Blob(['x']), fileName: 'server.docx', warnings: [{ kind: 'warning', title: 'w' }] }));
    const save = vi.fn();
    const controller = new DownloadController({ native, save });
    await controller.start({ path: 'a.qmd', format: DOCX, content: '# hi' });
    expect(native).toHaveBeenCalledWith({ path: 'a.qmd', format: 'docx', content: '# hi' }, expect.anything());
    expect(save).toHaveBeenCalledWith(expect.any(Blob), 'server.docx');
    expect(controller.getSnapshot()).toMatchObject({ phase: 'done' });
  });

  it('native 422 shows the diagnostics and downloads nothing', async () => {
    const native = vi.fn(async () => ({ kind: 'failed' as const, diagnostics: [{ kind: 'error', title: 'bad' }] }));
    const save = vi.fn();
    const controller = new DownloadController({ native, save });
    await controller.start({ path: 'a.qmd', format: DOCX, content: '' });
    expect(save).not.toHaveBeenCalled();
    expect(controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'native-failed' });
  });
});
