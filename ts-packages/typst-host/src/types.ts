/** A file in the compile's virtual filesystem. `path` is absolute and `/`-normalized. */
export interface TypstFile {
  path: string;
  bytes: Uint8Array;
}

export interface CompileInput {
  /** Absolute path of the `.typ` to compile; it must be one of `files`. */
  main: string;
  /** Compile root; typst resolves `/`-rooted paths and refuses to read outside it. Default `/`. */
  root?: string;
  /** The `.typ`, the project's files, images, template partials and brand fonts' companions. */
  files: TypstFile[];
  /** `sys.inputs`. */
  inputs?: Record<string, string>;
}

export type HostDiagnosticCode =
  | 'invalid-input'
  | 'limit-exceeded'
  | 'package-not-found'
  | 'package-fetch-failed'
  | 'typst-oom'
  | 'typst-crash'
  | 'no-fonts'
  // Raised by hub-client's runner, not by the session.
  | 'typst-timeout'
  | 'wasm-unsupported'
  | 'worker-blocked'
  | 'compile-blocked'
  | 'download-failed'
  | 'checksum-mismatch'
  | 'offline';

/** A problem the host found (as opposed to one typst reported). */
export interface HostDiagnostic {
  origin: 'host';
  kind: 'error' | 'warning';
  code: HostDiagnosticCode;
  message: string;
  /** `@preview/name:version` for the package diagnostics. */
  package?: string;
  stage: 'typst';
}

/** A diagnostic typst reported, with typst.ts's fields passed through. */
export interface TypstDiagnostic {
  origin: 'typst';
  kind: 'error' | 'warning';
  message: string;
  /** Source path as typst saw it (`/main.typ`, or `@preview/pkg:1.0.0/lib.typ` for a package file). */
  path: string;
  /** `line:col-line:col` (0-based, from typst.ts). */
  range: string;
  /** Set when the diagnostic is inside a package. */
  package?: string;
  stage: 'typst';
}

export type Diagnostic = HostDiagnostic | TypstDiagnostic;

export interface CompileStats {
  /** Mapping the files and prefetching packages. */
  prepareMs: number;
  /** Compile attempts, including retries for packages found by diagnostics. */
  attempts: number;
  compileMs: number;
  /** Packages fetched from the registry (not vendored, not already cached). */
  packagesFetched: number;
  packageBytes: number;
}

export interface CompileSuccess {
  ok: true;
  pdf: Uint8Array;
  pages: number;
  diagnostics: Diagnostic[];
  stats: CompileStats;
}

export type CompileFailureKind =
  | 'invalid-input'
  /** typst reported errors (the document is wrong), including a package it could not find. */
  | 'typst-error'
  /** A package could not be fetched (offline, HTTP error, limit). */
  | 'package-fetch'
  | 'oom'
  | 'crash';

export interface CompileFailure {
  ok: false;
  kind: CompileFailureKind;
  diagnostics: Diagnostic[];
  stats?: CompileStats;
}

export type CompileResult = CompileSuccess | CompileFailure;

export interface PackageSpec {
  namespace: string;
  name: string;
  version: string;
}

/** A tarball source. Resolve `undefined` for "no such package" (HTTP 404); throw for anything else. */
export type PackageFetcher = (spec: PackageSpec, signal?: AbortSignal) => Promise<Uint8Array | undefined>;
