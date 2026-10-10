/**
 * E2E test hooks: expose a couple of internal services on `window` so the
 * Playwright suite can bypass UI to seed projects (`projectStorage`) and
 * render qmd directly (`wasmRenderer`).
 *
 * Why this exists: the E2E suite used to reach into the source tree via
 * `await import('/src/services/...ts')`, which only works under `vite dev`.
 * The CI run uses `vite preview` (production bundle) for throughput; this
 * module is the bridge so the same tests work against the prod bundle.
 *
 * Inclusion is gated on `import.meta.env.VITE_E2E === '1'` at build time
 * (see `src/main.tsx`). Without that flag, vite tree-shakes this module
 * out of the production bundle entirely.
 */
import * as projectStorage from './services/projectStorage';
import * as projectSet from './services/projectSetService';
import { reconcileIntoConnectedProjectSet } from './services/projectSetReconciler';
import * as wasmRenderer from '@quarto/preview-runtime';
import { PandocLoader, type LoaderConfig } from './pandoc/pandocLoader';
import { PandocRunner, uiStateFor, type RunOutcome } from './pandoc/pandocRunner';
import { createBrowserWorker, getPandoc, getPreviewPandocRunner } from './pandoc/pandocService';
import type { Fault, PandocRequest, RunStats, ShareTree } from '@quarto/pandoc-host';
import { PANDOC_WASM_SHA256, smokeJob } from './pandoc/smokeJob';
import { importJob } from './pandoc/importJob';
import { convertMetafileToSvg } from './pandoc/metafileToSvg';
import { getImportService } from './pandoc/importService';
import { installDevHarness, pandocDownload } from './pandoc/devHarness';
import { createPdfPreviewController, formatByKey, getDownloadController, onPdfCompiled, setPreviewTrace } from './pandoc/downloadService';
import type { TraceEvent } from './pandoc/downloadController';
import { mountPdfViewer } from './pandoc/pdfViewer';
import { createElement } from 'react';
import { createRoot } from 'react-dom/client';
import PdfPreviewPane from './components/render/PdfPreviewPane';
import { createTypstLoader, TypstFontsLoader, TYPST_WASM_SHA256 } from './typst/typstAssets';
import { splitTypstAssets } from './typst/typstAssetSplit';
import { TypstRunner, typstUiStateFor } from './typst/typstRunner';
import { createTypstBrowserWorker } from './typst/typstService';
import { cacheApiTarballs } from './typst/typstPackageCache';
import { TYPST_PDF_KEY } from './pandoc/formatKeys';

const toHex = (b: ArrayBuffer): string => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, '0')).join('');

/** What a hook run reports about one render (bytes are reported as a SHA-256, so a page.evaluate result stays small). */
export interface HookRun {
  ok: boolean;
  kind?: string;
  status: number | null;
  outputSha?: string;
  outputBytes?: number;
  stderr: string;
  /** The runner's elapsed time around `run()` (the pandoc leg alone: no request build, no typst, no viewer). */
  elapsedMs: number;
  stats?: RunStats;
  diagnostics: unknown[];
}

let cachedShareTree: ShareTree | undefined;
const shareTreeOnce = (): ShareTree => (cachedShareTree ??= wasmRenderer.getPandocShareTree() as ShareTree);

/**
 * Hooks for the warm-path specs (`pandoc-warm.harness.spec.ts`, H10b Task 5). Each runs a request built by the
 * real Rust export through the warm runner (the preview's) or the fresh one (the app-wide), chosen per call.
 */
function warmHooks() {
  async function buildRequest(path: string, format: string, env?: Record<string, string>, fonts?: string[]): Promise<PandocRequest> {
    await wasmRenderer.initWasm();
    const envelope = await wasmRenderer.renderPandocRequest(path, format, { sourceDateEpoch: 1_700_000_000, typstAvailableFonts: fonts });
    if (!envelope.request) throw new Error(`no request for ${path}: ${JSON.stringify(envelope.diagnostics)} ${envelope.error ?? ''}`);
    const request = envelope.request as unknown as PandocRequest;
    if (env) request.env = { ...request.env, ...env };
    return request;
  }
  async function runOne(
    path: string,
    mode: 'warm' | 'fresh',
    options: { format?: string; env?: Record<string, string>; fault?: Fault; fonts?: string[]; wallTimeoutMs?: number; docKey?: string } = {},
  ): Promise<HookRun> {
    const request = await buildRequest(path, options.format ?? 'typst', options.env, options.fonts);
    const runner = mode === 'warm' ? getPreviewPandocRunner() : getPandoc().runner;
    const t0 = performance.now();
    const outcome: RunOutcome = await runner.run(request, shareTreeOnce(), { fault: options.fault, wallTimeoutMs: options.wallTimeoutMs, docKey: options.docKey });
    const elapsedMs = performance.now() - t0;
    if (outcome.ok) {
      return {
        ok: true,
        status: 0,
        outputSha: toHex(await crypto.subtle.digest('SHA-256', outcome.output as Uint8Array<ArrayBuffer>)),
        outputBytes: outcome.output.byteLength,
        stderr: outcome.stderr,
        elapsedMs,
        stats: outcome.stats,
        diagnostics: outcome.diagnostics,
      };
    }
    return { ok: false, kind: outcome.kind, status: outcome.status, stderr: outcome.stderr, elapsedMs, stats: outcome.stats, diagnostics: outcome.diagnostics };
  }
  return {
    /** One render of the document at `path` through the warm or the fresh runner. `env` overrides request env entries; `fault` is per run. */
    runRequest: runOne,
    /** `order` indexes `paths`: a seeded A,B,A sequence in one page round trip. `envs[i]` (optional) is step i's env override. */
    async runSequence(paths: string[], order: number[], mode: 'warm' | 'fresh', options: { format?: string; fonts?: string[]; envs?: (Record<string, string> | undefined)[] } = {}) {
      const out: (HookRun & { doc: number })[] = [];
      for (let i = 0; i < order.length; i++) out.push({ doc: order[i], ...(await runOne(paths[order[i]], mode, { format: options.format, fonts: options.fonts, env: options.envs?.[i] })) });
      return out;
    },
    /** `n` renders of one document (the 100-render memory loop; the pandoc-leg timing): per-run elapsed, stats and output digest. */
    async runLoop(path: string, n: number, mode: 'warm' | 'fresh', options: { format?: string; fonts?: string[] } = {}) {
      const out: HookRun[] = [];
      for (let i = 0; i < n; i++) out.push(await runOne(path, mode, options));
      return out;
    },
    /** The preview runner's pool counters. */
    warmRunnerStats() {
      const r = getPreviewPandocRunner();
      return { workerCount: r.workerCount, created: r.created, graceTerminations: r.graceTerminations, recycles: r.recycles };
    },
    /**
     * The real preview pane under a short debounce, edited at the given delays (ms after the previous edit; the
     * first is after the initial compile has shown a frame). Returns the log of frames shown `{ seq, tMs }`, of
     * run starts `{ seq, live, tMs }` (`live` counts the runs in flight including the new one), of edits and of
     * run ends, all on one clock (ms since the hook began), once every run has ended.
     */
    async typing(path: string, baseText: string, editDelaysMs: number[], options: { debounceMs: number; warm: boolean; projectKey?: string }) {
      await wasmRenderer.initWasm();
      const t0 = performance.now();
      const now = () => performance.now() - t0;
      const frames: { seq: number; tMs: number }[] = [];
      const starts: { seq: number; live: number; tMs: number }[] = [];
      const ends: { seq: number; tMs: number }[] = [];
      const edits: { tMs: number }[] = [];
      let live = 0;
      setPreviewTrace((e: TraceEvent) => {
        if (e.type === 'start') {
          live++;
          starts.push({ seq: e.seq, live: e.live, tMs: now() });
        } else if (e.type === 'shown') frames.push({ seq: e.seq, tMs: now() });
        else {
          live--;
          ends.push({ seq: e.seq, tMs: now() });
        }
      });
      wasmRenderer.vfsAddFile(path, baseText);
      const host = document.createElement('div');
      host.id = 'pdf-viewer-host';
      host.style.cssText = 'position:fixed;inset:0;z-index:99999;background:#fff';
      document.body.appendChild(host);
      const root = createRoot(host);
      const render = (text: string) => root.render(createElement(PdfPreviewPane, { path, content: text, debounceMs: options.debounceMs, projectKey: options.projectKey, warm: options.warm }));
      const until = async (pred: () => boolean, what: string, timeoutMs = 120_000) => {
        const start = performance.now();
        while (!pred()) {
          if (performance.now() - start > timeoutMs) throw new Error(`timed out waiting for ${what}`);
          await new Promise((r) => setTimeout(r, 20));
        }
      };
      try {
        render(baseText);
        await until(() => frames.length > 0, 'the first frame');
        for (let i = 0; i < editDelaysMs.length; i++) {
          await new Promise((r) => setTimeout(r, editDelaysMs[i]));
          const text = `${baseText}\n\nEdit ${i + 1}.\n`;
          wasmRenderer.vfsAddFile(path, text);
          edits.push({ tMs: now() });
          render(text);
        }
        // The last edit always starts a run once the pane's debounce fires, and nothing starts after that run.
        // Wait for that start, then for every run to end: a run's frame is shown before it ends. (A fixed sleep
        // of one debounce here returned early when the viewer's iframe, booting for the first frame on the same
        // thread, delayed the React commit and the debounce timer past it; bd-c72wsugj.)
        if (edits.length > 0) {
          const lastEdit = edits[edits.length - 1].tMs;
          await until(() => starts.some((s) => s.tMs >= lastEdit), 'a run to start for the last edit');
        }
        await until(() => live === 0, 'every run to end');
      } finally {
        setPreviewTrace(undefined);
        root.unmount();
        host.remove();
      }
      return { frames, starts, ends, edits };
    },
  };
}

/**
 * Pandoc loader/worker hook for the `pandoc-*.harness.spec.ts` smoke tests. The runner it
 * builds is the production one; the options exist so a test can shorten the wall timeout
 * (the hang case would otherwise take 120 s) and observe the loader.
 */
/** The runner `pandoc.runImport` keeps resident, so a series of imports loads the wasm once. */
let importRunner: { loader: PandocLoader; runner: PandocRunner } | undefined;

export const pandoc = {
  createRunner(options: { wallTimeoutMs?: number; idleMs?: number; loader?: LoaderConfig } = {}) {
    const loader = new PandocLoader({ idleMs: options.idleMs, ...options.loader });
    const runner = new PandocRunner({ loader, createWorker: createBrowserWorker, wallTimeoutMs: options.wallTimeoutMs });
    return { loader, runner };
  },
  smokeJob,
  /** EMF/WMF to SVG with rtf.js on this page (the import's image converter, P4 T2); base64 in, the SVG's text and size out. */
  async convertMetafile(base64: string, format: 'emf' | 'wmf'): Promise<{ svg: string; width: number; height: number }> {
    const out = await convertMetafileToSvg(Uint8Array.from(atob(base64), (c) => c.charCodeAt(0)), format);
    return { svg: new TextDecoder().decode(out.svg), width: out.width, height: out.height };
  },
  /**
   * `getImportService().importDocument` (the real service: Rust wasm, the import runner, the rtf.js converter) on
   * a file given as base64; media bytes come back as base64 so the result stays small and cloneable (P4 T6).
   */
  async importDocument(base64: string, fileName: string, targetQmdPath: string) {
    const toBase64 = (b: Uint8Array) => {
      let bin = '';
      for (const x of b) bin += String.fromCharCode(x);
      return btoa(bin);
    };
    const file = new File([Uint8Array.from(atob(base64), (c) => c.charCodeAt(0))], fileName);
    const out = await getImportService().importDocument(file, targetQmdPath);
    if (!out.ok) return { ok: false as const, cancelled: out.cancelled === true, diagnostics: out.diagnostics, uiState: out.uiState };
    return {
      ok: true as const,
      qmd: out.qmd,
      diagnostics: out.diagnostics,
      media: out.media.map((m) => ({ projectPath: m.projectPath, mimeType: m.mimeType, base64: toBase64(m.bytes) })),
    };
  },
  /**
   * One document-import run (interfaces 1 and 2, hand-built request): `base64` is the source file,
   * run through one resident runner so the wasm loads once. Reports what a memory measurement needs
   * (the runner's wall time around `run()`, the stats, how much was collected) without the bytes.
   */
  async runImport(base64: string, format = 'docx', options: { wallTimeoutMs?: number } = {}) {
    const source = Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
    importRunner ??= pandoc.createRunner();
    const { request, shareTree, inputs } = await importJob(source, format);
    const t0 = performance.now();
    const outcome: RunOutcome = await importRunner.runner.run(request, shareTree, { inputs, wallTimeoutMs: options.wallTimeoutMs });
    const elapsedMs = Math.round(performance.now() - t0);
    if (!outcome.ok) return { ok: false as const, kind: outcome.kind, status: outcome.status, stderr: outcome.stderr, elapsedMs, diagnostics: outcome.diagnostics };
    return {
      ok: true as const,
      elapsedMs,
      stats: outcome.stats,
      outputBytes: outcome.output.byteLength,
      collectedCount: outcome.collected.length,
      collectedBytes: outcome.collected.reduce((n, f) => n + f.bytes.byteLength, 0),
      diagnostics: outcome.diagnostics,
    };
  },
  /**
   * The production `DownloadController` (what the menu drives) for the document at `path` in the
   * VFS: resolves once the click's chain has finished or was cancelled. `cancelOnStage` cancels the
   * click when the status first reaches that stage (the abort-during-a-stage tests). The saved file
   * goes through the real `saveBlob`, so the page emits a download.
   */
  async startDownload(path: string, formatKey: string, options: { cancelOnStage?: string } = {}) {
    await wasmRenderer.initWasm();
    const format = formatByKey(formatKey);
    if (!format) throw new Error(`no such download format: ${formatKey}`);
    const controller = getDownloadController();
    const stages: string[] = [];
    const phases: string[] = [];
    const unsubscribe = controller.subscribe(() => {
      const s = controller.getSnapshot();
      phases.push(s.phase);
      if (s.phase !== 'working') return;
      if (stages[stages.length - 1] !== s.stage) stages.push(s.stage);
      if (options.cancelOnStage === s.stage) controller.cancel();
    });
    try {
      await controller.start({ path, format });
    } finally {
      unsubscribe();
    }
    const s = controller.getSnapshot();
    return {
      phase: s.phase,
      stages,
      cancelledCount: phases.filter((p) => p === 'cancelled').length,
      state: s.phase === 'failed' ? s.state : undefined,
      fileName: s.phase === 'done' ? s.fileName : undefined,
      diagnostics: s.phase === 'failed' ? s.diagnostics : s.phase === 'done' ? s.warnings : [],
    };
  },
  /**
   * The PDF viewer (H9) mounted into `#pdf-viewer-host` (created if missing) and fed by every
   * compiled PDF; the pdf.js app is then reachable at `iframe.contentWindow.PDFViewerApplication`.
   */
  mountPdfViewer() {
    let host = document.getElementById('pdf-viewer-host');
    if (!host) {
      host = document.createElement('div');
      host.id = 'pdf-viewer-host';
      host.style.cssText = 'position:fixed;inset:0;z-index:99999;background:#fff';
      document.body.appendChild(host);
    }
    const viewer = mountPdfViewer(host);
    onPdfCompiled((pdf, info) => void viewer.show(pdf, { key: info.path, fileName: info.fileName }));
    return viewer;
  },
  /**
   * The production PDF preview pane (third preview kind) in `#pdf-viewer-host`; `update(content)`
   * is an edit (the pane recompiles after its debounce). The viewer is the same iframe as above.
   */
  mountPdfPreviewPane(path: string, content: string, debounceMs = 100, projectKey?: string) {
    const host = document.createElement('div');
    host.id = 'pdf-viewer-host';
    host.style.cssText = 'position:fixed;inset:0;z-index:99999;background:#fff';
    document.body.appendChild(host);
    const root = createRoot(host);
    const render = (text: string) => root.render(createElement(PdfPreviewPane, { path, content: text, debounceMs, projectKey }));
    render(content);
    return { update: render };
  },
  /**
   * Time the whole PDF preview refresh (H10a Task 0(f)): the production preview controller and viewer, the two
   * objects `PdfPreviewPane` wires together, with a timestamp at every stage change and when the viewer has drawn
   * the new PDF. Each run rewrites the document (a changing last line) so that it is a real edit. Run 0 is
   * the cold run (module fetch and compile) and is returned like the others for the caller to discard.
   */
  async measurePdfRefresh(path: string, baseText: string, runs: number, options: { warm?: boolean } = {}) {
    await wasmRenderer.initWasm();
    let host = document.getElementById('pdf-viewer-host');
    if (!host) {
      host = document.createElement('div');
      host.id = 'pdf-viewer-host';
      host.style.cssText = 'position:fixed;inset:0;z-index:99999;background:#fff';
      document.body.appendChild(host);
    }
    const viewer = mountPdfViewer(host);
    const format = formatByKey(TYPST_PDF_KEY);
    if (!format) throw new Error('no pdf format');
    let tPdf = 0;
    let tShown = 0;
    let shown: Promise<void> = Promise.resolve();
    // `warm` chooses the path at run time (the build-time flag is not consulted), so one bundle compares both.
    const controller = createPdfPreviewController(
      (pdf, info) => {
        tPdf = performance.now();
        shown = viewer.show(pdf, { key: info.path, fileName: info.fileName }).then(() => {
          tShown = performance.now();
        });
      },
      { warm: options.warm === true },
    );
    controller.acquire();
    const marks: { stage: string; t: number }[] = [];
    controller.subscribe(() => {
      const s = controller.getSnapshot();
      const stage = s.phase === 'working' ? s.stage : s.phase;
      if (marks[marks.length - 1]?.stage !== stage) marks.push({ stage, t: performance.now() });
    });
    const out: { t0: number; marks: { stage: string; t: number }[]; tPdf: number; tShown: number; phase: string }[] = [];
    for (let i = 0; i <= runs; i++) {
      wasmRenderer.vfsAddFile(path, `${baseText}\n\nEdit ${i}.\n`);
      marks.length = 0;
      tPdf = tShown = 0;
      const t0 = performance.now();
      await controller.start({ path, format });
      await shown;
      out.push({ t0, marks: [...marks], tPdf, tShown, phase: controller.getSnapshot().phase });
    }
    controller.cancel();
    controller.release();
    viewer.dispose();
    return out;
  },
  ...warmHooks(),
  /** The dev harness: Rust request -> worker -> output (src/pandoc/devHarness.ts). */
  download: pandocDownload,
  uiStateFor,
  sha256: PANDOC_WASM_SHA256,
  /**
   * Byte length of the main thread's Rust wasm linear memory (the VFS, request builds and
   * their copies live there). It only ever grows, so it is the high-water mark of the page
   * (host phase H6 memory test). `default()` returns the cached exports once initialised.
   */
  async rustWasmMemoryBytes(): Promise<number> {
    const wasm = await import('wasm-quarto-hub-client');
    const exports = (await wasm.default()) as unknown as { memory: WebAssembly.Memory };
    return exports.memory.buffer.byteLength;
  },
};

/**
 * Typst loader/worker hook for the `typst-*.harness.spec.ts` tests (host phase H7). The
 * runner is the production one; the options exist so a test can shorten the wall timeout and
 * the idle drop.
 */
export const typst = {
  createRunner(options: { wallTimeoutMs?: number; idleMs?: number } = {}) {
    const loader = createTypstLoader({ idleMs: options.idleMs });
    const fonts = new TypstFontsLoader({ idleMs: options.idleMs });
    const runner = new TypstRunner({ loader, fonts, createWorker: createTypstBrowserWorker, wallTimeoutMs: options.wallTimeoutMs, cache: cacheApiTarballs() });
    return { loader, fonts, runner };
  },
  /** The vendored packages and Font Awesome fonts from the Rust wasm's `get_typst_assets()`. */
  async vendoredAssets() {
    await wasmRenderer.initWasm();
    return splitTypstAssets(wasmRenderer.getTypstAssets().files);
  },
  uiStateFor: typstUiStateFor,
  sha256: TYPST_WASM_SHA256,
};

declare global {
  interface Window {
    __quartoTest?: {
      projectStorage: typeof projectStorage;
      // The live project-set service singleton (same instance the app uses),
      // so the E2E suite can observe real connection/sync state — e.g. wait
      // for `isConnected()` and `getProject(indexDocId)` after seeding before
      // navigating, instead of racing the implicit reconcile-on-connect.
      projectSet: typeof projectSet;
      // Idempotent IDB→synced-set reconciler. The app runs this only on the
      // status→connected transition, which does not re-fire for a project
      // seeded after the set is already connected; the suite invokes it
      // explicitly so a seeded project deterministically lands in the set.
      reconcileProjectSet: typeof reconcileIntoConnectedProjectSet;
      wasmRenderer: typeof wasmRenderer;
      pandoc: typeof pandoc;
      typst: typeof typst;
    };
  }
}

installDevHarness();

window.__quartoTest = {
  projectStorage,
  projectSet,
  reconcileProjectSet: reconcileIntoConnectedProjectSet,
  wasmRenderer,
  pandoc,
  typst,
};
