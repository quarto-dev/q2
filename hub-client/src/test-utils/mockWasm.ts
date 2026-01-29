/**
 * Mock WASM Renderer for Testing
 *
 * Provides a mock implementation of the WASM renderer service for
 * unit and integration tests. Supports:
 * - VFS operations (add, remove, list, read files)
 * - Rendering QMD to HTML
 * - SASS compilation
 * - Configurable responses and error injection
 */

import type { Diagnostic, RenderResponse } from '../types/diagnostic';

/**
 * VFS response type matching the real wasmRenderer.ts
 */
export interface VfsResponse {
  success: boolean;
  error?: string;
  files?: string[];
  content?: string;
}

/**
 * Render result matching the real wasmRenderer.ts
 */
export interface RenderResult {
  html: string;
  success: boolean;
  error?: string;
  diagnostics?: Diagnostic[];
  warnings?: Diagnostic[];
}

/**
 * Options for configuring the mock WASM renderer.
 */
export interface MockWasmOptions {
  /** Default HTML to return from render operations */
  renderResult?: string;
  /** Error to throw from render operations */
  renderError?: Error;
  /** Initial VFS files */
  vfsFiles?: Map<string, string | Uint8Array>;
  /** Whether SASS is available */
  sassAvailable?: boolean;
  /** CSS to return from SASS compilation */
  compiledCss?: string;
  /** Error to throw from SASS compilation */
  sassError?: Error;
  /** Diagnostics to include in render results */
  diagnostics?: Diagnostic[];
  /** Warnings to include in render results */
  warnings?: Diagnostic[];
}

/**
 * Mock WASM renderer interface with test helpers.
 */
export interface MockWasmRenderer {
  // Lifecycle
  initWasm(): Promise<void>;
  isWasmReady(): boolean;

  // VFS operations
  vfsAddFile(path: string, content: string): VfsResponse;
  vfsAddBinaryFile(path: string, content: Uint8Array): VfsResponse;
  vfsRemoveFile(path: string): VfsResponse;
  vfsListFiles(): VfsResponse;
  vfsClear(): VfsResponse;
  vfsReadFile(path: string): VfsResponse;
  vfsReadBinaryFile(path: string): VfsResponse;

  // Rendering operations
  renderQmd(path: string): Promise<RenderResponse>;
  renderQmdContent(content: string, templateBundle?: string): Promise<RenderResponse>;
  renderToHtml(content: string, options?: { sourceLocation?: boolean; documentPath?: string }): Promise<RenderResult>;

  // SASS operations
  sassAvailable(): Promise<boolean>;
  compileScss(scss: string, options?: { minified?: boolean }): Promise<string>;
  compileDocumentCss(content: string, options?: { minified?: boolean; documentPath?: string }): Promise<string>;

  // Test helpers
  _getVfs(): Map<string, string | Uint8Array>;
  _setRenderResult(html: string): void;
  _setRenderError(error: Error | null): void;
  _setDiagnostics(diagnostics: Diagnostic[]): void;
  _setWarnings(warnings: Diagnostic[]): void;
  _setSassAvailable(available: boolean): void;
  _setSassError(error: Error | null): void;
  _reset(): void;
}

/**
 * Create a mock WASM renderer for testing.
 *
 * @param options - Configuration options
 * @returns A mock WASM renderer with test helpers
 *
 * @example
 * ```typescript
 * const mockWasm = createMockWasmRenderer({
 *   renderResult: '<div>Rendered content</div>',
 *   sassAvailable: true,
 * });
 *
 * await mockWasm.initWasm();
 * const result = await mockWasm.renderToHtml('# Hello World');
 * expect(result.success).toBe(true);
 * expect(result.html).toContain('Rendered content');
 * ```
 */
export function createMockWasmRenderer(options: MockWasmOptions = {}): MockWasmRenderer {
  const vfs = new Map<string, string | Uint8Array>(options.vfsFiles || []);
  let initialized = false;
  let renderResult = options.renderResult || '<div>Mock rendered content</div>';
  let renderError: Error | null = options.renderError || null;
  let isSassAvailable = options.sassAvailable ?? true;
  let compiledCss = options.compiledCss || '/* mock compiled CSS */';
  let sassError: Error | null = options.sassError || null;
  let diagnostics: Diagnostic[] = options.diagnostics || [];
  let warnings: Diagnostic[] = options.warnings || [];

  const renderer: MockWasmRenderer = {
    async initWasm(): Promise<void> {
      initialized = true;
    },

    isWasmReady(): boolean {
      return initialized;
    },

    // VFS operations
    vfsAddFile(path: string, content: string): VfsResponse {
      vfs.set(path, content);
      return { success: true };
    },

    vfsAddBinaryFile(path: string, content: Uint8Array): VfsResponse {
      vfs.set(path, content);
      return { success: true };
    },

    vfsRemoveFile(path: string): VfsResponse {
      const existed = vfs.has(path);
      vfs.delete(path);
      return { success: existed };
    },

    vfsListFiles(): VfsResponse {
      return { success: true, files: Array.from(vfs.keys()) };
    },

    vfsClear(): VfsResponse {
      vfs.clear();
      return { success: true };
    },

    vfsReadFile(path: string): VfsResponse {
      const content = vfs.get(path);
      if (content === undefined) {
        return { success: false, error: `File not found: ${path}` };
      }
      if (content instanceof Uint8Array) {
        return { success: false, error: `File is binary: ${path}` };
      }
      return { success: true, content };
    },

    vfsReadBinaryFile(path: string): VfsResponse {
      const content = vfs.get(path);
      if (content === undefined) {
        return { success: false, error: `File not found: ${path}` };
      }
      if (typeof content === 'string') {
        return { success: false, error: `File is text: ${path}` };
      }
      // Return base64-encoded content as the real implementation does
      const base64 = btoa(String.fromCharCode(...content));
      return { success: true, content: base64 };
    },

    // Rendering operations
    async renderQmd(path: string): Promise<RenderResponse> {
      if (renderError) {
        return {
          success: false,
          error: renderError.message,
          diagnostics,
          warnings,
        };
      }

      const content = vfs.get(path);
      if (content === undefined) {
        return {
          success: false,
          error: `File not found: ${path}`,
          diagnostics,
          warnings,
        };
      }

      return {
        success: true,
        html: renderResult,
        warnings,
      };
    },

    async renderQmdContent(_content: string, _templateBundle?: string): Promise<RenderResponse> {
      if (renderError) {
        return {
          success: false,
          error: renderError.message,
          diagnostics,
          warnings,
        };
      }

      return {
        success: true,
        html: renderResult,
        warnings,
      };
    },

    async renderToHtml(
      _content: string,
      _options?: { sourceLocation?: boolean; documentPath?: string },
    ): Promise<RenderResult> {
      if (renderError) {
        return {
          html: '',
          success: false,
          error: renderError.message,
          diagnostics,
          warnings,
        };
      }

      return {
        html: renderResult,
        success: true,
        warnings: warnings.length > 0 ? warnings : undefined,
      };
    },

    // SASS operations
    async sassAvailable(): Promise<boolean> {
      return isSassAvailable;
    },

    async compileScss(_scss: string, _options?: { minified?: boolean }): Promise<string> {
      if (sassError) {
        throw sassError;
      }
      if (!isSassAvailable) {
        throw new Error('SASS compilation is not available');
      }
      return compiledCss;
    },

    async compileDocumentCss(
      _content: string,
      _options?: { minified?: boolean; documentPath?: string },
    ): Promise<string> {
      if (sassError) {
        throw sassError;
      }
      if (!isSassAvailable) {
        throw new Error('SASS compilation is not available');
      }
      return compiledCss;
    },

    // Test helpers
    _getVfs(): Map<string, string | Uint8Array> {
      return new Map(vfs);
    },

    _setRenderResult(html: string): void {
      renderResult = html;
    },

    _setRenderError(error: Error | null): void {
      renderError = error;
    },

    _setDiagnostics(newDiagnostics: Diagnostic[]): void {
      diagnostics = newDiagnostics;
    },

    _setWarnings(newWarnings: Diagnostic[]): void {
      warnings = newWarnings;
    },

    _setSassAvailable(available: boolean): void {
      isSassAvailable = available;
    },

    _setSassError(error: Error | null): void {
      sassError = error;
    },

    _reset(): void {
      vfs.clear();
      initialized = false;
      renderResult = '<div>Mock rendered content</div>';
      renderError = null;
      isSassAvailable = true;
      compiledCss = '/* mock compiled CSS */';
      sassError = null;
      diagnostics = [];
      warnings = [];
    },
  };

  return renderer;
}
