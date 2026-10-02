/**
 * The "Download as" state machine (pandoc-host H5; design D8.5, failure taxonomy).
 *
 * Framework-free so it can be tested without React or a browser: the UI subscribes through
 * `useSyncExternalStore`. One controller per page. Each `start()` takes a click id; every
 * await re-checks it, so a response that belongs to an earlier click is dropped (and the
 * runner itself supersedes the previous live render). A document with an error diagnostic
 * or a failed run produces no Blob and no download; warnings still download.
 *
 * Two executors, one state: pandoc.wasm in the browser (`runner`, `buildRequest`), or the
 * preview server's native render in the `q2 preview` embed (`native`).
 */
import { looksLikeOom } from '@quarto/pandoc-host';
import type { Diagnostic as PandocDiagnostic, PandocRequest, ShareTree } from '@quarto/pandoc-host';
import type { Diagnostic as TypstDiagnostic } from '@quarto/typst-host';
import type { LoadProgress } from './pandocLoader';
import type { RunOutcome, RunStage, UiState, RunOptions } from './pandocRunner';
import { uiStateFor } from './pandocRunner';
import type { NativeRenderOutcome, NativeRenderRequest } from './nativeRender';
import { sanitizeDownloadName } from './downloadName';
import type { FontListOutcome, TypstJob, TypstRunFailure, TypstRunOptions, TypstRunOutcome, TypstUiState } from '../typst/typstRunner';
import { typstUiStateFor } from '../typst/typstRunner';
import type { TypstFile } from '@quarto/typst-host';

/** One channel for both stages of a chain: pandoc's diagnostics (Rust or host) and typst's. */
export type Diagnostic = PandocDiagnostic | TypstDiagnostic;

/** What the controller needs to know about the chosen format (a row of the Rust-owned table). */
export interface DownloadFormat {
  key: string;
  label: string;
  /** No dot. */
  extension: string;
  mime: string;
}

/** `typst-*` stages are the PDF chain's second half (and its font-list prelude); the others are pandoc's. */
export type DownloadStage = 'preparing' | RunStage | 'native' | 'typst-loading' | 'typst-starting' | 'typst-compiling';

export type FailureState = UiState | TypstUiState | 'request-failed' | 'native-failed' | 'native-error';

export type DownloadStatus =
  | { phase: 'idle' }
  | {
      phase: 'working';
      clickId: number;
      format: DownloadFormat;
      stage: DownloadStage;
      /** Set while the pandoc.wasm download or compile is in progress. */
      load?: LoadProgress;
    }
  | {
      phase: 'done';
      clickId: number;
      format: DownloadFormat;
      fileName: string;
      /** Every warning: the request's, pandoc's (classified Q-11-1) and the host's. */
      warnings: Diagnostic[];
      notices: string[];
      unexecutedCells: number;
    }
  | {
      phase: 'failed';
      clickId: number;
      format: DownloadFormat;
      state: FailureState;
      diagnostics: Diagnostic[];
      /** Plain-language text when there is no diagnostic to carry it. */
      message?: string;
      notices: string[];
    }
  | { phase: 'cancelled'; clickId: number; format: DownloadFormat };

export interface RequestEnvelope {
  success: boolean;
  error?: string;
  diagnostics: unknown[];
  stats?: { unexecuted_cells: number };
  request?: unknown;
}

export interface ClassifiedCompletion {
  success: boolean;
  diagnostics: unknown[];
}

/** What the PDF chain needs from the typst side (`getTypst().runner`, `splitTypstAssets`, `typstDatePrelude`). */
export interface TypstChainDeps {
  runner: {
    run(job: TypstJob, options?: TypstRunOptions): Promise<TypstRunOutcome>;
    listFonts(fonts?: Uint8Array[], options?: TypstRunOptions): Promise<FontListOutcome | TypstRunFailure>;
  };
  /** The vendored packages and Font Awesome fonts (`get_typst_assets()`, split). */
  assets: () => { vendoredPackages: TypstFile[]; fonts: Uint8Array[] };
  /** First line of the `.typ`: pins the document date (`typst_date_prelude`). */
  datePrelude: (sourceDateEpoch: number) => string;
}

export interface DownloadDeps {
  /** Rust `render_pandoc_request`. Absent in the embed. */
  /** `signal` is the click's: it aborts the remote-image fetches (R6) when the click is cancelled or superseded. */
  /** `typstAvailableFonts` is passed (as a fifth argument) only for the PDF chain. */
  buildRequest?: (path: string, format: string, sourceDateEpoch: number, signal: AbortSignal, typstAvailableFonts?: string[]) => Promise<RequestEnvelope>;
  getShareTree?: () => ShareTree;
  runner?: { run(request: PandocRequest, shareTree: ShareTree, options?: RunOptions): Promise<RunOutcome> };
  classify?: (stageName: string, success: boolean, status: string, stderr: string, jsonPath: string) => ClassifiedCompletion;
  /** The PDF chain's second half. Absent when typst is not shipped; a `pdf` download then fails as unavailable. */
  typst?: TypstChainDeps;
  /** The embed's executor (`renderNatively`); when set, pandoc.wasm is not used. */
  native?: (request: NativeRenderRequest, opts: { signal: AbortSignal }) => Promise<NativeRenderOutcome>;
  save: (blob: Blob, fileName: string) => void;
  /** Called with each compiled PDF's bytes (after `save`) so a viewer can show them (H9). */
  onPdf?: (pdf: Uint8Array, info: { path: string; fileName: string }) => void;
  nowMs?: () => number;
  nowSeconds?: () => number;
}

export interface StartOptions {
  path: string;
  format: DownloadFormat;
  /** The editor's current text; only the native executor uses it. */
  content?: string;
}

/** Progress events closer together than this are folded (phase changes always pass). */
export const PROGRESS_THROTTLE_MS = 100;

const isError = (d: unknown): boolean => (d as { kind?: string } | null)?.kind === 'error';
const asDiagnostics = (list: unknown[] | undefined): Diagnostic[] => (list ?? []) as Diagnostic[];
/** Tag diagnostics with the chain stage that raised them (H8); an existing tag is kept. */
const tag = (stage: 'pandoc' | 'typst', list: Diagnostic[]): Diagnostic[] => list.map((d) => (d.stage ? d : ({ ...d, stage } as Diagnostic)));
const copy = (files: { path: string; bytes: Uint8Array }[]): TypstFile[] => files.map((f) => ({ path: f.path, bytes: f.bytes.slice() }));
const concat = (a: Uint8Array, b: Uint8Array): Uint8Array => {
  const out = new Uint8Array(a.length + b.length);
  out.set(a, 0);
  out.set(b, a.length);
  return out;
};
const textEncoder = new TextEncoder();

export class DownloadController {
  private status: DownloadStatus = { phase: 'idle' };
  private readonly listeners = new Set<() => void>();
  private clickId = 0;
  private abort: AbortController | undefined;

  private readonly deps: DownloadDeps;

  constructor(deps: DownloadDeps) {
    this.deps = deps;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): DownloadStatus => this.status;

  /** True while a render is live. */
  get busy(): boolean {
    return this.status.phase === 'working';
  }

  /** Back to idle (dismiss a finished or failed status). Does not stop a live render. */
  dismiss(): void {
    if (this.status.phase !== 'working') this.set({ phase: 'idle' });
  }

  /** The user's cancel: abort the live render. */
  cancel(): void {
    const s = this.status;
    if (s.phase !== 'working') return;
    this.abort?.abort(new Error('cancelled'));
    this.abort = undefined;
    this.set({ phase: 'cancelled', clickId: s.clickId, format: s.format });
  }

  /** Render on click. A click while one is live supersedes it. */
  async start(options: StartOptions): Promise<void> {
    this.abort?.abort(new Error('superseded'));
    const own = new AbortController();
    this.abort = own;
    const id = ++this.clickId;
    const { format } = options;
    const current = () => id === this.clickId && !own.signal.aborted;
    const fail = (state: FailureState, diagnostics: Diagnostic[], notices: string[] = [], message?: string) => {
      if (current()) this.set({ phase: 'failed', clickId: id, format, state, diagnostics, notices, message });
    };

    this.set({ phase: 'working', clickId: id, format, stage: this.deps.native ? 'native' : 'preparing' });
    try {
      if (this.deps.native) await this.runNative(options, id, own, current, fail);
      else await this.runWasm(options, id, own, current, fail);
    } catch (e) {
      // The Rust request build and the worker hand-off run on this thread, so a memory
      // failure there throws here rather than coming back as a host `oom` outcome.
      fail(looksLikeOom(e) ? 'out-of-memory' : 'crashed', [], [], e instanceof Error ? e.message : String(e));
    } finally {
      if (this.abort === own) this.abort = undefined;
    }
  }

  private async runNative(
    { path, format, content }: StartOptions,
    id: number,
    own: AbortController,
    current: () => boolean,
    fail: (state: FailureState, diagnostics: Diagnostic[], notices?: string[], message?: string) => void,
  ): Promise<void> {
    const outcome = await this.deps.native!({ path, format: format.key, content: content ?? '' }, { signal: own.signal });
    if (!current()) return;
    if (outcome.kind === 'failed') return fail('native-failed', asDiagnostics(outcome.diagnostics as unknown[]), [], outcome.error);
    if (outcome.kind === 'error') return fail('native-error', [], [], outcome.message);
    const warnings = asDiagnostics(outcome.warnings as unknown[]);
    if (warnings.some(isError)) return fail('native-failed', warnings);
    const fileName = outcome.fileName || sanitizeDownloadName(path, format.extension);
    this.deps.save(outcome.blob, fileName);
    this.set({ phase: 'done', clickId: id, format, fileName, warnings, notices: [], unexecutedCells: 0 });
  }

  private async runWasm(
    { path, format }: StartOptions,
    id: number,
    own: AbortController,
    current: () => boolean,
    fail: (state: FailureState, diagnostics: Diagnostic[], notices?: string[], message?: string) => void,
  ): Promise<void> {
    const { buildRequest, getShareTree, runner, classify } = this.deps;
    if (!buildRequest || !getShareTree || !runner || !classify) throw new Error('pandoc.wasm is not available in this build');
    const nowSeconds = this.deps.nowSeconds ?? (() => Math.floor(Date.now() / 1000));
    const nowMs = this.deps.nowMs ?? (() => performance.now());

    const sourceDateEpoch = nowSeconds();
    const pdf = format.key === 'pdf';
    let fontNotices: string[] = [];
    let families: string[] | undefined;
    if (pdf) {
      const typst = this.deps.typst;
      if (!typst) throw new Error('The PDF compiler is not available in this build');
      // The compiler is loaded first so its font families can reach the pandoc request (D8.6).
      // The runner takes ownership of (transfers) the buffers it is given, so each job gets its own copy.
      const listed = await typst.runner.listFonts(typst.assets().fonts, this.typstOptions(own, 'typst-loading', nowMs, current));
      if (!current()) return;
      if (!listed.ok) {
        if (listed.kind === 'aborted' || listed.kind === 'superseded') return;
        return fail(typstUiStateFor(listed), tag('typst', listed.diagnostics), listed.notices);
      }
      families = listed.families;
      fontNotices = listed.notices;
      if (!current()) return;
      this.setStage('preparing');
    }

    const envelope = pdf
      ? await buildRequest(path, format.key, sourceDateEpoch, own.signal, families)
      : await buildRequest(path, format.key, sourceDateEpoch, own.signal);
    if (!current()) return;
    const requestDiagnostics = asDiagnostics(envelope.diagnostics);
    if (!envelope.request || !envelope.success || requestDiagnostics.some(isError)) {
      return fail('request-failed', requestDiagnostics, [], envelope.error);
    }
    const request = envelope.request as PandocRequest;
    const unexecutedCells = envelope.stats?.unexecuted_cells ?? 0;
    const chain = pdf && request.post === 'compile_typst';
    if (pdf && !chain) throw new Error('The PDF request does not ask for a typst compile');

    // The pandoc run transfers (detaches) the request's buffers, and the compile needs the same files.
    const typstFiles: TypstFile[] = chain ? [...copy(request.files), ...copy(request.resource_refs)] : [];
    const shareTree = getShareTree();

    let lastProgress = -Infinity;
    const outcome = await runner.run(request, shareTree, {
      signal: own.signal,
      onStage: (stage) => {
        if (!current()) return;
        const s = this.status;
        if (s.phase === 'working') this.set({ ...s, stage, load: stage === 'loading' ? s.load : undefined });
      },
      onLoadProgress: (load) => {
        if (!current()) return;
        const s = this.status;
        if (s.phase !== 'working') return;
        const t = nowMs();
        const phaseChanged = s.load?.phase !== load.phase;
        if (!phaseChanged && t - lastProgress < PROGRESS_THROTTLE_MS) return;
        lastProgress = t;
        this.set({ ...s, load });
      },
    });
    if (!current()) return;

    if (!outcome.ok) {
      // A user cancel already set its own status; a supersession is the newer click's business.
      if (outcome.kind === 'aborted' || outcome.kind === 'superseded') return;
      let diagnostics: Diagnostic[] = outcome.diagnostics;
      if (outcome.kind === 'pandoc-exit') {
        diagnostics = asDiagnostics(classify(request.stage_name, false, `exit status: ${outcome.status ?? 'unknown'}`, outcome.stderr, request.json_path).diagnostics);
      }
      return fail(uiStateFor(outcome), chain ? tag('pandoc', diagnostics) : diagnostics, outcome.notices);
    }

    const completion = classify(request.stage_name, true, 'exit status: 0', outcome.stderr, request.json_path);
    const pandocDiagnostics = [...requestDiagnostics, ...asDiagnostics(completion.diagnostics), ...outcome.diagnostics];
    const warnings = chain ? tag('pandoc', pandocDiagnostics) : pandocDiagnostics;
    if (!completion.success || warnings.some(isError)) return fail('pandoc-error', warnings, outcome.notices);

    if (!chain) {
      const fileName = sanitizeDownloadName(path, format.extension);
      this.deps.save(new Blob([outcome.output as BlobPart], { type: format.mime }), fileName);
      this.set({ phase: 'done', clickId: id, format, fileName, warnings, notices: outcome.notices, unexecutedCells });
      return;
    }

    // Stage two: the typst worker compiles the `.typ` against the tree pandoc saw.
    const typst = this.deps.typst!;
    const prelude = textEncoder.encode(typst.datePrelude(Number(request.env.SOURCE_DATE_EPOCH ?? sourceDateEpoch)));
    const main = request.output_path;
    const files: TypstFile[] = [
      ...shareTree.files.map((f) => ({ path: `${request.share_tree_path}/${f.path}`, bytes: f.bytes.slice() })),
      ...typstFiles.filter((f) => f.path !== main),
      { path: main, bytes: concat(prelude, outcome.output) },
    ];
    const { vendoredPackages, fonts } = typst.assets();
    const compiled = await typst.runner.run(
      { input: { main, root: '/', files }, fonts, vendoredPackages },
      this.typstOptions(own, 'typst-loading', nowMs, current),
    );
    if (!current()) return;
    const notices = [...outcome.notices, ...fontNotices, ...compiled.notices];
    if (!compiled.ok) {
      if (compiled.kind === 'aborted' || compiled.kind === 'superseded') return;
      return fail(typstUiStateFor(compiled), [...warnings, ...tag('typst', compiled.diagnostics)], notices);
    }
    const all = [...warnings, ...tag('typst', compiled.diagnostics)];
    if (all.some(isError)) return fail('typst-error', all, notices);

    const fileName = sanitizeDownloadName(path, format.extension);
    this.deps.save(new Blob([compiled.pdf as BlobPart], { type: format.mime }), fileName);
    this.deps.onPdf?.(compiled.pdf, { path, fileName });
    this.set({ phase: 'done', clickId: id, format, fileName, warnings: all, notices, unexecutedCells });
  }

  private setStage(stage: DownloadStage): void {
    const s = this.status;
    if (s.phase === 'working') this.set({ ...s, stage, load: undefined });
  }

  /** Runner options for a typst job: its stages and load progress land in the shared status. */
  private typstOptions(own: AbortController, first: DownloadStage, nowMs: () => number, current: () => boolean): TypstRunOptions {
    let last = -Infinity;
    return {
      signal: own.signal,
      onStage: (stage) => {
        if (!current()) return;
        this.setStage(stage === 'compiling' ? 'typst-compiling' : stage === 'starting' ? 'typst-starting' : first);
      },
      onLoadProgress: (load) => {
        if (!current()) return;
        const s = this.status;
        if (s.phase !== 'working') return;
        const t = nowMs();
        const phaseChanged = s.load?.phase !== load.phase;
        if (!phaseChanged && t - last < PROGRESS_THROTTLE_MS) return;
        last = t;
        this.set({ ...s, load });
      },
    };
  }

  private set(status: DownloadStatus): void {
    this.status = status;
    for (const l of [...this.listeners]) l();
  }
}
