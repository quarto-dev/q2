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
  parse_qmd_to_ast: (content: string) => string;
  lsp_analyze_document: (path: string) => string;
  lsp_get_symbols: (path: string) => string;
  lsp_get_folding_ranges: (path: string) => string;
  lsp_get_diagnostics: (path: string) => string;
  // SASS compilation functions (new)
  sass_available: () => boolean;
  sass_compiler_name: () => string | undefined;
  // Hash of embedded SCSS resources (for cache invalidation)
  get_scss_resources_version: () => string;
  compile_scss: (scss: string, minified: boolean, loadPathsJson: string) => Promise<string>;
  compile_scss_with_bootstrap: (scss: string, minified: boolean) => Promise<string>;
  // Theme-aware CSS compilation (extracts theme from frontmatter)
  compile_document_css: (content: string, documentPath: string) => Promise<string>;
  compile_theme_css_by_name: (themeName: string, minified: boolean) => Promise<string>;
  compile_default_bootstrap_css: (minified: boolean) => Promise<string>;
  // Content-based hash for cache keys
  compute_theme_content_hash: (content: string, documentPath: string) => string;
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

        // Set up VFS callbacks for SASS importer
        // This allows dart-sass to read Bootstrap SCSS files from the VFS
        await setupSassVfsCallbacks();

        // Check if embedded SCSS resources changed and invalidate cache if needed
        await checkAndInvalidateSassCache();

        console.log('WASM module initialized successfully, template loaded');
      } catch (err) {
        initPromise = null;
        throw err;
      }
    })();
  }

  return initPromise;
}

// Key for storing SCSS resources version in localStorage
const SCSS_VERSION_STORAGE_KEY = 'quarto-scss-resources-version';

/**
 * Check if the embedded SCSS resources have changed and invalidate cache if needed.
 *
 * This compares the current SCSS resources hash (computed at WASM build time)
 * against the stored version. If they differ, the SASS cache is cleared.
 * This ensures that when hub-client is updated with new SCSS files, users
 * don't see stale cached CSS.
 */
async function checkAndInvalidateSassCache(): Promise<void> {
  const wasm = getWasm();

  try {
    const currentVersion = wasm.get_scss_resources_version();
    const storedVersion = localStorage.getItem(SCSS_VERSION_STORAGE_KEY);

    if (storedVersion !== currentVersion) {
      console.log(
        '[SASS Cache] SCSS resources version changed:',
        storedVersion,
        '->',
        currentVersion
      );

      // Clear the SASS cache
      const cache = getSassCache();
      await cache.clear();
      console.log('[SASS Cache] Cache cleared due to SCSS resources update');

      // Store the new version
      localStorage.setItem(SCSS_VERSION_STORAGE_KEY, currentVersion);
    } else {
      console.log('[SASS Cache] SCSS resources version unchanged:', currentVersion);
    }
  } catch (err) {
    console.warn('[SASS Cache] Failed to check SCSS resources version:', err);
  }
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

/**
 * Parse QMD content to Pandoc AST JSON.
 *
 * This function parses QMD markdown into a Pandoc AST representation,
 * which can be used for programmatic manipulation, analysis, or rendering
 * with custom React components.
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
 * @param content - QMD source text to parse
 * @returns Pandoc AST as a JSON string
 * @throws Error with diagnostic information if parsing fails
 */
export async function parseQmdToAst(
  content: string
): Promise<string> {
  await initWasm();
  const wasm = getWasm();
  const responseJson = wasm.parse_qmd_to_ast(content);

  // Parse the response to check for errors
  interface ParseResponse {
    success: boolean;
    ast?: string;
    error?: string;
    diagnostics?: Array<{ message: string }>;
  }

  const response: ParseResponse = JSON.parse(responseJson);

  if (!response.success) {
    // Construct error message with diagnostics
    let errorMsg = response.error || 'Failed to parse QMD content';
    if (response.diagnostics && response.diagnostics.length > 0) {
      const diagMessages = response.diagnostics.map(d => d.message).join('\n\n');
      errorMsg = `${errorMsg}\n\n${diagMessages}`;
    }
    throw new Error(errorMsg);
  }

  return response.ast || '{}';
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

  /**
   * Path to the document being rendered in the VFS.
   *
   * Used to resolve relative paths in theme specifications. For example,
   * if a document at `docs/index.qmd` references `editorial_marks.scss`,
   * the theme file will be looked up at `/project/docs/editorial_marks.scss`.
   *
   * Default: "input.qmd" (VFS normalizes to "/project/input.qmd")
   */
  documentPath?: string;
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
      // Compile theme CSS and update VFS
      // The cssVersion changes when CSS content changes, ensuring HTML differs
      // even when document structure is the same (e.g., only theme name changed)
      let cssVersion = 'default';
      // Use relative path as default so VFS normalizes it correctly (e.g., "input.qmd" -> "/project/input.qmd")
      const documentPath = options.documentPath ?? 'input.qmd';
      try {
        cssVersion = await compileAndInjectThemeCss(qmdContent, documentPath);
      } catch (cssErr) {
        console.warn('[renderToHtml] Theme CSS compilation failed, using default CSS:', cssErr);
      }

      // Append CSS version as HTML comment to ensure HTML changes when CSS changes
      // This forces DoubleBufferedIframe to swap and re-apply CSS even when
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
 * Compile theme CSS from document content and inject into VFS.
 *
 * This replaces the default static CSS at /.quarto/project-artifacts/styles.css
 * with compiled theme CSS based on the document's frontmatter.
 *
 * @param qmdContent - The QMD document content
 * @param documentPath - Path to the document in VFS (e.g., "/docs/index.qmd")
 * @returns A version string that changes when CSS content changes (for cache busting)
 * @internal
 */
async function compileAndInjectThemeCss(qmdContent: string, documentPath: string): Promise<string> {
  const wasm = getWasm();

  // Check if SASS is available
  if (!wasm.sass_available()) {
    console.log('[compileAndInjectThemeCss] SASS not available, keeping default CSS');
    return 'no-sass';
  }

  // Extract theme config for versioning - this determines the CSS output
  const themeConfig = extractThemeConfigForCacheKey(qmdContent);

  // Compile CSS with caching, passing the document path for relative theme resolution
  console.log('[compileAndInjectThemeCss] documentPath:', documentPath);
  const css = await compileDocumentCss(qmdContent, { minified: true, documentPath });

  // Update VFS with compiled CSS
  const cssPath = '/.quarto/project-artifacts/styles.css';
  vfsAddFile(cssPath, css);
  console.log('[compileAndInjectThemeCss] Updated VFS with compiled theme CSS, theme:', themeConfig);

  // Return the theme config as the version - this changes exactly when the theme changes
  return themeConfig;
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

// ============================================================================
// Theme CSS Compilation
// ============================================================================

/**
 * Response from theme CSS compilation.
 */
interface ThemeCssResponse {
  success: boolean;
  css?: string;
  error?: string;
}

/**
 * Response from theme content hash computation.
 */
interface ThemeHashResponse {
  success: boolean;
  hash?: string;
  error?: string;
}

/**
 * Extract theme configuration string from QMD frontmatter for cache key computation.
 *
 * Returns a normalized string representation of the theme config that can be
 * used as part of a cache key. Returns 'default' if no theme is specified.
 *
 * @param content - QMD document content with YAML frontmatter
 * @returns Normalized theme config string for cache key
 */
export function extractThemeConfigForCacheKey(content: string): string {
  // Find YAML frontmatter
  const trimmed = content.trimStart();
  if (!trimmed.startsWith('---')) {
    return 'default';
  }

  // Find closing ---
  const afterFirst = trimmed.slice(3);
  const endPos = afterFirst.indexOf('\n---');
  if (endPos === -1) {
    return 'default';
  }

  const yaml = afterFirst.slice(0, endPos);

  // Simple regex to extract theme value from format.html.theme
  // This handles:
  // - theme: cosmo
  // - theme: [cosmo, custom.scss]
  // - theme:
  //     - cosmo
  //     - custom.scss
  const themeMatch = yaml.match(/^\s*theme:\s*(.+?)(?:\n(?=\s*\w+:)|\n(?=---)|\n*$)/ms);
  if (!themeMatch) {
    // Check if there's a format.html section
    const formatMatch = yaml.match(/format:\s*\n\s+html:\s*\n([\s\S]*?)(?:\n(?=\s*\w+:)|\n(?=---)|\n*$)/m);
    if (formatMatch) {
      const htmlSection = formatMatch[1];
      const innerThemeMatch = htmlSection.match(/^\s*theme:\s*(.+?)(?:\n(?=\s{2,}\w+:)|\n(?=---)|\n*$)/ms);
      if (innerThemeMatch) {
        return innerThemeMatch[1].trim();
      }
    }
    return 'default';
  }

  return themeMatch[1].trim();
}

/**
 * Compile CSS for a QMD document's theme configuration with caching.
 *
 * Extracts the theme from the document's YAML frontmatter and compiles
 * the appropriate Bootstrap/Bootswatch CSS. Results are cached in IndexedDB
 * based on the theme configuration and minification setting.
 *
 * @param content - The QMD document content (must include YAML frontmatter)
 * @param options - Compilation options
 * @returns The compiled CSS
 * @throws Error if compilation fails
 *
 * @example
 * ```typescript
 * const qmd = `---
 * title: My Document
 * format:
 *   html:
 *     theme: cosmo
 * ---
 *
 * # Hello World
 * `;
 * const css = await compileDocumentCss(qmd, { documentPath: '/index.qmd' });
 * ```
 */
export async function compileDocumentCss(
  content: string,
  options: { minified?: boolean; skipCache?: boolean; documentPath?: string } = {}
): Promise<string> {
  await initWasm();
  const wasm = getWasm();

  // Check if SASS is available
  if (!wasm.sass_available()) {
    throw new Error('SASS compilation is not available');
  }

  const minified = options.minified ?? true;
  const skipCache = options.skipCache ?? false;
  // Use relative path as default so VFS normalizes it correctly (e.g., "input.qmd" -> "/project/input.qmd")
  const documentPath = options.documentPath ?? 'input.qmd';

  // Compute content-based hash for cache key
  // This hash changes when any source file (built-in or custom SCSS) changes
  const hashResult: ThemeHashResponse = JSON.parse(
    wasm.compute_theme_content_hash(content, documentPath)
  );

  if (!hashResult.success) {
    throw new Error(hashResult.error || 'Failed to compute theme content hash');
  }

  const contentHash = hashResult.hash!;
  // Use "theme-v2" prefix to avoid conflicts with old filename-based cache entries
  const cacheKey = `theme-v2:${contentHash}:minified=${minified}`;

  // Check cache first (unless explicitly skipped)
  const cache = getSassCache();

  if (!skipCache) {
    const cached = await cache.get(cacheKey);
    if (cached !== null) {
      console.log('[compileDocumentCss] Cache hit for hash:', contentHash.slice(0, 8));
      return cached;
    }
    console.log('[compileDocumentCss] Cache miss for hash:', contentHash.slice(0, 8));
  }

  // Compile via WASM (extracts theme from frontmatter and compiles)
  // Pass document path for resolving relative theme file paths
  const result: ThemeCssResponse = JSON.parse(
    await wasm.compile_document_css(content, documentPath)
  );

  if (!result.success) {
    throw new Error(result.error || 'Theme CSS compilation failed');
  }

  const css = result.css || '';

  // Cache the result using the content hash as source identifier
  if (!skipCache) {
    await cache.set(cacheKey, css, contentHash, minified);
  }

  return css;
}

/**
 * Compile CSS for a specific Bootswatch theme by name with caching.
 *
 * @param themeName - The theme name (e.g., "cosmo", "darkly", "flatly")
 * @param options - Compilation options
 * @returns The compiled CSS
 * @throws Error if compilation fails
 *
 * @example
 * ```typescript
 * const css = await compileThemeCssByName('cosmo');
 * ```
 */
export async function compileThemeCssByName(
  themeName: string,
  options: { minified?: boolean; skipCache?: boolean } = {}
): Promise<string> {
  await initWasm();
  const wasm = getWasm();

  if (!wasm.sass_available()) {
    throw new Error('SASS compilation is not available');
  }

  const minified = options.minified ?? true;
  const skipCache = options.skipCache ?? false;

  // Cache key based on theme name and minification
  const cacheInput = `theme:${themeName}:minified=${minified}`;
  const cache = getSassCache();
  const cacheKey = await cache.computeKey(cacheInput, minified);

  if (!skipCache) {
    const cached = await cache.get(cacheKey);
    if (cached !== null) {
      console.log('[compileThemeCssByName] Cache hit for:', themeName);
      return cached;
    }
    console.log('[compileThemeCssByName] Cache miss for:', themeName);
  }

  // Compile via WASM
  const result: ThemeCssResponse = JSON.parse(
    await wasm.compile_theme_css_by_name(themeName, minified)
  );

  if (!result.success) {
    throw new Error(result.error || `Failed to compile theme: ${themeName}`);
  }

  const css = result.css || '';

  if (!skipCache) {
    const sourceHash = await computeHash(cacheInput);
    await cache.set(cacheKey, css, sourceHash, minified);
  }

  return css;
}

/**
 * Compile default Bootstrap CSS (no theme customization) with caching.
 *
 * @param options - Compilation options
 * @returns The compiled CSS
 * @throws Error if compilation fails
 */
export async function compileDefaultBootstrapCss(
  options: { minified?: boolean; skipCache?: boolean } = {}
): Promise<string> {
  await initWasm();
  const wasm = getWasm();

  if (!wasm.sass_available()) {
    throw new Error('SASS compilation is not available');
  }

  const minified = options.minified ?? true;
  const skipCache = options.skipCache ?? false;

  // Cache key for default Bootstrap
  const cacheInput = `theme:default-bootstrap:minified=${minified}`;
  const cache = getSassCache();
  const cacheKey = await cache.computeKey(cacheInput, minified);

  if (!skipCache) {
    const cached = await cache.get(cacheKey);
    if (cached !== null) {
      console.log('[compileDefaultBootstrapCss] Cache hit');
      return cached;
    }
    console.log('[compileDefaultBootstrapCss] Cache miss');
  }

  // Compile via WASM
  const result: ThemeCssResponse = JSON.parse(
    await wasm.compile_default_bootstrap_css(minified)
  );

  if (!result.success) {
    throw new Error(result.error || 'Failed to compile default Bootstrap CSS');
  }

  const css = result.css || '';

  if (!skipCache) {
    const sourceHash = await computeHash(cacheInput);
    await cache.set(cacheKey, css, sourceHash, minified);
  }

  return css;
}
