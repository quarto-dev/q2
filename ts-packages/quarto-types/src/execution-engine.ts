// parity: vendored from external-sources/quarto-cli/packages/quarto-types
/**
 * Execution engine interfaces for Quarto
 */

import type { MappedString } from "./text.js";
import type { Format } from "./format.js";
import type { Metadata } from "./metadata.js";
import type { EngineProjectContext } from "./project-context.js";
import type { Command } from "./cli.js";
import type { QuartoAPI } from "./quarto-api.js";
import type {
  ExecuteOptions,
  ExecuteResult,
  DependenciesOptions,
  DependenciesResult,
  PostProcessOptions,
  RunOptions,
} from "./execution.js";
import type {
  RenderFlags,
  RenderOptions,
  RenderResultFile,
} from "./render.js";
import type { PartitionedMarkdown } from "./markdown.js";
import type { PandocIncludes, PandocIncludeLocation } from "./pandoc.js";
import type { CheckConfiguration } from "./check.js";

/**
 * Execution target (filename and context)
 */
export interface ExecutionTarget {
  /** Original source file */
  source: string;

  /** Input file after preprocessing */
  input: string;

  /** Markdown content */
  markdown: MappedString;

  /** Document metadata */
  metadata: Metadata;

  /** Optional target-specific data */
  data?: unknown;
}

/**
 * Kind-tagged language claim returned by {@link ExecutionEngineDiscovery.claimsLanguage}.
 *
 * `priority` is **optional** — the harness fills in defaults at normalization time
 * (primary defaults to 1, interop and fallback default to 0).
 * Use the constructors in `@quarto/api` (`primary()`, `interop()`, `fallback()`)
 * rather than hand-writing this shape.
 *
 * `interop` is presence-gated: it only extends engine ownership to a language when
 * the engine is already in the sequence via a `primary` claim. `fallback` is a
 * universal-kernel signal (lowest precedence). A bare `number` return is always
 * treated as `primary` — `interop` and `fallback` are reachable **only** via this
 * object form.
 */
export interface LanguageClaim {
  kind: "primary" | "interop" | "fallback";
  priority?: number;
}

/**
 * Interface for execution engine discovery
 * Responsible for the static aspects of engine discovery (not requiring project context)
 */
export interface ExecutionEngineDiscovery {
  /**
   * Initialize the engine with the Quarto API (optional).
   *
   * **Timing:** called during `loadEngine` handling, after the engine module's
   * exports have been validated and before `launch()` is ever called.
   *
   * **What is available at init time (everything the engine needs pre-launch):**
   *
   * - *Pure namespaces* (no host I/O; correct in any environment):
   *   `quarto.text`, `quarto.markdownRegex`, `quarto.format`, `quarto.crypto`.
   * - *Host-only namespaces* (backed by `PlatformHost` I/O; built via factory):
   *   `quarto.console`, `quarto.mappedString`, `quarto.path`, `quarto.system`.
   * - *Ambient methods* (resolved from the `Init { global }` config injected at
   *   harness startup): `quarto.path.runtime`, `quarto.path.resource`,
   *   `quarto.path.dataDir`, `quarto.system.pandoc`. These reflect the host
   *   environment's runtime directories and Pandoc binary path.
   * - `quarto.format.*` — format predicates take a `Format` argument on every
   *   call; they are never gated by init state.
   *
   * **Usage contract:**
   * - Engines **MUST NOT** access `quarto.*` at module top-level — only from
   *   inside `init()` or other method bodies. The `quarto` object is not yet
   *   available until `init()` is called by the harness.
   * - Store the received `quarto` reference in the module or closure scope for
   *   reuse in all other engine methods.
   * - May be called multiple times, but always with the same `QuartoAPI` object.
   *
   * **Async behaviour:** `init()` is synchronous per Q1's contract; the harness
   * defensively `await`s its return value, so an `async init()` also works.
   *
   * **Error handling:** throwing or rejecting from `init()` is a fatal load
   * failure — the engine will not proceed to `launch()`.
   *
   * **Project context:** the per-render project context (`EngineProjectContext`)
   * arrives separately on each `launch()` call (captured in the returned
   * `ExecutionEngineInstance` closure). It is **not** passed via `init()`.
   *
   * For the canonical namespace × host-use table see the "Engine API contract"
   * section in `claude-notes/plans/2026-04-16-plan1b-engine-host-deno.md`.
   *
   * @param quarto - The fully assembled Quarto API object for this engine
   */
  init?: (quarto: QuartoAPI) => void;

  /**
   * Name of the engine
   */
  name: string;

  /**
   * Default extension for files using this engine
   */
  defaultExt: string;

  /**
   * Generate default YAML for this engine
   */
  defaultYaml: (kernel?: string) => string[];

  /**
   * Generate default content for this engine
   */
  defaultContent: (kernel?: string) => string[];

  /**
   * List of file extensions this engine supports
   */
  validExtensions: () => string[];

  /**
   * Whether this engine can handle the given file
   *
   * @param file - The file path to check
   * @param ext - The file extension
   * @returns True if this engine can handle the file
   */
  claimsFile: (file: string, ext: string) => boolean;

  /**
   * Whether this engine can handle the given language.
   *
   * @param language - The language identifier (e.g., "python", "r", "julia")
   * @param firstClass - Optional first class from code block attributes (e.g., "marimo" from `{python .marimo}`)
   * @returns
   *   - `false` / `null` — don't claim this language.
   *   - `true` — claim as primary with priority 1 (Q1-compatible shorthand).
   *   - `number n` — claim as primary with priority n. Negative values are
   *     low-priority primary claims; a bare number is **always** primary, never interop.
   *   - `LanguageClaim` object — use the object form to return `interop` or `fallback`
   *     kinds, or to pass an explicit priority alongside the kind. `priority` is optional;
   *     the harness fills defaults (primary→1, interop/fallback→0). Use the constructors
   *     in `@quarto/api` (`primary()`, `interop()`, `fallback()`) to build this shape.
   */
  claimsLanguage: (language: string, firstClass?: string) => boolean | number | LanguageClaim | null;

  /**
   * Whether this engine supports freezing
   */
  canFreeze: boolean;

  /**
   * Whether this engine generates figures
   */
  generatesFigures: boolean;

  /**
   * Directories to ignore during processing (optional)
   */
  ignoreDirs?: () => string[] | undefined;

  /**
   * Semver range specifying the minimum required Quarto version for this engine
   * Examples: ">= 1.6.0", "^1.5.0", "1.*"
   *
   * When specified, Quarto will check at engine registration time whether the
   * current version satisfies this requirement. If not, an error will be thrown.
   */
  quartoRequired?: string;

  /**
   * Populate engine-specific CLI commands (optional)
   * Called at module initialization to register commands like 'quarto enginename status'
   *
   * @param command - The CLI command to populate with subcommands
   */
  populateCommand?: (command: Command) => void;

  /**
   * Check installation and capabilities for this engine (optional)
   * Used by `quarto check <engine-name>` command
   *
   * Engines implementing this method will automatically be available as targets
   * for the check command (e.g., `quarto check jupyter`, `quarto check knitr`).
   *
   * @param conf - Check configuration with output settings and services
   */
  checkInstallation?: (conf: CheckConfiguration) => Promise<void>;

  /**
   * Launch a dynamic execution engine with project context
   * This is called when the engine is needed for execution
   *
   * @param context The restricted project context
   * @returns ExecutionEngineInstance that can execute documents
   */
  launch: (context: EngineProjectContext) => ExecutionEngineInstance;
}

/**
 * Interface for a launched execution engine
 * This represents an engine that has been instantiated with a project context
 * and is ready to execute documents
 */
export interface ExecutionEngineInstance {
  /**
   * Name of the engine
   */
  name: string;

  /**
   * Whether this engine supports freezing
   */
  canFreeze: boolean;

  /**
   * Get the markdown content for a file
   */
  markdownForFile(file: string): Promise<MappedString>;

  /**
   * Create an execution target for the given file
   */
  target: (
    file: string,
    quiet?: boolean,
    markdown?: MappedString,
  ) => Promise<ExecutionTarget | undefined>;

  /**
   * Get a partitioned view of the markdown
   */
  partitionedMarkdown: (
    file: string,
    format?: Format,
  ) => Promise<PartitionedMarkdown>;

  /**
   * Filter the format based on engine requirements
   */
  filterFormat?: (
    source: string,
    options: RenderOptions,
    format: Format,
  ) => Format;

  /**
   * Execute the target
   */
  execute: (options: ExecuteOptions) => Promise<ExecuteResult>;

  /**
   * Handle skipped execution targets
   */
  executeTargetSkipped?: (
    target: ExecutionTarget,
    format: Format,
  ) => void;

  /**
   * Get dependencies for the target
   */
  dependencies: (options: DependenciesOptions) => Promise<DependenciesResult>;

  /**
   * Post-process the execution result
   */
  postprocess: (options: PostProcessOptions) => Promise<void>;

  /**
   * Whether this engine can keep source for this target
   */
  canKeepSource?: (target: ExecutionTarget) => boolean;

  /**
   * Get a list of intermediate files generated by this engine
   */
  intermediateFiles?: (input: string) => string[] | undefined;

  /**
   * Run the engine (for interactivity)
   */
  run?: (options: RunOptions) => Promise<void>;

  /**
   * Post-render processing
   */
  postRender?: (file: RenderResultFile) => Promise<void>;
}
