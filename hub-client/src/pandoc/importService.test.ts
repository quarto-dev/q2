import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Diagnostic, RequestFile } from '@quarto/pandoc-host';
import { inferMimeType } from '@quarto/preview-renderer/types/project';
import { FILE_SIZE_LIMITS } from '../services/resourceService';
import {
  createImportService,
  createStubImportService,
  getImportService,
  importAvailable,
  setImportServiceForTests,
  type ImportDeps,
  type ImportProgress,
} from './importService';
import { PandocLoadError } from './pandocLoader';
import { failure, type RunOutcome, type RunStage } from './pandocRunner';
import * as featureFlag from './featureFlag';

const SOURCE_PATH = '/__q2_share__/import/source.docx';
const MEDIA = '/__q2_share__/import/media';
const CODES: Record<string, string> = { 'pandoc-exit': 'Q-24-3', 'no-output': 'Q-24-3', oom: 'Q-24-13', crash: 'Q-24-13', timeout: 'Q-24-13', 'invalid-request': 'Q-24-12', superseded: 'Q-24-12' };
const rust = (code: string, kind: 'error' | 'warning' | 'info' = 'error'): Diagnostic => ({ origin: 'rust', kind, code, title: code });

const docx = (name = 'a.docx', bytes = [1, 2, 3]) => new File([new Uint8Array(bytes)], name);

function success(over: Partial<{ output: string; collected: RequestFile[]; stderr: string; diagnostics: Diagnostic[] }> = {}): RunOutcome {
  return {
    ok: true,
    status: 0,
    output: new TextEncoder().encode(over.output ?? '{"blocks":[]}'),
    outputPath: '/__q2_share__/import/out.json',
    collected: over.collected ?? [],
    stderr: over.stderr ?? '',
    stdout: '',
    diagnostics: over.diagnostics ?? [],
    stats: {} as never,
    notices: [],
  };
}

function setup(over: { run?: (...a: unknown[]) => Promise<RunOutcome>; stages?: RunStage[]; finish?: unknown; deps?: Partial<ImportDeps> } = {}) {
  const run = vi.fn(
    over.run ??
      (async (_req: unknown, _tree: unknown, opts: { onStage?: (s: RunStage) => void }) => {
        for (const s of over.stages ?? ['loading', 'starting', 'mounting', 'running']) opts.onStage?.(s);
        return success();
      }),
  );
  const wasm = {
    ready: vi.fn(async () => undefined),
    getImportFormatTable: vi.fn(() => ({ formats: [{ id: 'docx', label: 'Word', extensions: ['.docx'], mime_types: ['application/x-docx'] }], max_source_bytes: 1000 })),
    prepareImport: vi.fn((name: string, size: number, sha: string) => {
      if (!name.endsWith('.docx')) return { success: false, diagnostics: [rust('Q-24-1')] };
      if (size > 1000) return { success: false, diagnostics: [rust('Q-24-2')] };
      if (sha === '') return { success: true, diagnostics: [], format: 'docx' };
      return { success: true, diagnostics: [], format: 'docx', request: { job_id: 'r' }, share_tree: { share_tree_version: 'v', files: [] }, source_path: SOURCE_PATH };
    }),
    finishImport: vi.fn(() => over.finish ?? { success: true, diagnostics: [rust('Q-24-4', 'warning')], qmd: '# Hello\n', media_plan: [] }),
    classifyImportFailure: vi.fn((kind: string) => ({ diagnostics: [rust(CODES[kind] ?? 'Q-24-12')] })),
  };
  const deps: ImportDeps = {
    runner: { run: run as unknown as ImportDeps['runner']['run'] },
    wasm: wasm as unknown as ImportDeps['wasm'],
    convertImage: vi.fn(async () => new Uint8Array([0x89, 0x50])),
    sha256: async (b) => `sha-${b.byteLength}`,
    now: () => 0,
    maxImageBytes: FILE_SIZE_LIMITS.MAX_FILE_SIZE,
    ...over.deps,
  };
  return { svc: createImportService(deps), run, wasm, deps };
}

afterEach(() => vi.restoreAllMocks());

describe('getImportFormats', () => {
  it('maps the snake_case table to camelCase, once', async () => {
    const { svc, wasm } = setup();
    expect(await svc.getImportFormats()).toEqual({ formats: [{ id: 'docx', label: 'Word', extensions: ['.docx'], mimeTypes: ['application/x-docx'] }], maxSourceBytes: 1000 });
    await svc.getImportFormats();
    expect(wasm.getImportFormatTable).toHaveBeenCalledTimes(1);
    expect(wasm.ready).toHaveBeenCalledTimes(1);
  });
});

describe('validation without reading or loading pandoc', () => {
  it('validateImportSource is [] for an importable file and Rust\'s Q-24-1 / Q-24-2 otherwise', async () => {
    const { svc, wasm } = setup();
    expect(await svc.validateImportSource(docx())).toEqual([]);
    expect((await svc.validateImportSource(docx('notes.md')))[0]).toMatchObject({ code: 'Q-24-1' });
    expect((await svc.validateImportSource(docx('big.docx', new Array(1001).fill(0))))[0]).toMatchObject({ code: 'Q-24-2' });
    expect(wasm.prepareImport).toHaveBeenCalledWith('a.docx', 3, '');
  });

  it('importDocument returns the refusal without reading the file or running pandoc', async () => {
    const { svc, run } = setup();
    const file = docx('notes.md');
    const read = vi.spyOn(file, 'arrayBuffer');
    const out = await svc.importDocument(file, 'notes.qmd');
    expect(out).toMatchObject({ ok: false, diagnostics: [{ code: 'Q-24-1' }] });
    expect(read).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();

    const big = docx('big.docx', new Array(1001).fill(0));
    const out2 = await svc.importDocument(big, 'big.qmd');
    expect(out2).toMatchObject({ ok: false, diagnostics: [{ code: 'Q-24-2' }] });
    expect(run).not.toHaveBeenCalled();
  });
});

describe('importDocument: the happy path', () => {
  it('prepares with the real length and hash, runs with the source as an input, and finishes with prepare\'s format', async () => {
    const { svc, run, wasm } = setup();
    const out = await svc.importDocument(docx(), 'dir/a.qmd');
    expect(out).toMatchObject({ ok: true, qmd: '# Hello\n', media: [] });
    expect(wasm.prepareImport).toHaveBeenLastCalledWith('a.docx', 3, 'sha-3');
    const [req, tree, opts] = run.mock.calls[0] as [unknown, unknown, { inputs: Record<string, Uint8Array> }];
    expect(req).toEqual({ job_id: 'r' });
    expect(tree).toEqual({ share_tree_version: 'v', files: [] });
    expect(Object.keys(opts.inputs)).toEqual([SOURCE_PATH]);
    expect(wasm.finishImport).toHaveBeenCalledWith('{"blocks":[]}', '', 'dir/a.qmd', '[]', 'docx');
  });

  it('joins media_plan with the stored bytes, in plan order, with MIME types from inferMimeType', async () => {
    const collected: RequestFile[] = [
      { path: `${MEDIA}/media/a.png`, bytes: new Uint8Array([1]) },
      { path: `${MEDIA}/media/b.jpeg`, bytes: new Uint8Array([2, 2]) },
    ];
    const finish = {
      success: true,
      diagnostics: [],
      qmd: 'x',
      media_plan: [
        { pandoc_path: `${MEDIA}/media/b.jpeg`, project_path: 'a_media/bbbbbbbbbbbb.jpeg' },
        { pandoc_path: `${MEDIA}/media/a.png`, project_path: 'a_media/aaaaaaaaaaaa.png' },
      ],
    };
    const { svc, wasm } = setup({ run: async () => success({ collected }), finish });
    const out = await svc.importDocument(docx(), 'a.qmd');
    if (!out.ok) throw new Error('expected success');
    expect(out.media.map((m) => [m.projectPath, [...m.bytes], m.mimeType])).toEqual([
      ['a_media/bbbbbbbbbbbb.jpeg', [2, 2], inferMimeType('x.jpeg')],
      ['a_media/aaaaaaaaaaaa.png', [1], inferMimeType('x.png')],
    ]);
    const manifest = JSON.parse((wasm.finishImport.mock.calls[0] as unknown[])[3] as string);
    expect(manifest).toEqual([
      { pandoc_path: `${MEDIA}/media/a.png`, status: 'stored', sha256: 'sha-1', ext: 'png' },
      { pandoc_path: `${MEDIA}/media/b.jpeg`, status: 'stored', sha256: 'sha-2', ext: 'jpeg' },
    ]);
  });

  it('converts an EMF with the injected converter and passes the SVG to the plan', async () => {
    const collected: RequestFile[] = [{ path: `${MEDIA}/media/x.emf`, bytes: new Uint8Array([5, 5, 5]) }];
    const finish = { success: true, diagnostics: [], qmd: 'x', media_plan: [{ pandoc_path: `${MEDIA}/media/x.emf`, project_path: 'a_media/cccccccccccc.svg' }] };
    const { svc, deps, wasm } = setup({ run: async () => success({ collected }), finish });
    const out = await svc.importDocument(docx(), 'a.qmd');
    expect(deps.convertImage).toHaveBeenCalledWith(expect.any(Uint8Array), 'emf');
    if (!out.ok) throw new Error('expected success');
    expect([...out.media[0].bytes]).toEqual([0x89, 0x50]);
    expect(JSON.parse((wasm.finishImport.mock.calls[0] as unknown[])[3] as string)[0]).toMatchObject({ ext: 'svg', converted_from: 'emf' });
  });

  it('records a collect-limit warning as a skipped manifest entry and returns it among the diagnostics', async () => {
    const warning: Diagnostic = { origin: 'host', kind: 'warning', code: 'collect-limit', message: 'dropped', path: `${MEDIA}/media/big.bmp`, size: 30_000_000 };
    const { svc, wasm } = setup({ run: async () => success({ diagnostics: [warning] }) });
    const out = await svc.importDocument(docx(), 'a.qmd');
    expect(JSON.parse((wasm.finishImport.mock.calls[0] as unknown[])[3] as string)).toEqual([
      { pandoc_path: `${MEDIA}/media/big.bmp`, status: 'skipped', reason: 'too-large', size: 30_000_000 },
    ]);
    if (!out.ok) throw new Error('expected success');
    expect(out.diagnostics).toEqual([expect.objectContaining({ code: 'Q-24-4' }), warning]);
  });

  it('returns finish_import\'s Q-24-12 as a failure with its diagnostics', async () => {
    const { svc } = setup({ finish: { success: false, diagnostics: [rust('Q-24-12')] } });
    expect(await svc.importDocument(docx(), 'a.qmd')).toEqual({ ok: false, diagnostics: [expect.objectContaining({ code: 'Q-24-12' })] });
  });

  it('is a Q-24-12 when the plan names a file that was not stored', async () => {
    const finish = { success: true, diagnostics: [], qmd: 'x', media_plan: [{ pandoc_path: `${MEDIA}/ghost.png`, project_path: 'a_media/x.png' }] };
    const { svc } = setup({ finish });
    expect(await svc.importDocument(docx(), 'a.qmd')).toMatchObject({ ok: false, diagnostics: [{ code: 'Q-24-12' }] });
  });
});

describe('importDocument: run failures (the table in P4 T4)', () => {
  const failing = (kind: Parameters<typeof failure>[0], extra: Partial<ReturnType<typeof failure>> = {}) => async (): Promise<RunOutcome> => failure(kind, [], [], { status: 1, stderr: 'boom', ...extra });

  it.each([
    ['pandoc-exit', 'Q-24-3'],
    ['no-output', 'Q-24-3'],
    ['oom', 'Q-24-13'],
    ['crash', 'Q-24-13'],
    ['timeout', 'Q-24-13'],
    ['superseded', 'Q-24-12'],
  ] as const)('%s classifies to %s', async (kind, code) => {
    const { svc, wasm } = setup({ run: failing(kind) });
    const out = await svc.importDocument(docx(), 'a.qmd');
    expect(out).toEqual({ ok: false, diagnostics: [expect.objectContaining({ code })] });
    expect(wasm.classifyImportFailure).toHaveBeenCalledWith(kind, 1, 'boom');
  });

  it('invalid-request is Q-24-12 with the host\'s diagnostics appended', async () => {
    const host: Diagnostic = { origin: 'host', kind: 'error', code: 'input-mismatch', message: 'size differs' };
    const { svc } = setup({ run: async () => failure('invalid-request', [host], [], { stderr: '' }) });
    expect(await svc.importDocument(docx(), 'a.qmd')).toEqual({ ok: false, diagnostics: [expect.objectContaining({ code: 'Q-24-12' }), host] });
  });

  it('aborted is cancelled with no diagnostics, and does not call classify', async () => {
    const { svc, wasm } = setup({ run: failing('aborted') });
    expect(await svc.importDocument(docx(), 'a.qmd')).toEqual({ ok: false, cancelled: true, diagnostics: [] });
    expect(wasm.classifyImportFailure).not.toHaveBeenCalled();
  });

  it('load-failed and worker-blocked carry uiState from uiStateFor and the runner\'s own diagnostics', async () => {
    const host: Diagnostic = { origin: 'host', kind: 'error', code: 'offline', message: 'no network' };
    const loadError = new PandocLoadError('offline', 'no network');
    const a = setup({ run: async () => failure('load-failed', [host], [], { loadError }) });
    expect(await a.svc.importDocument(docx(), 'a.qmd')).toEqual({ ok: false, diagnostics: [host], uiState: 'offline' });
    const blocked: Diagnostic = { origin: 'host', kind: 'error', code: 'worker-blocked', message: 'blocked' };
    const b = setup({ run: async () => failure('worker-blocked', [blocked], []) });
    expect(await b.svc.importDocument(docx(), 'a.qmd')).toEqual({ ok: false, diagnostics: [blocked], uiState: 'blocked' });
    expect(a.wasm.classifyImportFailure).not.toHaveBeenCalled();
  });

  it('a file that cannot be read is an import-read-failed host diagnostic, and pandoc does not run', async () => {
    const { svc, run } = setup();
    const gone = { name: 'a.docx', size: 3, arrayBuffer: () => Promise.reject(new Error('NotReadableError')) } as unknown as File;
    const out = await svc.importDocument(gone, 'a.qmd');
    expect(out).toMatchObject({ ok: false, diagnostics: [{ origin: 'host', kind: 'error', code: 'import-read-failed' }] });
    expect(JSON.stringify(out)).toContain('NotReadableError');
    expect(run).not.toHaveBeenCalled();
  });
});

describe('importDocument: progress, cancellation and serialization', () => {
  it('reports reading, loading-pandoc, converting, images, finishing once each, in order, and passes onLoadProgress and signal through', async () => {
    const seen: ImportProgress[] = [];
    const onLoadProgress = vi.fn();
    const signal = new AbortController().signal;
    const { svc, run } = setup();
    await svc.importDocument(docx(), 'a.qmd', { onProgress: (p) => seen.push(p), onLoadProgress, signal });
    expect(seen).toEqual(['reading', 'loading-pandoc', 'converting', 'images', 'finishing']);
    const opts = run.mock.calls[0][2] as { onLoadProgress: unknown; signal: unknown };
    expect(opts.onLoadProgress).toBe(onLoadProgress);
    expect(opts.signal).toBe(signal);
  });

  it('an abort during image conversion returns cancelled, and finish_import is not called', async () => {
    const ctl = new AbortController();
    const collected: RequestFile[] = [{ path: `${MEDIA}/media/x.emf`, bytes: new Uint8Array([1]) }];
    const convertImage = vi.fn(async () => {
      ctl.abort();
      return new Uint8Array([1]);
    });
    const { svc, wasm } = setup({ run: async () => success({ collected }), deps: { convertImage } });
    expect(await svc.importDocument(docx(), 'a.qmd', { signal: ctl.signal })).toEqual({ ok: false, cancelled: true, diagnostics: [] });
    expect(wasm.finishImport).not.toHaveBeenCalled();
  });

  it('a signal already aborted cancels without doing anything', async () => {
    const ctl = new AbortController();
    ctl.abort();
    const { svc, wasm, run } = setup();
    expect(await svc.importDocument(docx(), 'a.qmd', { signal: ctl.signal })).toEqual({ ok: false, cancelled: true, diagnostics: [] });
    expect(wasm.prepareImport).not.toHaveBeenCalled();
    expect(run).not.toHaveBeenCalled();
  });

  it('serializes imports: the second run starts only after the first has finished', async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const events: string[] = [];
    let n = 0;
    const { svc } = setup({
      run: async () => {
        const me = ++n;
        events.push(`start ${me}`);
        if (me === 1) await gate;
        events.push(`end ${me}`);
        return success();
      },
    });
    const first = svc.importDocument(docx('a.docx'), 'a.qmd');
    const second = svc.importDocument(docx('b.docx'), 'b.qmd');
    await vi.waitFor(() => expect(events).toEqual(['start 1']));
    await new Promise((r) => setTimeout(r, 20));
    expect(events).toEqual(['start 1']);
    release();
    await Promise.all([first, second]);
    expect(events).toEqual(['start 1', 'end 1', 'start 2', 'end 2']);
  });
});

describe('availability and the default instance', () => {
  it('importAvailable is pandocWasmEnabled() && !isPreviewEmbed(), unlike downloadAvailable()', () => {
    const wasm = vi.spyOn(featureFlag, 'pandocWasmEnabled');
    const embed = vi.spyOn(featureFlag, 'isPreviewEmbed');
    for (const [w, e, want] of [
      [true, false, true],
      [true, true, false],
      [false, false, false],
      [false, true, false],
    ] as const) {
      wasm.mockReturnValue(w);
      embed.mockReturnValue(e);
      expect(importAvailable(), `wasm=${w} embed=${e}`).toBe(want);
    }
  });

  it('setImportServiceForTests installs a service and undefined restores the default', async () => {
    const stub = createStubImportService();
    setImportServiceForTests(stub);
    expect(getImportService()).toBe(stub);
    setImportServiceForTests(undefined);
    expect(getImportService()).not.toBe(stub);
  });
});
