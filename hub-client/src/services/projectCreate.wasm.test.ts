/**
 * WASM Tests for project creation (create_project / get_project_choices)
 *
 * Verifies the project-scaffolding WASM exports after the EJS →
 * quarto-doctemplate migration (bd-kuxzj8su): template rendering is pure
 * Rust, synchronous, and requires no JS template bridge.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

// Type for the WASM module
interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  get_project_choices: () => string;
  create_project: (choiceId: string, title: string) => string;
}

interface ProjectChoicesResponse {
  success: boolean;
  choices: Array<{ id: string; name: string; description: string }>;
}

interface CreateProjectResponse {
  success: boolean;
  error?: string;
  files?: Array<{
    path: string;
    content_type: 'text' | 'binary';
    content: string;
    mime_type?: string;
  }>;
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

describe('get_project_choices', () => {
  it('returns implemented choices including default, website, and blog', () => {
    const response = JSON.parse(wasm.get_project_choices()) as ProjectChoicesResponse;
    expect(response.success).toBe(true);

    const ids = response.choices.map((c) => c.id);
    expect(ids).toContain('default');
    expect(ids).toContain('website');
    expect(ids).toContain('blog');
  });
});

describe('create_project', () => {
  it('returns a string synchronously (no Promise — rendering is pure Rust)', () => {
    const result = wasm.create_project('default', 'Sync Check');
    expect(typeof result).toBe('string');
  });

  it('creates a website project with rendered titles', () => {
    const response = JSON.parse(
      wasm.create_project('website', 'My Website'),
    ) as CreateProjectResponse;
    expect(response.success).toBe(true);

    const byPath = new Map(response.files!.map((f) => [f.path, f]));
    expect([...byPath.keys()].sort()).toEqual([
      '_quarto.yml',
      'about.qmd',
      'index.qmd',
      'styles.css',
    ]);

    const quartoYml = byPath.get('_quarto.yml')!.content;
    // Title lives under `website:` (what Q2's website pipeline reads),
    // not under `project:`.
    expect(quartoYml).toContain('website:\n  title: "My Website"');
    expect(quartoYml).toContain('type: website');
    expect(quartoYml).toContain('theme: cosmo');
    // Q2 hard-errors on an unconfigured `brand` theme marker (Q-14-1);
    // the scaffold must not emit one.
    expect(quartoYml).not.toContain('brand');

    const indexQmd = byPath.get('index.qmd')!.content;
    expect(indexQmd).toContain('title: "My Website"');

    expect(byPath.get('about.qmd')!.content).toContain('title: "About"');
    expect(byPath.get('styles.css')!.content).toContain('/* css styles */');

    // No template-syntax residue of either engine
    for (const file of response.files!) {
      expect(file.content).not.toContain('$title$');
      expect(file.content).not.toContain('<%');
    }
  });

  it('creates a default project with a starter document', () => {
    const response = JSON.parse(
      wasm.create_project('default', 'Test Project'),
    ) as CreateProjectResponse;
    expect(response.success).toBe(true);

    const byPath = new Map(response.files!.map((f) => [f.path, f]));
    expect([...byPath.keys()].sort()).toEqual(['_quarto.yml', 'index.qmd']);
    expect(byPath.get('_quarto.yml')!.content).toContain('title: "Test Project"');

    const indexQmd = byPath.get('index.qmd')!.content;
    expect(indexQmd).toContain('title: "Test Project"');
    expect(indexQmd).toContain('## Quarto');
  });

  it('creates a blog project with binary post images (bd-r1by4u2a)', () => {
    const response = JSON.parse(
      wasm.create_project('blog', 'My Blog'),
    ) as CreateProjectResponse;
    expect(response.success).toBe(true);

    const byPath = new Map(response.files!.map((f) => [f.path, f]));
    expect([...byPath.keys()].sort()).toEqual([
      '_quarto.yml',
      'about.qmd',
      'index.qmd',
      'posts/_metadata.yml',
      'posts/post-with-code/image.jpg',
      'posts/post-with-code/index.qmd',
      'posts/welcome/index.qmd',
      'posts/welcome/thumbnail.jpg',
      'styles.css',
    ]);

    const quartoYml = byPath.get('_quarto.yml')!.content;
    expect(quartoYml).toContain('title: "My Blog"');
    expect(quartoYml).toContain('description: "A blog built with Quarto"');
    expect(quartoYml).not.toContain('brand');

    // The listing page carries Q1's canonical listing config.
    const indexQmd = byPath.get('index.qmd')!.content;
    expect(indexQmd).toContain('contents: posts');
    expect(indexQmd).toContain('sort: "date desc"');

    // Binary files arrive base64-encoded with a JPEG mime; decoded
    // bytes must start with the JPEG magic (FF D8).
    for (const p of ['posts/welcome/thumbnail.jpg', 'posts/post-with-code/image.jpg']) {
      const f = byPath.get(p)!;
      expect(f.content_type).toBe('binary');
      expect(f.mime_type).toBe('image/jpeg');
      const bytes = Uint8Array.from(atob(f.content), (c) => c.charCodeAt(0));
      expect(bytes.length).toBeGreaterThan(1000);
      expect(bytes[0]).toBe(0xff);
      expect(bytes[1]).toBe(0xd8);
    }

    // Posts are date-stamped (today / today-minus-3) with no residue.
    const welcome = byPath.get('posts/welcome/index.qmd')!.content;
    expect(welcome).toMatch(/date: "\d{4}-\d{2}-\d{2}"/);
    for (const file of response.files!) {
      if (file.content_type === 'text') {
        expect(file.content).not.toContain('$');
      }
    }
  });

  it('YAML-escapes special characters without HTML-escaping', () => {
    const response = JSON.parse(
      wasm.create_project('default', 'R & D "quoted" \\ backslash'),
    ) as CreateProjectResponse;
    expect(response.success).toBe(true);

    const content = response.files!.find((f) => f.path === '_quarto.yml')!.content;
    // `&` passes through raw. (The old EJS path HTML-escaped it to `&amp;`
    // — a latent bug for YAML output.)
    expect(content).not.toContain('&amp;');
    // `"` and `\` are escaped for the YAML double-quoted context.
    expect(content).toContain('title: "R & D \\"quoted\\" \\\\ backslash"');
  });

  it('fails cleanly on an unknown choice id', () => {
    const response = JSON.parse(
      wasm.create_project('nonexistent', 'X'),
    ) as CreateProjectResponse;
    expect(response.success).toBe(false);
    expect(response.error).toContain('nonexistent');
  });
});
