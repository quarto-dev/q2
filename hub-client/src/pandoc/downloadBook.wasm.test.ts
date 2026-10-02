/**
 * The book download end to end against the real hub wasm (pandoc-host H5, R9): the resolver's
 * `book` field, the controller's per-click scope, the per-chapter progress and capture keys, and
 * the book's file name. pandoc itself is a fake runner: this is about what the host asks of Rust.
 *
 * Run with: npm run test:wasm
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import type { ShareTree } from '@quarto/pandoc-host';
import { bookInfoFrom } from './bookInfo';
import { withOutputExt } from './downloadService';
import { captureDocIdsFor } from './captureFetch';
import { DownloadController, type BuildRequestExtra, type DownloadFormat, type RequestEnvelope } from './downloadController';
import type { RunOutcome } from './pandocRunner';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  render_pandoc_request: (path: string, format: string, sde?: number, cap?: Uint8Array, fonts?: string[], signal?: AbortSignal, options?: unknown) => Promise<RequestEnvelope>;
  resolve_pandoc_formats: (path: string) => string;
}

let wasm: WasmModule;

beforeAll(async () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const bytes = await readFile(join(here, '../../wasm-quarto-hub-client', 'wasm_quarto_hub_client_bg.wasm'));
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(bytes);
});

beforeEach(() => {
  wasm.vfs_clear();
  wasm.vfs_set_runtime_metadata('');
});

const ROOT = '/project';
const EPUB: DownloadFormat = { key: 'epub', label: 'EPUB', extension: 'epub', mime: 'application/epub+zip' };

function smallBook() {
  wasm.vfs_add_file(`${ROOT}/_quarto.yml`, 'project:\n  type: book\nbook:\n  title: Small Book\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n');
  wasm.vfs_add_file(`${ROOT}/index.qmd`, '# Preface\n\nHello\n');
  wasm.vfs_add_file(`${ROOT}/one.qmd`, '# One\n\nFirst chapter.\n');
  wasm.vfs_add_file(`${ROOT}/two.qmd`, '# Two\n\nSecond chapter.\n');
  wasm.vfs_add_file(`${ROOT}/notes.qmd`, '# Notes\n\nOutside the book.\n');
}

const resolveBook = (rel: string) => bookInfoFrom(JSON.parse(wasm.resolve_pandoc_formats(`${ROOT}/${rel}`)));

function controllerFor(seen: { extra?: BuildRequestExtra; progress: string[] }) {
  const saved: { name: string }[] = [];
  const controller = new DownloadController({
    buildRequest: async (path, format, epoch, _signal, fonts, extra) => {
      seen.extra = extra;
      const wrapped: BuildRequestExtra | undefined = extra && {
        ...extra,
        onProgress: (i, n, f) => {
          seen.progress.push(`${i}/${n} ${f}`);
          extra.onProgress?.(i, n, f);
        },
      };
      return wasm.render_pandoc_request(path, format, epoch, undefined, fonts, undefined, wrapped);
    },
    fetchCaptures: async (docIds) => ({ byPath: Object.fromEntries(Object.keys(docIds).map((k) => [k, new Uint8Array([1])])), failed: [] }),
    getShareTree: () => ({ share_tree_version: 'v', files: [] }) as ShareTree,
    runner: { run: async () => ({ ok: true, status: 0, output: new Uint8Array([1]), outputPath: '/o', collected: [], stderr: '', stdout: '', diagnostics: [], stats: {}, notices: [] }) as unknown as RunOutcome },
    classify: () => ({ success: true, diagnostics: [] }),
    save: (_blob, name) => saved.push({ name }),
  });
  return { controller, saved };
}

describe('book download against the real wasm', () => {
  it('the resolver reports the chapters as the keys the captures sidecar uses, and which pages are chapters', () => {
    smallBook();
    expect(resolveBook('one.qmd')).toEqual({ chapter: true, chapters: ['index.qmd', 'one.qmd', 'two.qmd'] });
    expect(resolveBook('notes.qmd')?.chapter).toBe(false);
  });

  it('a project without a book has no book information', () => {
    wasm.vfs_add_file(`${ROOT}/solo.qmd`, '# Solo\n');
    expect(resolveBook('solo.qmd')).toBeNull();
  });

  it('"Download book as" asks for the whole book with every chapter\'s capture key and reports chapter i of N', async () => {
    smallBook();
    const seen = { progress: [] as string[] };
    const { controller, saved } = controllerFor(seen);
    const book = resolveBook('one.qmd')!;
    const captures = Object.fromEntries(book.chapters.map((c) => [c, { captureDocId: `doc-${c}`, state: 'idle' as const }]));
    await controller.start({ path: `${ROOT}/one.qmd`, format: EPUB, scope: 'auto', captureDocIds: captureDocIdsFor(book.chapters, captures as never) });
    expect(Object.keys(seen.extra!.capturesByPath!)).toEqual(['index.qmd', 'one.qmd', 'two.qmd']);
    expect(seen.progress).toEqual(['1/3 index.qmd', '2/3 one.qmd', '3/3 two.qmd']);
    const done = controller.getSnapshot();
    expect(done).toMatchObject({ phase: 'done', book: { chapters: 3 } });
    // Named after the book, not the chapter the click came from.
    expect(saved[0].name).toBe('Small-Book.epub');
  });

  it('"This chapter only" is the page alone and is named after the chapter', async () => {
    smallBook();
    const seen = { progress: [] as string[] };
    const { controller, saved } = controllerFor(seen);
    await controller.start({ path: `${ROOT}/one.qmd`, format: EPUB, scope: 'chapter' });
    expect(seen.progress).toEqual([]);
    expect(saved[0].name).toBe('one.epub');
    const s = controller.getSnapshot();
    expect(s.phase === 'done' && s.book).toBeFalsy();
  });

  it("a book's output-ext names the whole-book typst source with that literal extension", async () => {
    smallBook();
    wasm.vfs_add_file(`${ROOT}/_quarto.yml`, 'project:\n  type: book\nformat:\n  typst:\n    output-ext: typst\nbook:\n  title: Small Book\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n');
    const resolved = JSON.parse(wasm.resolve_pandoc_formats(`${ROOT}/one.qmd`));
    expect(resolved.formats).toEqual([{ key: 'typst', class: 'download', extension: 'typst' }]);
    const row: DownloadFormat = { key: 'typst', label: 'Typst source (.typ)', extension: 'typ', mime: 'text/plain' };
    const seen = { progress: [] as string[] };
    const { controller, saved } = controllerFor(seen);
    const book = resolveBook('one.qmd')!;
    const captures = Object.fromEntries(book.chapters.map((c) => [c, { captureDocId: `doc-${c}`, state: 'idle' as const }]));
    await controller.start({ path: `${ROOT}/one.qmd`, format: withOutputExt(row, resolved), scope: 'auto', captureDocIds: captureDocIdsFor(book.chapters, captures as never) });
    expect(seen.progress).toEqual(['1/3 index.qmd', '2/3 one.qmd', '3/3 two.qmd']);
    expect(saved[0].name).toBe('Small-Book.typst');
  });

  it('a page outside the book downloads alone even if asked for scope auto', async () => {
    smallBook();
    const seen = { progress: [] as string[] };
    const { controller, saved } = controllerFor(seen);
    await controller.start({ path: `${ROOT}/notes.qmd`, format: EPUB, scope: 'auto' });
    expect(saved[0].name).toBe('notes.epub');
  });
});
