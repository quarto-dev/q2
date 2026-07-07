/**
 * WASM test for capture-aware hub-client rendering (bd-sfet3264, Phase 1A).
 *
 * hub-client's main preview path calls `render_page_in_project_with_attribution`.
 * For the remote-execution-provider feature, that entry must be able to
 * consume a recorded engine capture (the same gzipped-JSON `EngineCapture[]`
 * wire format the capture binary doc holds) and splice the recorded engine
 * output into the rendered AST — exactly as `render_page_for_preview` already
 * does for the q2-preview SPA, but *without* losing attribution.
 *
 * The inner WASM helpers already accept both captures and attribution; this
 * test pins the outer entry's new 4th argument (`capture_gz_json`).
 *
 * Strategy: render a single-file q2-preview doc with one `{r}` cell, passing
 * a hand-built capture whose post-engine markdown contains a marker string
 * (`SPLICEDOUTPUT42`) that appears ONLY in the capture, never in the source.
 * If the marker shows up in the rendered AST, the splice fired through this
 * entry. A no-capture baseline confirms the marker is genuinely capture-only.
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

const MARKER = 'SPLICEDOUTPUT42';

// A single-file q2-preview doc with one knitr cell. `format: q2-preview`
// forces the preview pipeline branch (the only branch that runs the
// CaptureSpliceStage); `engine: knitr` marks the cell as executable.
const DOC = [
  '---',
  'format: q2-preview',
  'engine: knitr',
  '---',
  '',
  '```{r}',
  '1 + 1',
  '```',
  '',
].join('\n');

// One capture, knitr engine. `input_qmd` carries the same `{r}` cell so its
// content-hash matches the doc's cell; `result.markdown` is the post-engine
// markdown — a `.cell` wrapper whose stdout output is the marker.
function captureBytes(): Uint8Array {
  const captures = [
    {
      engine_name: 'knitr',
      input_qmd: '```{r}\n1 + 1\n```\n',
      result: {
        markdown: [
          '::: {.cell}',
          '```{.r .cell-code}',
          '1 + 1',
          '```',
          '',
          '::: {.cell-output .cell-output-stdout}',
          '```',
          MARKER,
          '```',
          '',
          ':::',
          '',
          ':::',
          '',
        ].join('\n'),
      },
    },
  ];
  return new Uint8Array(gzipSync(Buffer.from(JSON.stringify(captures))));
}

let wasm: WasmModule;

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);

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
});

beforeEach(() => {
  wasm.vfs_clear();
});

describe('render_page_in_project_with_attribution capture splicing (Phase 1A)', () => {
  it('baseline: without a capture, the source renders without the engine-output marker', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);

    const json = await wasm.render_page_in_project_with_attribution(
      '/project/doc.qmd',
      undefined,
      undefined,
      undefined,
    );
    const result = JSON.parse(json) as RenderResponse;

    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.ast_json).toBeTruthy();
    expect(
      result.ast_json!.includes(MARKER),
      'the marker must be capture-only — it must NOT appear in a no-capture render',
    ).toBe(false);
  });

  it('with a capture, the recorded engine output is spliced into the rendered AST', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);

    const json = await wasm.render_page_in_project_with_attribution(
      '/project/doc.qmd',
      undefined,
      undefined,
      captureBytes(),
    );
    const result = JSON.parse(json) as RenderResponse;

    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.ast_json).toBeTruthy();
    expect(
      result.ast_json!.includes(MARKER),
      'the capture output marker must appear in the AST when a capture is threaded through this entry',
    ).toBe(true);
  });

  it('captures and attribution coexist: a capture splices even when an attribution payload is also supplied', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);

    // `{}` is a valid no-op attribution payload (runs/identities both default
    // to empty). The point is that supplying attribution must NOT cause the
    // capture argument to be dropped — the inner renderer attaches both.
    const json = await wasm.render_page_in_project_with_attribution(
      '/project/doc.qmd',
      undefined,
      '{}',
      captureBytes(),
    );
    const result = JSON.parse(json) as RenderResponse;

    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(
      result.ast_json!.includes(MARKER),
      'capture must still splice when an attribution payload is also passed (both coexist)',
    ).toBe(true);
  });
});
