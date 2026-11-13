/**
 * WASM test for engine-record payloads (EXR-13, reader half).
 *
 * The capture wire may now carry an engine execution record (an nbformat notebook with
 * `quarto_` metadata) where it carried an `EngineCapture[]`. The TS layer passes the bytes
 * through untouched as `capture_gz_json`; Rust decides the shape from the payload.
 *
 * EXR-14 presents a record: both WASM entry points splice the recorded output of every
 * cell whose unit hash still matches the document, and leave an edited cell as source.
 *
 * The record is the checked-in fixture `crates/quarto-core/tests/fixtures/records/basic.json`
 * (a Rust test keeps it valid). Its stdout is rewritten to a marker that appears only in the
 * recorded output, never in the source.
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { gzipSync } from 'zlib';
import { setVfsCallbacks } from '/src/wasm-js-bridge/sass.js';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_read_file: (path: string) => string;
  render_page_for_preview: (
    path: string,
    user_grammars?: unknown,
    capture_gz_json?: Uint8Array,
  ) => Promise<string>;
  render_page_in_project_with_attribution: (
    path: string,
    user_grammars?: unknown,
    attribution_json?: string,
    capture_gz_json?: Uint8Array,
  ) => Promise<string>;
}

interface RenderResponse {
  success: boolean;
  error?: string;
  ast_json?: string;
}

const MARKER = 'RECORDEDOUTPUT42';

const DOC = ['---', 'format: q2-preview', '---', '', '```{python}', 'print(1)', '```', ''].join(
  '\n',
);

let wasm: WasmModule;
let recordBytes: Uint8Array;

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmBytes = await readFile(join(wasmDir, 'wasm_quarto_hub_client_bg.wasm'));

  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);

  setVfsCallbacks(
    (path: string): string | null => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path)) as {
          success: boolean;
          content?: string;
        };
        return result.success && result.content !== undefined ? result.content : null;
      } catch {
        return null;
      }
    },
    (path: string): boolean => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path)) as {
          success: boolean;
          content?: string;
        };
        return result.success && result.content !== undefined;
      } catch {
        return false;
      }
    },
  );

  const fixture = join(
    __dirname,
    '../../../crates/quarto-core/tests/fixtures/records/basic.json',
  );
  const record = JSON.parse(await readFile(fixture, 'utf8')) as {
    cells: { outputs: { text: string[] }[] }[];
  };
  record.cells[0].outputs[0].text = [`${MARKER}\n`];
  recordBytes = new Uint8Array(gzipSync(Buffer.from(JSON.stringify(record))));
});

beforeEach(() => {
  wasm.vfs_clear();
});

// A display block shows a `{python}` cell with the same text *before* the real cell. The
// nested-cell mask keeps it from being a unit; if it were claimed it would take the record's
// output (the first occurrence of that hash) and the real cell would fall back to source.
const DISPLAY_DOC = [
  '---',
  'format: q2-preview',
  '---',
  '',
  '````markdown',
  '```{python}',
  'print(1)',
  '```',
  '````',
  '',
  '```{python}',
  'print(1)',
  '```',
  '',
].join('\n');

const EDITED_DOC = DOC.replace('print(1)', 'print(2)');

function parse(json: string): RenderResponse {
  const result = JSON.parse(json) as RenderResponse;
  expect(result.success, `Render failed: ${result.error}`).toBe(true);
  expect(result.ast_json).toBeTruthy();
  return result;
}

function expectPresented(json: string): void {
  const ast = parse(json).ast_json!;
  expect(ast.includes(MARKER), 'the recorded output is spliced in').toBe(true);
  expect(ast.includes('print(1)'), 'the source is still shown (echo defaults on)').toBe(true);
}

function expectSourceOnly(json: string): void {
  const ast = parse(json).ast_json!;
  expect(ast.includes('print(2)'), 'the edited cell renders as source').toBe(true);
  expect(ast.includes(MARKER), 'an edited cell has no recorded output').toBe(false);
}

describe('engine-record payloads (EXR-13, EXR-14)', () => {
  it('render_page_for_preview presents the recorded output of a matching cell', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);
    expectPresented(await wasm.render_page_for_preview('/project/doc.qmd', undefined, recordBytes));
  });

  it('render_page_for_preview leaves an edited cell as source', async () => {
    wasm.vfs_add_file('/project/doc.qmd', EDITED_DOC);
    expectSourceOnly(await wasm.render_page_for_preview('/project/doc.qmd', undefined, recordBytes));
  });

  it('render_page_in_project_with_attribution presents the recorded output', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);
    expectPresented(
      await wasm.render_page_in_project_with_attribution(
        '/project/doc.qmd',
        undefined,
        undefined,
        recordBytes,
      ),
    );
  });

  it('render_page_in_project_with_attribution leaves an edited cell as source', async () => {
    wasm.vfs_add_file('/project/doc.qmd', EDITED_DOC);
    expectSourceOnly(
      await wasm.render_page_in_project_with_attribution(
        '/project/doc.qmd',
        undefined,
        undefined,
        recordBytes,
      ),
    );
  });

  it('a cell shown inside a display block is not a unit: the real cell takes the output', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DISPLAY_DOC);
    const ast = parse(
      await wasm.render_page_for_preview('/project/doc.qmd', undefined, recordBytes),
    ).ast_json!;
    expect(ast.split(MARKER).length - 1, 'one recorded output').toBe(1);
    expect(
      ast.indexOf(MARKER),
      'the output follows the real cell, after the display block',
    ).toBeGreaterThan(ast.lastIndexOf('print(1)'));
  });

  // OPR-9g: `format: html` runs the engine stage, which now coalesces a replayed record in the
  // browser. A record recorded before the flip must still render there.
  it('a record recorded before the flip still renders through the html branch', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC.replace('format: q2-preview', 'format: html'));
    const result = JSON.parse(
      await wasm.render_page_in_project_with_attribution(
        '/project/doc.qmd',
        undefined,
        undefined,
        recordBytes,
      ),
    ) as RenderResponse & { html?: string };
    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html!.includes(MARKER), 'the recorded output is presented').toBe(true);
  });

  it('an unrecognized payload still fails with an error naming both shapes', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);
    const junk = new Uint8Array(gzipSync(Buffer.from('{"hello":1}')));
    const result = JSON.parse(
      await wasm.render_page_in_project_with_attribution(
        '/project/doc.qmd',
        undefined,
        undefined,
        junk,
      ),
    ) as RenderResponse;
    expect(result.success).toBe(false);
    expect(result.error).toContain('nbformat');
    expect(result.error).toContain('engine_name');
  });
});
