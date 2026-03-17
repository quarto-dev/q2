/**
 * WASM End-to-End Tests for format detection
 *
 * These tests verify that format detection from YAML frontmatter works
 * correctly through the WASM entry points, and that the format name
 * is injected into the merged metadata.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  parse_qmd_to_ast: (content: string) => Promise<string>;
  render_qmd: (path: string) => Promise<string>;
  render_qmd_content: (content: string, template: string) => Promise<string>;
}

interface AstResponse {
  success: boolean;
  ast?: string;
  qmd?: string;
  error?: string;
  diagnostics?: unknown[];
  warnings?: unknown[];
}

interface RenderResponse {
  success: boolean;
  html?: string;
  error?: string;
  diagnostics?: unknown[];
  warnings?: unknown[];
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function getMetaFormat(ast: any): string | undefined {
  const fmt = ast?.meta?.format;
  if (!fmt) return undefined;
  // MetaString: { t: "MetaString", c: "html" }
  if (fmt.t === 'MetaString') return fmt.c;
  // MetaInlines: { t: "MetaInlines", c: [{ t: "Str", c: "html" }] }
  if (fmt.t === 'MetaInlines') return fmt.c?.[0]?.c;
  return undefined;
}

let wasm: WasmModule;

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);

  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
});

beforeEach(() => {
  wasm.vfs_clear();
  wasm.vfs_set_runtime_metadata('');
});

describe('format detection via parse_qmd_to_ast', () => {
  it('detects format: q2-slides', async () => {
    const content = '---\nformat: q2-slides\ntitle: My Slides\n---\n\n# Slide 1\n';
    const response: AstResponse = JSON.parse(await wasm.parse_qmd_to_ast(content));
    expect(response.success).toBe(true);
    const ast = JSON.parse(response.ast!);
    expect(getMetaFormat(ast)).toBe('q2-slides');
  });

  it('detects format: html', async () => {
    const content = '---\nformat: html\ntitle: Doc\n---\n\n# Hello\n';
    const response: AstResponse = JSON.parse(await wasm.parse_qmd_to_ast(content));
    expect(response.success).toBe(true);
    const ast = JSON.parse(response.ast!);
    expect(getMetaFormat(ast)).toBe('html');
  });

  it('defaults to html when no format key', async () => {
    const content = '---\ntitle: No Format\n---\n\n# Hello\n';
    const response: AstResponse = JSON.parse(await wasm.parse_qmd_to_ast(content));
    expect(response.success).toBe(true);
    const ast = JSON.parse(response.ast!);
    expect(getMetaFormat(ast)).toBe('html');
  });

  it('detects format from map: format: { html: { toc: true } }', async () => {
    const content = '---\nformat:\n  html:\n    toc: true\n---\n\n# Hello\n';
    const response: AstResponse = JSON.parse(await wasm.parse_qmd_to_ast(content));
    expect(response.success).toBe(true);
    const ast = JSON.parse(response.ast!);
    expect(getMetaFormat(ast)).toBe('html');
  });

  it('defaults to html with no frontmatter', async () => {
    const content = '# Just Markdown\n\nNo frontmatter at all.\n';
    const response: AstResponse = JSON.parse(await wasm.parse_qmd_to_ast(content));
    expect(response.success).toBe(true);
    const ast = JSON.parse(response.ast!);
    expect(getMetaFormat(ast)).toBe('html');
  });

  it('defaults to html with malformed YAML', async () => {
    const content = '---\n{bad yaml\n---\n\n# Hello\n';
    const response: AstResponse = JSON.parse(await wasm.parse_qmd_to_ast(content));
    // Should fall back to html, not fail on format detection
    expect(response.success).toBe(true);
    const ast = JSON.parse(response.ast!);
    expect(getMetaFormat(ast)).toBe('html');
  });

  it('returns error for unknown format', async () => {
    const content = '---\nformat: unknown-garbage\n---\n\n# Hello\n';
    const response: AstResponse = JSON.parse(await wasm.parse_qmd_to_ast(content));
    expect(response.success).toBe(false);
    expect(response.error).toContain('Unknown format');
  });

  it('returns error for empty format string', async () => {
    // format: "" (empty string) is represented as format: '' in YAML
    const content = "---\nformat: ''\n---\n\n# Hello\n";
    const response: AstResponse = JSON.parse(await wasm.parse_qmd_to_ast(content));
    expect(response.success).toBe(false);
    expect(response.error).toContain('Unknown format');
  });
});

describe('format detection via render_qmd', () => {
  it('detects format: q2-slides from VFS file', async () => {
    wasm.vfs_add_file(
      '/project/slides.qmd',
      '---\nformat: q2-slides\ntitle: My Slides\n---\n\n# Slide 1\n'
    );

    const response: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/slides.qmd')
    );
    expect(response.success).toBe(true);
  });

  it('returns error for unknown format in VFS file', async () => {
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\nformat: totally-bogus\n---\n\n# Hello\n'
    );

    const response: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(response.success).toBe(false);
    expect(response.error).toContain('Unknown format');
  });
});

describe('format detection via render_qmd_content', () => {
  it('detects format: q2-slides from content', async () => {
    const content = '---\nformat: q2-slides\ntitle: My Slides\n---\n\n# Slide 1\n';
    const response: RenderResponse = JSON.parse(
      await wasm.render_qmd_content(content, '')
    );
    expect(response.success).toBe(true);
  });

  it('returns error for unknown format from content', async () => {
    const content = '---\nformat: totally-bogus\n---\n\n# Hello\n';
    const response: RenderResponse = JSON.parse(
      await wasm.render_qmd_content(content, '')
    );
    expect(response.success).toBe(false);
    expect(response.error).toContain('Unknown format');
  });
});
