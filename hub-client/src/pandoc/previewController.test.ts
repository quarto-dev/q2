/**
 * The preview controller's overlapping-runs mode (pandoc-host H10b Task 3): several runs of the same document
 * in flight at once, whose results are applied or dropped by the rules in `DownloadController`.
 * The pandoc and typst runners are scripted per call, so each test chooses the order in which runs finish.
 */
import { describe, expect, it, vi } from 'vitest';
import type { PandocRequest, ShareTree } from '@quarto/pandoc-host';
import { DownloadController, type DownloadFormat, type RequestEnvelope } from './downloadController';
import type { RunOptions, RunOutcome } from './pandocRunner';
import type { TypstJob, TypstRunOptions, TypstRunOutcome } from '../typst/typstRunner';

const PDF: DownloadFormat = { key: 'pdf', label: 'PDF', extension: 'pdf', mime: 'application/pdf' };
const tree: ShareTree = { share_tree_version: 'v1', files: [] };
const pdfRequest = {
  stage_name: 'pandoc',
  json_path: '/x.json',
  post: 'compile_typst',
  output_path: '/project/doc.typ',
  share_tree_path: '/__q2_share__/pandoc-share',
  env: { SOURCE_DATE_EPOCH: '1700000000' },
  files: [{ path: '/project/doc.qmd', bytes: new Uint8Array([1]) }],
  resource_refs: [],
} as unknown as PandocRequest;

const pandocOk = (): RunOutcome =>
  ({ ok: true, status: 0, output: new TextEncoder().encode('#set page()\n'), outputPath: '/project/doc.typ', stderr: '', stdout: '', diagnostics: [], stats: {}, notices: [] }) as unknown as RunOutcome;
const pandocFail = (kind = 'crash'): RunOutcome => ({ ok: false, kind, status: null, stderr: '', stdout: '', diagnostics: [], notices: [] }) as unknown as RunOutcome;
const typstOk = (tag = 0): TypstRunOutcome =>
  ({ ok: true, pdf: new Uint8Array([37, 80, 68, 70, tag]), pages: 1, diagnostics: [], stats: {}, notices: [], fontFamilies: [] }) as unknown as TypstRunOutcome;
const typstFail = (): TypstRunOutcome => ({ ok: false, kind: 'typst-error', diagnostics: [], notices: [] }) as unknown as TypstRunOutcome;

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

function harness() {
  const pandoc: { d: ReturnType<typeof deferred<RunOutcome>>; options: RunOptions }[] = [];
  const typst: { d: ReturnType<typeof deferred<TypstRunOutcome>>; options?: TypstRunOptions; job: TypstJob }[] = [];
  const pdfs: { pdf: Uint8Array; info: { path: string; fileName: string; seq?: number } }[] = [];
  const save = vi.fn();
  const controller = new DownloadController({
    overlap: true,
    buildRequest: async () => ({ success: true, diagnostics: [], stats: { unexecuted_cells: 0 }, request: structuredClone(pdfRequest) }) as RequestEnvelope,
    getShareTree: () => tree,
    runner: {
      run: (_r, _t, options = {}) => {
        const d = deferred<RunOutcome>();
        pandoc.push({ d, options });
        return d.promise;
      },
    },
    typst: {
      runner: {
        run: (job, options) => {
          const d = deferred<TypstRunOutcome>();
          typst.push({ d, options, job });
          return d.promise;
        },
        listFonts: async () => ({ ok: true, families: ['Inter'], notices: [] }),
      },
      assets: () => ({ vendoredPackages: [], fonts: [] }),
      datePrelude: () => '',
    },
    classify: () => ({ success: true, diagnostics: [] }),
    save,
    onPdf: (pdf, info) => pdfs.push({ pdf, info }),
    nowSeconds: () => 1700000000,
  });
  /** Start a run and wait until it is in pandoc (its request has been built). */
  const startRun = async (path = 'doc.qmd', projectKey?: string) => {
    const before = pandoc.length;
    const p = controller.start({ path, format: PDF, projectKey });
    await vi.waitFor(() => expect(pandoc.length).toBe(before + 1), { interval: 1 });
    return { index: before, done: p };
  };
  /** Finish run `i`'s pandoc leg and wait for its typst compile to be submitted. */
  const intoTypst = async (i: number) => {
    const before = typst.length;
    pandoc[i].d.resolve(pandocOk());
    await vi.waitFor(() => expect(typst.length).toBe(before + 1), { interval: 1 });
    return typst.length - 1;
  };
  return { controller, pandoc, typst, pdfs, save, startRun, intoTypst };
}

describe('overlap mode: starting runs', () => {
  it('a newer start of the same document does not abort the older run', async () => {
    const h = harness();
    await h.startRun('a.qmd', 'p1');
    await h.startRun('a.qmd', 'p1');
    expect(h.pandoc[0].options.signal?.aborted).toBe(false);
    expect(h.pandoc[1].options.signal?.aborted).toBe(false);
  });

  it('a start with a different path detaches the older run, whose result is never applied', async () => {
    const h = harness();
    const first = await h.startRun('a.qmd', 'p1');
    await h.startRun('b.qmd', 'p1');
    expect(h.pandoc[0].options.signal?.aborted).toBe(true);
    h.pandoc[0].d.resolve(pandocOk());
    await first.done;
    expect(h.typst).toHaveLength(0); // the detached run never reached typst
    expect(h.pdfs).toHaveLength(0);
  });

  it('a start in a different project with the same path detaches the older run too', async () => {
    const h = harness();
    const first = await h.startRun('index.qmd', 'p1');
    await h.startRun('index.qmd', 'p2');
    expect(h.pandoc[0].options.signal?.aborted).toBe(true);
    h.pandoc[0].d.resolve(pandocOk());
    await first.done;
    expect(h.typst).toHaveLength(0);
  });

  it('passes the document key (project and path) to the runner, and none without a project key', async () => {
    const h = harness();
    await h.startRun('a.qmd', 'p1');
    await h.startRun('a.qmd');
    expect(h.pandoc[0].options.docKey).toBe('p1\na.qmd');
    expect(h.pandoc[1].options.docKey).toBeUndefined();
  });

  it("an older run's stage and load progress do not touch the status of the newest", async () => {
    const h = harness();
    await h.startRun('a.qmd', 'p1');
    await h.startRun('a.qmd', 'p1');
    const before = h.controller.getSnapshot();
    h.pandoc[0].options.onStage?.('running');
    h.pandoc[0].options.onLoadProgress?.({ phase: 'download', loaded: 1, total: 2 });
    expect(h.controller.getSnapshot()).toEqual(before);
    h.pandoc[1].options.onStage?.('running');
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'working', stage: 'running' });
  });
});

describe('overlap mode: applying results', () => {
  it('out-of-order completion shows only the newest', async () => {
    const h = harness();
    const r0 = await h.startRun();
    const r1 = await h.startRun();
    const t1 = await h.intoTypst(r1.index);
    h.typst[t1].d.resolve(typstOk(2));
    await r1.done;
    expect(h.pdfs.map((p) => p.info.seq)).toEqual([2]);
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'done', clickId: 2 });
    // The older run now finishes its pandoc leg: it ends silently, without a compile.
    h.pandoc[r0.index].d.resolve(pandocOk());
    await r0.done;
    expect(h.typst).toHaveLength(1);
    expect(h.pdfs).toHaveLength(1);
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'done', clickId: 2 });
  });

  it('a slow old render that is already compiling and finishes after a newer one has been shown is dropped', async () => {
    const h = harness();
    const r0 = await h.startRun();
    const r1 = await h.startRun();
    const t0 = await h.intoTypst(r0.index);
    const t1 = await h.intoTypst(r1.index);
    h.typst[t1].d.resolve(typstOk(2));
    await r1.done;
    h.typst[t0].d.resolve(typstOk(1));
    await r0.done;
    expect(h.pdfs.map((p) => p.info.seq)).toEqual([2]);
  });

  it('an older render finishing its pandoc leg after a newer one has entered typst does not displace it', async () => {
    const h = harness();
    const r0 = await h.startRun();
    const r1 = await h.startRun();
    const t1 = await h.intoTypst(r1.index);
    h.pandoc[r0.index].d.resolve(pandocOk());
    await r0.done;
    expect(h.typst).toHaveLength(1); // the stage gate: no compile ahead of the newer one's
    h.typst[t1].d.resolve(typstOk(2));
    await r1.done;
    expect(h.pdfs.map((p) => p.info.seq)).toEqual([2]);
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'done' });
  });

  it("an older render finishing its pandoc leg while the newest is still in pandoc is shown, and does not leave the newest unrendered or the status on working", async () => {
    const h = harness();
    const r0 = await h.startRun();
    const r1 = await h.startRun();
    const t0 = await h.intoTypst(r0.index);
    h.typst[t0].d.resolve(typstOk(1));
    await r0.done;
    expect(h.pdfs.map((p) => p.info.seq)).toEqual([1]);
    // The newest has not settled: the status is still its `working`, not the older run's `done`.
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'working', clickId: 2 });
    const t1 = await h.intoTypst(r1.index);
    h.typst[t1].d.resolve(typstOk(2));
    await r1.done;
    expect(h.pdfs.map((p) => p.info.seq)).toEqual([1, 2]);
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'done', clickId: 2 });
  });

  it('the newest failing, then an older success finishing: the error stays', async () => {
    const h = harness();
    const r0 = await h.startRun();
    const r1 = await h.startRun();
    const t0 = await h.intoTypst(r0.index);
    h.pandoc[r1.index].d.resolve(pandocFail());
    await r1.done;
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'failed', clickId: 2 });
    h.typst[t0].d.resolve(typstOk(1));
    await r0.done;
    expect(h.pdfs).toHaveLength(0);
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'failed', clickId: 2 });
  });

  it('a typst failure of the newest run is shown; the previous PDF is not withdrawn', async () => {
    const h = harness();
    const r0 = await h.startRun();
    const t0 = await h.intoTypst(r0.index);
    h.typst[t0].d.resolve(typstOk(1));
    await r0.done;
    const r1 = await h.startRun();
    const t1 = await h.intoTypst(r1.index);
    h.typst[t1].d.resolve(typstFail());
    await r1.done;
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'failed', state: 'typst-error' });
    expect(h.pdfs.map((p) => p.info.seq)).toEqual([1]);
  });

  it('an older failure while a newer run is in flight is silent, and so is one after a newer success', async () => {
    const h = harness();
    const r0 = await h.startRun();
    const r1 = await h.startRun();
    h.pandoc[r0.index].d.resolve(pandocFail());
    await r0.done;
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'working', clickId: 2 });
    const t1 = await h.intoTypst(r1.index);
    h.typst[t1].d.resolve(typstOk(2));
    await r1.done;
    const r2 = await h.startRun();
    const r3 = await h.startRun();
    const t3 = await h.intoTypst(r3.index);
    h.typst[t3].d.resolve(typstOk(4));
    await r3.done;
    h.pandoc[r2.index].d.resolve(pandocFail());
    await r2.done;
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'done', clickId: 4 });
    expect(h.pdfs.map((p) => p.info.seq)).toEqual([2, 4]);
  });

  it('hands each shown PDF to onPdf exactly once, with its path, name and seq', async () => {
    const h = harness();
    const r0 = await h.startRun('dir/Doc.qmd');
    const t0 = await h.intoTypst(r0.index);
    h.typst[t0].d.resolve(typstOk(1));
    await r0.done;
    expect(h.pdfs).toHaveLength(1);
    expect(h.pdfs[0].info).toEqual({ path: 'dir/Doc.qmd', fileName: 'Doc.pdf', seq: 1 });
    expect(h.save).not.toHaveBeenCalled(); // the preview saves nothing
  });

  it('starts about 150 ms apart with runs finishing in a shuffled order never show a frame older than one already shown', async () => {
    // A seeded PRNG, so a failure reproduces.
    let seed = 12345;
    const rnd = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32);
    for (let round = 0; round < 25; round++) {
      const h = harness();
      const runs = [];
      for (let i = 0; i < 6; i++) runs.push(await h.startRun());
      // Finish pandoc and typst legs in a random interleaving; some pandoc legs fail.
      const pendingPandoc = new Set(runs.map((r) => r.index));
      const handled = new Set<number>(); // typst jobs already resolved
      let guard = 0;
      while ((pendingPandoc.size > 0 || [...Array(h.typst.length).keys()].some((j) => !handled.has(j))) && guard++ < 500) {
        const typstOpen = [...Array(h.typst.length).keys()].filter((j) => !handled.has(j));
        const pickTypst = typstOpen.length > 0 && (pendingPandoc.size === 0 || rnd() < 0.5);
        if (pickTypst) {
          const j = typstOpen[Math.floor(rnd() * typstOpen.length)];
          handled.add(j);
          h.typst[j].d.resolve(rnd() < 0.15 ? typstFail() : typstOk(j));
        } else {
          const open = [...pendingPandoc];
          const i = open[Math.floor(rnd() * open.length)];
          pendingPandoc.delete(i);
          h.pandoc[i].d.resolve(rnd() < 0.15 ? pandocFail() : pandocOk());
        }
        await new Promise((r) => setTimeout(r, 0));
      }
      await Promise.all(runs.map((r) => r.done));
      const seqs = h.pdfs.map((p) => p.info.seq);
      expect(seqs, `round ${round}`).toEqual([...seqs].sort((a, b) => a - b));
      expect(new Set(seqs).size).toBe(seqs.length);
      // The status is settled (nothing left on `working`).
      expect(h.controller.getSnapshot().phase, `round ${round}`).not.toBe('working');
    }
  });
});

describe('overlap mode: cancel and the pool', () => {
  it('cancel() after a result is shown still aborts an older run that is in flight, and leaves the status alone', async () => {
    const h = harness();
    await h.startRun();
    const r1 = await h.startRun();
    const t1 = await h.intoTypst(r1.index);
    h.typst[t1].d.resolve(typstOk(2));
    await r1.done;
    expect(h.controller.getSnapshot().phase).toBe('done');
    h.controller.cancel();
    expect(h.pandoc[0].options.signal?.aborted).toBe(true);
    expect(h.controller.getSnapshot()).toMatchObject({ phase: 'done', clickId: 2 });
  });

  it('cancel() while working aborts every live run and shows cancelled', async () => {
    const h = harness();
    await h.startRun();
    await h.startRun();
    h.controller.cancel();
    expect(h.pandoc.map((p) => p.options.signal?.aborted)).toEqual([true, true]);
    expect(h.controller.getSnapshot().phase).toBe('cancelled');
  });

  it('a cancelled run that completes later shows nothing', async () => {
    const h = harness();
    const r0 = await h.startRun();
    h.controller.cancel();
    h.pandoc[r0.index].d.resolve(pandocOk());
    await r0.done;
    expect(h.typst).toHaveLength(0);
    expect(h.pdfs).toHaveLength(0);
    expect(h.controller.getSnapshot().phase).toBe('cancelled');
  });

  it('acquire and release go to the pool, and are no-ops without one', () => {
    const pool = { acquire: vi.fn(), release: vi.fn() };
    const withPool = new DownloadController({ overlap: true, pool, save: vi.fn() });
    withPool.acquire();
    withPool.release();
    expect(pool.acquire).toHaveBeenCalledTimes(1);
    expect(pool.release).toHaveBeenCalledTimes(1);
    expect(() => {
      const none = new DownloadController({ save: vi.fn() });
      none.acquire();
      none.release();
    }).not.toThrow();
  });
});
