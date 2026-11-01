/**
 * WASM test for engine-record payloads (EXR-13, reader half).
 *
 * The capture wire may now carry an engine execution record (an nbformat notebook with
 * `quarto_` metadata) where it carried an `EngineCapture[]`. The TS layer passes the bytes
 * through untouched as `capture_gz_json`; Rust decides the shape from the payload.
 *
 * Presenting a record is EXR-14. Until then both WASM entry points must accept a record
 * without erroring and render its code cells as source, so a record reaching an older
 * renderer than its writer degrades to source instead of an error overlay.
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

function expectSourceOnly(json: string): void {
  const result = JSON.parse(json) as RenderResponse;
  expect(result.success, `Render failed: ${result.error}`).toBe(true);
  expect(result.ast_json).toBeTruthy();
  expect(
    result.ast_json!.includes('print(1)'),
    'the code cell renders as source',
  ).toBe(true);
  expect(
    result.ast_json!.includes(MARKER),
    'a record is not presented until EXR-14: its output must not appear',
  ).toBe(false);
}

describe('engine-record payloads (EXR-13)', () => {
  it('render_page_for_preview accepts a record and renders code cells as source', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);
    expectSourceOnly(await wasm.render_page_for_preview('/project/doc.qmd', undefined, recordBytes));
  });

  it('render_page_in_project_with_attribution accepts a record and renders code cells as source', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);
    expectSourceOnly(
      await wasm.render_page_in_project_with_attribution(
        '/project/doc.qmd',
        undefined,
        undefined,
        recordBytes,
      ),
    );
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
