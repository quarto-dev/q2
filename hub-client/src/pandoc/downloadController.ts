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
import { download } from '../strings';
import type { FontListOutcome, TypstJob, TypstRunFailure, TypstRunOptions, TypstRunOutcome, TypstUiState } from '../typst/typstRunner';
import { typstUiStateFor } from '../typst/typstRunner';
import type { TypstFile } from '@quarto/typst-host';
import { TYPST_PDF_KEY } from './formatKeys';

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

/** Which chapter a whole-book render is on (`onProgress`'s arguments; `index` is 1-based). */
export interface ChapterProgress {
  index: number;
  total: number;
  file: string;
}

/** `typst-*` stages are the PDF chain's second half (and its font-list prelude); the others are pandoc's. */
export type DownloadStage = 'preparing' | 'chapter' | RunStage | 'native' | 'typst-loading' | 'typst-starting' | 'typst-compiling';

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
      /** Set (with stage `chapter`) while a whole book's chapters are being rendered. */
      chapter?: ChapterProgress;
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
      /** Set when the file is a whole book (`stats.book.scope === 'book'`). */
      book?: { chapters: number };
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
  stats?: { unexecuted_cells: number; book?: { scope: 'book' | 'chapter'; chapters: number } | null };
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
  /**
   * The family names for the pandoc request. Absent: the controller asks `runner.listFonts` with the assets' fonts.
   * The preview supplies a memoized one (`createFontMemo`), so overlapping runs do not queue a typst job each.
   */
  fontFamilies?: (options: TypstRunOptions) => Promise<FontListOutcome | TypstRunFailure>;
  /** The vendored packages and Font Awesome fonts (`get_typst_assets()`, split). */
  assets: () => { vendoredPackages: TypstFile[]; fonts: Uint8Array[] };
  /** First line of the `.typ`: pins the document date (`typst_date_prelude`). */
  datePrelude: (sourceDateEpoch: number) => string;
}

/** The sixth `buildRequest` argument, passed only when the click asked for a scope (a book chapter's menu items). */
export interface BuildRequestExtra {
  scope: 'auto' | 'chapter';
  /** Capture blobs by chapter path (sidecar keys); only for a whole-book click. */
  capturesByPath?: Record<string, Uint8Array>;
  /** Called before each chapter of a whole-book render. */
  onProgress?: (index: number, total: number, file: string) => void;
}

/** What `fetchCaptures` returns: the bytes that arrived, and the chapter paths that did not. */
export interface CaptureFetchResult {
  byPath: Record<string, Uint8Array>;
  failed: string[];
}

export interface DownloadDeps {
  /** Rust `render_pandoc_request`. Absent in the embed. */
  /** `signal` is the click's: it aborts the remote-image fetches (R6) when the click is cancelled or superseded. */
  /** `typstAvailableFonts` is passed (as a fifth argument) only for the PDF chain or a scoped click. */
  /** `extra` (a sixth argument) is passed only for a scoped click, so an ordinary click's call is unchanged. */
  buildRequest?: (
    path: string,
    format: string,
    sourceDateEpoch: number,
    signal: AbortSignal,
    typstAvailableFonts?: string[],
    extra?: BuildRequestExtra,
  ) => Promise<RequestEnvelope>;
  /** Fetches the capture bytes of a book's chapters (path to capture doc id) at click time; abort-aware. */
  fetchCaptures?: (docIds: Record<string, string>, signal: AbortSignal) => Promise<CaptureFetchResult>;
  getShareTree?: () => ShareTree;
  runner?: { run(request: PandocRequest, shareTree: ShareTree, options?: RunOptions): Promise<RunOutcome> };
  classify?: (stageName: string, success: boolean, status: string, stderr: string, jsonPath: string) => ClassifiedCompletion;
  /** The PDF chain's second half. Absent when typst is not shipped; a `pdf` download then fails as unavailable. */
  typst?: TypstChainDeps;
  /** The embed's executor (`renderNatively`); when set, pandoc.wasm is not used. */
  native?: (request: NativeRenderRequest, opts: { signal: AbortSignal }) => Promise<NativeRenderOutcome>;
  save: (blob: Blob, fileName: string) => void;
  /** Called with each compiled PDF's bytes (after `save`) so a viewer can show them (H9). */
  onPdf?: (pdf: Uint8Array, info: PdfInfo) => void;
  nowMs?: () => number;
  nowSeconds?: () => number;
  /**
   * Overlapping runs (the warm PDF preview, H10b): a new `start()` does not abort older runs of the same
   * document, and each run's result is applied or dropped by the rules in `applySuccess` and `failRun`.
   * Without it every guard is the single-run one ("Download as").
   */
  overlap?: boolean;
  /** A test seam (the E2E harness hooks): called when a run starts, when its PDF is shown and when it ends. Never set in production. */
  trace?: (event: TraceEvent) => void;
  /** The warm pool's lifetime (the preview's warm runner): the pane `acquire()`s it at mount and `release()`s it at unmount. */
  pool?: { acquire(): void; release(): void };
}

export interface StartOptions {
  path: string;
  format: DownloadFormat;
  /** The editor's current text; only the native executor uses it. */
  content?: string;
  /**
   * A book chapter's request scope: `'auto'` is the whole book for typst, pdf and epub ("Download book
   * as"), `'chapter'` the page alone ("This chapter only"). Absent: the controller's default (chapter).
   */
  scope?: 'auto' | 'chapter';
  /** For a whole-book click: chapter path (sidecar key) to capture doc id, for every chapter that has a capture. */
  captureDocIds?: Record<string, string>;
  /**
   * Which project the path belongs to (the preview passes `project.id`): two projects can share a path such as
   * `index.qmd`, and the pane stays mounted when the project changes. Download does not pass it.
   */
  projectKey?: string;
}

/** What `onPdf` is told about a compiled PDF; `seq` is the run's number (the preview shows no frame older than one already shown). */
export interface PdfInfo {
  path: string;
  fileName: string;
  seq: number;
}

/** What `DownloadDeps.trace` receives; `live` counts the runs in flight including the new one. */
export type TraceEvent = { type: 'start'; seq: number; live: number } | { type: 'shown'; seq: number } | { type: 'end'; seq: number };

/** One live run in overlap mode. */
interface Run {
  seq: number;
  path: string;
  projectKey: string | undefined;
  abort: AbortController;
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
  /** Overlap mode: the runs still live, the highest `seq` started, and the watermark (the highest `seq` whose result was shown). */
  private readonly runs = new Set<Run>();
  private latestStarted = 0;
  private latestRun: Run | undefined;
  /** The watermark: the highest `seq` whose success or failure was shown. */
  private lastSettled = 0;
  /** The highest `seq` that has submitted its typst compile (the stage gate). */
  private compileSubmitted = 0;

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

  /**
   * The user's cancel: abort the live render. In overlap mode every live run is aborted (detached), whatever the
   * status; when no run is `working` the status is left as it is.
   */
  cancel(): void {
    if (this.deps.overlap === true) {
      for (const r of this.runs) r.abort.abort(new Error('cancelled'));
      this.runs.clear();
    }
    const s = this.status;
    if (s.phase !== 'working') return;
    this.abort?.abort(new Error('cancelled'));
    this.abort = undefined;
    this.set({ phase: 'cancelled', clickId: s.clickId, format: s.format });
  }

  /** Take the warm pool (a refcount: the pane calls this at mount). A no-op without a pool. */
  acquire(): void {
    this.deps.pool?.acquire();
  }

  /** Give the pool back (the pane, at unmount); its workers linger for a while, then go. */
  release(): void {
    this.deps.pool?.release();
  }

  /** Render on click. A click while one is live supersedes it (in overlap mode: the older run keeps going). */
  async start(options: StartOptions): Promise<void> {
    const overlap = this.deps.overlap === true;
    const own = new AbortController();
    const id = ++this.clickId;
    const run: Run = { seq: id, path: options.path, projectKey: options.projectKey, abort: own };
    if (overlap) {
      // A file switch or a project change invalidates the work in flight: those runs are detached, never applied.
      for (const r of this.runs) {
        if (r.path !== run.path || r.projectKey !== run.projectKey) {
          r.abort.abort(new Error('superseded'));
          this.runs.delete(r);
        }
      }
      this.runs.add(run);
      this.latestStarted = id;
      this.latestRun = run;
    } else {
      this.abort?.abort(new Error('superseded'));
      this.abort = own;
    }
    this.deps.trace?.({ type: 'start', seq: id, live: overlap ? this.runs.size : 1 });
    const { format } = options;
    const current = overlap ? () => !own.signal.aborted : () => id === this.clickId && !own.signal.aborted;
    const fail = (state: FailureState, diagnostics: Diagnostic[], notices: string[] = [], message?: string) => {
      if (!current()) return;
      if (overlap) {
        // A failure is shown only by the newest started run: an older one is dropped silently, whether a newer
        // run is pending or has already shown a result. Showing it advances the watermark, so an older success
        // that finishes later is dropped and the banner stays.
        if (id !== this.latestStarted || id <= this.lastSettled) return;
        this.lastSettled = id;
      }
      this.set({ phase: 'failed', clickId: id, format, state, diagnostics, notices, message });
    };

    this.set({ phase: 'working', clickId: id, format, stage: this.deps.native ? 'native' : 'preparing' });
    try {
      if (this.deps.native) await this.runNative(options, id, own, current, fail);
      else await this.runWasm(options, run, own, current, fail);
    } catch (e) {
      // The Rust request build and the worker hand-off run on this thread, so a memory
      // failure there throws here rather than coming back as a host `oom` outcome.
      fail(looksLikeOom(e) ? 'out-of-memory' : 'crashed', [], [], e instanceof Error ? e.message : String(e));
    } finally {
      if (this.abort === own) this.abort = undefined;
      this.runs.delete(run);
      this.deps.trace?.({ type: 'end', seq: id });
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
    { path, format, scope, captureDocIds, projectKey }: StartOptions,
    run: Run,
    own: AbortController,
    current: () => boolean,
    fail: (state: FailureState, diagnostics: Diagnostic[], notices?: string[], message?: string) => void,
  ): Promise<void> {
    const id = run.seq;
    // In overlap mode only the newest started run drives the shared status; an older run's stage and progress events are ignored.
    const driving = () => current() && (this.deps.overlap !== true || id === this.latestStarted);
    const { buildRequest, getShareTree, runner, classify } = this.deps;
    if (!buildRequest || !getShareTree || !runner || !classify) throw new Error('pandoc.wasm is not available in this build');
    const nowSeconds = this.deps.nowSeconds ?? (() => Math.floor(Date.now() / 1000));
    const nowMs = this.deps.nowMs ?? (() => performance.now());

    const sourceDateEpoch = nowSeconds();
    const pdf = format.key === TYPST_PDF_KEY;
    let fontNotices: string[] = [];
    let families: string[] | undefined;
    if (pdf) {
      const typst = this.deps.typst;
      if (!typst) throw new Error('The PDF compiler is not available in this build');
      // The compiler is loaded first so its font families can reach the pandoc request (D8.6).
      // The runner takes ownership of (transfers) the buffers it is given, so each job gets its own copy.
      const fontOptions = this.typstOptions(own, 'typst-loading', nowMs, driving);
      const listed = await (typst.fontFamilies ? typst.fontFamilies(fontOptions) : typst.runner.listFonts(typst.assets().fonts, fontOptions));
      if (!current()) return;
      if (!listed.ok) {
        if (listed.kind === 'aborted' || listed.kind === 'superseded') return;
        return fail(typstUiStateFor(listed), tag('typst', listed.diagnostics), listed.notices);
      }
      families = listed.families;
      fontNotices = listed.notices;
      if (!current()) return;
      if (driving()) this.setStage('preparing');
    }

    let extra: BuildRequestExtra | undefined;
    let captureNotices: string[] = [];
    if (scope) {
      extra = { scope };
      if (scope === 'auto') {
        const ids = captureDocIds ?? {};
        if (this.deps.fetchCaptures && Object.keys(ids).length > 0) {
          const got = await this.deps.fetchCaptures(ids, own.signal);
          if (!current()) return;
          extra.capturesByPath = got.byPath;
          if (got.failed.length > 0) captureNotices = [download.captureFetchFailed(got.failed.length)];
        }
        extra.onProgress = (index, total, file) => {
          if (!current()) return;
          const s = this.status;
          if (s.phase === 'working') this.set({ ...s, stage: 'chapter', load: undefined, chapter: { index, total, file } });
        };
      }
    }

    const envelope = extra
      ? await buildRequest(path, format.key, sourceDateEpoch, own.signal, families, extra)
      : pdf
        ? await buildRequest(path, format.key, sourceDateEpoch, own.signal, families)
        : await buildRequest(path, format.key, sourceDateEpoch, own.signal);
    if (!current()) return;
    const requestDiagnostics = asDiagnostics(envelope.diagnostics);
    if (!envelope.request || !envelope.success || requestDiagnostics.some(isError)) {
      return fail('request-failed', requestDiagnostics, [], envelope.error);
    }
    const request = envelope.request as PandocRequest;
    const unexecutedCells = envelope.stats?.unexecuted_cells ?? 0;
    // A whole book is named after the book (its output path), not the chapter the click came from.
    const book = envelope.stats?.book?.scope === 'book' ? { chapters: envelope.stats.book.chapters } : undefined;
    const nameFrom = book ? request.output_path : path;
    const chain = pdf && request.post === 'compile_typst';
    if (pdf && !chain) throw new Error('The PDF request does not ask for a typst compile');

    // The pandoc run transfers (detaches) the request's buffers, and the compile needs the same files.
    const typstFiles: TypstFile[] = chain ? [...copy(request.files), ...copy(request.resource_refs)] : [];
    const shareTree = getShareTree();

    let lastProgress = -Infinity;
    const outcome = await runner.run(request, shareTree, {
      signal: own.signal,
      docKey: projectKey === undefined ? undefined : `${projectKey}\n${path}`,
      onStage: (stage) => {
        if (!driving()) return;
        const s = this.status;
        if (s.phase === 'working') this.set({ ...s, stage, load: stage === 'loading' ? s.load : undefined, chapter: undefined });
      },
      onLoadProgress: (load) => {
        if (!driving()) return;
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
      if (!driving()) return;
      const fileName = sanitizeDownloadName(nameFrom, format.extension);
      this.deps.save(new Blob([outcome.output as BlobPart], { type: format.mime }), fileName);
      this.set({ phase: 'done', clickId: id, format, fileName, warnings, notices: [...captureNotices, ...outcome.notices], unexecutedCells, book });
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
    if (this.deps.overlap === true) {
      // The stage gate: a run whose compile a newer run has already passed (submitted, or shown) ends silently,
      // so an older render that finishes its pandoc leg late cannot queue a compile ahead of a newer one's.
      if (this.compileSubmitted > id || this.lastSettled > id) return;
      this.compileSubmitted = id;
    }
    const compiled = await typst.runner.run(
      { input: { main, root: '/', files }, fonts, vendoredPackages },
      this.typstOptions(own, 'typst-loading', nowMs, driving),
    );
    if (!current()) return;
    const notices = [...captureNotices, ...outcome.notices, ...fontNotices, ...compiled.notices];
    if (!compiled.ok) {
      if (compiled.kind === 'aborted' || compiled.kind === 'superseded') return;
      return fail(typstUiStateFor(compiled), [...warnings, ...tag('typst', compiled.diagnostics)], notices);
    }
    const all = [...warnings, ...tag('typst', compiled.diagnostics)];
    if (all.some(isError)) return fail('typst-error', all, notices);

    const fileName = sanitizeDownloadName(nameFrom, format.extension);
    if (this.deps.overlap === true) {
      // A success is shown iff nothing at or after its `seq` has been shown and it is for the newest started
      // run's document; the viewer gets it exactly once. An older run's success does not end the newest run's
      // `working` status: only the newest run settles that.
      if (!this.mayShow(run)) return;
      this.deps.trace?.({ type: 'shown', seq: id });
      this.deps.onPdf?.(compiled.pdf, { path, fileName, seq: id });
      if (id === this.latestStarted) this.set({ phase: 'done', clickId: id, format, fileName, warnings: all, notices, unexecutedCells, book });
      return;
    }
    this.deps.save(new Blob([compiled.pdf as BlobPart], { type: format.mime }), fileName);
    this.deps.onPdf?.(compiled.pdf, { path, fileName, seq: id });
    this.set({ phase: 'done', clickId: id, format, fileName, warnings: all, notices, unexecutedCells, book });
  }

  /** Overlap mode: may `run`'s success be shown? If so, the watermark advances. */
  private mayShow(run: Run): boolean {
    const newest = this.latestRun;
    if (run.abort.signal.aborted || !newest || run.seq <= this.lastSettled || run.path !== newest.path || run.projectKey !== newest.projectKey) return false;
    this.lastSettled = run.seq;
    return true;
  }

  private setStage(stage: DownloadStage): void {
    const s = this.status;
    if (s.phase === 'working') this.set({ ...s, stage, load: undefined, chapter: undefined });
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
