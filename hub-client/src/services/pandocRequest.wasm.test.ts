/**
 * WASM end-to-end tests for the pandoc request exports (pandoc-wasm epic,
 * request phase R2): `render_pandoc_request`, the share tree, the completion
 * classifier, the format table and the project-aware format resolver.
 *
 * The request is built by Rust inside the built wasm from the VFS. The golden
 * (`crates/quarto-core/schemas/pandoc-request.golden.json`) is a request for
 * `/project/doc.qmd`; rendering the same document here must give, byte for
 * byte, that request, with `Uint8Array`s where the golden has base64.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { readFile } from 'fs/promises';
import { gzipSync, unzipSync } from 'fflate';
import { execute, type PandocRequest } from '@quarto/pandoc-host';
import { WASM_PATH, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface RequestFile {
  path: string;
  bytes: Uint8Array;
}

interface Envelope {
  success: boolean;
  error?: string;
  diagnostics: { kind: string; title: string; code?: string }[];
  stats: { unexecuted_cells: number; book: { scope: 'book' | 'chapter'; chapters: number } | null };
  request?: Record<string, unknown> & {
    files: RequestFile[];
    resource_refs: RequestFile[];
    share_tree_version: string;
    argv: string[];
    env: Record<string, string>;
    job_id: string;
    writer: string;
    output_path: string;
    post: string;
  };
}

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_add_binary_file: (path: string, content: Uint8Array) => string;
  vfs_clear: () => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  render_pandoc_request: (
    path: string,
    format: string,
    source_date_epoch?: number,
    capture_gz_json?: Uint8Array,
    typst_available_fonts?: string[],
    abort_signal?: AbortSignal,
    options?: {
      scope?: 'auto' | 'chapter';
      capturesByPath?: Record<string, Uint8Array>;
      onProgress?: (index: number, total: number, file: string) => void | Promise<void>;
    },
  ) => Promise<Envelope>;
  get_pandoc_share_tree_version: () => string;
  get_pandoc_share_tree: () => { share_tree_version: string; files: RequestFile[] };
  classify_pandoc_completion: (
    stage_name: string,
    success: boolean,
    status: string,
    stderr: string,
    json_path: string,
  ) => string;
  get_pandoc_formats: () => string;
  resolve_pandoc_formats: (path: string) => string;
  get_typst_assets_version: () => string;
  get_typst_assets: () => { typst_assets_version: string; files: RequestFile[] };
  typst_date_prelude: (source_date_epoch: number) => string;
}

let wasm: WasmModule;
let here: string;

beforeAll(async () => {
  here = dirname(fileURLToPath(import.meta.url));
  const wasmPath = join(here, '../../wasm-quarto-hub-client', 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
});

beforeEach(() => {
  wasm.vfs_clear();
  wasm.vfs_set_runtime_metadata('');
});

// The document and figure the golden was made from (pandoc_request_prepare.rs).
const GOLDEN_DOC = '---\ntitle: Golden\n---\n\n# Hello\n\nA *docx* paragraph.\n\n![A figure](figure.png)\n';
const FIGURE = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const SDE = 1_700_000_000;

const fromBase64 = (s: string): Uint8Array => new Uint8Array(Buffer.from(s, 'base64'));

/** The golden, with base64 byte fields decoded to Uint8Array (the wire form). */
async function loadGolden(): Promise<Record<string, unknown>> {
  const path = join(here, '../../../crates/quarto-core/schemas/pandoc-request.golden.json');
  const golden = JSON.parse(await readFile(path, 'utf8'));
  for (const key of ['files', 'resource_refs'] as const) {
    golden[key] = golden[key].map((f: { path: string; bytes: string }) => ({
      path: f.path,
      bytes: fromBase64(f.bytes),
    }));
  }
  return golden;
}

/**
 * What must match the golden exactly: everything except the input JSON's
 * `astContext` and `job_id` (which hashes those bytes). `astContext` carries
 * file ids hashed to `usize`, so they are 64-bit on the native run that made
 * the golden and 32-bit in wasm32; each is deterministic on its platform, and
 * the Pandoc `blocks` and `meta` in the same JSON are compared in full.
 */
function comparable(request: Record<string, unknown>): Record<string, unknown> {
  const files = (request.files as RequestFile[]).map((f) => {
    if (!f.path.endsWith('/pandoc-input.json')) return f;
    const json = JSON.parse(new TextDecoder().decode(f.bytes));
    delete json.astContext;
    return { path: f.path, bytes: json };
  });
  return { ...request, job_id: '<hash of the bytes above>', files };
}

async function goldenEnvelope(): Promise<Envelope> {
  wasm.vfs_add_file('/project/doc.qmd', GOLDEN_DOC);
  wasm.vfs_add_binary_file('/project/figure.png', FIGURE);
  return wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE);
}

describe('render_pandoc_request', () => {
  it('returns Uint8Array bytes and a request equal to the golden', async () => {
    const out = await goldenEnvelope();
    expect(out.error).toBeUndefined();
    expect(out.success).toBe(true);
    const request = out.request!;
    expect(request).toBeDefined();
    expect(request.files.length).toBeGreaterThan(0);
    for (const f of [...request.files, ...request.resource_refs]) {
      expect(f.bytes).toBeInstanceOf(Uint8Array);
    }
    expect(request.resource_refs.map((f) => f.path)).toEqual(['/project/figure.png']);
    expect(request.resource_refs[0].bytes).toEqual(FIGURE);
    expect(comparable(request)).toEqual(comparable(await loadGolden()));
    expect(out.stats).toEqual({ unexecuted_cells: 0, book: null });
  });

  it('returns copies: nothing in the request aliases wasm memory', async () => {
    const out = await goldenEnvelope();
    for (const f of out.request!.files) {
      // A view of wasm memory would have the (resizable) memory's buffer.
      expect(f.bytes.buffer.byteLength).toBe(f.bytes.byteLength);
    }
  });

  it('is deterministic: the same document and epoch give the same job id', async () => {
    const a = await goldenEnvelope();
    const b = await wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE + 99);
    expect(b.request!.job_id).toBe(a.request!.job_id);
  });

  it('reports a path absent from the VFS without a request', async () => {
    const out = await wasm.render_pandoc_request('/project/missing.qmd', 'docx');
    expect(out.success).toBe(false);
    expect(out.request).toBeUndefined();
    expect(out.error).toMatch(/Failed to read file/);
  });

  it('returns no request for a document with errors', async () => {
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\nformat:\n  docx:\n    reference-doc: nope.docx\n---\n\nHi\n',
    );
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'docx');
    expect(out.success).toBe(false);
    expect(out.request).toBeUndefined();
    expect(out.diagnostics.some((d) => d.kind === 'error' && d.code === 'Q-5-30')).toBe(true);
  });

  it('refuses a format pandoc.wasm cannot produce', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '# Hi\n');
    for (const format of ['latex', 'html', 'nonsense']) {
      const out = await wasm.render_pandoc_request('/project/doc.qmd', format);
      expect(out.success, format).toBe(false);
      expect(out.error, format).toMatch(/cannot be rendered by pandoc/);
    }
  });

  it('renders a document inside a _quarto.yml project as its active page, with image targets relative to the source', async () => {
    wasm.vfs_add_file('/p/_quarto.yml', 'project:\n  type: default\nformat:\n  html: default\n');
    wasm.vfs_add_binary_file('/p/img/a.png', FIGURE);
    wasm.vfs_add_binary_file('/p/sub/pic.png', FIGURE);
    wasm.vfs_add_file('/p/other.qmd', '# Other\n\nNot in the request.\n');
    wasm.vfs_add_file('/p/sub/doc.qmd', '# Hi\n\n![a](/img/a.png)\n\n![b](pic.png)\n');
    const out = await wasm.render_pandoc_request('/p/sub/doc.qmd', 'docx', SDE);
    expect(out.error).toBeUndefined();
    expect(out.success).toBe(true);
    const request = out.request!;
    // The requested format wins over the project's `format: html`.
    expect(request.writer).toBe('docx');
    const input = JSON.parse(
      new TextDecoder().decode(request.files.find((f) => f.path.endsWith('/pandoc-input.json'))!.bytes),
    );
    const targets: string[] = [];
    const walk = (v: unknown) => {
      if (Array.isArray(v)) v.forEach(walk);
      else if (v && typeof v === 'object') {
        const o = v as { t?: string; c?: unknown };
        if (o.t === 'Image') targets.push((o.c as [unknown, unknown, [string]])[2][0]);
        Object.values(o).forEach(walk);
      }
    };
    walk(input.blocks);
    // The site-root target is no longer written as `/img/a.png`, which pandoc
    // (working directory `/`) could not find.
    expect(targets).toEqual(['../img/a.png', 'pic.png']);
    expect(request.resource_refs.map((f) => f.path).sort()).toEqual(['/p/img/a.png', '/p/sub/pic.png']);
    expect(JSON.stringify(input.blocks)).not.toContain('Not in the request');
  });

  it('says the project render scripts did not run, on every download', async () => {
    wasm.vfs_add_file('/p/_quarto.yml', 'project:\n  type: default\n  pre-render: prepare.py\n');
    wasm.vfs_add_file('/p/prepare.py', 'print(1)\n');
    wasm.vfs_add_file('/p/doc.qmd', '# Hi\n');
    for (let i = 0; i < 2; i += 1) {
      const out = await wasm.render_pandoc_request('/p/doc.qmd', 'docx', SDE);
      expect(out.success).toBe(true);
      expect(out.diagnostics.map((d) => d.code)).toContain('Q-5-12');
    }
  });

  // The whole-book default is tested in `pandocBook.wasm.test.ts`.
  it("renders the active chapter of a book alone with scope 'chapter'", async () => {
    wasm.vfs_add_file(
      '/b/_quarto.yml',
      'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n',
    );
    wasm.vfs_add_file('/b/index.qmd', '# Preface\n\nHello\n');
    wasm.vfs_add_file('/b/one.qmd', '# One\n\nFirst chapter.\n');
    wasm.vfs_add_file('/b/two.qmd', '# Two\n\nSecond chapter.\n');
    const out = await wasm.render_pandoc_request('/b/one.qmd', 'typst', SDE, undefined, undefined, undefined, {
      scope: 'chapter',
    });
    expect(out.error).toBeUndefined();
    expect(out.stats.book).toEqual({ scope: 'chapter', chapters: 3 });
    const text = new TextDecoder().decode(
      out.request!.files.find((f) => f.path.endsWith('/pandoc-input.json'))!.bytes,
    );
    expect(text).toContain('chapter.');
    expect(text).not.toContain('Second');
  });

  it('counts code cells without a cached result, and splices the ones with one', async () => {
    const doc = '---\ntitle: T\n---\n\nBefore.\n\n```{r}\nSRC_R\n```\n\nMiddle.\n\n```{python}\nSRC_PY\n```\n';
    wasm.vfs_add_file('/project/doc.qmd', doc);
    const bare = await wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE);
    expect(bare.success).toBe(true);
    expect(bare.stats).toEqual({ unexecuted_cells: 2, book: null });

    const markdown =
      '---\ntitle: T\n---\n\nBefore.\n\n::: {.cell}\n::: {.cell-output .cell-output-stdout}\nOUT_R\n:::\n:::\n\n' +
      'Middle.\n\n```{python}\nSRC_PY\n```\n';
    const capture = [{ engine_name: 'r', input_qmd: doc, result: { markdown }, files: [] }];
    const gz = gzipSync(new TextEncoder().encode(JSON.stringify(capture)));
    const cached = await wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE, gz);
    expect(cached.success).toBe(true);
    expect(cached.stats).toEqual({ unexecuted_cells: 1, book: null });
    const input = cached.request!.files.find((f) => f.path.endsWith('/pandoc-input.json'))!;
    const text = new TextDecoder().decode(input.bytes);
    expect(text).toContain('OUT_R');
    expect(text).not.toContain('SRC_R');
    expect(text).toContain('SRC_PY');
  });

  it('echoes typst_available_fonts', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '# Hi\n');
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'docx', undefined, undefined, ['Inter']);
    expect(out.request!.typst_available_fonts).toEqual(['Inter']);
  });
});

function addBook(root: string) {
  wasm.vfs_add_file(
    `${root}/_quarto.yml`,
    'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n',
  );
  wasm.vfs_add_file(`${root}/index.qmd`, '# Preface\n\nHello\n');
  wasm.vfs_add_file(`${root}/one.qmd`, '# One\n\nFirst chapter.\n');
}

describe('built-in extension filters are mounted (R9 task 1a)', () => {
  it('puts the orange-book filter and its directory in files, not resource_refs', async () => {
    addBook('/b');
    const out = await wasm.render_pandoc_request('/b/one.qmd', 'typst', SDE);
    expect(out.error).toBeUndefined();
    expect(out.diagnostics.map((d) => d.code)).not.toContain('Q-11-1');
    const request = out.request!;
    const files = request.files.map((f) => f.path);
    const prefix = '/__quarto_resources__/extension-subtrees/orange-book/_extensions/orange-book/';
    expect(files).toContain(`${prefix}orange-book.lua`);
    expect(files).toContain(`${prefix}_extension.yml`);
    expect(request.resource_refs.map((f) => f.path).filter((p) => p.includes('orange-book'))).toEqual([]);
    // Every filter entry point is a normalized, mounted path.
    const params = JSON.parse(
      new TextDecoder().decode(
        Uint8Array.from(atob(request.env.QUARTO_FILTER_PARAMS), (c) => c.charCodeAt(0)),
      ),
    );
    const entries: string[] = params['quarto-filters'].entryPoints.map((e: { path: string }) => e.path);
    expect(entries.length).toBeGreaterThan(0);
    for (const entry of entries) expect(files).toContain(entry);
  });
});

describe('citeproc reads its files through the runtime (R9)', () => {
  const REFS_BIB =
    '@book{knuth1984,\n  author = {Knuth, Donald E.},\n  title = {The TeXbook},\n  year = {1984},\n  publisher = {Addison-Wesley}\n}\n';
  // A numeric style that only exists in the VFS: its `[1]` marks that it was read.
  const NUMERIC_CSL =
    '<?xml version="1.0" encoding="utf-8"?>\n<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text" default-locale="en-US">\n' +
    '  <info><title>Mem</title><id>http://example.com/mem</id><updated>2026-01-01T00:00:00+00:00</updated></info>\n' +
    '  <citation><layout><text variable="citation-number" prefix="[" suffix="]"/></layout></citation>\n' +
    '  <bibliography><layout><text variable="citation-number" prefix="[" suffix="] "/><text variable="title"/></layout></bibliography>\n</style>\n';

  const inputText = (out: Envelope): string =>
    new TextDecoder().decode(out.request!.files.find((f) => f.path.endsWith('/pandoc-input.json'))!.bytes);

  it('resolves a cite against a .bib in the VFS, for a single document', async () => {
    wasm.vfs_add_file('/p/refs.bib', REFS_BIB);
    wasm.vfs_add_file(
      '/p/doc.qmd',
      '---\nbibliography: refs.bib\nciteproc: true\n---\n\nKnuth wrote it [@knuth1984].\n',
    );
    const out = await wasm.render_pandoc_request('/p/doc.qmd', 'docx', SDE);
    expect(out.error).toBeUndefined();
    expect(out.success).toBe(true);
    const text = inputText(out);
    expect(text).not.toContain('"t":"Cite"');
    expect(text).toContain('TeXbook');
  });

  it('resolves a cite for a book chapter whose bibliography is set in _quarto.yml', async () => {
    wasm.vfs_add_file(
      '/b/_quarto.yml',
      'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\nbibliography: refs.bib\nciteproc: true\n',
    );
    wasm.vfs_add_file('/b/refs.bib', REFS_BIB);
    wasm.vfs_add_file('/b/index.qmd', '# Preface\n\nHello\n');
    wasm.vfs_add_file('/b/one.qmd', '# One\n\nKnuth wrote it [@knuth1984].\n');
    const out = await wasm.render_pandoc_request('/b/one.qmd', 'typst', SDE);
    expect(out.error).toBeUndefined();
    expect(out.success).toBe(true);
    const text = inputText(out);
    expect(text).not.toContain('"t":"Cite"');
    expect(text).toContain('TeXbook');
  });

  it('reads a csl: file from the VFS', async () => {
    wasm.vfs_add_file('/p/refs.bib', REFS_BIB);
    wasm.vfs_add_file('/p/mem.csl', NUMERIC_CSL);
    wasm.vfs_add_file(
      '/p/doc.qmd',
      '---\nbibliography: refs.bib\ncsl: mem.csl\nciteproc: true\n---\n\nKnuth wrote it [@knuth1984].\n',
    );
    const out = await wasm.render_pandoc_request('/p/doc.qmd', 'docx', SDE);
    expect(out.error).toBeUndefined();
    expect(out.success).toBe(true);
    expect(inputText(out)).toContain('[1]');
  });

  it('names the missing file, not a platform error', async () => {
    wasm.vfs_add_file(
      '/p/doc.qmd',
      '---\nbibliography: refs.bib\nciteproc: true\n---\n\nKnuth wrote it [@knuth1984].\n',
    );
    const out = await wasm.render_pandoc_request('/p/doc.qmd', 'docx', SDE);
    expect(out.request).toBeUndefined();
    const message = JSON.stringify([out.error, out.diagnostics]);
    expect(message).toContain('refs.bib');
    expect(message).not.toContain('not supported on this platform');
  });
});

describe('share tree export', () => {
  it('matches the request and carries Uint8Array bytes', async () => {
    const out = await goldenEnvelope();
    const version = wasm.get_pandoc_share_tree_version();
    expect(version).toBe(out.request!.share_tree_version);
    const tree = wasm.get_pandoc_share_tree();
    expect(tree.share_tree_version).toBe(version);
    const paths = tree.files.map((f) => f.path);
    expect(paths).toContain('filters/main.lua');
    expect(paths.some((p) => p.startsWith('pandoc/datadir/'))).toBe(true);
    expect(paths.some((p) => p.startsWith('formats/docx/'))).toBe(true);
    for (const f of tree.files) expect(f.bytes).toBeInstanceOf(Uint8Array);
  });
});

describe('classify_pandoc_completion', () => {
  const classify = (success: boolean, status: string, stderr: string) =>
    JSON.parse(wasm.classify_pandoc_completion('pandoc-write', success, status, stderr, '/__q2_share__/pandoc-input.json')) as {
      success: boolean;
      diagnostics: { kind: string; title: string; code?: string }[];
    };

  it('gives Q-20-3 carrying json_path on a non-zero exit, without claiming it was retained', () => {
    const out = classify(false, 'exit status: 64', 'pandoc: boom\n');
    expect(out.success).toBe(false);
    expect(out.diagnostics).toHaveLength(1);
    expect(out.diagnostics[0].code).toBe('Q-20-3');
    expect(out.diagnostics[0].title).toContain('/__q2_share__/pandoc-input.json');
    expect(out.diagnostics[0].title).toContain('pandoc: boom');
    expect(out.diagnostics[0].title).not.toContain('retained');
  });

  it('gives Q-11-1 for warnings on a zero exit', () => {
    const out = classify(true, 'exit status: 0', '[WARNING] Could not fetch resource a.png\nnoise\n');
    expect(out.success).toBe(true);
    expect(out.diagnostics.map((d) => d.code)).toEqual(['Q-11-1']);
  });
});

describe('format table and resolver', () => {
  const formats = () =>
    JSON.parse(wasm.get_pandoc_formats()).formats as { key: string; available: boolean; hidden: boolean }[];
  const resolve = (path: string) =>
    JSON.parse(wasm.resolve_pandoc_formats(path)) as {
      success: boolean;
      error?: string;
      source?: string;
      formats?: { key: string; class: string; extension?: string }[];
    };

  it('lists the downloadable formats in menu order', () => {
    expect(formats().map((f) => f.key)).toEqual(['docx', 'pptx', 'epub', 'typst', 'typst-pdf']);
    expect(formats().every((f) => f.available)).toBe(true);
    // `pdf` was hidden until host H8 wired the chain; nothing is hidden now.
    expect(formats().filter((f) => f.hidden)).toEqual([]);
  });

  it('takes the first key of a format map', () => {
    wasm.vfs_add_file('/d/doc.qmd', '---\nformat:\n  docx: default\n  html: default\n---\n');
    const r = resolve('/d/doc.qmd');
    expect(r.success).toBe(true);
    expect(r.source).toBe('document');
    expect(r.formats).toEqual([
      { key: 'docx', class: 'download' },
      { key: 'html', class: 'preview' },
    ]);
  });

  it('applies the _quarto.yml format when the document names none, and lets the document win', () => {
    wasm.vfs_add_file('/p/_quarto.yml', 'project:\n  type: default\nformat:\n  pptx: default\n');
    wasm.vfs_add_file('/p/bare.qmd', '# Hi\n');
    wasm.vfs_add_file('/p/own.qmd', '---\nformat: docx\n---\n');
    expect(resolve('/p/bare.qmd')).toMatchObject({ source: 'project', formats: [{ key: 'pptx', class: 'download' }] });
    expect(resolve('/p/own.qmd')).toMatchObject({ source: 'document', formats: [{ key: 'docx' }] });
  });

  it('classes an unknown format as neither and no format as html', () => {
    wasm.vfs_add_file('/d/odd.qmd', '---\nformat: nonsense\n---\n');
    wasm.vfs_add_file('/d/none.qmd', '# Hi\n');
    expect(resolve('/d/odd.qmd').formats).toEqual([{ key: 'nonsense', class: 'neither' }]);
    expect(resolve('/d/none.qmd')).toMatchObject({ source: 'default', formats: [{ key: 'html', class: 'preview' }] });
  });

  it('resolves typst by output-ext: pdf (the default) is the compile chain, anything else the source under that extension', () => {
    wasm.vfs_add_file('/t/default.qmd', '---\nformat: typst\n---\n');
    wasm.vfs_add_file('/t/pdf.qmd', '---\nformat: typst\noutput-ext: pdf\n---\n');
    wasm.vfs_add_file('/t/typ.qmd', '---\nformat: typst\noutput-ext: typ\n---\n');
    wasm.vfs_add_file('/t/literal.qmd', '---\nformat:\n  typst:\n    output-ext: typst\n---\n');
    expect(resolve('/t/default.qmd').formats).toEqual([{ key: 'typst-pdf', class: 'download' }]);
    expect(resolve('/t/pdf.qmd').formats).toEqual([{ key: 'typst-pdf', class: 'download' }]);
    expect(resolve('/t/typ.qmd').formats).toEqual([{ key: 'typst', class: 'download', extension: 'typ' }]);
    expect(resolve('/t/literal.qmd').formats).toEqual([{ key: 'typst', class: 'download', extension: 'typst' }]);
  });

  it('reads output-ext from the project when the document names none (document wins)', () => {
    wasm.vfs_add_file('/q/_quarto.yml', 'project:\n  type: default\nformat:\n  typst:\n    output-ext: typ\n');
    wasm.vfs_add_file('/q/bare.qmd', '# Hi\n');
    wasm.vfs_add_file('/q/own.qmd', '---\noutput-ext: pdf\n---\n');
    expect(resolve('/q/bare.qmd').formats).toEqual([{ key: 'typst', class: 'download', extension: 'typ' }]);
    expect(resolve('/q/own.qmd').formats).toEqual([{ key: 'typst-pdf', class: 'download' }]);
  });

  it("a document's own format: pdf is neither (LaTeX; use format: typst)", () => {
    wasm.vfs_add_file('/d/pdf.qmd', '---\nformat: pdf\n---\n');
    expect(resolve('/d/pdf.qmd').formats).toEqual([{ key: 'pdf', class: 'neither' }]);
  });

  it('a document cannot name the internal typst-pdf artifact key', () => {
    wasm.vfs_add_file('/d/art.qmd', '---\nformat: typst-pdf\n---\n');
    expect(resolve('/d/art.qmd').formats).toEqual([{ key: 'typst-pdf', class: 'neither' }]);
  });

  it('reports a missing file', () => {
    expect(resolve('/d/missing.qmd').success).toBe(false);
  });
});

describe('typst request (R4)', () => {
  afterEach(() => vi.unstubAllGlobals());

  const DOC = '---\ntitle: Typst Doc\n---\n\n# Hello\n\nA *typst* paragraph.\n\n![A figure](figure.png)\n';

  it('carries the template as files, no images, and the output as .typ', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);
    wasm.vfs_add_binary_file('/project/figure.png', FIGURE);
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'typst', SDE);
    expect(out.success).toBe(true);
    const request = out.request!;
    expect(request.writer).toBe('typst');
    expect(request.output_path).toBe('/project/doc.typ');
    expect(request.post).toBe('none');
    const templateFiles = request.files.filter((f) => f.path.includes('/pandoc-typst-template/'));
    expect(templateFiles.length).toBeGreaterThanOrEqual(8);
    // pandoc never reads a typst image; only the pdf request mounts it.
    expect(request.resource_refs).toEqual([]);
    const pdf = await wasm.render_pandoc_request('/project/doc.qmd', 'typst-pdf', SDE);
    expect(pdf.success).toBe(true);
    expect(pdf.request!.post).toBe('compile_typst');
    expect(pdf.request!.output_path).toBe('/project/doc.typ');
    expect(pdf.request!.resource_refs.map((f) => f.path)).toEqual(['/project/figure.png']);
    expect(pdf.request!.job_id).not.toBe(request.job_id);
  });

  it('reads a user template and partials from the VFS', async () => {
    wasm.vfs_add_file('/project/my.typ', '// USER-TEMPLATE\n$body$\n');
    wasm.vfs_add_file('/project/p/typst-show.typ', '// USER-SHOW\n');
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\ntitle: T\nformat:\n  typst:\n    template: my.typ\n    template-partials:\n      - p/typst-show.typ\n---\n\n# Hi\n',
    );
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'typst', SDE);
    expect(out.success).toBe(true);
    const text = (suffix: string) =>
      new TextDecoder().decode(out.request!.files.find((f) => f.path.endsWith(suffix))!.bytes);
    expect(text('/pandoc-typst-template/template.typ')).toBe('// USER-TEMPLATE\n$body$\n');
    expect(text('/pandoc-typst-template/typst-show.typ')).toBe('// USER-SHOW\n');
  });

  it('reports a failed remote image, CSS-inlines the styled table in Rust, and sends no quarto-cli-path', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('no', { status: 404, statusText: 'Not Found' })));
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\ntitle: T\n---\n\n![remote](https://example.com/a.png)\n\n```{=html}\n<style>td{color:red}</style><table><tr><td>x</td></tr></table>\n```\n',
    );
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'typst', SDE);
    expect(out.success).toBe(true);
    expect(out.diagnostics.map((d) => d.code).filter(Boolean)).toEqual(['Q-20-9']);
    const input = new TextDecoder().decode(out.request!.files.find((f) => f.path.endsWith('/pandoc-input.json'))!.bytes);
    expect(input).not.toContain('<style');
    expect(input).toContain('color');
    const blob = JSON.parse(Buffer.from(out.request!.env.QUARTO_FILTER_PARAMS, 'base64').toString());
    expect(blob['quarto-cli-path']).toBeUndefined();
  });

  it('feeds the host font list to the filter params', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '# Hi\n');
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'typst-pdf', SDE, undefined, ['Inter']);
    const blob = JSON.parse(Buffer.from(out.request!.env.QUARTO_FILTER_PARAMS, 'base64').toString());
    expect(blob['typst-available-fonts']).toEqual(['Inter']);
  });

  it('exports the typst assets separately from the share tree, and the date prelude', () => {
    const assets = wasm.get_typst_assets();
    expect(assets.typst_assets_version).toBe(wasm.get_typst_assets_version());
    expect(assets.typst_assets_version).not.toBe(wasm.get_pandoc_share_tree_version());
    expect(assets.files.some((f) => f.path.startsWith('packages/preview/'))).toBe(true);
    expect(assets.files.some((f) => f.path.startsWith('fonts/'))).toBe(true);
    expect(assets.files.every((f) => f.bytes instanceof Uint8Array)).toBe(true);
    expect(wasm.typst_date_prelude(SDE)).toBe(
      '#set document(date: datetime(year: 2023, month: 11, day: 14, hour: 22, minute: 13, second: 20))\n',
    );
  });
});

// The point of the phase: a docx request produced inside the hub's wasm runs in
// the real pandoc.wasm (host phase H1's `execute`) and yields a docx with the
// image. Needs `node scripts/fetch-pandoc-wasm.mjs` (a missing wasm skips locally,
// fails in CI via pandocHost.wasm.test.ts's environment check).
describe.skipIf(!pandocWasmAvailable())('request built in wasm, run in pandoc.wasm', () => {
  // A valid 1x1 PNG (the golden's 8-byte stub is not an image pandoc can size).
  const PNG_1X1 = fromBase64(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  );

  it('produces a docx that carries the document text and the image', async () => {
    const module = await WebAssembly.compile(readFileSync(WASM_PATH));
    wasm.vfs_add_file('/project/doc.qmd', GOLDEN_DOC);
    wasm.vfs_add_binary_file('/project/figure.png', PNG_1X1);
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE);
    expect(out.success).toBe(true);
    const tree = wasm.get_pandoc_share_tree();
    const result = await execute(out.request as unknown as PandocRequest, tree, { module });
    if (!result.ok) throw new Error(`pandoc failed (${result.kind}): ${result.stderr}`);
    const entries = unzipSync(result.output);
    expect(Object.keys(entries)).toContain('word/document.xml');
    const document = new TextDecoder().decode(entries['word/document.xml']);
    expect(document).toContain('A figure');
    expect(document).toContain('docx');
    expect(Object.keys(entries).some((n) => n.startsWith('word/media/'))).toBe(true);
    // Classification of a clean run: no error diagnostics.
    const classified = JSON.parse(
      wasm.classify_pandoc_completion('pandoc-write', true, 'exit status: 0', result.stderr, out.request!.json_path as string),
    );
    expect(classified.diagnostics.filter((d: { kind: string }) => d.kind === 'error')).toEqual([]);
  }, 120_000);

  it('produces the .typ source of a typst request, template partials included', async () => {
    const module = await WebAssembly.compile(readFileSync(WASM_PATH));
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Typst Doc\n---\n\n# Hello\n\nA *typst* paragraph.\n\n![A figure](figure.png)\n');
    wasm.vfs_add_binary_file('/project/figure.png', PNG_1X1);
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'typst', SDE);
    expect(out.success).toBe(true);
    const tree = wasm.get_pandoc_share_tree();
    const result = await execute(out.request as unknown as PandocRequest, tree, { module });
    if (!result.ok) throw new Error(`pandoc failed (${result.kind}): ${result.stderr}`);
    const typ = new TextDecoder().decode(result.output);
    expect(typ).toContain('Typst Doc');
    expect(typ).toContain('A figure');
    expect(typ).toContain('typst');
    // The vendored template's partials were found next to template.typ.
    expect(typ).toContain('#show: doc => article(');
    expect(typ).toContain('image("figure.png")');
  }, 120_000);

  it('runs a book chapter\'s typst request, orange-book filter included, with no "cannot open" error', async () => {
    const module = await WebAssembly.compile(readFileSync(WASM_PATH));
    addBook('/b');
    const out = await wasm.render_pandoc_request('/b/one.qmd', 'typst', SDE);
    expect(out.success).toBe(true);
    const tree = wasm.get_pandoc_share_tree();
    const result = await execute(out.request as unknown as PandocRequest, tree, { module });
    if (!result.ok) throw new Error(`pandoc failed (${result.kind}): ${result.stderr}`);
    expect(new TextDecoder().decode(result.output)).toContain('First chapter');
  }, 120_000);

  it('hands a chapter-alone typst request the book title and author, so the orange-book title page compiles', async () => {
    const module = await WebAssembly.compile(readFileSync(WASM_PATH));
    wasm.vfs_add_file('/b/_quarto.yml', 'project:\n  type: book\nbook:\n  title: B\n  author: "Gordon"\n  chapters:\n    - index.qmd\n    - one.qmd\n');
    wasm.vfs_add_file('/b/index.qmd', '# Preface\n\nHello\n');
    wasm.vfs_add_file('/b/one.qmd', '# One\n\nFirst chapter.\n');
    const out = await wasm.render_pandoc_request('/b/one.qmd', 'typst', SDE, undefined, undefined, undefined, { scope: 'chapter' });
    const tree = wasm.get_pandoc_share_tree();
    const result = await execute(out.request as unknown as PandocRequest, tree, { module });
    if (!result.ok) throw new Error('fail ' + result.stderr);
    const typ = new TextDecoder().decode(result.output);
    // Without these orange-book's `author: ()` default reaches `text()`.
    expect(typ).toContain('title: [B]');
    expect(typ).toContain('author: "Gordon"');
  }, 120_000);

  it('links a whole-book cross-chapter link to the chapter heading, never an empty #link()', async () => {
    const module = await WebAssembly.compile(readFileSync(WASM_PATH));
    wasm.vfs_add_file('/b/_quarto.yml', 'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n');
    wasm.vfs_add_file('/b/index.qmd', '# Preface\n\nSee [one](one.qmd).\n');
    // A `title:` plus the chapter's own heading: the merge puts an id-less
    // synthesized title heading first, which the link must not resolve to.
    wasm.vfs_add_file('/b/one.qmd', '---\ntitle: One Title\n---\n\n# One {#sec-one}\n\nFirst chapter.\n');
    const out = await wasm.render_pandoc_request('/b/index.qmd', 'typst', SDE, undefined, undefined, undefined, { scope: 'auto' });
    const tree = wasm.get_pandoc_share_tree();
    const result = await execute(out.request as unknown as PandocRequest, tree, { module });
    if (!result.ok) throw new Error(`pandoc failed (${result.kind}): ${result.stderr}`);
    const typ = new TextDecoder().decode(result.output);
    // `#link()[one]` is the Typst compile error "expected string, dictionary,
    // location, or label, found content".
    expect(typ).not.toContain('#link()');
    expect(typ).toContain('#link(<sec-one>)[one]');
  }, 120_000);

  it('produces a pptx that carries the slide text and the image', async () => {
    const module = await WebAssembly.compile(readFileSync(WASM_PATH));
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Deck\n---\n\n# First slide\n\nA *pptx* paragraph.\n\n![A figure](figure.png)\n');
    wasm.vfs_add_binary_file('/project/figure.png', PNG_1X1);
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'pptx', SDE);
    expect(out.success).toBe(true);
    expect(out.request!.post).not.toBe('compile_typst');
    const tree = wasm.get_pandoc_share_tree();
    const result = await execute(out.request as unknown as PandocRequest, tree, { module });
    if (!result.ok) throw new Error(`pandoc failed (${result.kind}): ${result.stderr}`);
    const entries = unzipSync(result.output);
    const names = Object.keys(entries);
    expect(names).toContain('ppt/slides/slide1.xml');
    expect(names.some((n) => n.startsWith('ppt/media/'))).toBe(true);
    const slides = names.filter((n) => /^ppt\/slides\/slide\d+\.xml$/.test(n));
    const text = slides.map((n) => new TextDecoder().decode(entries[n])).join('\n');
    expect(text).toContain('First slide');
    expect(text).toContain('pptx');
  }, 120_000);

  it('produces an epub with the user stylesheet and cover image from the VFS, each once', async () => {
    const module = await WebAssembly.compile(readFileSync(WASM_PATH));
    wasm.vfs_add_file(
      '/project/doc.qmd',
      '---\ntitle: Book\nformat:\n  epub:\n    epub-cover-image: cover.png\n    css: book.css\n---\n\n# Chapter\n\nAn *epub* paragraph.\n\n![A figure](figure.png)\n',
    );
    wasm.vfs_add_binary_file('/project/figure.png', PNG_1X1);
    wasm.vfs_add_binary_file('/project/cover.png', PNG_1X1);
    wasm.vfs_add_file('/project/book.css', 'p { color: red; }\n');
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'epub', SDE);
    expect(out.success).toBe(true);
    const tree = wasm.get_pandoc_share_tree();
    const result = await execute(out.request as unknown as PandocRequest, tree, { module });
    if (!result.ok) throw new Error(`pandoc failed (${result.kind}): ${result.stderr}`);
    const entries = unzipSync(result.output);
    const names = Object.keys(entries);
    expect(names).toContain('mimetype');
    // `css:` reaches pandoc only as the absolute `--css=` flag, so the
    // stylesheet is embedded once (pandoc renames it stylesheetN.css).
    const css = names.filter((n) => n.endsWith('.css'));
    const mine = css.filter((n) => new TextDecoder().decode(entries[n]).includes('color: red'));
    expect(mine).toHaveLength(1);
    expect(names.some((n) => n.includes('media/') && n.endsWith('.png'))).toBe(true);
    const chapters = names.filter((n) => n.endsWith('.xhtml'));
    const text = chapters.map((n) => new TextDecoder().decode(entries[n])).join('\n');
    expect(text).toContain('An <em>epub</em> paragraph');
    // The vendored callout stylesheet arrived through the request's files.
    expect(text).toMatch(/callout/);
  }, 120_000);
});

// R6: remote images are fetched in the browser (through the bridge's hardened
// `fetch`, stubbed here) into a click-time snapshot of the VFS.
describe('remote images (R6)', () => {
  const REMOTE = 'https://img.example.com/pic.png';
  const REMOTE_DOC = `---\ntitle: Remote\n---\n\nBefore ![the alt](${REMOTE}) after.\n`;
  const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]);
  const png = () => new Response(PNG, { status: 200, headers: { 'content-type': 'image/png' } });
  const inputJson = (request: NonNullable<Envelope['request']>) =>
    new TextDecoder().decode(request.files.find((f) => f.path.endsWith('/pandoc-input.json'))!.bytes);

  afterEach(() => vi.unstubAllGlobals());

  it('fetches with credentials omitted, mounts the bytes and rewrites the src', async () => {
    const seen: [string, RequestInit][] = [];
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, init: RequestInit) => {
        seen.push([url, init]);
        return png();
      }),
    );
    wasm.vfs_add_file('/project/doc.qmd', REMOTE_DOC);
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE);
    expect(out.success).toBe(true);
    expect(seen.map(([u]) => u)).toEqual([REMOTE]);
    expect(seen[0][1].credentials).toBe('omit');
    const mounted = out.request!.resource_refs.filter((f) => f.path.startsWith('/project/_remote/'));
    expect(mounted).toHaveLength(1);
    expect(mounted[0].path).toMatch(/^\/project\/_remote\/[0-9a-f]{16}\.png$/);
    expect(Array.from(mounted[0].bytes)).toEqual(Array.from(PNG));
    const json = inputJson(out.request!);
    expect(json).toContain(mounted[0].path);
    expect(json).toContain('q2-remote-src');
    // The live VFS did not receive the mount (it went into the snapshot).
    expect(JSON.parse(wasm.vfs_list_files()).files.some((f: string) => f.includes('_remote'))).toBe(false);
  });

  it('refuses a non-https URL without calling fetch, leaving the URL and a warning', async () => {
    const fetchMock = vi.fn(async () => png());
    vi.stubGlobal('fetch', fetchMock);
    wasm.vfs_add_file('/project/doc.qmd', REMOTE_DOC.replace('https://', 'http://'));
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE);
    expect(out.success).toBe(true);
    expect(fetchMock).not.toHaveBeenCalled();
    expect(out.diagnostics.some((d) => d.title.startsWith('Remote image'))).toBe(true);
    expect(inputJson(out.request!)).toContain('http://img.example.com/pic.png');
  });

  it('keeps the click-time bytes when the VFS changes during the fetch', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => (release = resolve));
    const fetchMock = vi.fn(async () => {
      await gate;
      return png();
    });
    vi.stubGlobal('fetch', fetchMock);
    wasm.vfs_add_file('/project/doc.qmd', `${REMOTE_DOC}\nClick time text.\n\n![local](local.png)\n`);
    wasm.vfs_add_binary_file('/project/local.png', new Uint8Array([1, 1, 1]));
    const pending = wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE);
    // The request is now parked on the network (the first await).
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalled());
    // Automerge keeps syncing while the request awaits the network.
    wasm.vfs_add_file('/project/doc.qmd', '---\ntitle: Remote\n---\n\nEdited after the click.\n');
    wasm.vfs_add_binary_file('/project/local.png', new Uint8Array([9, 9, 9]));
    wasm.vfs_remove_file('/project/doc.qmd');
    release();
    const out = await pending;
    expect(out.error).toBeUndefined();
    expect(out.success).toBe(true);
    const json = inputJson(out.request!);
    expect(json).toContain('Click');
    expect(json).not.toContain('Edited');
    const local = out.request!.resource_refs.find((f) => f.path === '/project/local.png');
    expect(Array.from(local!.bytes)).toEqual([1, 1, 1]);
  });

  it('stops the download when the click is aborted, and still builds a request', async () => {
    let seenSignal: AbortSignal | undefined;
    vi.stubGlobal(
      'fetch',
      vi.fn(
        (_url: string, init: RequestInit) =>
          new Promise((_resolve, reject) => {
            seenSignal = init.signal!;
            init.signal!.addEventListener('abort', () => reject(init.signal!.reason));
          }),
      ),
    );
    wasm.vfs_add_file('/project/doc.qmd', REMOTE_DOC);
    const click = new AbortController();
    const pending = wasm.render_pandoc_request('/project/doc.qmd', 'docx', SDE, undefined, undefined, click.signal);
    await vi.waitFor(() => expect(seenSignal).toBeDefined());
    click.abort(new Error('download cancelled'));
    const out = await pending;
    expect(seenSignal!.aborted).toBe(true);
    expect(out.diagnostics.some((d) => d.title.includes('download cancelled'))).toBe(true);
  });
});

describe.skipIf(!pandocWasmAvailable())('remote images run in pandoc.wasm (R6)', () => {
  const PNG_1X1 = fromBase64(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  );
  const REMOTE = 'https://img.example.com/pic.png';
  const ok = () => new Response(PNG_1X1, { status: 200, headers: { 'content-type': 'image/png' } });
  const gone = () => new Response('no', { status: 404, statusText: 'Not Found' });

  afterEach(() => vi.unstubAllGlobals());

  async function run(format: string, qmd: string, fetchReply: () => Response) {
    vi.stubGlobal('fetch', vi.fn(async () => fetchReply()));
    const module = await WebAssembly.compile(readFileSync(WASM_PATH));
    wasm.vfs_add_file('/project/doc.qmd', qmd);
    const out = await wasm.render_pandoc_request('/project/doc.qmd', format, SDE);
    expect(out.success).toBe(true);
    const tree = wasm.get_pandoc_share_tree();
    const result = await execute(out.request as unknown as PandocRequest, tree, { module });
    if (!result.ok) throw new Error(`pandoc failed (${result.kind}): ${result.stderr}`);
    return { out, result };
  }

  it('docx embeds the fetched image, with no fetch warning from pandoc', async () => {
    const { result } = await run('docx', `# Hi\n\n![the alt](${REMOTE})\n`, ok);
    const entries = unzipSync(result.output);
    expect(Object.keys(entries).some((n) => n.startsWith('word/media/'))).toBe(true);
    expect(result.stderr).not.toMatch(/Could not fetch|fetch/i);
  }, 120_000);

  it('docx with a failed fetch still builds, showing the alt text', async () => {
    const { out, result } = await run('docx', `# Hi\n\n![the alt text](${REMOTE})\n`, gone);
    expect(out.diagnostics.some((d) => d.title.startsWith('Remote image'))).toBe(true);
    const document = new TextDecoder().decode(unzipSync(result.output)['word/document.xml']);
    expect(document).toContain('the alt text');
  }, 120_000);

  it('typst source names the mounted image; a failed fetch gives alt text, not exit 83', async () => {
    const good = await run('typst', `# Hi\n\n![the alt](${REMOTE})\n`, ok);
    expect(new TextDecoder().decode(good.result.output)).toMatch(/_remote\/[0-9a-f]{16}\.png/);
    const bad = await run('typst', `# Hi\n\nBefore ![the alt text](${REMOTE}) after.\n`, gone);
    const typ = new TextDecoder().decode(bad.result.output);
    expect(typ).toContain('the alt text');
    expect(typ).not.toContain('img.example.com');
    expect(bad.out.diagnostics.map((d) => d.code)).toContain('Q-20-9');
  }, 120_000);
});
