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
// `/src/wasm-js-bridge` is aliased to `@quarto/wasm-js-bridge/src` in
// hub-client's vite + vitest configs (the same alias the Rust WASM
// module's `raw_module` annotation uses). Going through the alias
// rather than a relative path keeps hub-client unaware of the bridge
// package's filesystem location.
import { setVfsCallbacks } from '/src/wasm-js-bridge/sass.js';

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

  it('default CSS (no theme specified) includes syntax-highlight rules', async () => {
    // Regression guard for Phase 3 of syntax highlighting: a document
    // with no `theme:` frontmatter entry and no project config theme
    // still needs the `.hl-*` color rules, because the HTML writer
    // emits span classes like `hl-keyword` / `hl-function-builtin` for
    // every code block it highlights.
    //
    // Before the fix, the wasm32 `compile_default_css` loaded only the
    // title-block layer (not highlight.scss), so spans had no colors
    // unless the user explicitly set a theme. See the native fix in
    // commit 50745caa ("HTML writer emits nested highlight spans +
    // default SCSS") and its WASM follow-up discussed in
    // `claude-notes/plans/2026-04-20-syntax-highlighting-phase-3.md`.
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Test Project"\n');
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\ntitle: "No Theme"\n---\n\n```python\ndef f():\n    pass\n```\n',
    );

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd'),
    );

    const css = extractCss(result);
    expect(css.length).toBeGreaterThan(0);
    expect(css, 'default CSS must include .hl-keyword rule').toContain('.hl-keyword');
    expect(
      css,
      'default CSS must include nested-capture .hl-function-builtin rule',
    ).toContain('.hl-function-builtin');
  });
});
