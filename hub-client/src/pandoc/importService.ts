/**
 * The document-import service (document import epic, P4): give it a `File` and a target
 * qmd path, get back the qmd, the image files to store and the report. No UI.
 *
 * Rust owns the request, paths, format table, transforms, qmd and every user-facing
 * diagnostic (I9); this module runs pandoc on its own runner (I10), keeps image bytes in
 * TS, converts EMF/WMF to PNG (I8), applies the 10 MB image rule (I16) and maps Rust's
 * snake_case responses to the camelCase types P5 sees.
 */
import type { Diagnostic, HostDiagnostic, PandocRequest, ShareTree } from '@quarto/pandoc-host';
import * as runtime from '@quarto/preview-runtime';
import { computeSHA256 } from '@quarto/quarto-sync-client';
import { inferMimeType } from '@quarto/preview-renderer/types/project';
import { DEFAULT_CONVERT_BUDGET_MS, DEFAULT_CONVERT_TIMEOUT_MS, buildMedia } from './importMedia';
import { FILE_SIZE_LIMITS } from '../services/resourceService';
import { isPreviewEmbed, pandocWasmEnabled } from './featureFlag';
import type { LoadProgress } from './pandocLoader';
import { getImportRunner } from './pandocService';
import { uiStateFor, type RunFailure, type RunOptions, type RunOutcome, type RunStage, type UiState } from './pandocRunner';

export interface ImportFormat {
  id: string;
  label: string;
  /** Lowercase, with the leading dot. */
  extensions: string[];
  mimeTypes: string[];
}
/** Mapped from `get_import_formats`' snake_case. */
export interface ImportFormats {
  formats: ImportFormat[];
  maxSourceBytes: number;
}
export interface ImportedMedia {
  /** Project-relative, `/`-separated, no leading slash. */
  projectPath: string;
  bytes: Uint8Array;
  mimeType: string;
}
export type ImportProgress = 'reading' | 'loading-pandoc' | 'converting' | 'images' | 'finishing';
/** TS-side import diagnostics (epic interface 3). */
export type ImportHostCode = 'import-read-failed' | 'import-write-failed' | 'import-cleanup-failed';
export type ImportHostDiagnostic = Omit<HostDiagnostic, 'code'> & { code: ImportHostCode };
export type ImportDiagnostic = Diagnostic | ImportHostDiagnostic;
export type ImportOutcome =
  | { ok: true; qmd: string; media: ImportedMedia[]; diagnostics: ImportDiagnostic[] }
  /** `uiState` comes from `uiStateFor`, for `load-failed` and `worker-blocked` only. */
  | { ok: false; cancelled?: true; diagnostics: ImportDiagnostic[]; uiState?: UiState };
export interface ImportOptions {
  signal?: AbortSignal;
  onProgress?: (p: ImportProgress) => void;
  /** First-use pandoc download/verify progress, passed through from `RunOptions.onLoadProgress`. */
  onLoadProgress?: (p: LoadProgress) => void;
}
export interface ImportService {
  getImportFormats(): Promise<ImportFormats>;
  /** Validation-only `prepare_import` on name and size: `[]` when importable, else Rust's Q-24-1 / Q-24-2. Reads no bytes, loads no pandoc. */
  validateImportSource(file: File): Promise<ImportDiagnostic[]>;
  importDocument(file: File, targetQmdPath: string, opts?: ImportOptions): Promise<ImportOutcome>;
}

// ---- the stub ----------------------------------------------------------------------------------
//
// Hand-written canned answers, so P5's UI tests can run without pandoc: `setImportServiceForTests` installs any `ImportService`, normally this one.

const STUB_FORMATS: ImportFormats = {
  formats: [
    { id: 'docx', label: 'Word document', extensions: ['.docx'], mimeTypes: ['application/vnd.openxmlformats-officedocument.wordprocessingml.document'] },
    { id: 'odt', label: 'OpenDocument text', extensions: ['.odt'], mimeTypes: ['application/vnd.oasis.opendocument.text'] },
    { id: 'rtf', label: 'Rich Text Format', extensions: ['.rtf'], mimeTypes: ['application/rtf', 'text/rtf'] },
    { id: 'epub', label: 'EPUB book', extensions: ['.epub'], mimeTypes: ['application/epub+zip'] },
    { id: 'pptx', label: 'PowerPoint presentation', extensions: ['.pptx'], mimeTypes: ['application/vnd.openxmlformats-officedocument.presentationml.presentation'] },
  ],
  maxSourceBytes: 25 * 1024 * 1024,
};

/** 1x1 transparent PNG. */
const STUB_PNG = Uint8Array.from(
  atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=='),
  (c) => c.charCodeAt(0),
);

const stubRust = (kind: 'error' | 'warning' | 'info', code: string, title: string, problem: string): Diagnostic => ({ origin: 'rust', kind, code, title, problem });

/** Q-24-1 / Q-24-2 by extension and size, like `prepare_import`'s validation. */
function stubValidate(file: { name: string; size: number }): ImportDiagnostic[] {
  const dot = file.name.lastIndexOf('.');
  const ext = dot < 0 ? '' : file.name.slice(dot).toLowerCase();
  if (!STUB_FORMATS.formats.some((f) => f.extensions.includes(ext))) {
    return [stubRust('error', 'Q-24-1', 'Unsupported file type', `${file.name} is not a file type the hub can import.`)];
  }
  if (file.size > STUB_FORMATS.maxSourceBytes) {
    return [stubRust('error', 'Q-24-2', 'File too large', `${file.name} is larger than the 25 MB import limit.`)];
  }
  return [];
}

/**
 * The canned service. `importDocument` returns a short qmd, one media entry and one diagnostic
 * of each kind; a name containing "corrupt" gives a canned Q-24-3 failure instead.
 */
export function createStubImportService(): ImportService {
  return {
    getImportFormats: async () => STUB_FORMATS,
    validateImportSource: async (file) => stubValidate(file),
    importDocument: async (file, targetQmdPath, opts) => {
      const invalid = stubValidate(file);
      if (invalid.length) return { ok: false, diagnostics: invalid };
      if (/corrupt/i.test(file.name)) {
        return { ok: false, diagnostics: [stubRust('error', 'Q-24-3', "Pandoc couldn't read this file", `${file.name} looks corrupt, encrypted or the wrong type.`)] };
      }
      for (const p of ['reading', 'loading-pandoc', 'converting', 'images', 'finishing'] as const) opts?.onProgress?.(p);
      const stem = targetQmdPath.replace(/\.qmd$/, '').split('/').pop() ?? 'import';
      return {
        ok: true,
        qmd: `---\ntitle: ${stem}\n---\n\nImported from ${file.name}.\n\n![A pixel](${encodeURI(stem)}_media/000000000000.png)\n`,
        media: [{ projectPath: `${targetQmdPath.replace(/\.qmd$/, '')}_media/000000000000.png`, bytes: STUB_PNG.slice(), mimeType: 'image/png' }],
        diagnostics: [
          stubRust('warning', 'Q-24-4', 'Reader warning', 'pandoc warned about something in the source.'),
          stubRust('info', 'Q-24-10', 'Image converted', 'image1.emf was converted to PNG.'),
          { origin: 'host', kind: 'warning', code: 'collect-limit', message: 'A large image was left out.', path: '/__q2_share__/import/media/big.bmp', size: 30_000_000 },
        ],
      };
    },
  };
}

// ---- the real service -------------------------------------------------------------------------------

/** The Rust wasm wrappers the service calls: synchronous, returning raw snake_case JSON, parsed. `ready` is awaited once before the first call. */
export interface ImportWasm {
  ready(): Promise<unknown>;
  getImportFormatTable: typeof runtime.getImportFormatTable;
  prepareImport: typeof runtime.prepareImport;
  finishImport: typeof runtime.finishImport;
  classifyImportFailure: typeof runtime.classifyImportFailure;
}

/** Runner, wasm wrappers, image converter and clock, injected like `DownloadDeps`. */
export interface ImportDeps {
  /** The import runner (`getImportRunner()`); tests pass one built on `nodePandocWorker`. */
  runner: { run(request: PandocRequest, shareTree: ShareTree, options?: RunOptions): Promise<RunOutcome> };
  wasm: ImportWasm;
  /** EMF/WMF to PNG bytes; rejects on failure. Main thread only (it needs the DOM), so tests inject a fake. */
  convertImage: (bytes: Uint8Array, format: 'emf' | 'wmf') => Promise<Uint8Array>;
  /** Lowercase hex SHA-256. */
  sha256: (bytes: Uint8Array) => Promise<string>;
  /** Monotonic milliseconds, for the conversion budget. */
  now: () => number;
  /** A stored image's size limit (`FILE_SIZE_LIMITS.MAX_FILE_SIZE`, I16). */
  maxImageBytes: number;
  /** Per-image conversion timeout (default 10 s). */
  convertTimeoutMs?: number;
  /** Total conversion budget across one import (default 60 s); images after it are not converted. */
  convertBudgetMs?: number;
}

const asDiagnostics = (d: unknown): ImportDiagnostic[] => (d ?? []) as ImportDiagnostic[];

const readFailed = (name: string, e: unknown): ImportHostDiagnostic => ({
  origin: 'host',
  kind: 'error',
  code: 'import-read-failed',
  message: `${name} could not be read (${e instanceof Error ? e.message : String(e)}). It may have been moved, deleted or made unreadable; try choosing it again.`,
});

/** The runner's stages as import progress: the wasm load and worker start, then mounting and running pandoc. */
const progressOf = (stage: RunStage): ImportProgress => (stage === 'loading' || stage === 'starting' ? 'loading-pandoc' : 'converting');

export function createImportService(deps: ImportDeps): ImportService {
  const { wasm } = deps;
  const cancelled = (): ImportOutcome => ({ ok: false, cancelled: true, diagnostics: [] });
  let readyOnce: Promise<unknown> | undefined;
  const ready = () =>
    (readyOnce ??= wasm.ready().catch((e) => {
      readyOnce = undefined;
      throw e;
    }));
  const classify = (kind: string, status: number | null, stderr: string): ImportDiagnostic[] => asDiagnostics(wasm.classifyImportFailure(kind, status, stderr).diagnostics);
  /** A fatal Q-24-12 for a state that should not happen (the response lacked what Rust promised). */
  const internalError = (what: string) => classify('invalid-request', null, what);
  /** Rust's diagnostics for a failed call, or a Q-24-12 if it failed without saying why. */
  const orInternal = (diagnostics: unknown, what: string): ImportDiagnostic[] => {
    const d = asDiagnostics(diagnostics);
    return d.length ? d : internalError(what);
  };

  let formats: ImportFormats | undefined;
  async function getImportFormats(): Promise<ImportFormats> {
    if (!formats) {
      await ready();
      const t = wasm.getImportFormatTable();
      formats = {
        formats: t.formats.map((f) => ({ id: f.id, label: f.label, extensions: f.extensions, mimeTypes: f.mime_types })),
        maxSourceBytes: t.max_source_bytes,
      };
    }
    return formats;
  }

  async function validateImportSource(file: File): Promise<ImportDiagnostic[]> {
    await ready();
    const r = wasm.prepareImport(file.name, file.size, '');
    return r.success ? [] : orInternal(r.diagnostics, 'prepare_import refused the file without a diagnostic');
  }

  /** The table in P4 T4, by `RunFailureKind`. */
  function failureOutcome(out: RunFailure): ImportOutcome {
    switch (out.kind) {
      case 'aborted':
        return cancelled();
      case 'load-failed':
      case 'worker-blocked':
        return { ok: false, diagnostics: asDiagnostics(out.diagnostics), uiState: uiStateFor(out) };
      case 'invalid-request':
      case 'superseded':
        return { ok: false, diagnostics: [...classify(out.kind, out.status, out.stderr), ...asDiagnostics(out.diagnostics)] };
      default:
        // pandoc-exit, no-output, oom, crash, timeout: Rust picks Q-24-3 or Q-24-13 from the kind.
        return { ok: false, diagnostics: classify(out.kind, out.status, out.stderr) };
    }
  }

  async function run(file: File, targetQmdPath: string, opts: ImportOptions): Promise<ImportOutcome> {
    const { signal, onProgress } = opts;
    if (signal?.aborted) return cancelled();
    await ready();

    // Refuse by name and size before reading anything or loading pandoc.
    const refused = await validateImportSource(file);
    if (refused.length) return { ok: false, diagnostics: refused };

    let last: ImportProgress | undefined;
    const progress = (p: ImportProgress) => {
      if (p !== last) onProgress?.((last = p));
    };
    progress('reading');
    let source: Uint8Array;
    try {
      source = new Uint8Array(await file.arrayBuffer());
    } catch (e) {
      return { ok: false, diagnostics: [readFailed(file.name, e)] };
    }
    if (signal?.aborted) return cancelled();
    const sha256 = await deps.sha256(source);
    // The real length: the host checks it against the bytes it is given.
    const prepared = wasm.prepareImport(file.name, source.byteLength, sha256);
    if (!prepared.success || !prepared.request || !prepared.share_tree || !prepared.source_path) {
      return { ok: false, diagnostics: orInternal(prepared.diagnostics, 'prepare_import returned no request') };
    }

    progress('loading-pandoc');
    const out = await deps.runner.run(prepared.request as unknown as PandocRequest, prepared.share_tree as unknown as ShareTree, {
      signal,
      inputs: { [prepared.source_path]: source },
      onLoadProgress: opts.onLoadProgress,
      onStage: (stage) => progress(progressOf(stage)),
    });
    if (!out.ok) return failureOutcome(out);

    progress('images');
    const hostDiagnostics = asDiagnostics(out.diagnostics).filter((d): d is HostDiagnostic => d.origin === 'host');
    const media = await buildMedia(out.collected, hostDiagnostics, { ...deps, convertTimeoutMs: deps.convertTimeoutMs ?? DEFAULT_CONVERT_TIMEOUT_MS, convertBudgetMs: deps.convertBudgetMs ?? DEFAULT_CONVERT_BUDGET_MS }, signal);
    if (media.cancelled) return cancelled();

    progress('finishing');
    const finished = wasm.finishImport(new TextDecoder().decode(out.output), out.stderr, targetQmdPath, JSON.stringify(media.manifest), prepared.format);
    if (!finished.success || finished.qmd === undefined) {
      return { ok: false, diagnostics: orInternal(finished.diagnostics, 'finish_import failed without a diagnostic') };
    }

    const stored: ImportedMedia[] = [];
    for (const entry of finished.media_plan ?? []) {
      const m = media.stored.get(entry.pandoc_path);
      if (!m) return { ok: false, diagnostics: internalError(`the media plan names ${entry.pandoc_path}, which was not stored`) };
      stored.push({ projectPath: entry.project_path, bytes: m.bytes, mimeType: inferMimeType(entry.project_path) });
    }
    return { ok: true, qmd: finished.qmd, media: stored, diagnostics: [...asDiagnostics(finished.diagnostics), ...hostDiagnostics] };
  }

  // Imports are serialized: the runner supersedes its current run, so a second call waits for the first.
  let tail: Promise<unknown> = Promise.resolve();
  function importDocument(file: File, targetQmdPath: string, opts: ImportOptions = {}): Promise<ImportOutcome> {
    const result = tail.then(() => run(file, targetQmdPath, opts));
    tail = result.catch(() => undefined);
    return result;
  }

  return { getImportFormats, validateImportSource, importDocument };
}

/** Import is offered where pandoc.wasm is shipped and the browser build renders: not in the `q2 preview` embed, which has no pandoc.wasm (D7). Unlike `downloadAvailable()`, false in the embed. */
export function importAvailable(): boolean {
  return pandocWasmEnabled() && !isPreviewEmbed();
}

/** The real dependencies: the import runner, the Rust wasm (initialised on first use) and the lazy rtf.js converter. */
function defaultDeps(): ImportDeps {
  return {
    // Looked up per call, so building the service creates no runner or loader.
    runner: { run: (request, shareTree, options) => getImportRunner().run(request, shareTree, options) },
    wasm: {
      ready: () => runtime.initWasm(),
      getImportFormatTable: runtime.getImportFormatTable,
      prepareImport: runtime.prepareImport,
      finishImport: runtime.finishImport,
      classifyImportFailure: runtime.classifyImportFailure,
    },
    // rtf.js and its DOM use stay out of the main bundle until an EMF or WMF turns up.
    convertImage: async (bytes, format) => (await import('./metafileToPng')).convertMetafileToPng(bytes, format),
    sha256: computeSHA256,
    now: () => performance.now(),
    maxImageBytes: FILE_SIZE_LIMITS.MAX_FILE_SIZE,
  };
}

let installed: ImportService | undefined;
/** The app-wide service. Tests replace it with `setImportServiceForTests` (normally `createStubImportService()`). */
export function getImportService(): ImportService {
  return (installed ??= createImportService(defaultDeps()));
}
export function setImportServiceForTests(s: ImportService | undefined): void {
  installed = s;
}
