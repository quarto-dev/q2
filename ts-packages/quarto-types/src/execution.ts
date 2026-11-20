// parity: vendored from external-sources/quarto-cli/packages/quarto-types
/**
 * Execution workflow types for Quarto engines
 */

import type { MappedString } from "./text.js";
import type { Format } from "./format.js";
import type { Metadata } from "./metadata.js";
import type { ExecutionTarget, ExecutionEngineInstance } from "./execution-engine.js";
import type { EngineProjectContext } from "./project-context.js";
import type { PandocIncludes } from "./pandoc.js";

/**
 * Options for execution
 */
export interface ExecuteOptions {
  /** The execution target */
  target: ExecutionTarget;

  /** Format to render to */
  format: Format;

  /** Directory for resources */
  resourceDir: string;

  /** Directory for temporary files */
  tempDir: string;

  /** Whether to include dependencies */
  dependencies: boolean;

  /** Project directory if applicable */
  projectDir?: string;

  /** Library directory */
  libDir?: string;

  /** Current working directory */
  cwd: string;

  /** Parameters passed to document */
  params?: { [key: string]: unknown };

  /** Whether to suppress output */
  quiet?: boolean;

  /** Whether execution is for preview server */
  previewServer?: boolean;

  /** List of languages handled by cell language handlers */
  handledLanguages: string[];

  /** Positive set: languages present in this render that q2 assigned to THIS
   *  engine. Read `ownedLanguages.includes("L")` to ask "did q2 give me L?";
   *  the complement of `handledLanguages` is ambiguous (it also contains
   *  languages owned by nobody). q2 extension; Q1 does not send it. */
  ownedLanguages: string[];

  /** Environment variables for the processes that run user code: the project
   *  environment (`_environment` files, `QUARTO_PROFILE`), then the per-document
   *  execute environment (`QUARTO_DOCUMENT_*`, `QUARTO_EXECUTE_INFO`,
   *  `QUARTO_FIG_*`, `QUARTO_PROJECT_ROOT`) on top. Merge these into the
   *  environment of every process that evaluates user code, and never cache them
   *  across executes: they change from one render to the next. Do not mutate
   *  `process.env`. May carry secrets: never log it. Absent when empty.
   *  q2 extension; Q1 does not send it. */
  executeEnv?: Readonly<Record<string, string>>;

  /** Project context */
  project: EngineProjectContext;
}

/**
 * Result of execution
 */
export interface ExecuteResult {
  /** Resulting markdown content */
  markdown: string;

  /** Supporting files */
  supporting: string[];

  /** Filter scripts */
  filters: string[];

  /** Updated metadata */
  metadata?: Metadata;

  /** Pandoc options */
  pandoc?: Record<string, unknown>;

  /** Pandoc includes */
  includes?: PandocIncludes;

  /** Engine name */
  engine?: string;

  /** Engine-specific dependencies */
  engineDependencies?: Record<string, Array<unknown>>;

  /** Content to preserve during processing */
  preserve?: Record<string, string>;

  /** Whether post-processing is required */
  postProcess?: boolean;

  /** Additional resource files */
  resourceFiles?: string[];
}

/**
 * Options for retrieving dependencies
 */
export interface DependenciesOptions {
  /** The execution target */
  target: ExecutionTarget;

  /** Format to render to */
  format: Format;

  /** Output file path */
  output: string;

  /** Directory for resources */
  resourceDir: string;

  /** Directory for temporary files */
  tempDir: string;

  /** Project directory if applicable */
  projectDir?: string;

  /** Library directory */
  libDir?: string;

  /** Dependencies to include */
  dependencies?: Array<unknown>;

  /** Whether to suppress output */
  quiet?: boolean;
}

/**
 * Result of retrieving dependencies
 */
export interface DependenciesResult {
  /** Pandoc includes */
  includes: PandocIncludes;
}

/**
 * Options for post-processing
 */
export interface PostProcessOptions {
  /** The execution engine */
  engine: ExecutionEngineInstance;

  /** The execution target */
  target: ExecutionTarget;

  /** Format to render to */
  format: Format;

  /** Output file path */
  output: string;

  /** Directory for temporary files */
  tempDir: string;

  /** Project directory if applicable */
  projectDir?: string;

  /** Content to preserve during processing */
  preserve?: Record<string, string>;

  /** Whether to suppress output */
  quiet?: boolean;
}

/**
 * Options for running the engine
 */
export interface RunOptions {
  /** Input file path */
  input: string;

  /** Whether to render */
  render: boolean;

  /** Whether to open in browser */
  browser: boolean;

  /** Directory for temporary files */
  tempDir: string;

  /** Whether to reload */
  reload?: boolean;

  /** Target format */
  format?: string;

  /** Project directory if applicable */
  projectDir?: string;

  /** Port for server */
  port?: number;

  /** Host for server */
  host?: string;

  /** Whether to suppress output */
  quiet?: boolean;

  /** Callback when ready */
  onReady?: () => Promise<void>;
}
