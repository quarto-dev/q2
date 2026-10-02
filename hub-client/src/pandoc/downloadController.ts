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
import type { Diagnostic, PandocRequest, ShareTree } from '@quarto/pandoc-host';
import type { LoadProgress } from './pandocLoader';
import type { RunOutcome, RunStage, UiState, RunOptions } from './pandocRunner';
import { uiStateFor } from './pandocRunner';
import type { NativeRenderOutcome, NativeRenderRequest } from './nativeRender';
import { sanitizeDownloadName } from './downloadName';

/** What the controller needs to know about the chosen format (a row of the Rust-owned table). */
export interface DownloadFormat {
  key: string;
  label: string;
  /** No dot. */
  extension: string;
  mime: string;
}

export type DownloadStage = 'preparing' | RunStage | 'native';

export type FailureState = UiState | 'request-failed' | 'native-failed' | 'native-error';

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

export interface DownloadDeps {
  /** Rust `render_pandoc_request`. Absent in the embed. */
  /** `signal` is the click's: it aborts the remote-image fetches (R6) when the click is cancelled or superseded. */
  buildRequest?: (path: string, format: string, sourceDateEpoch: number, signal: AbortSignal) => Promise<RequestEnvelope>;
  getShareTree?: () => ShareTree;
  runner?: { run(request: PandocRequest, shareTree: ShareTree, options?: RunOptions): Promise<RunOutcome> };
  classify?: (stageName: string, success: boolean, status: string, stderr: string, jsonPath: string) => ClassifiedCompletion;
  /** The embed's executor (`renderNatively`); when set, pandoc.wasm is not used. */
  native?: (request: NativeRenderRequest, opts: { signal: AbortSignal }) => Promise<NativeRenderOutcome>;
  save: (blob: Blob, fileName: string) => void;
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
      fail('crashed', [], [], e instanceof Error ? e.message : String(e));
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

    const envelope = await buildRequest(path, format.key, nowSeconds(), own.signal);
    if (!current()) return;
    const requestDiagnostics = asDiagnostics(envelope.diagnostics);
    if (!envelope.request || !envelope.success || requestDiagnostics.some(isError)) {
      return fail('request-failed', requestDiagnostics, [], envelope.error);
    }
    const request = envelope.request as PandocRequest;
    const unexecutedCells = envelope.stats?.unexecuted_cells ?? 0;

    let lastProgress = -Infinity;
    const outcome = await runner.run(request, getShareTree(), {
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
      let diagnostics = outcome.diagnostics;
      if (outcome.kind === 'pandoc-exit') {
        diagnostics = asDiagnostics(classify(request.stage_name, false, `exit status: ${outcome.status ?? 'unknown'}`, outcome.stderr, request.json_path).diagnostics);
      }
      return fail(uiStateFor(outcome), diagnostics, outcome.notices);
    }

    const completion = classify(request.stage_name, true, 'exit status: 0', outcome.stderr, request.json_path);
    const warnings = [...requestDiagnostics, ...asDiagnostics(completion.diagnostics), ...outcome.diagnostics];
    if (!completion.success || warnings.some(isError)) return fail('pandoc-error', warnings, outcome.notices);

    const fileName = sanitizeDownloadName(path, format.extension);
    this.deps.save(new Blob([outcome.output as BlobPart], { type: format.mime }), fileName);
    this.set({ phase: 'done', clickId: id, format, fileName, warnings, notices: outcome.notices, unexecutedCells });
  }

  private set(status: DownloadStatus): void {
    this.status = status;
    for (const l of [...this.listeners]) l();
  }
}
