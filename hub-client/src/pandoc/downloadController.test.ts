import { describe, expect, it, vi } from 'vitest';
import type { PandocRequest, ShareTree } from '@quarto/pandoc-host';
import { DownloadController, PROGRESS_THROTTLE_MS, type DownloadDeps, type DownloadFormat, type RequestEnvelope } from './downloadController';
import type { RunOptions, RunOutcome } from './pandocRunner';
import type { TypstJob, TypstRunOutcome } from '../typst/typstRunner';

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

// ---- the PDF chain (host H8) ----------------------------------------------------------
describe('DownloadController: the PDF chain', () => {
  const PDF: DownloadFormat = { key: 'pdf', label: 'PDF', extension: 'pdf', mime: 'application/pdf' };
  const pdfRequest = {
    stage_name: 'pandoc',
    json_path: '/x.json',
    post: 'compile_typst',
    output_path: '/project/doc.typ',
    share_tree_path: '/__q2_share__/pandoc-share',
    env: { SOURCE_DATE_EPOCH: '1700000000' },
    files: [{ path: '/project/doc.qmd', bytes: new Uint8Array([1]) }],
    resource_refs: [{ path: '/project/fig.png', bytes: new Uint8Array([2]) }],
  } as unknown as PandocRequest;
  const shareTree: ShareTree = { share_tree_version: 'v1', files: [{ path: 'typst/t.typ', bytes: new Uint8Array([9]) }] };
  const typstOk = (extra: Record<string, unknown> = {}) =>
    ({ ok: true, pdf: new Uint8Array([37, 80, 68, 70]), pages: 1, diagnostics: [], stats: {}, notices: [], fontFamilies: ['Inter'], ...extra }) as unknown as TypstRunOutcome;
  const typstFail = (kind: string, extra: Record<string, unknown> = {}) => ({ ok: false, kind, diagnostics: [], notices: [], ...extra }) as unknown as TypstRunOutcome;

  function chain(over: { typstRun?: ReturnType<typeof vi.fn>; listFonts?: ReturnType<typeof vi.fn>; pandocRun?: ReturnType<typeof vi.fn>; deps?: Partial<DownloadDeps> } = {}) {
    const order: string[] = [];
    const listFonts = over.listFonts ?? vi.fn(async () => (order.push('fonts'), { ok: true, families: ['Inter', 'Font Awesome'], notices: [] }));
    const typstRun = over.typstRun ?? vi.fn(async () => (order.push('typst'), typstOk()));
    const pandocRun =
      over.pandocRun ??
      vi.fn(async (r: PandocRequest) => {
        order.push('pandoc');
        // The real runner transfers (detaches) these buffers.
        for (const f of [...r.files, ...r.resource_refs]) f.bytes = new Uint8Array(0);
        return success({ output: new TextEncoder().encode('#set page()\n'), outputPath: '/project/doc.typ' });
      });
    const buildRequest = vi.fn(async () => (order.push('request'), okEnvelope({ request: structuredClone(pdfRequest) })));
    const save = vi.fn();
    const assets = vi.fn(() => ({ vendoredPackages: [{ path: 'preview/x/1.0.0/typst.toml', bytes: new Uint8Array([7]) }], fonts: [new Uint8Array([8])] }));
    const controller = new DownloadController({
      buildRequest,
      getShareTree: () => shareTree,
      runner: { run: pandocRun },
      typst: { runner: { run: typstRun, listFonts }, assets, datePrelude: (e) => `#set document(date: ${e})\n` },
      classify: () => ({ success: true, diagnostics: [] }),
      save,
      nowSeconds: () => 1700000000,
      ...over.deps,
    });
    return { controller, order, listFonts, typstRun, pandocRun, buildRequest, save, assets };
  }

  it('lists fonts, builds the request with them, runs pandoc, then compiles and saves the PDF', async () => {
    const { controller, order, buildRequest, save, typstRun } = chain();
    await controller.start({ path: 'dir/Report.qmd', format: PDF });
    expect(order).toEqual(['fonts', 'request', 'pandoc', 'typst']);
    expect(buildRequest).toHaveBeenCalledWith('dir/Report.qmd', 'pdf', 1700000000, expect.any(AbortSignal), ['Inter', 'Font Awesome']);
    const [blob, name] = save.mock.calls[0];
    expect(name).toBe('Report.pdf');
    expect((blob as Blob).type).toBe('application/pdf');
    expect(controller.getSnapshot()).toMatchObject({ phase: 'done', fileName: 'Report.pdf' });

    const job = typstRun.mock.calls[0][0] as TypstJob;
    expect(job.input.main).toBe('/project/doc.typ');
    expect(job.input.root).toBe('/');
    const byPath = new Map(job.input.files.map((f) => [f.path, f.bytes]));
    // The compile sees the share tree at the same absolute path, the request's files and refs (copied
    // before pandoc detached them), and the produced .typ with the date prelude first.
    expect([...(byPath.get('/__q2_share__/pandoc-share/typst/t.typ') ?? [])]).toEqual([9]);
    expect([...(byPath.get('/project/doc.qmd') ?? [])]).toEqual([1]);
    expect([...(byPath.get('/project/fig.png') ?? [])]).toEqual([2]);
    expect(new TextDecoder().decode(byPath.get('/project/doc.typ'))).toBe('#set document(date: 1700000000)\n#set page()\n');
    expect(job.vendoredPackages?.map((p) => p.path)).toEqual(['preview/x/1.0.0/typst.toml']);
    expect([...(job.fonts?.[0] ?? [])]).toEqual([8]);
    // The share tree is reused across renders: the compile gets copies.
    expect(byPath.get('/__q2_share__/pandoc-share/typst/t.typ')).not.toBe(shareTree.files[0].bytes);
  });

  it('gives each typst job its own font buffers (the runner transfers them)', async () => {
    const { listFonts, typstRun } = chain();
    const { controller } = chain({ listFonts, typstRun });
    await controller.start({ path: 'a.qmd', format: PDF });
    expect(listFonts.mock.calls[0][0]).not.toBe((typstRun.mock.calls[0][0] as TypstJob).fonts);
  });

  it('tags each stage\'s diagnostics and concatenates warnings in stage order', async () => {
    const pw = { kind: 'warning', title: 'pandoc warns', code: 'Q-11-1' };
    const tw = { origin: 'typst', kind: 'warning', message: 'typst warns', path: '/p', range: '0:0-0:1', stage: 'typst' };
    const { controller } = chain({
      deps: { classify: () => ({ success: true, diagnostics: [pw] }) },
      typstRun: vi.fn(async () => typstOk({ diagnostics: [tw] })),
    });
    await controller.start({ path: 'a.qmd', format: PDF });
    const s = controller.getSnapshot();
    expect(s.phase).toBe('done');
    if (s.phase === 'done') expect(s.warnings.map((w) => [w.kind, w.stage])).toEqual([['warning', 'pandoc'], ['warning', 'typst']]);
  });

  it('a pandoc error blocks the download and never starts the compile', async () => {
    const { controller, typstRun, save } = chain({ deps: { classify: () => ({ success: false, diagnostics: [{ kind: 'error', title: 'bad' }] }) } });
    await controller.start({ path: 'a.qmd', format: PDF });
    expect(typstRun).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
    const s = controller.getSnapshot();
    expect(s).toMatchObject({ phase: 'failed', state: 'pandoc-error' });
    if (s.phase === 'failed') expect(s.diagnostics[0].stage).toBe('pandoc');
  });

  it('a typst error blocks the download, keeping pandoc\'s warnings ahead of typst\'s diagnostics', async () => {
    const te = { origin: 'typst', kind: 'error', message: 'unknown variable', path: '/project/doc.typ', range: '1:0-1:3', stage: 'typst' };
    const pw = { kind: 'warning', title: 'pandoc warns' };
    const { controller, save } = chain({
      deps: { classify: () => ({ success: true, diagnostics: [pw] }) },
      typstRun: vi.fn(async () => typstFail('typst-error', { diagnostics: [te] })),
    });
    await controller.start({ path: 'a.qmd', format: PDF });
    expect(save).not.toHaveBeenCalled();
    const s = controller.getSnapshot();
    expect(s).toMatchObject({ phase: 'failed', state: 'typst-error' });
    if (s.phase === 'failed') expect(s.diagnostics.map((d) => d.stage)).toEqual(['pandoc', 'typst']);
  });

  it('a package fetch failure is its own state; a font-list load failure stops before pandoc', async () => {
    const a = chain({ typstRun: vi.fn(async () => typstFail('package-fetch')) });
    await a.controller.start({ path: 'a.qmd', format: PDF });
    expect(a.controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'package-error' });

    const load = { code: 'offline' };
    const b = chain({ listFonts: vi.fn(async () => typstFail('load-failed', { loadError: load, diagnostics: [{ origin: 'host', kind: 'error', code: 'offline', message: 'offline', stage: 'typst' }] })) });
    await b.controller.start({ path: 'a.qmd', format: PDF });
    expect(b.buildRequest).not.toHaveBeenCalled();
    expect(b.pandocRun).not.toHaveBeenCalled();
    expect(b.controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'offline' });
  });

  it('a cancel during the pandoc stage reports once, never compiles, and aborts the shared signal', async () => {
    const gate = deferred<RunOutcome>();
    let signal: AbortSignal | undefined;
    const { controller, typstRun, save } = chain({
      pandocRun: vi.fn(async (_r: PandocRequest, _t: ShareTree, o?: RunOptions) => ((signal = o?.signal), gate.promise)),
    });
    const seen: string[] = [];
    controller.subscribe(() => seen.push(controller.getSnapshot().phase));
    const p = controller.start({ path: 'a.qmd', format: PDF });
    await vi.waitFor(() => expect(signal).toBeDefined());
    controller.cancel();
    gate.resolve(failure('aborted'));
    await p;
    expect(signal!.aborted).toBe(true);
    expect(typstRun).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
    expect(seen.filter((s) => s === 'cancelled')).toHaveLength(1);
    expect(controller.getSnapshot().phase).toBe('cancelled');
  });

  it('a cancel during the typst stage reports once and drops the late result', async () => {
    const gate = deferred<TypstRunOutcome>();
    let signal: AbortSignal | undefined;
    const typstRun = vi.fn(async (_j: TypstJob, o?: { signal?: AbortSignal }) => ((signal = o?.signal), gate.promise));
    const { controller, save } = chain({ typstRun });
    const seen: string[] = [];
    controller.subscribe(() => seen.push(controller.getSnapshot().phase));
    const p = controller.start({ path: 'a.qmd', format: PDF });
    await vi.waitFor(() => expect(signal).toBeDefined());
    controller.cancel();
    gate.resolve(typstOk());
    await p;
    expect(signal!.aborted).toBe(true);
    expect(save).not.toHaveBeenCalled();
    expect(seen.filter((s) => s === 'cancelled')).toHaveLength(1);
    expect(controller.getSnapshot().phase).toBe('cancelled');
  });

  it('a newer click supersedes a PDF chain in flight', async () => {
    const gate = deferred<TypstRunOutcome>();
    let first = true;
    const typstRun = vi.fn(async () => (first ? ((first = false), gate.promise) : typstOk()));
    const { controller, save } = chain({ typstRun });
    const p1 = controller.start({ path: 'one.qmd', format: PDF });
    await vi.waitFor(() => expect(typstRun).toHaveBeenCalledTimes(1));
    const p2 = controller.start({ path: 'two.qmd', format: PDF });
    gate.resolve(typstOk());
    await Promise.all([p1, p2]);
    expect(save).toHaveBeenCalledTimes(1);
    expect(save.mock.calls[0][1]).toBe('two.pdf');
  });

  it('shows the typst stages in the working status', async () => {
    const stages: string[] = [];
    const typstRun = vi.fn(async (_j: TypstJob, o?: { onStage?: (s: 'loading' | 'starting' | 'compiling') => void }) => {
      for (const s of ['loading', 'starting', 'compiling'] as const) {
        o?.onStage?.(s);
        const st = controller.getSnapshot();
        if (st.phase === 'working') stages.push(st.stage);
      }
      return typstOk();
    });
    const { controller } = chain({ typstRun });
    await controller.start({ path: 'a.qmd', format: PDF });
    expect(stages).toEqual(['typst-loading', 'typst-starting', 'typst-compiling']);
  });

  it('a non-PDF format never touches the typst runner', async () => {
    const { controller, listFonts, typstRun } = chain();
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(listFonts).not.toHaveBeenCalled();
    expect(typstRun).not.toHaveBeenCalled();
  });
});

describe('DownloadController: whole-book downloads (R9)', () => {
  const EPUB: DownloadFormat = { key: 'epub', label: 'EPUB', extension: 'epub', mime: 'application/epub+zip' };
  const bookEnvelope = (scope: 'book' | 'chapter' = 'book', chapters = 3) =>
    okEnvelope({
      stats: { unexecuted_cells: 0, book: { scope, chapters } },
      request: { stage_name: 'pandoc', json_path: '/x.json', output_path: '/project/_book/My Book.epub' } as unknown as PandocRequest,
    });

  it('an ordinary start calls buildRequest exactly as before (no scope, no extra argument)', async () => {
    const { controller, buildRequest } = setup();
    await controller.start({ path: 'a.qmd', format: DOCX });
    expect(buildRequest.mock.calls[0]).toHaveLength(4);
  });

  it('a book start passes scope auto, the fetched captures and a progress callback as the sixth argument', async () => {
    const fetchCaptures = vi.fn(async () => ({ byPath: { 'one.qmd': new Uint8Array([1]) }, failed: [] as string[] }));
    const buildRequest = vi.fn(async () => bookEnvelope());
    const { controller } = setup({ buildRequest, fetchCaptures });
    await controller.start({ path: 'one.qmd', format: EPUB, scope: 'auto', captureDocIds: { 'one.qmd': 'doc-1' } });
    expect(fetchCaptures).toHaveBeenCalledWith({ 'one.qmd': 'doc-1' }, expect.any(AbortSignal));
    const call = buildRequest.mock.calls[0] as unknown[];
    expect(call).toHaveLength(6);
    expect(call[4]).toBeUndefined();
    expect(call[5]).toMatchObject({ scope: 'auto', capturesByPath: { 'one.qmd': new Uint8Array([1]) }, onProgress: expect.any(Function) });
  });

  it('"this chapter only" passes scope chapter and fetches no captures', async () => {
    const fetchCaptures = vi.fn();
    const buildRequest = vi.fn(async () => bookEnvelope('chapter', 1));
    const { controller } = setup({ buildRequest, fetchCaptures });
    await controller.start({ path: 'one.qmd', format: EPUB, scope: 'chapter', captureDocIds: { 'one.qmd': 'doc-1' } });
    expect(fetchCaptures).not.toHaveBeenCalled();
    expect((buildRequest.mock.calls[0] as unknown[])[5]).toEqual({ scope: 'chapter' });
  });

  it('the PDF chain passes the font families and then the book extras', async () => {
    const buildRequest = vi.fn(async () => bookEnvelope());
    const listFonts = vi.fn(async () => ({ ok: true, families: ['Lato'], notices: [] }));
    const { controller } = setup({
      buildRequest,
      typst: { runner: { run: vi.fn(), listFonts }, assets: () => ({ vendoredPackages: [], fonts: [] }), datePrelude: () => '' } as unknown as DownloadDeps['typst'],
    });
    await controller.start({ path: 'one.qmd', format: { ...DOCX, key: 'pdf', extension: 'pdf' }, scope: 'auto' });
    const call = buildRequest.mock.calls[0] as unknown[];
    expect(call[4]).toEqual(['Lato']);
    expect(call[5]).toMatchObject({ scope: 'auto' });
  });

  it('shows "chapter i of N" while rendering, then the book summary; the file is named after the book', async () => {
    const seen: unknown[] = [];
    const live: { controller?: DownloadController } = {};
    const buildRequest = vi.fn(async (...args: unknown[]) => {
      const extra = args[5] as { onProgress: (i: number, n: number, f: string) => void };
      extra.onProgress(1, 3, 'index.qmd');
      seen.push(live.controller!.getSnapshot());
      extra.onProgress(2, 3, 'one.qmd');
      seen.push(live.controller!.getSnapshot());
      return bookEnvelope('book', 3);
    });
    const s = setup({ buildRequest });
    const { controller } = s;
    live.controller = controller;
    await controller.start({ path: 'one.qmd', format: EPUB, scope: 'auto' });
    expect(seen).toMatchObject([
      { phase: 'working', stage: 'chapter', chapter: { index: 1, total: 3, file: 'index.qmd' } },
      { phase: 'working', stage: 'chapter', chapter: { index: 2, total: 3, file: 'one.qmd' } },
    ]);
    expect(s.save.mock.calls[0][1]).toBe('My Book.epub');
    expect(controller.getSnapshot()).toMatchObject({ phase: 'done', fileName: 'My Book.epub', book: { chapters: 3 } });
  });

  it('a chapter-scope result is named after the active document and carries no book summary', async () => {
    const buildRequest = vi.fn(async () => bookEnvelope('chapter', 1));
    const { controller, save } = setup({ buildRequest });
    await controller.start({ path: 'dir/one.qmd', format: EPUB, scope: 'chapter' });
    expect(save.mock.calls[0][1]).toBe('one.epub');
    const s = controller.getSnapshot();
    expect(s.phase === 'done' && s.book).toBeFalsy();
  });

  it('a chapter whose capture could not be fetched renders as source, with a note on the finished download', async () => {
    const fetchCaptures = vi.fn(async () => ({ byPath: {}, failed: ['one.qmd', 'two.qmd'] }));
    const { controller } = setup({ buildRequest: vi.fn(async () => bookEnvelope()), fetchCaptures });
    await controller.start({ path: 'one.qmd', format: EPUB, scope: 'auto', captureDocIds: { 'one.qmd': 'a', 'two.qmd': 'b' } });
    const s = controller.getSnapshot();
    expect(s.phase).toBe('done');
    if (s.phase === 'done') expect(s.notices.join(' ')).toMatch(/2 chapters/);
  });

  it('a cancel during the capture fetch ends the click: no request is built and nothing is saved', async () => {
    const gate = deferred<{ byPath: Record<string, Uint8Array>; failed: string[] }>();
    const fetchCaptures = vi.fn(() => gate.promise);
    const buildRequest = vi.fn(async () => bookEnvelope());
    const { controller, save } = setup({ buildRequest, fetchCaptures });
    const p = controller.start({ path: 'one.qmd', format: EPUB, scope: 'auto', captureDocIds: { 'one.qmd': 'a' } });
    await Promise.resolve();
    controller.cancel();
    gate.resolve({ byPath: {}, failed: [] });
    await p;
    expect(buildRequest).not.toHaveBeenCalled();
    expect(save).not.toHaveBeenCalled();
    expect(controller.getSnapshot().phase).toBe('cancelled');
  });

  it('progress from a superseded click is dropped', async () => {
    const gate = deferred<RequestEnvelope>();
    let extra!: { onProgress: (i: number, n: number, f: string) => void };
    const buildRequest = vi.fn((...args: unknown[]) => {
      extra = args[5] as typeof extra;
      return gate.promise;
    });
    const { controller } = setup({ buildRequest: buildRequest as unknown as DownloadDeps['buildRequest'] });
    const first = controller.start({ path: 'one.qmd', format: EPUB, scope: 'auto' });
    await Promise.resolve();
    const stale = extra;
    void controller.start({ path: 'one.qmd', format: DOCX });
    stale.onProgress(1, 3, 'old.qmd');
    expect(controller.getSnapshot()).not.toMatchObject({ stage: 'chapter' });
    gate.resolve(bookEnvelope());
    await first;
  });
});
