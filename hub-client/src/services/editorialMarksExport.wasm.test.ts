/**
 * WASM test for the editorial-marks export (document import P6, I23), on the browser's
 * "Download as" path: a docx request built by `render_pandoc_request` carries the converted
 * span classes in the JSON pandoc.wasm will read, so tracked changes, highlights and comments
 * reach Word. No pandoc run is needed, only the Rust wasm; deliberately not gated on
 * `pandocWasmAvailable()`, so a missing pandoc asset cannot skip it silently.
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

interface RequestFile {
  path: string;
  bytes: Uint8Array;
}

interface Envelope {
  success: boolean;
  error?: string;
  request?: { files: RequestFile[] };
}

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_set_runtime_metadata: (yaml: string) => string;
  render_pandoc_request: (path: string, format: string, source_date_epoch?: number) => Promise<Envelope>;
}

interface JsonInline {
  t: string;
  c?: unknown;
}

let wasm: WasmModule;

beforeAll(async () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const wasmBytes = await readFile(join(here, '../../wasm-quarto-hub-client', 'wasm_quarto_hub_client_bg.wasm'));
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
});

beforeEach(() => {
  wasm.vfs_clear();
  wasm.vfs_set_runtime_metadata('');
});

const SDE = 1_700_000_000;

const DOC = [
  '---',
  'title: Marks',
  '---',
  '',
  'Added [++ new words]{author="Ann" date="2026-09-01T10:00:00Z"} and [-- old]{author="Ann"} and [!! marked].',
  '',
  'A [commented range[>> Please reword.]{author="Bob"}[>> Done.]{author="Cy"}] here.',
  '',
  '::: ++ {author="Dee"}',
  '',
  'A block insertion.',
  '',
  ':::',
  '',
].join('\n');

/** The pandoc JSON the request carries as its input file. */
async function inputJson(format: string): Promise<{ text: string; json: { blocks: { t: string; c: unknown }[] } }> {
  wasm.vfs_add_file('/p/doc.qmd', DOC);
  const out = await wasm.render_pandoc_request('/p/doc.qmd', format, SDE);
  expect(out.error).toBeUndefined();
  expect(out.success).toBe(true);
  const file = out.request!.files.find((f) => f.path.endsWith('/pandoc-input.json'));
  expect(file).toBeDefined();
  const text = new TextDecoder().decode(file!.bytes);
  return { text, json: JSON.parse(text) };
}

/** Every span's first class and attributes, anywhere in the JSON. */
function spans(value: unknown, found: { cls: string; attrs: Record<string, string> }[] = []) {
  if (Array.isArray(value)) {
    value.forEach((v) => spans(v, found));
  } else if (value && typeof value === 'object') {
    const node = value as JsonInline;
    if (node.t === 'Span' && Array.isArray(node.c)) {
      const [attr] = node.c as [[string, string[], [string, string][]], unknown];
      found.push({ cls: attr[1][0] ?? '', attrs: Object.fromEntries(attr[2]) });
    }
    Object.values(value).forEach((v) => spans(v, found));
  }
  return found;
}

describe('editorial marks in a docx download request', () => {
  it('carries pandoc’s own classes, not the quarto-* marks', async () => {
    const { text, json } = await inputJson('docx');
    const found = spans(json.blocks);
    const classes = found.map((s) => s.cls);
    expect(classes).toEqual(
      expect.arrayContaining(['insertion', 'deletion', 'mark', 'comment-start', 'comment-end']),
    );
    expect(text).not.toContain('quarto-insert');
    expect(text).not.toContain('quarto-delete');
    expect(text).not.toContain('quarto-highlight');
    expect(text).not.toContain('quarto-edit-comment');
  });

  it('numbers comments from 0 in document order, gives every one an author, and keeps replies on the range', async () => {
    const { json } = await inputJson('docx');
    const comments = spans(json.blocks).filter((s) => s.cls === 'comment-start');
    expect(comments.map((c) => c.attrs.id)).toEqual(['0', '1']);
    expect(comments.map((c) => c.attrs.author)).toEqual(['Bob', 'Cy']);
    const ends = spans(json.blocks).filter((s) => s.cls === 'comment-end');
    expect(ends.map((e) => e.attrs.id)).toEqual(['0', '1']);
  });

  it('keeps the explicit author and date of a tracked change, and wraps a block insertion', async () => {
    const { json } = await inputJson('docx');
    const insertions = spans(json.blocks).filter((s) => s.cls === 'insertion');
    expect(insertions).toContainEqual({ cls: 'insertion', attrs: { author: 'Ann', date: '2026-09-01T10:00:00Z' } });
    // The block insertion has no Div left to ignore: its paragraph is wrapped, with its author.
    expect(insertions).toContainEqual({ cls: 'insertion', attrs: { author: 'Dee' } });
  });
});

describe('editorial marks in a pptx download request', () => {
  it('become raw openxml runs', async () => {
    const { text } = await inputJson('pptx');
    expect(text).toContain('openxml');
    expect(text).toContain('u=\\"sng\\"');
    expect(text).not.toContain('"quarto-insert"');
  });
});
