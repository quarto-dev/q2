/** Raw bytes on the wasm wire (the JSON form carries base64; see the schema). */
export interface RequestFile {
  path: string;
  bytes: Uint8Array;
}

/**
 * Everything one pandoc run needs (crates/quarto-core/schemas/pandoc-request.schema.json).
 * `request.test.ts` checks this field list against the schema.
 */
export interface PandocRequest {
  schema_version: number;
  kind?: 'pandoc';
  job_id: string;
  writer: string;
  argv: string[];
  env: Record<string, string>;
  files: RequestFile[];
  dirs: string[];
  resource_refs: RequestFile[];
  share_root: string;
  share_tree_path: string;
  doc_dir: string;
  project_root: string;
  output_path: string;
  stage_name: string;
  json_path: string;
  post?: 'none' | 'compile_typst';
  expected_pandoc_wasm_sha256: string;
  share_tree_version: string;
  typst_available_fonts: string[] | null;
}

/** The share tree the main thread reads from the Rust export once per `share_tree_version`. */
export interface ShareTree {
  share_tree_version: string;
  /** Paths relative to `request.share_tree_path` (`filters/main.lua`, `pandoc/datadir/...`). */
  files: RequestFile[];
}

/** A diagnostic raised by the TS host. Has no `Q-` code; `code` is a stable kebab-case id. */
export interface HostDiagnostic {
  origin: 'host';
  kind: 'error' | 'warning';
  code: HostDiagnosticCode;
  message: string;
  /** The offending path, when there is one. */
  path?: string;
  /** Which stage of a chained job raised it (H8). */
  stage?: 'pandoc' | 'typst';
}

export type HostDiagnosticCode =
  | 'unsupported-schema-version'
  | 'malformed-request'
  | 'share-root-mismatch'
  | 'share-tree-mismatch'
  | 'path-not-normalized'
  | 'path-outside-root'
  | 'reserved-path'
  | 'mount-conflict'
  | 'limit-exceeded'
  | 'pandoc-oom'
  | 'pandoc-crash'
  | 'no-output'
  // Raised by hub-client's loader and runner (host phase H2), not by `execute`.
  | 'pandoc-timeout'
  | 'wasm-unsupported'
  | 'worker-blocked'
  | 'compile-blocked'
  | 'download-failed'
  | 'checksum-mismatch'
  | 'offline';

/** The shape of a diagnostic produced by Rust (the classify export). Fields beyond these pass through. */
export interface RustDiagnostic {
  origin?: 'rust';
  kind: 'error' | 'warning' | 'info' | 'note';
  title: string;
  code?: string;
  problem?: string;
  hints?: string[];
  stage?: 'pandoc' | 'typst';
  [extra: string]: unknown;
}

export type Diagnostic = HostDiagnostic | RustDiagnostic;

export type FailureKind =
  /** The request broke a mount rule, a limit or the schema; nothing ran. */
  | 'invalid-request'
  /** pandoc exited non-zero (other than 251); Rust classifies status + stderr (Q-20-3). */
  | 'pandoc-exit'
  /** `+RTS -M` exceeded (exit 251) or the wasm ran out of memory. */
  | 'oom'
  /** The wasm trapped or an import threw; there may be no stderr. */
  | 'crash'
  /** pandoc exited 0 but did not write `output_path`. */
  | 'no-output';

export interface RunStats {
  /** Building the in-memory file tree from the share tree, `files` and `resource_refs`. */
  mountMs: number;
  instanceMs: number;
  runMs: number;
  /** Linear memory size after the run. */
  memoryBytes: number;
  mountedBytes: number;
  /** The run was served by a warm instance (H10a). */
  warm?: true;
  /** A warm-eligible request that the argv translator rejected ran through the fresh path instead. */
  fallback?: true;
  /** The instance is to be dropped when idle (`WarmSession`: its memory passed the retire threshold). */
  retire?: true;
}

export interface ExecuteSuccess {
  ok: true;
  status: 0;
  output: Uint8Array;
  outputPath: string;
  stderr: string;
  stdout: string;
  diagnostics: Diagnostic[];
  stats: RunStats;
}

export interface ExecuteFailure {
  ok: false;
  kind: FailureKind;
  /** Exit code, or null when the run trapped or never started. */
  status: number | null;
  stderr: string;
  stdout: string;
  diagnostics: Diagnostic[];
  stats?: RunStats;
}

/** Success is exit 0 and the output file present. */
export type ExecuteResult = ExecuteSuccess | ExecuteFailure;

/**
 * Typed fault injection for tests: `oom` adds `+RTS -M<limit> -RTS` (exit 251),
 * `crash` throws from the first `path_open`, `hang` never returns from it. (The
 * design names `poll_oneoff` for the hang, but H0 measured zero `poll_oneoff` calls,
 * so it would never fire; `path_open` is always reached because pandoc opens its input.)
 * Hang tests must run the core in a terminable `worker_threads` Worker.
 */
export type Fault =
  | { kind: 'oom'; limit?: string }
  | { kind: 'crash'; message?: string }
  | { kind: 'hang' };

export interface ExecuteOptions {
  /** The compiled pandoc.wasm, held by the caller (instantiated fresh per run). */
  module: WebAssembly.Module;
  fault?: Fault;
  limits?: import('./limits.ts').Limits;
  /** Defaults to the constants file's `share_root`. */
  shareRoot?: string;
  onProgress?: (stage: 'mounting' | 'running') => void;
}
