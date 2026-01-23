/**
 * WASM Renderer Service
 *
 * Provides typed access to the wasm-quarto-hub-client module for
 * VFS operations, QMD rendering, and SASS compilation.
 */

import type { Diagnostic, RenderResponse } from '../types/diagnostic';
import { getSassCache, computeHash } from './sassCache';

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
  render_qmd: (path: string) => Promise<string>;
  render_qmd_content: (content: string, templateBundle: string) => Promise<string>;
  render_qmd_content_with_options: (content: string, templateBundle: string, options: string) => Promise<string>;
  get_builtin_template: (name: string) => string;
  get_project_choices: () => string;
  create_project: (choiceId: string, title: string) => Promise<string>;
  lsp_analyze_document: (path: string) => string;
  lsp_get_symbols: (path: string) => string;
  lsp_get_folding_ranges: (path: string) => string;
  lsp_get_diagnostics: (path: string) => string;
  // SASS compilation functions (new)
  sass_available: () => boolean;
  sass_compiler_name: () => string | undefined;
  compile_scss: (scss: string, minified: boolean, loadPathsJson: string) => Promise<string>;
  compile_scss_with_bootstrap: (scss: string, minified: boolean) => Promise<string>;
}

// WASM module state
let wasmModule: WasmModuleExtended | null = null;
let initPromise: Promise<void> | null = null;
let htmlTemplateBundle: string | null = null;

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
// Rendering Operations
// ============================================================================

/**
 * Render a QMD file from the virtual filesystem
 */
export async function renderQmd(path: string): Promise<RenderResponse> {
  const wasm = getWasm();
  return JSON.parse(await wasm.render_qmd(path));
}

/**
 * Render QMD content directly (without VFS)
 */
export async function renderQmdContent(content: string, templateBundle: string = ''): Promise<RenderResponse> {
  const wasm = getWasm();
  return JSON.parse(await wasm.render_qmd_content(content, templateBundle));
}

/**
 * Options for rendering QMD content.
 */
export interface WasmRenderOptions {
  /**
   * Enable source location tracking in HTML output.
   *
   * When true, adds `data-loc` attributes to HTML elements for scroll sync.
   */
  sourceLocation?: boolean;
}

/**
 * Render QMD content with options (without VFS)
 */
export async function renderQmdContentWithOptions(
  content: string,
  templateBundle: string = '',
  options: WasmRenderOptions = {}
): Promise<RenderResponse> {
  const wasm = getWasm();
  const optionsJson = JSON.stringify({
    source_location: options.sourceLocation ?? false,
  });
  return JSON.parse(await wasm.render_qmd_content_with_options(content, templateBundle, optionsJson));
}

/**
 * Get a built-in template bundle
 */
export function getBuiltinTemplate(name: string): string {
  const wasm = getWasm();
  return wasm.get_builtin_template(name);
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
 */
export interface RenderToHtmlOptions {
  /**
   * Enable source location tracking in HTML output.
   *
   * When true, adds `data-loc` attributes to HTML elements for scroll sync.
   * Default: false
   */
  sourceLocation?: boolean;
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
 * Render QMD content to HTML, handling errors gracefully.
 *
 * Returns structured diagnostics with source locations that can be
 * converted to Monaco editor markers using diagnosticsToMarkers().
 *
 * @param qmdContent - The QMD source content to render
 * @param options - Optional render options (e.g., enable source location tracking)
 */
export async function renderToHtml(
  qmdContent: string,
  options: RenderToHtmlOptions = {}
): Promise<RenderResult> {
  try {
    await initWasm();

    console.log('[renderToHtml] sourceLocation option:', options.sourceLocation);

    // Use the options-aware render function if options are specified
    const result: RenderResponse = options.sourceLocation
      ? await renderQmdContentWithOptions(qmdContent, htmlTemplateBundle || '', {
          sourceLocation: options.sourceLocation,
        })
      : await renderQmdContent(qmdContent, htmlTemplateBundle || '');

    console.log('[renderToHtml] HTML has data-loc:', result.html?.includes('data-loc'));

    if (result.success) {
      return {
        html: result.html || '',
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

// ============================================================================
// SASS Compilation Operations
// ============================================================================

/**
 * Response from SASS compilation.
 */
interface SassCompileResponse {
  success: boolean;
  css?: string;
  error?: string;
}

/**
 * Options for SASS compilation.
 */
export interface SassCompileOptions {
  /** Whether to produce minified output */
  minified?: boolean;
  /** Additional load paths for @use/@import resolution */
  loadPaths?: string[];
  /** Whether to skip caching (for debugging) */
  skipCache?: boolean;
}

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

/**
 * Compile SCSS to CSS with caching.
 *
 * Uses IndexedDB cache to avoid recompilation of unchanged SCSS.
 * The cache key is based on the SCSS content and compilation options.
 *
 * @param scss - The SCSS source code to compile
 * @param options - Compilation options (minified, loadPaths, etc.)
 * @returns The compiled CSS
 * @throws Error if compilation fails
 *
 * @example
 * ```typescript
 * const css = await compileScss('$primary: blue; .btn { color: $primary; }');
 * console.log(css);
 * // .btn { color: blue; }
 * ```
 */
export async function compileScss(
  scss: string,
  options: SassCompileOptions = {}
): Promise<string> {
  await initWasm();
  const wasm = getWasm();

  const minified = options.minified ?? false;
  const loadPaths = options.loadPaths ?? [];
  const skipCache = options.skipCache ?? false;

  // Compute cache key
  const cache = getSassCache();
  const cacheKey = await cache.computeKey(scss, minified);

  // Check cache first (unless explicitly skipped)
  if (!skipCache) {
    const cached = await cache.get(cacheKey);
    if (cached !== null) {
      console.log('[compileScss] Cache hit');
      return cached;
    }
    console.log('[compileScss] Cache miss');
  }

  // Compile via WASM
  const loadPathsJson = JSON.stringify(loadPaths);
  const result: SassCompileResponse = JSON.parse(
    await wasm.compile_scss(scss, minified, loadPathsJson)
  );

  if (!result.success) {
    throw new Error(result.error || 'SASS compilation failed');
  }

  const css = result.css || '';

  // Cache the result
  if (!skipCache) {
    const sourceHash = await computeHash(scss);
    await cache.set(cacheKey, css, sourceHash, minified);
  }

  return css;
}

/**
 * Compile SCSS with Bootstrap included in load paths.
 *
 * Convenience function that automatically includes the embedded Bootstrap SCSS
 * files in the load paths. Use this when compiling SCSS that depends on Bootstrap.
 *
 * @param scss - The SCSS source code to compile
 * @param options - Additional compilation options
 * @returns The compiled CSS
 *
 * @example
 * ```typescript
 * // Compile SCSS that uses Bootstrap variables
 * const css = await compileScssWithBootstrap(`
 *   @import "bootstrap";
 *   .custom-btn { color: $primary; }
 * `);
 * ```
 */
export async function compileScssWithBootstrap(
  scss: string,
  options: Omit<SassCompileOptions, 'loadPaths'> = {}
): Promise<string> {
  // Include embedded Bootstrap SCSS in load paths
  const bootstrapLoadPath = '/__quarto_resources__/bootstrap/scss';
  return compileScss(scss, {
    ...options,
    loadPaths: [bootstrapLoadPath],
  });
}

/**
 * Clear the SASS compilation cache.
 *
 * Use this to force recompilation of all SCSS files.
 */
export async function clearSassCache(): Promise<void> {
  const cache = getSassCache();
  await cache.clear();
  console.log('[clearSassCache] Cache cleared');
}

/**
 * Get statistics about the SASS cache.
 */
export async function getSassCacheStats() {
  const cache = getSassCache();
  return cache.getStats();
}
