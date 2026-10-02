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

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFileSync } from 'node:fs';
import { readFile } from 'fs/promises';
import { unzipSync } from 'fflate';
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
  stats: { unexecuted_cells: number };
  request?: Record<string, unknown> & {
    files: RequestFile[];
    resource_refs: RequestFile[];
    share_tree_version: string;
    argv: string[];
    job_id: string;
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
    expect(out.stats).toEqual({ unexecuted_cells: 0 });
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

  it('refuses a format pandoc.wasm cannot produce and an unavailable one', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '# Hi\n');
    const pdf = await wasm.render_pandoc_request('/project/doc.qmd', 'pdf');
    expect(pdf.success).toBe(false);
    expect(pdf.error).toMatch(/cannot be rendered by pandoc/);
    const typst = await wasm.render_pandoc_request('/project/doc.qmd', 'typst');
    expect(typst.success).toBe(false);
    expect(typst.error).toMatch(/not available in the browser yet/);
  });

  it('says projects are not supported yet for a document inside a _quarto.yml project', async () => {
    wasm.vfs_add_file('/p/_quarto.yml', 'project:\n  type: default\n');
    wasm.vfs_add_file('/p/doc.qmd', '# Hi\n');
    const out = await wasm.render_pandoc_request('/p/doc.qmd', 'docx');
    expect(out.success).toBe(false);
    expect(out.request).toBeUndefined();
    expect(out.error).toMatch(/projects not yet supported/);
  });

  it('echoes typst_available_fonts', async () => {
    wasm.vfs_add_file('/project/doc.qmd', '# Hi\n');
    const out = await wasm.render_pandoc_request('/project/doc.qmd', 'docx', undefined, undefined, ['Inter']);
    expect(out.request!.typst_available_fonts).toEqual(['Inter']);
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
  const formats = () => JSON.parse(wasm.get_pandoc_formats()).formats as { key: string; available: boolean }[];
  const resolve = (path: string) =>
    JSON.parse(wasm.resolve_pandoc_formats(path)) as {
      success: boolean;
      error?: string;
      source?: string;
      formats?: { key: string; class: string }[];
    };

  it('lists the downloadable formats in menu order', () => {
    expect(formats().map((f) => f.key)).toEqual(['docx', 'pptx', 'epub', 'typst']);
    expect(formats().find((f) => f.key === 'typst')!.available).toBe(false);
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

  it('reports a missing file', () => {
    expect(resolve('/d/missing.qmd').success).toBe(false);
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
});
