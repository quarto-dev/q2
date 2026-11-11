/**
 * WASM test: capture splicing in the DEFAULT `format: html` render (bd-uy4uygha).
 *
 * The sibling `captureSplice.wasm.test.ts` covers the `format: q2-preview` AST
 * path. hub-client's *default* preview for a plain document (and every website
 * page) renders `format: html` via a different WASM branch, which historically
 * ignored captures — so a document executed by a connected `q2 provide-hub`
 * showed source instead of output. This test pins that the HTML branch now
 * splices the recorded engine output into the rendered `html`.
 *
 * Strategy mirrors the q2-preview test but with `format: html` and asserts on
 * the `html` field (not `ast_json`). WASM has no native engines, so the `{r}`
 * cell takes the markdown fallback; the splice (which runs before engine
 * execution) is the only thing that can produce the marker.
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
  html?: string;
  ast_json?: string;
}

const MARKER = 'SPLICEDHTML42';

// A single-file `format: html` doc with one knitr cell.
const DOC = ['---', 'format: html', 'engine: knitr', '---', '', '```{r}', '1 + 1', '```', ''].join(
  '\n',
);

function captureBytes(engine = 'knitr', lang = 'r'): Uint8Array {
  const captures = [
    {
      engine_name: engine,
      input_qmd: `\`\`\`{${lang}}\n1 + 1\n\`\`\`\n`,
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

describe('format: html capture splicing (bd-uy4uygha)', () => {
  it('baseline: without a capture, the html has no engine-output marker', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);

    const json = await wasm.render_page_in_project_with_attribution(
      '/project/doc.qmd',
      undefined,
      undefined,
      undefined,
    );
    const result = JSON.parse(json) as RenderResponse;

    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html, 'html branch must return html').toBeTruthy();
    expect(
      result.html!.includes(MARKER),
      'the marker must be capture-only — absent in a no-capture render',
    ).toBe(false);
  });

  it('with a capture, the recorded engine output is spliced into the rendered html', async () => {
    wasm.vfs_add_file('/project/doc.qmd', DOC);

    const json = await wasm.render_page_in_project_with_attribution(
      '/project/doc.qmd',
      undefined,
      undefined,
      captureBytes(),
    );
    const result = JSON.parse(json) as RenderResponse;

    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html).toBeTruthy();
    expect(
      result.html!.includes(MARKER),
      'the capture output marker must appear in the html when a capture is threaded through the html branch',
    ).toBe(true);
  });
});

// OPR-9g: the engine stage now presents from a record by default, and in the browser it
// coalesces any record a replayed capture carries (before the flip it did not). A capture that
// was stored before the flip must still render. A Jupyter capture from then has markdown and no
// record, so it takes the replay engine's markdown path.
describe('captures stored before the present-from-record flip (OPR-9g)', () => {
  const JUPYTER_DOC = [
    '---',
    'format: html',
    'engine: jupyter',
    '---',
    '',
    '```{python}',
    '1 + 1',
    '```',
    '',
  ].join('\n');

  it('a stored Jupyter capture (markdown, no record) still renders and splices its output', async () => {
    wasm.vfs_add_file('/project/doc.qmd', JUPYTER_DOC);

    const json = await wasm.render_page_in_project_with_attribution(
      '/project/doc.qmd',
      undefined,
      undefined,
      captureBytes('jupyter', 'python'),
    );
    const result = JSON.parse(json) as RenderResponse;

    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html!.includes(MARKER), 'the stored output is spliced in').toBe(true);
  });
});
