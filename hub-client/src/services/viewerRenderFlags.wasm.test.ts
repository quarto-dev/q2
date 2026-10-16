/**
 * WASM end-to-end tests for the live viewer's render flags (format-resolution epic, F8):
 * `to` (the requested format, `q2 preview --to`) and `metadata` (`-M`) on the trailing options of
 * `render_page_for_preview`, `render_pandoc_request` and `resolve_pandoc_formats`.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { initWasm } from '@quarto/preview-runtime';

interface Resolved {
  success: boolean;
  error?: string;
  key: string;
  class: 'preview' | 'download' | 'neither';
  pseudoFormat: string | null;
  formats: { key: string; class: string }[];
}

interface RenderResponse {
  success: boolean;
  error?: string;
  format?: string;
  is_slides?: boolean;
  html?: string;
  ast_json?: string;
}

interface ViewerOptions {
  to?: string;
  metadata?: Record<string, unknown>;
}

interface PandocRequestResponse {
  success: boolean;
  error?: string;
  request?: { files: { path: string; bytes: Uint8Array }[] };
}

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  resolve_pandoc_formats: (path: string, options?: ViewerOptions & { contentOverride?: string }) => string;
  render_page_for_preview: (
    path: string,
    userGrammars?: unknown,
    capture?: Uint8Array,
    options?: ViewerOptions,
  ) => Promise<string>;
  render_pandoc_request: (
    path: string,
    format: string,
    sde?: number,
    capture?: Uint8Array,
    fonts?: string[],
    signal?: AbortSignal,
    options?: ViewerOptions,
  ) => Promise<PandocRequestResponse>;
}

let wasm: WasmModule;

beforeAll(async () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const bytes = await readFile(join(here, '../../wasm-quarto-hub-client', 'wasm_quarto_hub_client_bg.wasm'));
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(bytes);
  await initWasm();
});

beforeEach(() => {
  wasm.vfs_clear();
  wasm.vfs_set_runtime_metadata('');
});

const SDE = 1_700_000_000;
const resolve = (path: string, options?: ViewerOptions): Resolved =>
  JSON.parse(wasm.resolve_pandoc_formats(path, options));
const preview = async (path: string, options?: ViewerOptions) =>
  JSON.parse(await wasm.render_page_for_preview(path, undefined, undefined, options)) as RenderResponse;

function htmlBook(files: Record<string, string>) {
  wasm.vfs_add_file(
    '/book/_quarto.yml',
    'project:\n  type: book\nbook:\n  title: T\n  chapters:\n    - index.qmd\n    - ch1.qmd\nformat: html\n',
  );
  for (const [name, content] of Object.entries(files)) wasm.vfs_add_file(`/book/${name}`, content);
}

describe('`to` is the requested format', () => {
  it('makes a front-matter-less chapter of an html project resolve to typst', () => {
    htmlBook({ 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    expect(resolve('/book/ch1.qmd').key).toBe('html');
    expect(resolve('/book/ch1.qmd', { to: 'typst' })).toMatchObject({ key: 'typst-pdf', class: 'download', pseudoFormat: null });
  });

  it('beats a chapter whose own front matter says `format: html`', () => {
    htmlBook({ 'index.qmd': '# Index\n', 'ch1.qmd': '---\nformat: html\n---\n\n# One\n' });
    expect(resolve('/book/ch1.qmd').key).toBe('html');
    expect(resolve('/book/ch1.qmd', { to: 'typst' }).key).toBe('typst-pdf');
  });

  it('keeps the settings of a flow-map `format` in `metadata` (the reason `to` is its own option)', () => {
    htmlBook({ 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    const r = resolve('/book/ch1.qmd', { to: 'typst', metadata: { format: { typst: { toc: true } } } });
    expect(r).toMatchObject({ success: true, key: 'typst-pdf', class: 'download' });
  });

  it('the resolved key, not formats[0], names an undeclared format', () => {
    htmlBook({ 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    const r = resolve('/book/ch1.qmd', { to: 'revealjs' });
    expect(r).toMatchObject({ key: 'revealjs', pseudoFormat: 'revealjs' });
  });

  it('without options nothing changes', () => {
    htmlBook({ 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    expect(resolve('/book/ch1.qmd', {})).toMatchObject({ key: 'html' });
    expect(resolve('/book/ch1.qmd', undefined)).toMatchObject({ key: 'html' });
  });

  it('rejects a malformed `to` or `metadata`', () => {
    htmlBook({ 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    expect(resolve('/book/ch1.qmd', { to: 5 as unknown as string })).toMatchObject({ success: false });
    expect(resolve('/book/ch1.qmd', { metadata: [] as unknown as Record<string, unknown> })).toMatchObject({ success: false });
  });
});

describe('render_page_for_preview takes { to, metadata }', () => {
  it('`to: revealjs` previews an html page as slides', async () => {
    htmlBook({ 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n\nText.\n' });
    const plain = await preview('/book/ch1.qmd');
    expect(plain.success).toBe(true);
    expect(plain.is_slides).toBeFalsy();
    const slides = await preview('/book/ch1.qmd', { to: 'revealjs' });
    expect(slides.success).toBe(true);
    expect(slides.is_slides).toBe(true);
  });

  it('`metadata` reaches the html preview render', async () => {
    htmlBook({ 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    const plain = await preview('/book/ch1.qmd');
    expect(plain.ast_json).not.toContain('ZZVIEWERTITLE');
    const flagged = await preview('/book/ch1.qmd', { metadata: { title: 'ZZVIEWERTITLE' } });
    expect(flagged.success).toBe(true);
    expect(flagged.ast_json).toContain('ZZVIEWERTITLE');
    // Per call: the next render without options does not see it.
    expect((await preview('/book/ch1.qmd')).ast_json).not.toContain('ZZVIEWERTITLE');
  });

  it('a single document with no project takes the options too', async () => {
    wasm.vfs_add_file('/solo/doc.qmd', '# Solo\n');
    const slides = await preview('/solo/doc.qmd', { to: 'revealjs', metadata: { title: 'ZZSOLO' } });
    expect(slides.is_slides).toBe(true);
    expect(slides.ast_json).toContain('ZZSOLO');
  });
});

describe('render_pandoc_request takes { to, metadata }', () => {
  it('`metadata` reaches the pandoc request render, and `to` is accepted beside it', async () => {
    wasm.vfs_add_file('/p/doc.qmd', '# Hi\n\nBody.\n');
    const out = await wasm.render_pandoc_request('/p/doc.qmd', 'docx', SDE, undefined, undefined, undefined, {
      metadata: { title: 'ZZPANDOCTITLE' },
      to: 'docx',
    });
    expect(out.error).toBeUndefined();
    const input = new TextDecoder().decode(out.request!.files.find((f) => f.path.endsWith('/pandoc-input.json'))!.bytes);
    expect(input).toContain('ZZPANDOCTITLE');
  });
});
