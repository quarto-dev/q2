/**
 * WASM Renderer Service
 *
 * Provides typed access to the wasm-quarto-hub-client module for
 * VFS operations and QMD rendering.
 */

// Response types from WASM module
interface VfsResponse {
  success: boolean;
  error?: string;
  files?: string[];
  content?: string;
}

interface RenderResponse {
  success?: boolean;
  error?: string;
  message?: string;
  html?: string;
  output?: string;
  diagnostics?: string[];
}

// WASM module state
let wasmModule: typeof import('wasm-quarto-hub-client') | null = null;
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

        wasmModule = wasm;

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

// ============================================================================
// Rendering Operations
// ============================================================================

/**
 * Render a QMD file from the virtual filesystem
 */
export function renderQmd(path: string): RenderResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.render_qmd(path));
}

/**
 * Render QMD content directly (without VFS)
 */
export function renderQmdContent(content: string, templateBundle: string = ''): RenderResponse {
  const wasm = getWasm();
  return JSON.parse(wasm.render_qmd_content(content, templateBundle));
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
 * Render QMD content to HTML, handling errors gracefully
 */
export async function renderToHtml(qmdContent: string): Promise<{
  html: string;
  success: boolean;
  error?: string;
  diagnostics?: string[];
}> {
  try {
    await initWasm();

    // Debug: log what we're rendering
    console.log('Rendering QMD content, length:', qmdContent.length);
    console.log('Template bundle available:', !!htmlTemplateBundle);

    const result = renderQmdContent(qmdContent, htmlTemplateBundle || '');
    console.log('Render result:', result);
    console.log('Result keys:', Object.keys(result));

    // Check for success: either explicit success flag, or presence of output/html
    const hasOutput = !!(result.html || result.output);
    const hasError = !!(result.error || result.message);
    const isSuccess = result.success === true || (hasOutput && !hasError);

    if (isSuccess) {
      return {
        html: result.html || result.output || '',
        success: true,
      };
    } else {
      // Extract error message, handling various formats
      let errorMsg = 'Unknown render error';
      if (result.error) {
        errorMsg = typeof result.error === 'string'
          ? result.error
          : JSON.stringify(result.error);
      } else if (result.message) {
        errorMsg = result.message;
      }

      return {
        html: '',
        success: false,
        error: errorMsg,
        diagnostics: result.diagnostics,
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
