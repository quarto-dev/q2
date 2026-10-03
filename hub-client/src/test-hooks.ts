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
import { PandocRunner, uiStateFor } from './pandoc/pandocRunner';
import { createBrowserWorker } from './pandoc/pandocService';
import { PANDOC_WASM_SHA256, smokeJob } from './pandoc/smokeJob';
import { installDevHarness, pandocDownload } from './pandoc/devHarness';
import { createPdfPreviewController, formatByKey, getDownloadController, onPdfCompiled } from './pandoc/downloadService';
import { mountPdfViewer } from './pandoc/pdfViewer';
import { createElement } from 'react';
import { createRoot } from 'react-dom/client';
import PdfPreviewPane from './components/render/PdfPreviewPane';
import { createTypstLoader, TypstFontsLoader, TYPST_WASM_SHA256 } from './typst/typstAssets';
import { splitTypstAssets } from './typst/typstAssetSplit';
import { TypstRunner, typstUiStateFor } from './typst/typstRunner';
import { createTypstBrowserWorker } from './typst/typstService';
import { cacheApiTarballs } from './typst/typstPackageCache';

/**
 * Pandoc loader/worker hook for the `pandoc-*.harness.spec.ts` smoke tests. The runner it
 * builds is the production one; the options exist so a test can shorten the wall timeout
 * (the hang case would otherwise take 120 s) and observe the loader.
 */
export const pandoc = {
  createRunner(options: { wallTimeoutMs?: number; idleMs?: number; loader?: LoaderConfig } = {}) {
    const loader = new PandocLoader({ idleMs: options.idleMs, ...options.loader });
    const runner = new PandocRunner({ loader, createWorker: createBrowserWorker, wallTimeoutMs: options.wallTimeoutMs });
    return { loader, runner };
  },
  smokeJob,
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
  mountPdfPreviewPane(path: string, content: string, debounceMs = 100) {
    const host = document.createElement('div');
    host.id = 'pdf-viewer-host';
    host.style.cssText = 'position:fixed;inset:0;z-index:99999;background:#fff';
    document.body.appendChild(host);
    const root = createRoot(host);
    const render = (text: string) => root.render(createElement(PdfPreviewPane, { path, content: text, debounceMs }));
    render(content);
    return { update: render };
  },
  /**
   * Time the whole PDF preview refresh (H10a Task 0(f)): the production preview controller and viewer, the two
   * objects `PdfPreviewPane` wires together, with a timestamp at every stage change and when the viewer has drawn
   * the new PDF. Each run rewrites the document (a changing last line) so that it is a real edit. Run 0 is
   * the cold run (module fetch and compile) and is returned like the others for the caller to discard.
   */
  async measurePdfRefresh(path: string, baseText: string, runs: number) {
    await wasmRenderer.initWasm();
    let host = document.getElementById('pdf-viewer-host');
    if (!host) {
      host = document.createElement('div');
      host.id = 'pdf-viewer-host';
      host.style.cssText = 'position:fixed;inset:0;z-index:99999;background:#fff';
      document.body.appendChild(host);
    }
    const viewer = mountPdfViewer(host);
    const format = formatByKey('pdf');
    if (!format) throw new Error('no pdf format');
    let tPdf = 0;
    let tShown = 0;
    let shown: Promise<void> = Promise.resolve();
    const controller = createPdfPreviewController((pdf, info) => {
      tPdf = performance.now();
      shown = viewer.show(pdf, { key: info.path, fileName: info.fileName }).then(() => {
        tShown = performance.now();
      });
    });
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
    viewer.dispose();
    return out;
  },
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
