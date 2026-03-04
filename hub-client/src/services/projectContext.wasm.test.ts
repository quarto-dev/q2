/**
 * WASM End-to-End Tests for project context discovery
 *
 * These tests verify that render_qmd discovers _quarto.yml and _metadata.yml
 * from the VFS, enabling project-level and directory-level metadata inheritance.
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
});

beforeEach(() => {
  wasm.vfs_clear();
});

describe('project context discovery via render_qmd', () => {
  it('renders single file without _quarto.yml', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Hello\n---\n\nSome text.\n');

    const result: RenderResponse = JSON.parse(await wasm.render_qmd('/project/doc.qmd'));
    expect(result.success).toBe(true);
    expect(result.html).toContain('Hello');
  });

  it('inherits project title from _quarto.yml', async () => {
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Project Title"\n');
    wasm.vfs_add_file('/project/doc.qmd', '# Heading\n\nSome text.\n');

    const result: RenderResponse = JSON.parse(await wasm.render_qmd('/project/doc.qmd'));
    expect(result.success).toBe(true);
    expect(result.html).toContain('Project Title');
  });

  it('document title overrides project title', async () => {
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Project Title"\n');
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: "Doc Title"\n---\n\nSome text.\n');

    const result: RenderResponse = JSON.parse(await wasm.render_qmd('/project/doc.qmd'));
    expect(result.success).toBe(true);
    expect(result.html).toContain('Doc Title');
    expect(result.html).not.toContain('Project Title');
  });

  it('discovers _quarto.yml from parent directories', async () => {
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Deep Project"\n');
    wasm.vfs_add_file(
      '/project/chapters/intro/doc.qmd',
      '# Intro\n\nContent.\n'
    );

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/chapters/intro/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Deep Project');
  });

  it('picks up directory metadata from _metadata.yml', async () => {
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "My Project"\n');
    wasm.vfs_add_file(
      '/project/chapters/_metadata.yml',
      'author: "Chapter Author"\n'
    );
    wasm.vfs_add_file(
      '/project/chapters/doc.qmd',
      '# Chapter\n\nContent.\n'
    );

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/chapters/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Chapter Author');
  });

  it('merges directory metadata hierarchy correctly', async () => {
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "My Project"\n');
    wasm.vfs_add_file(
      '/project/chapters/_metadata.yml',
      'author: "Chapters Author"\n'
    );
    wasm.vfs_add_file(
      '/project/chapters/intro/_metadata.yml',
      'subtitle: "Intro Subtitle"\n'
    );
    wasm.vfs_add_file(
      '/project/chapters/intro/doc.qmd',
      '# Chapter\n\nContent.\n'
    );

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/chapters/intro/doc.qmd')
    );
    expect(result.success).toBe(true);
    // Both directory metadata layers should be merged
    expect(result.html).toContain('Chapters Author');
    expect(result.html).toContain('Intro Subtitle');
  });
});
