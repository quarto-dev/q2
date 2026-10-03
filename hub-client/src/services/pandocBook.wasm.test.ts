/**
 * WASM end-to-end tests for the whole-book request (pandoc-wasm epic, R9):
 * `render_pandoc_request` on a book chapter as typst, pdf or epub returns the
 * whole book, and its `options` object (`scope`, `capturesByPath`,
 * `onProgress`). The native twins are in
 * `crates/quarto-core/tests/integration/pandoc_request_books.rs`; the parity
 * book below is an inline copy of that file's `parity_book`, and its
 * `pandoc-input.json` must equal the golden recorded there.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { gzipSync } from 'fflate';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface RequestFile {
  path: string;
  bytes: Uint8Array;
}

interface Envelope {
  success: boolean;
  error?: string;
  diagnostics: { kind: string; title: string; code?: string; start_line?: number; rendered?: string }[];
  stats: { unexecuted_cells: number; book: { scope: 'book' | 'chapter'; chapters: number } | null };
  request?: {
    files: RequestFile[];
    resource_refs: RequestFile[];
    job_id: string;
    argv: string[];
  };
}

interface Options {
  scope?: 'auto' | 'chapter';
  capturesByPath?: Record<string, Uint8Array>;
  onProgress?: (index: number, total: number, file: string) => void | Promise<void>;
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
    options?: Options,
  ) => Promise<Envelope>;
  resolve_pandoc_formats: (path: string) => string;
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

const SDE = 1_700_000_000;
const PNG = new Uint8Array(
  Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==',
    'base64',
  ),
);
const REFS_BIB =
  '@book{knuth1984,\n  author = {Knuth, Donald E.},\n  title = {The TeXbook},\n  year = {1984},\n  publisher = {Addison-Wesley}\n}\n' +
  '@article{doe2020,\n  author = {Doe, Jane},\n  title = {A Paper},\n  year = {2020}\n}\n';

const ROOT = '/project';

function add(rel: string, content: string | Uint8Array) {
  if (typeof content === 'string') wasm.vfs_add_file(`${ROOT}/${rel}`, content);
  else wasm.vfs_add_binary_file(`${ROOT}/${rel}`, content);
}

function render(rel: string, format: string, options?: Options, signal?: AbortSignal): Promise<Envelope> {
  return wasm.render_pandoc_request(`${ROOT}/${rel}`, format, SDE, undefined, undefined, signal, options);
}

function inputJson(out: Envelope): string {
  const file = out.request!.files.find((f) => f.path.endsWith('/pandoc-input.json'))!;
  return new TextDecoder().decode(file.bytes);
}

/** The document's words as a reader sees them: every `Str`, space-joined. */
function plainText(out: Envelope): string {
  const words: string[] = [];
  const walk = (node: unknown) => {
    if (Array.isArray(node)) return node.forEach(walk);
    if (node && typeof node === 'object') {
      const n = node as { t?: string; c?: unknown };
      if (n.t === 'Str' && typeof n.c === 'string') words.push(n.c);
      else Object.values(n).forEach(walk);
    }
  };
  walk(JSON.parse(inputJson(out)).blocks);
  return words.join(' ');
}

/** Fixture A of the native tests: the parity book. */
function parityBook() {
  add(
    '_quarto.yml',
    'project:\n  type: book\n\nkeep-typ: true\n\nbook:\n  title: "Parity Book"\n  author: "A. Author"\n  cover-image: cover.png\n  chapters:\n    - index.qmd\n    - part: "Part One"\n      chapters:\n        - one.qmd\n        - sub/two.qmd\n    - part: "Part Two"\n      href: partpage.qmd\n      chapters:\n        - titleonly.qmd\n        - withh1.qmd\n        - neither.qmd\n        - textfirst.qmd\n  appendices:\n    - app.qmd\n\nbibliography: refs.bib\nciteproc: true\n',
  );
  add('refs.bib', REFS_BIB);
  add('index.qmd', '---\ntitle: Preface\n---\n\nWelcome. See @sec-one and [chapter two](sub/two.qmd).\n');
  add('one.qmd', '# One {#sec-one}\n\nKnuth wrote it [@knuth1984].\n\n![root local](rootlocal.png)\n\n![root site](/img/a.png)\n');
  add(
    'sub/two.qmd',
    '# Two {#sec-two}\n\nDoe and Knuth [@doe2020; @knuth1984].\n\n![local](local.png)\n\n![site root](/img/a.png)\n\n![up](../top.png)\n\nBack to @sec-one and [chapter one](../one.qmd).\n',
  );
  add('partpage.qmd', '---\ntitle: Part Two Page\n---\n\nIntro to part two.\n');
  add('titleonly.qmd', '---\ntitle: Title Only Chapter\n---\n\nText with no heading.\n');
  add('withh1.qmd', '---\ntitle: Meta Title\n---\n\n# Heading Y\n\nBody y.\n');
  add('neither.qmd', 'Just text, no title and no heading.\n');
  add('textfirst.qmd', 'Some leading text.\n\n## Early Heading\n\nBody.\n');
  add('app.qmd', '# Appendix Alpha\n\nAppendix body.\n');
  for (const f of ['rootlocal.png', 'top.png', 'img/a.png', 'sub/local.png', 'cover.png']) add(f, PNG);
}

function smallBook() {
  add(
    '_quarto.yml',
    'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n',
  );
  add('index.qmd', '# Preface\n\nHello\n');
  add('one.qmd', '# One\n\nFirst chapter.\n');
  add('two.qmd', '# Two\n\nSecond chapter.\n');
}

/** The extension partials are named by absolute path, which differs by target (see the native test). */
function normalizeExtensionPaths(value: unknown): unknown {
  const re = /^\/.*?\/orange-book\/(?:_extensions\/orange-book\/)?/;
  if (typeof value === 'string' && value.startsWith('/') && value.includes('/orange-book/')) {
    return value.replace(re, '<EXT>/');
  }
  if (Array.isArray(value)) return value.map(normalizeExtensionPaths);
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, normalizeExtensionPaths(v)]));
  }
  return value;
}

/** As the native test's `strip_unstable`: `s` source ids and the mtime-derived `date-modified`. */
function stripUnstable(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(stripUnstable);
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value)
        .filter(([k]) => k !== 's' && k !== 'date-modified')
        .map(([k, v]) => [k, stripUnstable(v)]),
    );
  }
  return value;
}

describe('whole-book request (R9)', () => {
  it('equals the native golden for the parity book, typst', async () => {
    parityBook();
    const out = await render('one.qmd', 'typst');
    expect(out.error).toBeUndefined();
    expect(out.stats.book).toEqual({ scope: 'book', chapters: 9 });
    const json = JSON.parse(inputJson(out));
    delete json.astContext; // 64-bit file ids natively, 32-bit here
    const golden = JSON.parse(
      await readFile(
        join(here, '../../../crates/quarto-core/tests/integration/pandoc_request_books/pandoc-input.golden.json'),
        'utf8',
      ),
    );
    expect(stripUnstable(normalizeExtensionPaths(json))).toEqual(golden);
    // No absolute path of the VFS leaks into it.
    expect(JSON.stringify(json)).not.toContain(`${ROOT}/`);
  });

  it('is one request containing every chapter, for typst, pdf and epub', async () => {
    smallBook();
    for (const format of ['typst', 'pdf', 'epub']) {
      const out = await render('one.qmd', format);
      expect(out.error, format).toBeUndefined();
      expect(out.stats.book, format).toEqual({ scope: 'book', chapters: 3 });
      const text = plainText(out);
      for (const word of ['Hello', 'First chapter.', 'Second chapter.']) expect(text, format).toContain(word);
    }
  });

  it("scope 'chapter' is the page alone; docx and pptx stay chapter-only without a warning", async () => {
    smallBook();
    const alone = await render('one.qmd', 'typst', { scope: 'chapter' });
    expect(alone.stats.book).toEqual({ scope: 'chapter', chapters: 3 });
    expect(plainText(alone)).toContain('First chapter.');
    expect(plainText(alone)).not.toContain('Second');
    for (const format of ['docx', 'pptx']) {
      const out = await render('one.qmd', format);
      expect(out.stats.book).toEqual({ scope: 'chapter', chapters: 3 });
      expect(plainText(out)).not.toContain('Second');
      expect(out.diagnostics.filter((d) => d.kind === 'warning')).toEqual([]);
    }
  });

  it('is the same request whichever chapter it is asked from', async () => {
    smallBook();
    const a = await render('one.qmd', 'typst');
    const b = await render('two.qmd', 'typst');
    expect(a.request!.job_id).toBe(b.request!.job_id);
  });

  it('renders a page that is not a chapter alone, with no mention of the book', async () => {
    smallBook();
    add('notes.qmd', '# Notes\n\nA page outside the book.\n');
    const out = await render('notes.qmd', 'typst');
    expect(out.error).toBeUndefined();
    expect(out.stats.book).toEqual({ scope: 'chapter', chapters: 3 });
    expect(plainText(out)).toContain('A page outside the book.');
    expect(plainText(out)).not.toContain('First');
    expect(out.diagnostics.every((d) => !d.title.includes('book'))).toBe(true);
  });

  it('fails the whole render when a chapter is broken, naming its file', async () => {
    smallBook();
    add('two.qmd', '# Two\n\n{{< include missing-file.qmd >}}\n');
    const out = await render('one.qmd', 'typst');
    expect(out.success).toBe(false);
    expect(out.request).toBeUndefined();
    expect(out.error).toContain('two.qmd');
  });

  it("reports a later chapter's warning against that chapter's file and line", async () => {
    smallBook();
    add('two.qmd', '# Two\n\nSecond chapter {{< nosuchshortcode >}}.\n');
    const out = await render('one.qmd', 'typst');
    expect(out.error).toBeUndefined();
    const warning = out.diagnostics.find((d) => d.code === 'Q-16-3')!;
    expect(warning).toBeDefined();
    // Resolved against chapter two's own source, not chapter one's or nothing.
    expect(warning.start_line).toBe(3);
    expect(warning.rendered).toContain('two.qmd:3:');
    expect(warning.rendered).not.toContain('one.qmd');
  });

  it('serves each chapter its own capture, never an identical cell elsewhere', async () => {
    add(
      '_quarto.yml',
      'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n',
    );
    add('index.qmd', '# Preface\n\nHello\n');
    const one = '# One\n\nBefore.\n\n```{r}\nSAME\n```\n';
    const two = '# Two\n\nBefore.\n\n```{r}\nSAME\n```\n';
    add('one.qmd', one);
    add('two.qmd', two);
    const result =
      '::: {.cell}\n::: {.cell-output .cell-output-stdout}\nOUT_TWO\n:::\n:::';
    const capture = [
      {
        engine_name: 'r',
        input_qmd: two,
        result: { markdown: two.replace('```{r}\nSAME\n```', result) },
        files: [],
      },
    ];
    const gz = gzipSync(new TextEncoder().encode(JSON.stringify(capture)));
    const out = await render('one.qmd', 'typst', {
      capturesByPath: { 'two.qmd': gz, 'not-a-chapter.qmd': new Uint8Array([1, 2, 3]) },
    });
    expect(out.error).toBeUndefined();
    const text = inputJson(out);
    expect(text).toContain('OUT_TWO');
    expect(text.match(/SAME/g)).toHaveLength(1);
    // Chapter 1's cell is source; chapter 2's is served.
    expect(out.stats.unexecuted_cells).toBe(1);
  });

  it('mounts each chapter-relative image under its own directory for pdf and epub, with no Q-11-1', async () => {
    parityBook();
    for (const format of ['pdf', 'epub']) {
      const out = await render('one.qmd', format);
      expect(out.error, format).toBeUndefined();
      expect(out.diagnostics.map((d) => d.code), format).not.toContain('Q-11-1');
      const refs = out.request!.resource_refs.map((f) => f.path);
      for (const rel of ['rootlocal.png', 'sub/local.png', 'top.png', 'img/a.png']) {
        expect(refs, `${format} ${rel}`).toContain(`${ROOT}/${rel}`);
      }
    }
  });

  it('puts margin citations’ bibliography into the pdf request only', async () => {
    add(
      '_quarto.yml',
      'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\nbibliography: refs.bib\ncitation-location: margin\n',
    );
    add('refs.bib', REFS_BIB);
    add('index.qmd', '# Preface\n\nHello\n');
    add('one.qmd', '# One\n\nKnuth wrote it [@knuth1984].\n');
    const pdf = await render('one.qmd', 'pdf');
    expect(pdf.error).toBeUndefined();
    expect(pdf.request!.resource_refs.map((f) => f.path)).toContain(`${ROOT}/refs.bib`);
    const typst = await render('one.qmd', 'typst');
    expect(typst.request!.resource_refs.map((f) => f.path)).not.toContain(`${ROOT}/refs.bib`);
  });
});

describe('captures per chapter (R9 D-6)', () => {
  const CELL = '```{r}\nSAME\n```';
  const OUT = (text: string) =>
    `::: {.cell}\n::: {.cell-output .cell-output-stdout}\n${text}\n:::\n:::`;
  const capture = (qmd: string, cell: string, out: string, extra: Record<string, unknown> = {}, files: unknown[] = []) => ({
    engine_name: 'r',
    input_qmd: qmd,
    result: { markdown: qmd.replace('```{r}\n' + cell + '\n```', OUT(out)), ...extra },
    files,
  });
  const gz = (captures: unknown[]) => gzipSync(new TextEncoder().encode(JSON.stringify(captures)));

  const ONE = '# One\n\nBefore.\n\n```{r}\nSAME\n```\n';
  const TWO = '# Two\n\nBefore.\n\n```{r}\nSAME\n```\n';
  const THREE = '# Three\n\nBefore.\n\n```{r}\nTHREE_SRC\n```\n';

  function captureBook() {
    add(
      '_quarto.yml',
      'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - one.qmd\n    - two.qmd\n    - sub/three.qmd\n',
    );
    add('index.qmd', '# Preface\n\nHello\n');
    add('one.qmd', ONE);
    add('two.qmd', TWO);
    add('sub/three.qmd', THREE);
  }

  it("mounts a captured figure under its own chapter's directory only", async () => {
    captureBook();
    const figure = { path: 'doc_files/fig.png', contents_base64: Buffer.from(PNG).toString('base64') };
    const cap = capture(THREE, 'THREE_SRC', '', {}, [figure]);
    cap.result.markdown = THREE.replace(
      '```{r}\nTHREE_SRC\n```',
      '::: {.cell}\n::: {.cell-output-display}\n![](doc_files/fig.png)\n:::\n:::',
    );
    const out = await render('one.qmd', 'pdf', { capturesByPath: { 'sub/three.qmd': gz([cap]) } });
    expect(out.error).toBeUndefined();
    const refs = out.request!.resource_refs.map((f) => f.path);
    expect(refs).toContain(`${ROOT}/sub/doc_files/fig.png`);
    expect(refs).not.toContain(`${ROOT}/doc_files/fig.png`);
  });

  it("chapter scope uses capture_gz_json, else the active file's map entry; a book ignores the blob", async () => {
    captureBook();
    const byPath = {
      'one.qmd': gz([capture(ONE, 'SAME', 'MAP_ONE')]),
      'two.qmd': gz([capture(TWO, 'SAME', 'MAP_TWO')]),
    };
    const alone = await render('one.qmd', 'typst', { scope: 'chapter', capturesByPath: byPath });
    expect(inputJson(alone)).toContain('MAP_ONE');
    expect(inputJson(alone)).not.toContain('MAP_TWO');
    const blob = gz([capture(ONE, 'SAME', 'BLOB_ONE')]);
    const withBlob = await wasm.render_pandoc_request(
      `${ROOT}/one.qmd`, 'typst', SDE, blob, undefined, undefined, { scope: 'chapter', capturesByPath: byPath },
    );
    expect(inputJson(withBlob)).toContain('BLOB_ONE');
    expect(inputJson(withBlob)).not.toContain('MAP_ONE');
    // Whole book: the active file's own blob changes nothing.
    const plain = await render('one.qmd', 'typst', { capturesByPath: byPath });
    const ignored = await wasm.render_pandoc_request(
      `${ROOT}/one.qmd`, 'typst', SDE, blob, undefined, undefined, { capturesByPath: byPath },
    );
    expect(ignored.request!.job_id).toBe(plain.request!.job_id);
    expect(inputJson(ignored)).not.toContain('BLOB_ONE');
  });

  it('splices a stale capture where it still matches and leaves the rest as source', async () => {
    captureBook();
    const stale = gz([capture(TWO.replace('SAME', 'OLD'), 'OLD', 'STALE_OUT')]);
    const out = await render('one.qmd', 'typst', { capturesByPath: { 'two.qmd': stale } });
    expect(out.error).toBeUndefined();
    expect(inputJson(out)).not.toContain('STALE_OUT');
    expect(inputJson(out).match(/SAME/g)).toHaveLength(2);
  });

  it("takes engine includes from the first chapter's capture only", async () => {
    add('_quarto.yml', 'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - two.qmd\n');
    const index = '# Preface\n\n```{r}\nSAME\n```\n';
    add('index.qmd', index);
    add('two.qmd', TWO);
    const inc = (marker: string) => ({
      includes: { header_includes: [marker], include_before: [], include_after: [] },
    });
    const later = await render('index.qmd', 'typst', {
      capturesByPath: { 'two.qmd': gz([capture(TWO, 'SAME', 'OUT', inc('LATER_HEADER'))]) },
    });
    expect(inputJson(later)).not.toContain('LATER_HEADER');
    const first = await render('index.qmd', 'typst', {
      capturesByPath: { 'index.qmd': gz([capture(index, 'SAME', 'OUT', inc('FIRST_HEADER'))]) },
    });
    expect(inputJson(first)).toContain('FIRST_HEADER');
  });

  it('ignores a key that is not a chapter and degrades a corrupt capture to source with a diagnostic naming the chapter', async () => {
    captureBook();
    const out = await render('one.qmd', 'typst', {
      capturesByPath: { 'nowhere.qmd': gz([]), 'two.qmd': new TextEncoder().encode('not gzip at all') },
    });
    expect(out.error).toBeUndefined();
    expect(out.request).toBeDefined();
    expect(out.diagnostics.some((d) => d.title.includes('two.qmd'))).toBe(true);
    expect(inputJson(out).match(/SAME/g)).toHaveLength(2);
  });
});

describe('progress, yield and abort (R9)', () => {
  function bigBook(n: number) {
    const chapters = Array.from({ length: n }, (_, i) => `    - c${i + 1}.qmd`).join('\n');
    add('_quarto.yml', `project:\n  type: book\nbook:\n  title: Big\n  chapters:\n    - index.qmd\n${chapters}\n`);
    add('index.qmd', '# Preface\n\nHello\n');
    for (let i = 1; i <= n; i++) add(`c${i}.qmd`, `# Chapter ${i}\n\nBody ${i}.\n`);
  }

  it('calls onProgress once per chapter in order, yielding to timers between them', async () => {
    bigBook(20);
    const calls: [number, number, string][] = [];
    const out = await render('c1.qmd', 'typst', {
      onProgress: async (i, n, file) => {
        calls.push([i, n, file]);
        await new Promise((r) => setTimeout(r, 0));
      },
    });
    expect(out.error).toBeUndefined();
    // `index.qmd` and the 20 chapters.
    expect(calls).toHaveLength(21);
    calls.forEach(([i, n, file], k) => {
      expect([i, n, file]).toEqual([k + 1, 21, k === 0 ? 'index.qmd' : `c${k}.qmd`]);
    });
  });

  it('stops with no request when aborted between chapters', async () => {
    bigBook(20);
    const ac = new AbortController();
    const calls: number[] = [];
    setTimeout(() => ac.abort(), 0);
    const out = await render(
      'c1.qmd',
      'typst',
      {
        onProgress: async (i) => {
          calls.push(i);
          await new Promise((r) => setTimeout(r, 0));
        },
      },
      ac.signal,
    );
    expect(out.success).toBe(false);
    expect(out.request).toBeUndefined();
    expect(calls.length).toBeLessThan(21);
  });

  it('stops on abort with no onProgress, because the Rust hook yields on its own', async () => {
    bigBook(20);
    const ac = new AbortController();
    setTimeout(() => ac.abort(), 0);
    const out = await render('c1.qmd', 'typst', undefined, ac.signal);
    expect(out.success).toBe(false);
    expect(out.request).toBeUndefined();
  });

  it('fails the call with the error of a rejecting onProgress', async () => {
    bigBook(5);
    const out = await render('c1.qmd', 'typst', {
      onProgress: (i) => {
        if (i === 3) return Promise.reject(new Error('status pane is gone'));
      },
    });
    expect(out.success).toBe(false);
    expect(out.request).toBeUndefined();
    expect(out.error).toContain('status pane is gone');
  });

  it('rejects a malformed options object without rendering', async () => {
    smallBook();
    const out = await render('one.qmd', 'typst', { scope: 'sideways' as 'auto' });
    expect(out.success).toBe(false);
    expect(out.error).toContain('scope');
  });
});

describe('the resolver\'s book field (R9)', () => {
  const resolve = (rel: string) =>
    JSON.parse(wasm.resolve_pandoc_formats(`${ROOT}/${rel}`)) as {
      success: boolean;
      book?: { chapter: boolean; chapters: string[] } | null;
    };

  it('lists the chapters as sidecar keys in book order and flags the active page', async () => {
    parityBook();
    const r = resolve('sub/two.qmd');
    expect(r.success).toBe(true);
    expect(r.book).toEqual({
      chapter: true,
      chapters: [
        'index.qmd', 'one.qmd', 'sub/two.qmd', 'partpage.qmd', 'titleonly.qmd',
        'withh1.qmd', 'neither.qmd', 'textfirst.qmd', 'app.qmd',
      ],
    });
  });

  it('agrees with the export\'s auto scope for every file, and is null outside a book', async () => {
    parityBook();
    add('notes.qmd', '# Notes\n\nOutside.\n');
    const files = [...resolve('one.qmd').book!.chapters, 'notes.qmd'];
    for (const file of files) {
      const says = resolve(file).book!.chapter;
      const out = await render(file, 'typst');
      expect(out.stats.book!.scope === 'book', file).toBe(says);
    }
    wasm.vfs_clear();
    add('solo.qmd', '# Solo\n');
    expect(resolve('solo.qmd').book ?? null).toBeNull();
  });

  it('degrades a broken chapter list to "not a chapter" while the export fails with the book error', async () => {
    add('_quarto.yml', 'project:\n  type: book\nbook:\n  title: B\n  chapters:\n    - index.qmd\n    - missing.qmd\n');
    add('index.qmd', '# Preface\n');
    expect(resolve('index.qmd').book).toEqual({ chapter: false, chapters: [] });
    const out = await render('index.qmd', 'typst');
    expect(out.success).toBe(false);
    expect(JSON.stringify(out)).toContain('Q-5-35');
  });
});
