/**
 * Dev-only harness (pandoc-host H3): the real chain, end to end, without any UI.
 *
 *   Rust `render_pandoc_request` (wasm, from the VFS) -> share tree -> `PandocRunner`
 *   (loader + worker + pandoc.wasm) -> output bytes -> a download.
 *
 * Installed as `window.q2PandocDownload` in `vite dev` and in `VITE_E2E=1` builds
 * (src/main.tsx); production bundles do not contain it. H5 builds the product UI on the
 * same two calls (`renderPandocRequest`, `PandocRunner.run`).
 *
 * From the browser console, with a project open (its files are already in the VFS):
 *
 *   await q2PandocDownload('/project/doc.qmd')             // saves doc.docx
 *   await q2PandocDownload('/project/doc.qmd', { save: false })  // just the outcome + timings
 */
import {
  getPandocShareTree,
  getPandocShareTreeVersion,
  initWasm,
  renderPandocRequest,
  type RenderPandocRequestResponse,
} from '@quarto/preview-runtime';
import type { Diagnostic, PandocRequest, ShareTree } from '@quarto/pandoc-host';
import type { PandocRunner, RunOutcome } from './pandocRunner';
import { getPandoc } from './pandocService';
import { sanitizeDownloadName } from './downloadName';
import { saveBlob } from './saveBlob';

export interface HarnessOptions {
  /** Pandoc format key from `getPandocFormats()`; default `docx`. */
  format?: string;
  /** Seconds since the epoch; default is the click time (what production does). */
  sourceDateEpoch?: number;
  /** Trigger a browser download of the output; default true. */
  save?: boolean;
  /** Runner to use; default is the app-wide one (so the module stays resident). */
  runner?: PandocRunner;
}

export interface HarnessTimings {
  /** Rust request build, in the main thread's wasm. */
  requestMs: number;
  /** Fetching the share tree from Rust (0 when the cached copy matched). */
  shareTreeMs: number;
  /** Runner: load + worker + mount + run + result transfer, as the caller sees it. */
  runnerMs: number;
  /** From the host core, when the run got that far. */
  instanceMs?: number;
  runMs?: number;
  /** pandoc's linear memory after the run. */
  memoryBytes?: number;
  mountedBytes?: number;
  /** Building the in-memory file tree (host core). */
  mountMs?: number;
  /**
   * Milliseconds from the start of the runner call to the first event of each load phase
   * (`download`, `cached`, `verify`, `compile`) and render stage (`loading`, `starting`,
   * `mounting`, `running`). Absent phases did not happen (a warm module has none).
   */
  phases: Record<string, number>;
  /** Total bytes of the request's files and resource refs, and of the share tree. */
  requestBytes: number;
  shareTreeBytes: number;
  shareTreeFiles: number;
}

export interface HarnessResult {
  ok: boolean;
  /** Set when the run produced output. */
  output?: Uint8Array;
  fileName?: string;
  /** Failure kind from the request build (`request`) or the runner. */
  failure?: string;
  diagnostics: Diagnostic[];
  /** Rust-side diagnostics and error from the request build. */
  requestError?: string;
  timings: HarnessTimings;
  notices: string[];
}

let cachedTree: ShareTree | undefined;

/** The share tree, re-read from Rust only when its version changes. */
function shareTree(): { tree: ShareTree; ms: number } {
  const t0 = performance.now();
  const version = getPandocShareTreeVersion();
  if (cachedTree?.share_tree_version === version) return { tree: cachedTree, ms: 0 };
  cachedTree = getPandocShareTree();
  return { tree: cachedTree, ms: performance.now() - t0 };
}

const sum = (files: { bytes: Uint8Array }[]) => files.reduce((n, f) => n + f.bytes.byteLength, 0);

const MIME: Record<string, string> = {
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
  epub: 'application/epub+zip',
  typ: 'text/plain',
};

/** Render the document at `path` (in the VFS) to `options.format` through pandoc.wasm. */
export async function pandocDownload(path: string, options: HarnessOptions = {}): Promise<HarnessResult> {
  const { format = 'docx', save = true } = options;
  const runner = options.runner ?? getPandoc().runner;
  const timings: HarnessTimings = { requestMs: 0, shareTreeMs: 0, runnerMs: 0, phases: {}, requestBytes: 0, shareTreeBytes: 0, shareTreeFiles: 0 };

  await initWasm();
  const t0 = performance.now();
  const envelope: RenderPandocRequestResponse = await renderPandocRequest(path, format, {
    sourceDateEpoch: options.sourceDateEpoch ?? Math.floor(Date.now() / 1000),
  });
  timings.requestMs = performance.now() - t0;
  if (!envelope.request) {
    return {
      ok: false,
      failure: 'request',
      requestError: envelope.error,
      diagnostics: envelope.diagnostics as unknown as Diagnostic[],
      timings,
      notices: [],
    };
  }
  const request = envelope.request as unknown as PandocRequest;
  timings.requestBytes = sum(request.files) + sum(request.resource_refs);

  const { tree, ms } = shareTree();
  timings.shareTreeMs = ms;
  timings.shareTreeBytes = sum(tree.files);
  timings.shareTreeFiles = tree.files.length;

  const t1 = performance.now();
  const mark = (name: string) => {
    timings.phases[name] ??= Math.round(performance.now() - t1);
  };
  const outcome: RunOutcome = await runner.run(request, tree, {
    onStage: (stage) => mark(stage),
    onLoadProgress: (p) => mark(p.phase),
  });
  timings.runnerMs = performance.now() - t1;
  const stats = 'stats' in outcome ? outcome.stats : undefined;
  if (stats) Object.assign(timings, stats);

  if (!outcome.ok) {
    return { ok: false, failure: outcome.kind, diagnostics: outcome.diagnostics, timings, notices: outcome.notices };
  }
  const fileName = sanitizeDownloadName(path, outcome.outputPath.split('.').pop() ?? 'out');
  if (save) {
    const ext = fileName.split('.').pop() ?? '';
    saveBlob(new Blob([outcome.output as BlobPart], { type: MIME[ext] ?? 'application/octet-stream' }), fileName);
  }
  return { ok: true, output: outcome.output, fileName, diagnostics: outcome.diagnostics, timings, notices: outcome.notices };
}

declare global {
  interface Window {
    q2PandocDownload?: typeof pandocDownload;
  }
}

export function installDevHarness(): void {
  window.q2PandocDownload = pandocDownload;
}
