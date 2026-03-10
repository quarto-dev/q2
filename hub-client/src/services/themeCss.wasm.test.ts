/**
 * WASM tests for theme CSS compilation through the render pipeline.
 *
 * These tests verify that theme configuration (from project config, document
 * frontmatter, and runtime metadata) correctly flows through MetadataMergeStage
 * and CompileThemeCssStage to produce the expected compiled CSS.
 *
 * IMPORTANT: These tests require setVfsCallbacks() for the dart-sass VFS
 * importer. Without it, SASS compilation silently falls back to DEFAULT_CSS.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { JSDOM } from 'jsdom';
import { setVfsCallbacks } from '../wasm-js-bridge/sass.js';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_read_file: (path: string) => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  render_qmd: (path: string) => Promise<string>;
}

interface RenderResponse {
  success: boolean;
  html?: string;
  error?: string;
  diagnostics?: unknown[];
  warnings?: unknown[];
}

let wasm: WasmModule;

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);

  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);

  // Wire up VFS callbacks for the dart-sass importer so that SASS compilation
  // can resolve @use/@import against the VFS (Bootstrap SCSS files, etc.)
  setVfsCallbacks(
    (path: string): string | null => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string };
        return result.success && result.content !== undefined ? result.content : null;
      } catch {
        return null;
      }
    },
    (path: string): boolean => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string };
        return result.success && result.content !== undefined;
      } catch {
        return false;
      }
    },
  );
});

beforeEach(() => {
  wasm.vfs_clear();
  wasm.vfs_set_runtime_metadata('');
});

/**
 * Read all CSS content from a render result by following <link rel="stylesheet">
 * hrefs and reading the files from the VFS.
 */
function extractCss(result: RenderResponse): string {
  expect(result.success, `Render failed: ${result.error}`).toBe(true);
  expect(result.html, 'No HTML in render result').toBeTruthy();

  const dom = new JSDOM(result.html!);
  const links = dom.window.document.querySelectorAll('link[rel="stylesheet"]');
  let combinedCss = '';

  for (const link of links) {
    const href = link.getAttribute('href');
    if (!href || href.startsWith('http://') || href.startsWith('https://') || href.startsWith('//')) {
      continue;
    }
    const vfsPath = href.startsWith('/') ? href : `/project/${href}`;
    try {
      const readResult = JSON.parse(wasm.vfs_read_file(vfsPath)) as { success: boolean; content?: string };
      if (readResult.success && readResult.content) {
        combinedCss += readResult.content + '\n';
      }
    } catch {
      // CSS file not readable
    }
  }

  return combinedCss;
}

describe('theme CSS compilation in WASM pipeline', () => {
  it('runtime metadata theme overrides document frontmatter theme', async () => {
    // Document has theme: flatly, but runtime metadata sets theme: darkly.
    // Runtime metadata has highest precedence, so darkly should win.
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Test Project"\n');
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\ntheme: flatly\n---\n\n# Hello\n\nContent.\n',
    );
    wasm.vfs_set_runtime_metadata('theme: darkly\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd'),
    );

    const css = extractCss(result);
    expect(css.length).toBeGreaterThan(0);
    // darkly primary color
    expect(css).toMatch(/--bs-primary:.*#375a7f/);
    // flatly primary color should NOT be present
    expect(css).not.toMatch(/--bs-primary:.*#2c3e50/);
  });
});
