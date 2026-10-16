/**
 * WASM end-to-end tests for the one format resolver in the browser entries
 * (format-resolution epic, F6): `resolve_pandoc_formats` takes the editor buffer, resolves for a
 * chapter through the project, caches the discovered project against the VFS generation, and the
 * render entries (`render_qmd`, `render_printable`, `render_page_in_project*`) honour a
 * project-level `format:` instead of reading only the document's own front matter.
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
  source: 'document' | 'project' | 'default';
  key: string;
  class: 'preview' | 'download' | 'neither';
  pseudoFormat: string | null;
  scope: string;
  formats: { key: string; class: string; extension?: string }[];
  book: { chapter: boolean; chapters: string[] } | null;
}

interface RenderResponse {
  success: boolean;
  error?: string;
  format?: string;
  is_slides?: boolean;
  html?: string;
  ast_json?: string;
}

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_remove_file: (path: string) => string;
  vfs_clear: () => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  resolve_pandoc_formats: (path: string, options?: { contentOverride?: string }) => string;
  project_cache_stats: () => string;
  render_qmd: (path: string) => Promise<string>;
  render_printable: (path: string) => Promise<string>;
  render_page_in_project: (path: string) => Promise<string>;
  render_page_for_preview: (path: string) => Promise<string>;
}

let wasm: WasmModule;

beforeAll(async () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const bytes = await readFile(join(here, '../../wasm-quarto-hub-client', 'wasm_quarto_hub_client_bg.wasm'));
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(bytes);
  // The HTML pipeline compiles theme SCSS through the JS sass bridge `initWasm` installs.
  await initWasm();
});

beforeEach(() => {
  wasm.vfs_clear();
  wasm.vfs_set_runtime_metadata('');
});

const resolve = (path: string, contentOverride?: string): Resolved =>
  JSON.parse(wasm.resolve_pandoc_formats(path, contentOverride === undefined ? undefined : { contentOverride }));
const stats = () => JSON.parse(wasm.project_cache_stats()) as { hits: number; misses: number };
const render = async (fn: (p: string) => Promise<string>, path: string) => JSON.parse(await fn(path)) as RenderResponse;

function project(yml: string, files: Record<string, string> = {}, root = '/project') {
  wasm.vfs_add_file(`${root}/_quarto.yml`, yml);
  for (const [name, content] of Object.entries(files)) wasm.vfs_add_file(`${root}/${name}`, content);
}
const book = (format: string) =>
  `project:\n  type: book\nbook:\n  title: T\n  chapters:\n    - index.qmd\n    - ch1.qmd\nformat: ${format}\n`;

describe('resolve_pandoc_formats resolves through the project', () => {
  it('a front-matter-less chapter of a `format: typst` project resolves to typst', () => {
    project(book('typst'), { 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    const r = resolve('/project/ch1.qmd');
    expect(r).toMatchObject({ success: true, source: 'project', key: 'typst-pdf', class: 'download', pseudoFormat: null, scope: 'chapter' });
  });

  it('a chapter whose front matter says `format: html` resolves to html (the document wins in chapter scope)', () => {
    project(book('typst'), { 'index.qmd': '# Index\n', 'ch1.qmd': '---\nformat: html\n---\n\n# One\n' });
    expect(resolve('/project/ch1.qmd')).toMatchObject({ source: 'document', key: 'html', class: 'preview', pseudoFormat: 'q2-preview' });
  });

  it('a project-level `format: revealjs` resolves to the reveal preview pseudo-format', () => {
    project('project:\n  type: default\nformat: revealjs\n', { 'a.qmd': '# A\n' });
    expect(resolve('/project/a.qmd')).toMatchObject({ key: 'revealjs', pseudoFormat: 'revealjs' });
  });

  it('a multi-key `format:` still returns every key', () => {
    project('project:\n  type: default\n', {
      'a.qmd': '---\nformat:\n  html: default\n  typst:\n    output-ext: typ\n---\n\n# A\n',
    });
    const r = resolve('/project/a.qmd');
    expect(r.formats.map((f) => f.key)).toEqual(['html', 'typst']);
    expect(r.formats[1].extension).toBe('typ');
    expect(r.key).toBe('html');
  });

  it('a book project still returns the book information', () => {
    project(book('typst'), { 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n', 'other.qmd': '# Other\n' });
    expect(resolve('/project/ch1.qmd').book).toEqual({ chapter: true, chapters: ['index.qmd', 'ch1.qmd'] });
    expect(resolve('/project/other.qmd').book).toMatchObject({ chapter: false });
  });

  it("the buffer's front matter wins over a different VFS copy", () => {
    project(book('typst'), { 'index.qmd': '# Index\n', 'ch1.qmd': '---\nformat: html\n---\n\n# One\n' });
    expect(resolve('/project/ch1.qmd').key).toBe('html');
    expect(resolve('/project/ch1.qmd', '# One\n').key).toBe('typst-pdf');
    expect(resolve('/project/ch1.qmd', '---\nformat: docx\n---\n').key).toBe('docx');
  });

  it('the buffer of a file the VFS does not have yet resolves too; with no buffer it is an error', () => {
    project(book('typst'), { 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    expect(resolve('/project/new.qmd', '# New\n').key).toBe('typst-pdf');
    const missing = resolve('/project/new.qmd');
    expect(missing.success).toBe(false);
    expect(missing.error).toMatch(/Failed to read file/);
  });

  it('a runtime `format` scalar reaches the resolver (the request layer)', () => {
    project('project:\n  type: default\nformat: html\n', { 'a.qmd': '# A\n' });
    wasm.vfs_set_runtime_metadata('format: typst\n');
    expect(resolve('/project/a.qmd').key).toBe('typst-pdf');
  });
});

describe('the project cache and its VFS generation', () => {
  /** Resolve once to warm, run `change`, resolve again: was the second lookup a hit? */
  function secondLookupHits(path: string, change: () => void): boolean {
    resolve(path);
    const before = stats();
    change();
    resolve(path);
    const after = stats();
    expect(after.hits + after.misses).toBe(before.hits + before.misses + 1);
    return after.hits === before.hits + 1;
  }
  const base = () => project(book('typst'), { 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });

  it('two lookups with no VFS change reuse the project', () => {
    base();
    expect(secondLookupHits('/project/ch1.qmd', () => {})).toBe(true);
  });

  it('a content edit of an existing .qmd does not invalidate (the keystroke case)', () => {
    base();
    expect(secondLookupHits('/project/ch1.qmd', () => wasm.vfs_add_file('/project/ch1.qmd', '# One, edited\n'))).toBe(true);
  });

  it('an artifact flush under /.quarto/ does not invalidate', () => {
    base();
    expect(secondLookupHits('/project/ch1.qmd', () => wasm.vfs_add_file('/.quarto/project-artifacts/theme.css', 'a{}'))).toBe(true);
  });

  it.each([
    ['a change to _quarto.yml', () => wasm.vfs_add_file('/project/_quarto.yml', book('docx'))],
    ['a created .qmd', () => wasm.vfs_add_file('/project/new.qmd', '# New\n')],
    ['a removed .qmd', () => wasm.vfs_remove_file('/project/index.qmd')],
    ['a created _metadata.yml', () => wasm.vfs_add_file('/project/_metadata.yml', 'format: docx\n')],
    ['a custom-named brand file', () => wasm.vfs_add_file('/project/my-brand.yml', 'color:\n  palette:\n    a: "#fff"\n')],
    ['an _extensions/ write', () => wasm.vfs_add_file('/project/_extensions/x/_extension.yml', 'title: x\n')],
    ['a clear', () => wasm.vfs_clear()],
  ])('%s invalidates', (_name, change) => {
    base();
    resolve('/project/ch1.qmd');
    const before = stats();
    change();
    // After a clear the project is gone; re-add it, which is also a change.
    if (_name === 'a clear') base();
    resolve('/project/ch1.qmd');
    const after = stats();
    expect(after.misses).toBe(before.misses + 1);
  });

  it('a changed _quarto.yml is never answered from a stale project', () => {
    base();
    expect(resolve('/project/ch1.qmd').key).toBe('typst-pdf');
    wasm.vfs_add_file('/project/_quarto.yml', book('docx'));
    expect(resolve('/project/ch1.qmd').key).toBe('docx');
  });

  it('two projects in one VFS do not share an entry', () => {
    project('project:\n  type: default\nformat: typst\n', { 'a.qmd': '# A\n' }, '/p1');
    project('project:\n  type: default\nformat: revealjs\n', { 'a.qmd': '# A\n' }, '/p2');
    expect(resolve('/p1/a.qmd').key).toBe('typst-pdf');
    expect(resolve('/p2/a.qmd').key).toBe('revealjs');
    const before = stats();
    expect(resolve('/p1/a.qmd').key).toBe('typst-pdf');
    expect(resolve('/p2/a.qmd').key).toBe('revealjs');
    expect(stats().hits).toBe(before.hits + 2);
  });
});

describe('the render entries honour the project format', () => {
  const slides = () => project('project:\n  type: default\nformat: revealjs\n', { 'a.qmd': '# A\n\n## B\n\nText\n' });

  it('render_printable of a project-only revealjs page renders the reveal deck', async () => {
    slides();
    const r = await render(wasm.render_printable, '/project/a.qmd');
    expect(r.success, r.error).toBe(true);
    expect(r.format).toBe('revealjs');
  });

  it('render_printable of a page with no project format still renders html', async () => {
    project('project:\n  type: default\n', { 'a.qmd': '# A\n' });
    const r = await render(wasm.render_printable, '/project/a.qmd');
    expect(r.success, r.error).toBe(true);
    expect(r.format).toBe('html');
  });

  it('render_printable of a project-only typst chapter uses the project format, not html', async () => {
    project(book('typst'), { 'index.qmd': '# Index\n', 'ch1.qmd': '# One\n' });
    const r = await render(wasm.render_printable, '/project/ch1.qmd');
    expect(r.format ?? 'error').not.toBe('html');
  });

  it('render_qmd of a project-only revealjs page renders with revealjs', async () => {
    slides();
    const r = await render(wasm.render_qmd, '/project/a.qmd');
    expect(r.success, r.error).toBe(true);
    expect(r.format).toBe('revealjs');
  });

  it('render_page_for_preview of a project-only revealjs page is the slide preview', async () => {
    slides();
    const r = await render(wasm.render_page_for_preview, '/project/a.qmd');
    expect(r.success, r.error).toBe(true);
    expect(r.format).toBe('q2-slides');
    expect(r.is_slides).toBe(true);
  });

  it('render_page_for_preview of a page with a revealjs chapter in a multi-file project honours the project', async () => {
    project('project:\n  type: website\nformat: revealjs\n', { 'index.qmd': '# I\n', 'a.qmd': '# A\n\n## B\n\nText\n' });
    const r = await render(wasm.render_page_for_preview, '/project/a.qmd');
    expect(r.success, r.error).toBe(true);
    expect(r.format).toBe('q2-slides');
  });

  it('a page whose own front matter says html previews as html inside a revealjs project', async () => {
    project('project:\n  type: default\nformat: revealjs\n', { 'a.qmd': '---\nformat: html\n---\n\n# A\n' });
    const r = await render(wasm.render_page_for_preview, '/project/a.qmd');
    expect(r.success, r.error).toBe(true);
    expect(r.format).toBe('q2-preview');
  });
});
