/**
 * WASM Renderer Service
 *
 * Provides typed access to the wasm-quarto-hub-client module for
 * VFS operations, QMD rendering, and SASS compilation.
 */

import type { Diagnostic, RenderResponse } from '../types/diagnostic';
import type { RustQmdJson } from '@quarto/pandoc-types'
import type { AstResponse } from 'wasm-quarto-hub-client'
import { discoverUserGrammars } from './userGrammarDiscovery';
import { UserGrammarCache } from './userGrammarCache';
import { loadUserGrammar } from './userGrammarHighlight';

// Response types from WASM module
interface VfsResponse {
  success: boolean;
  error?: string;
  files?: string[];
  content?: string;
}

// Re-export Diagnostic type for convenience
export type { Diagnostic } from '../types/diagnostic';

// Extended WASM module type with SASS compilation functions
interface WasmModuleExtended {
  // Existing functions
  default: () => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_add_binary_file: (path: string, content: Uint8Array) => string;
  vfs_remove_file: (path: string) => string;
  vfs_list_files: () => string;
  vfs_clear: () => string;
  vfs_read_file: (path: string) => string;
  vfs_read_binary_file: (path: string) => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  vfs_get_runtime_metadata: () => string;
  render_qmd: (
    path: string,
    user_grammars?: unknown,
  ) => Promise<string>;
  render_qmd_content: (
    content: string,
    templateBundle: string,
    user_grammars?: unknown,
  ) => Promise<string>;
  // Phase 9: project-aware render. Discovers the surrounding
  // `_quarto.yml` (if any) and either falls through to the
  // single-doc path (no project) or drives `ProjectPipeline`
  // with `RenderMode::ActivePage(path)`.
  render_page_in_project: (
    path: string,
    user_grammars?: unknown,
  ) => Promise<string>;
  get_builtin_template: (name: string) => string;
  get_project_choices: () => string;
  create_project: (choiceId: string, title: string) => Promise<string>;
  parse_qmd_to_ast: (content: string) => Promise<string>;
  write_qmd: (astJson: string) => Promise<string>;
  incremental_write_qmd(original_qmd: string, new_ast_json: string): string;
  convert: (document: string, inputFormat: string, outputFormat: string) => Promise<string>;
  lsp_analyze_document: (path: string) => string;
  lsp_get_symbols: (path: string) => string;
  lsp_get_folding_ranges: (path: string) => string;
  lsp_get_diagnostics: (path: string) => string;
  // SASS compilation functions
  sass_available: () => boolean;
  sass_compiler_name: () => string | undefined;
  // User-grammar bridge (Phase 4.3 of syntax-highlighting plan).
  JsUserGrammars: new () => JsUserGrammarsHandle;
}

/** Minimal surface of the wasm-bindgen `JsUserGrammars` type. */
interface JsUserGrammarsHandle {
  register(
    languageClass: string,
    highlightFn: (class_: string, source: string) => string | null | undefined,
  ): void;
  free(): void;
}

// WASM module state
let wasmModule: WasmModuleExtended | null = null;
let initPromise: Promise<void> | null = null;
let htmlTemplateBundle: string | null = null;

// Runtime settings managed by TypeScript, serialized to WASM as a single YAML blob.
// Multiple settings can coexist without clobbering each other.
let runtimeSettings: Record<string, unknown> = {};

/**
 * Initialize the WASM module. Safe to call multiple times - will only
 * initialize once.
 */
export async function initWasm(): Promise<void> {
  if (wasmModule) return;

  if (!initPromise) {
    initPromise = (async () => {
      try {
        // Dynamic import the WASM module
        const wasm = await import('wasm-quarto-hub-client');

        // Initialize the module (loads the .wasm file)
        await wasm.default();

        // Cast to extended type (includes SASS compilation functions)
        wasmModule = wasm as unknown as WasmModuleExtended;

        // Load the HTML template bundle
        htmlTemplateBundle = wasm.get_builtin_template('html');

        // Set up VFS callbacks for SASS importer
        // This allows dart-sass to read Bootstrap SCSS files from the VFS
        await setupSassVfsCallbacks();

        // NOTE on pre-warming the default-CSS cache: it's tempting to
        // fire `wasm.compile_default_bootstrap_css(true)` here so the
        // first render after a deploy doesn't pay the ~500ms SASS
        // compile. Do **not** do that. The SASS module in
        // `src/wasm-js-bridge/sass.js` is intentionally lazy-loaded
        // (~5 MB dart-sass dynamic import) to avoid blocking startup.
        // Triggering compilation at init-time forces dart-sass to load
        // in parallel with Monaco's own ~3 MB CDN load — on Firefox
        // and Chrome alike this manifests as "too much recursion"
        // errors inside Monaco's chunk (a Vite module-graph race
        // between two overlapping dynamic imports). If pre-warming
        // becomes valuable later, defer it to after Monaco is fully
        // loaded (e.g. run inside a React effect that fires once the
        // first document's file list is mounted). See the Phase 3
        // syntax-highlighting plan at
        // `claude-notes/plans/2026-04-20-syntax-highlighting-phase-3.md`
        // for the incident.

        console.log('WASM module initialized successfully, template loaded');
      } catch (err) {
        initPromise = null;
        throw err;
      }
    })();
  }

  return initPromise;
}

/**
 * Set up VFS callbacks for the SASS importer.
 *
 * The dart-sass compiler needs to read Bootstrap SCSS files from the VFS.
 * This connects the JS sass importer to the WASM VFS operations.
 */
async function setupSassVfsCallbacks(): Promise<void> {
  try {
    // Import the sass bridge module
    const sassModule = await import('../wasm-js-bridge/sass.js');

    // Create VFS read callback
    const readFn = (path: string): string | null => {
      const result = vfsReadFile(path);
      if (result.success && result.content !== undefined) {
        return result.content;
      }
      return null;
    };

    // Create VFS file check callback
    const isFileFn = (path: string): boolean => {
      const result = vfsReadFile(path);
      return result.success && result.content !== undefined;
    };

    // Create VFS list callback
    const listFn = (): string[] => {
      const result = vfsListFiles();
      if (result.success && result.files) {
        return result.files;
      }
      return [];
    };

    // Register callbacks with the SASS importer
    sassModule.setVfsCallbacks(readFn, isFileFn, listFn);
  } catch (err) {
    console.warn('[initWasm] Failed to set up SASS VFS callbacks:', err);
  }
}

/**
 * Check if WASM is initialized
 */
export function isWasmReady(): boolean {
  return wasmModule !== null;
}

/**
 * Get the WASM module, throwing if not initialized
 */
function getWasm() {
  if (!wasmModule) {
    throw new Error('WASM module not initialized. Call initWasm() first.');
  }
  return wasmModule;
}

// ============================================================================
// VFS Operations
// ============================================================================

/**
 * Add a text file to the virtual filesystem
 */
export function vfsAddFile(path: string, content: string): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_add_file(path, content));
}

/**
 * Add a binary file to the virtual filesystem
 */
export function vfsAddBinaryFile(path: string, content: Uint8Array): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_add_binary_file(path, content));
}

/**
 * Remove a file from the virtual filesystem
 */
export function vfsRemoveFile(path: string): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_remove_file(path));
}

/**
 * List all files in the virtual filesystem
 */
export function vfsListFiles(): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_list_files());
}

/**
 * Clear all files from the virtual filesystem
 */
export function vfsClear(): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_clear());
}

/**
 * Read a file from the virtual filesystem
 */
export function vfsReadFile(path: string): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_read_file(path));
}

/**
 * Read a binary file from the virtual filesystem.
 * Returns the content as a base64-encoded string.
 */
export function vfsReadBinaryFile(path: string): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_read_binary_file(path));
}

// ============================================================================
// Runtime Metadata Operations
// ============================================================================

/**
 * Set runtime metadata on the WASM module.
 *
 * Runtime metadata is merged at the highest precedence in the config pipeline,
 * above project, directory, and document metadata.
 *
 * @param yaml - YAML string of metadata to set, or empty string to clear
 */
export function setRuntimeMetadata(yaml: string): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_set_runtime_metadata(yaml));
}

/**
 * Get the current runtime metadata from the WASM module.
 */
export function getRuntimeMetadata(): VfsResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.vfs_get_runtime_metadata());
}

/**
 * Serialize an object to minimal YAML.
 *
 * Handles the subset of values used by runtime settings: strings, numbers,
 * booleans, and nested plain objects. Does not handle arrays or complex types.
 *
 * @internal Exported for testing only.
 */
export function toSimpleYaml(obj: Record<string, unknown>, indent: number = 0): string {
  const prefix = '  '.repeat(indent);
  const lines: string[] = [];
  for (const [key, value] of Object.entries(obj)) {
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      lines.push(`${prefix}${key}:`);
      lines.push(toSimpleYaml(value as Record<string, unknown>, indent + 1));
    } else {
      lines.push(`${prefix}${key}: ${value}`);
    }
  }
  return lines.join('\n');
}

/**
 * Enable or disable scroll sync via runtime metadata.
 *
 * When enabled, sets `format.html.source-location: full` in runtime metadata,
 * which causes `data-loc` attributes in rendered HTML for scroll sync.
 *
 * Manages a TypeScript-side settings object so multiple runtime settings can
 * coexist without clobbering each other.
 */
export function setScrollSyncEnabled(enabled: boolean): void {
  if (enabled) {
    runtimeSettings.format = { html: { 'source-location': 'full' } };
  } else {
    delete runtimeSettings.format;
  }

  // Serialize and flush to WASM
  if (Object.keys(runtimeSettings).length === 0) {
    setRuntimeMetadata('');
  } else {
    setRuntimeMetadata(toSimpleYaml(runtimeSettings) + '\n');
  }
}

// ============================================================================
// Rendering Operations
// ============================================================================

/**
 * Render a QMD file from the virtual filesystem.
 *
 * `userGrammars`, when provided, routes any code block whose language
 * class it recognizes through JS-backed tree-sitter grammars before
 * falling back to built-ins. See
 * `hub-client/src/services/userGrammarHighlight.ts` for the JS-side
 * highlighter and `JsUserGrammars` in `wasm-quarto-hub-client` for the
 * Rust bridge. Construct a fresh `JsUserGrammars` per call.
 */
export async function renderQmd(
  path: string,
  userGrammars?: unknown,
): Promise<RenderResponse> {
  const wasm = getWasm();
  return JSON.parse(await wasm.render_qmd(path, userGrammars));
}

/**
 * Render a single page **in the context of its surrounding project**.
 *
 * Phase 9 entry point: discovers the surrounding `_quarto.yml` from
 * the VFS and either falls through to the single-doc render (no
 * project ancestor) or drives `ProjectPipeline` with
 * `RenderMode::ActivePage(path)` so the active page renders with
 * project-level affordances (sidebar, navbar, prev/next strip,
 * cross-document link rewriting, deduplicated theme CSS).
 *
 * Returns the same `RenderResponse` shape as `renderQmd`; the
 * project flavor is opaque to callers.
 */
export async function renderPageInProject(
  path: string,
  userGrammars?: unknown,
): Promise<RenderResponse> {
  const wasm = getWasm();
  return JSON.parse(await wasm.render_page_in_project(path, userGrammars));
}

/**
 * Render QMD content directly (without VFS). See [`renderQmd`] for
 * the `userGrammars` parameter's semantics.
 */
export async function renderQmdContent(
  content: string,
  templateBundle: string = '',
  userGrammars?: unknown,
): Promise<RenderResponse> {
  const wasm = getWasm();
  return JSON.parse(
    await wasm.render_qmd_content(content, templateBundle, userGrammars),
  );
}


/**
 * Get a built-in template bundle
 */
export function getBuiltinTemplate(name: string): string {
  const wasm = getWasm();
  return wasm.get_builtin_template(name);
}

/**
 * Result of parsing QMD content to AST.
 */
export interface ParseResult {
  success: boolean;
  ast: string;
  error?: string;
  /** Structured error diagnostics with line/column information for Monaco. */
  diagnostics?: Diagnostic[];
  /** Structured warning diagnostics with line/column information for Monaco. */
  warnings?: Diagnostic[];
}

/**
 * Result of writing AST to QMD format.
 */
export interface WriteQmdResult {
  success: boolean;
  qmd: string;
  error?: string;
}

/**
 * Result of converting between formats.
 */
export interface ConvertResult {
  success: boolean;
  output: string;
  error?: string;
}

/**
 * Parse QMD content to Pandoc AST JSON, handling errors gracefully.
 *
 * This function parses QMD markdown into a Pandoc AST representation,
 * which can be used for programmatic manipulation, analysis, or rendering
 * with custom React components.
 *
 * Returns structured diagnostics with source locations that can be
 * converted to Monaco editor markers using diagnosticsToMarkers().
 *
 * **Example AST Structure:**
 * ```json
 * {
 *   "pandoc-api-version": [1, 23, 1],
 *   "meta": {},
 *   "blocks": [
 *     {
 *       "t": "Header",
 *       "c": [1, ["id", ["class"], [["key", "value"]]], [{"t": "Str", "c": "text"}]]
 *     },
 *     {
 *       "t": "Para",
 *       "c": [{"t": "Str", "c": "Paragraph text."}]
 *     }
 *   ]
 * }
 * ```
 *
 * @param qmdContent - QMD source text to parse
 * @returns Parse result with AST JSON string or error information
 */
export async function parseQmdToAst(
  qmdContent: string
): Promise<ParseResult> {
  try {
    await initWasm();
    const wasm = getWasm();
    const responseJson = await wasm.parse_qmd_to_ast(qmdContent);

    const response: ParseResult = JSON.parse(responseJson);

    if (response.success) {
      return {
        ast: response.ast || '{}',
        success: true,
        warnings: response.warnings,
      };
    } else {
      // Extract error message
      const errorMsg = response.error || 'Unknown parse error';

      return {
        ast: '',
        success: false,
        error: errorMsg,
        diagnostics: response.diagnostics,
        warnings: response.warnings,
      };
    }
  } catch (err) {
    console.error('Parse error:', err);
    return {
      ast: '',
      success: false,
      error: err instanceof Error ? err.message : JSON.stringify(err),
    };
  }
}

/**
 * Convert Pandoc AST JSON back to QMD format.
 *
 * This function takes a Pandoc AST represented as a JSON string and
 * converts it back to QMD markdown format.
 *
 * @param astJson - Pandoc AST as JSON string
 * @returns Write result with QMD string or error information
 *
 * @example
 * ```typescript
 * const ast = '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[...]}';
 * const result = await writeQmd(ast);
 * if (result.success) {
 *   console.log("QMD:", result.qmd);
 * }
 * ```
 */
export async function writeQmd(astJson: string): Promise<WriteQmdResult> {
  try {
    await initWasm();
    const wasm = getWasm();
    const responseJson = await wasm.write_qmd(astJson);

    // The response reuses AstResponse structure, but with "qmd" in the "ast" field
    const response: { success: boolean; ast?: string; error?: string } = JSON.parse(responseJson);

    if (response.success) {
      return {
        qmd: response.ast || '',
        success: true,
      };
    } else {
      return {
        qmd: '',
        success: false,
        error: response.error || 'Unknown write error',
      };
    }
  } catch (err) {
    console.error('Write QMD error:', err);
    return {
      qmd: '',
      success: false,
      error: err instanceof Error ? err.message : JSON.stringify(err),
    };
  }
}

/**
 * Incrementally write a modified AST back to QMD, preserving unchanged
 * portions of the original source text verbatim.
 *
 * Must call initWasm() before first use.
 */
export function incrementalWriteQmd(originalQmd: string, newAst: RustQmdJson): string {
  if (!wasmModule) {
    throw new Error('WASM not initialized. Call initWasm() first.')
  }

  const newAstJson = JSON.stringify(newAst)
  const responseJson = wasmModule.incremental_write_qmd(originalQmd, newAstJson)
  const response: AstResponse = JSON.parse(responseJson)

  if (!response.success || !response.qmd) {
    throw new Error(`Incremental write failed: ${response.error}`)
  }

  return response.qmd
}

/**
 * Convert between document formats (QMD <-> JSON).
 *
 * This function provides generic format conversion capabilities,
 * allowing you to convert between QMD and Pandoc AST JSON.
 *
 * @param document - Input document content
 * @param inputFormat - Input format: "qmd" or "json"
 * @param outputFormat - Output format: "qmd" or "json"
 * @returns Convert result with output string or error information
 *
 * @example
 * ```typescript
 * // Convert QMD to JSON
 * const jsonResult = await convert(qmdContent, "qmd", "json");
 * if (jsonResult.success) {
 *   const ast = JSON.parse(jsonResult.output);
 * }
 *
 * // Convert JSON back to QMD
 * const qmdResult = await convert(astJson, "json", "qmd");
 * if (qmdResult.success) {
 *   console.log("QMD:", qmdResult.output);
 * }
 * ```
 */
export async function convert(
  document: string,
  inputFormat: 'qmd' | 'json',
  outputFormat: 'qmd' | 'json'
): Promise<ConvertResult> {
  try {
    await initWasm();
    const wasm = getWasm();
    const responseJson = await wasm.convert(document, inputFormat, outputFormat);

    // The response reuses AstResponse structure, but with output in the "ast" field
    const response: { success: boolean; ast?: string; error?: string } = JSON.parse(responseJson);

    if (response.success) {
      return {
        output: response.ast || '',
        success: true,
      };
    } else {
      return {
        output: '',
        success: false,
        error: response.error || 'Unknown conversion error',
      };
    }
  } catch (err) {
    console.error('Convert error:', err);
    return {
      output: '',
      success: false,
      error: err instanceof Error ? err.message : JSON.stringify(err),
    };
  }
}

// ============================================================================
// High-Level API
// ============================================================================

/**
 * Result of rendering QMD content to HTML.
 */
export interface RenderResult {
  html: string;
  success: boolean;
  error?: string;
  /** Structured error diagnostics with line/column information for Monaco. */
  diagnostics?: Diagnostic[];
  /** Structured warning diagnostics with line/column information for Monaco. */
  warnings?: Diagnostic[];
}

/**
 * Options for the high-level renderToHtml function.
 *
 * Renders a document from the VFS using `render_qmd`. The document content
 * must already be in the VFS (e.g., via Automerge sync). Source location
 * tracking for scroll sync is controlled via runtime metadata, not per-render
 * options — use `setScrollSyncEnabled()` instead.
 */
export interface RenderToHtmlOptions {
  /**
   * Path to the document being rendered in the VFS.
   *
   * This is the Automerge path (e.g., "index.qmd" or "docs/chapter.qmd").
   * The VFS normalizes relative paths to `/project/` prefix internally.
   */
  documentPath: string;

  /**
   * Optional user-grammar discovery context. When present, `renderToHtml`
   * scans `files` for `_quarto/grammars/<name>/` subdirectories, loads
   * any new or changed grammars via `web-tree-sitter`, and passes a
   * `JsUserGrammars` handle to the render so the pipeline highlights
   * code blocks whose language class matches a loaded user grammar
   * before falling back to built-ins.
   *
   * Absent → render without user grammars (built-ins only), matching
   * pre-Phase-4.5 behavior.
   *
   * Phase 4.5 of `claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md`.
   */
  userGrammars?: UserGrammarDiscoveryContext;
}

/**
 * Callbacks the grammar cache needs to pull binary and text content
 * out of the project. Typically wired to `automergeSync` in
 * `App.tsx`. Assumed stable across renders (the cache memoizes on
 * them; changing them mid-lifetime invalidates nothing).
 */
export interface UserGrammarDiscoveryContext {
  /** Flat list of all project file paths, no leading slash. */
  files: readonly string[];
  getBinaryContent: (path: string) => Promise<Uint8Array | null>;
  getTextContent: (path: string) => Promise<string | null>;
}

// ============================================================================
// Project Creation Operations
// ============================================================================

/**
 * A project choice from the WASM module.
 */
export interface ProjectChoice {
  id: string;
  name: string;
  description: string;
}

/**
 * Response from get_project_choices()
 */
interface ProjectChoicesResponse {
  success: boolean;
  choices: ProjectChoice[];
}

/**
 * A project file from create_project()
 */
export interface ProjectFile {
  path: string;
  content_type: 'text' | 'binary';
  content: string;
  mime_type?: string;
}

/**
 * Response from create_project()
 */
export interface CreateProjectResponse {
  success: boolean;
  error?: string;
  files?: ProjectFile[];
}

/**
 * Get available project choices for the Create Project UI.
 *
 * Returns a list of project types that can be created.
 */
export async function getProjectChoices(): Promise<ProjectChoice[]> {
  await initWasm();
  const wasm = getWasm();
  const response: ProjectChoicesResponse = JSON.parse(wasm.get_project_choices());
  return response.choices;
}

/**
 * Create a new Quarto project.
 *
 * @param choiceId - The project choice ID (e.g., "website", "default")
 * @param title - The project title
 * @returns The list of files to create, or an error
 */
export async function createProject(choiceId: string, title: string): Promise<CreateProjectResponse> {
  await initWasm();
  const wasm = getWasm();
  return JSON.parse(await wasm.create_project(choiceId, title));
}

/**
 * Compute a SHA-256 hash of the given string, returned as hex.
 * Used for CSS version fingerprinting.
 */
async function computeHash(input: string): Promise<string> {
  const encoder = new TextEncoder();
  const data = encoder.encode(input);
  const hashBuffer = await crypto.subtle.digest('SHA-256', data);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map((b) => b.toString(16).padStart(2, '0')).join('');
}

// ============================================================================
// User-grammar cache + bridge prep (Phase 4.5)
// ============================================================================

/**
 * Module-scoped cache of loaded user-grammar highlighters. Persists
 * across renders so we don't re-parse `.wasm` bytes every keystroke.
 * Initialized lazily on the first render that supplies a user-grammar
 * context.
 *
 * Assumption: `UserGrammarDiscoveryContext`'s resolvers are stable for
 * the lifetime of the app session (hub-client wires them once at
 * startup to `automergeSync`). If this stops being true, the cache
 * must be re-initialized when the resolvers change.
 */
let userGrammarCache: UserGrammarCache | null = null;

/**
 * Scan `ctx.files` for `_quarto/grammars/<name>/` subdirectories,
 * reconcile the cache, and return a fresh `JsUserGrammars` handle
 * wired to the cached highlighters. Call sites must pass the handle
 * to `renderQmd` (or `renderQmdContent`); wasm-bindgen's owned-arg
 * semantics consume the handle per call, which is fine since
 * construction is cheap.
 */
async function prepareUserGrammarsHandle(
  ctx: UserGrammarDiscoveryContext,
): Promise<JsUserGrammarsHandle> {
  if (!userGrammarCache) {
    userGrammarCache = new UserGrammarCache({
      loadUserGrammar,
      getBinaryContent: ctx.getBinaryContent,
      getTextContent: ctx.getTextContent,
    });
  }

  const descriptors = discoverUserGrammars(ctx.files);
  const syncResult = await userGrammarCache.sync(descriptors);

  // Grammar failures degrade gracefully: the rest of the render
  // proceeds and the affected code blocks fall through to built-ins
  // (or render unstyled). We surface a console.warn so the user can
  // notice; promoting to a diagnostic in the rendered output is a
  // UX question we defer until we have a place to show it.
  for (const failure of syncResult.failures) {
    console.warn(
      `[userGrammars] failed to load grammar '${failure.class}': ${failure.reason}`,
    );
  }

  const wasm = getWasm();
  const handle = new wasm.JsUserGrammars();
  userGrammarCache.registerInto(handle);
  return handle;
}

/**
 * Test-only: drop the module-scoped user-grammar cache. Unit tests
 * that want to start from a clean slate across describe blocks call
 * this in `afterEach`.
 */
export function _resetUserGrammarCacheForTest(): void {
  userGrammarCache?.disposeAll();
  userGrammarCache = null;
}

/**
 * Render a VFS document to HTML, handling errors gracefully.
 *
 * The document must already be in the VFS (via Automerge sync or manual add).
 * Uses `render_qmd` which discovers project context (_quarto.yml, _metadata.yml)
 * and merges all metadata layers including runtime metadata.
 *
 * Returns structured diagnostics with source locations that can be
 * converted to Monaco editor markers using diagnosticsToMarkers().
 *
 * @param options - Render options with required documentPath
 */
export async function renderToHtml(
  options: RenderToHtmlOptions
): Promise<RenderResult> {
  try {
    await initWasm();

    const { documentPath, userGrammars } = options;

    // Resolve and register any user-defined tree-sitter grammars
    // before the render. Cache + bridge live at module scope so
    // repeated renders reuse loaded grammars; the bridge handle
    // itself is constructed per-call because wasm-bindgen consumes it.
    let grammarsHandle: JsUserGrammarsHandle | undefined;
    if (userGrammars) {
      grammarsHandle = await prepareUserGrammarsHandle(userGrammars);
    }

    // Phase 9: render with full project context. The WASM side
    // handles project discovery itself — single-file projects fall
    // through to the same path `renderQmd` used to take, so this
    // switch is unconditional.
    const result: RenderResponse = await renderPageInProject(
      documentPath,
      grammarsHandle,
    );

    if (result.success) {
      // Compute CSS version from the pipeline's CSS artifact in VFS.
      // CompileThemeCssStage writes correct theme CSS to the VFS artifact.
      // The cssVersion changes when CSS content changes, ensuring HTML differs
      // even when document structure is the same (e.g., only theme name changed).
      let cssVersion = 'default';
      try {
        const cssResult = vfsReadFile('/.quarto/project-artifacts/styles.css');
        if (cssResult.success && cssResult.content) {
          cssVersion = await computeHash(cssResult.content);
        }
      } catch (cssErr) {
        console.warn('[renderToHtml] Failed to read CSS artifact for versioning:', cssErr);
      }

      // Append CSS version as HTML comment to ensure HTML changes when CSS changes
      // This forces MorphIframe to re-apply CSS even when
      // only the theme changed (document structure unchanged)
      const htmlWithCssVersion = (result.html || '') + `<!-- css-version: ${cssVersion} -->`;

      return {
        html: htmlWithCssVersion,
        success: true,
        warnings: result.warnings,
      };
    } else {
      // Extract error message
      const errorMsg = result.error || 'Unknown render error';

      return {
        html: '',
        success: false,
        error: errorMsg,
        diagnostics: result.diagnostics,
        warnings: result.warnings,
      };
    }
  } catch (err) {
    console.error('Render error:', err);
    return {
      html: '',
      success: false,
      error: err instanceof Error ? err.message : JSON.stringify(err),
    };
  }
}

/**
 * Render standalone QMD content to HTML (no VFS or project context).
 *
 * Use this for rendering content that doesn't live in the VFS, such as
 * changelog markdown or static documentation. For VFS-based rendering
 * with project context, use `renderToHtml()` instead.
 *
 * @param qmdContent - The QMD source content to render
 */
export async function renderContentToHtml(
  qmdContent: string
): Promise<RenderResult> {
  try {
    await initWasm();

    const result: RenderResponse = await renderQmdContent(qmdContent, htmlTemplateBundle || '');

    if (result.success) {
      return {
        html: result.html || '',
        success: true,
        warnings: result.warnings,
      };
    } else {
      return {
        html: '',
        success: false,
        error: result.error || 'Unknown render error',
        diagnostics: result.diagnostics,
        warnings: result.warnings,
      };
    }
  } catch (err) {
    console.error('Render error:', err);
    return {
      html: '',
      success: false,
      error: err instanceof Error ? err.message : JSON.stringify(err),
    };
  }
}

// ============================================================================
// SASS Compilation Status
// ============================================================================

/**
 * Check if SASS compilation is available.
 */
export async function sassAvailable(): Promise<boolean> {
  await initWasm();
  const wasm = getWasm();
  return wasm.sass_available();
}

/**
 * Get the name of the SASS compiler being used.
 */
export async function sassCompilerName(): Promise<string | null> {
  await initWasm();
  const wasm = getWasm();
  return wasm.sass_compiler_name() ?? null;
}
