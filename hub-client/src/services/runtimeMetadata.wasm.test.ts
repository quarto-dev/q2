/**
 * WASM End-to-End Tests for runtime metadata
 *
 * These tests verify that vfs_set_runtime_metadata injects metadata into
 * the rendering pipeline as the highest-precedence layer, above project,
 * directory, and document metadata.
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
  vfs_get_runtime_metadata: () => string;
  render_qmd: (path: string) => Promise<string>;
}

interface VfsResponse {
  success: boolean;
  error?: string;
  content?: string | null;
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
  // Clear runtime metadata between tests
  wasm.vfs_set_runtime_metadata('');
});

describe('vfs_set_runtime_metadata API', () => {
  it('accepts valid YAML and returns success', () => {
    const result: VfsResponse = JSON.parse(
      wasm.vfs_set_runtime_metadata('source-location: full\n')
    );
    expect(result.success).toBe(true);
  });

  it('clears metadata with empty string', () => {
    // Set metadata
    wasm.vfs_set_runtime_metadata('source-location: full\n');

    // Clear it
    const result: VfsResponse = JSON.parse(wasm.vfs_set_runtime_metadata(''));
    expect(result.success).toBe(true);

    // Verify it's cleared
    const get: VfsResponse = JSON.parse(wasm.vfs_get_runtime_metadata());
    expect(get.success).toBe(true);
    expect(get.content).toBeNull();
  });

  it('rejects non-mapping YAML', () => {
    const result: VfsResponse = JSON.parse(
      wasm.vfs_set_runtime_metadata('just a string')
    );
    expect(result.success).toBe(false);
    expect(result.error).toContain('must be a YAML mapping');
  });

  it('rejects invalid YAML', () => {
    const result: VfsResponse = JSON.parse(
      wasm.vfs_set_runtime_metadata('{ invalid: yaml: :::')
    );
    expect(result.success).toBe(false);
    expect(result.error).toContain('Failed to parse YAML');
  });

  it('round-trips metadata through get', () => {
    wasm.vfs_set_runtime_metadata('source-location: full\n');

    const get: VfsResponse = JSON.parse(wasm.vfs_get_runtime_metadata());
    expect(get.success).toBe(true);
    expect(get.content).toContain('source-location');
    expect(get.content).toContain('full');
  });
});

describe('runtime metadata in render pipeline', () => {
  it('injects top-level metadata into rendered output', async () => {
    // Set runtime metadata with a custom author
    wasm.vfs_set_runtime_metadata('author: "Runtime Author"\n');

    // Add a simple document with no author
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Test Project"\n');
    wasm.vfs_add_file('/project/doc.qmd', '# Hello\n\nContent.\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Runtime Author');
  });

  it('runtime metadata overrides document frontmatter', async () => {
    // Runtime sets author to override document
    wasm.vfs_set_runtime_metadata('author: "Runtime Author"\n');

    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Test Project"\n');
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\nauthor: "Doc Author"\n---\n\n# Hello\n\nContent.\n'
    );

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Runtime Author');
    expect(result.html).not.toContain('Doc Author');
  });

  it('runtime metadata overrides project config', async () => {
    // Runtime overrides the project author
    wasm.vfs_set_runtime_metadata('author: "Runtime Author"\n');

    wasm.vfs_add_file(
      '/project/_quarto.yml',
      'title: "Test Project"\nauthor: "Project Author"\n'
    );
    wasm.vfs_add_file('/project/doc.qmd', '# Hello\n\nContent.\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Runtime Author');
    expect(result.html).not.toContain('Project Author');
  });

  it('supports format-specific runtime metadata', async () => {
    // Set format-specific metadata via runtime
    wasm.vfs_set_runtime_metadata(
      'format:\n  html:\n    source-location: full\n'
    );

    wasm.vfs_add_file('/project/doc.qmd', '# Hello\n\nSome paragraph text.\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(result.success).toBe(true);
    // source-location: full should cause data-loc attributes in output
    expect(result.html).toContain('data-loc');
  });

  it('no data-loc without runtime metadata', async () => {
    // No runtime metadata set — should NOT have data-loc
    wasm.vfs_add_file('/project/doc.qmd', '# Hello\n\nSome paragraph text.\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).not.toContain('data-loc');
  });

  it('cleared runtime metadata stops affecting renders', async () => {
    // Set and then clear
    wasm.vfs_set_runtime_metadata(
      'format:\n  html:\n    source-location: full\n'
    );
    wasm.vfs_set_runtime_metadata('');

    wasm.vfs_add_file('/project/doc.qmd', '# Hello\n\nSome paragraph text.\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).not.toContain('data-loc');
  });

  it('works without project config (single file)', async () => {
    // No _quarto.yml, just runtime metadata and a document
    wasm.vfs_set_runtime_metadata('author: "Runtime Author"\n');

    wasm.vfs_add_file('/project/doc.qmd', '# Hello\n\nContent.\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Runtime Author');
  });
});

describe('render_qmd with directory metadata', () => {
  it('picks up _metadata.yml from parent directory', async () => {
    // Set up project with directory metadata
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Test Project"\n');
    wasm.vfs_add_file(
      '/project/chapters/_metadata.yml',
      'author: "Dir Author"\n'
    );
    wasm.vfs_add_file('/project/chapters/doc.qmd', '# Hello\n\nContent.\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/chapters/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Dir Author');
  });

  it('document frontmatter overrides directory metadata', async () => {
    wasm.vfs_add_file('/project/_quarto.yml', 'title: "Test Project"\n');
    wasm.vfs_add_file(
      '/project/chapters/_metadata.yml',
      'author: "Dir Author"\n'
    );
    wasm.vfs_add_file(
      '/project/chapters/doc.qmd',
      '---\nauthor: "Doc Author"\n---\n\n# Hello\n\nContent.\n'
    );

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('/project/chapters/doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Doc Author');
    expect(result.html).not.toContain('Dir Author');
  });

  it('render_qmd works with relative paths', async () => {
    // Verify VFS path normalization: relative paths resolve to /project/
    wasm.vfs_add_file('doc.qmd', '# Hello\n\nContent.\n');

    const result: RenderResponse = JSON.parse(
      await wasm.render_qmd('doc.qmd')
    );
    expect(result.success).toBe(true);
    expect(result.html).toContain('Hello');
  });
});
