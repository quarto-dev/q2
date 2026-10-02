/**
 * The pandoc.wasm parity net for typst, pptx and epub (pandoc-host H3, formats half): each of
 * R0's recorded documents goes through the real chain (Rust `render_pandoc_request` in the
 * built wasm, then the host core on pandoc.wasm) and R0's extractor CLI compares the result
 * with the recording's native reference output. See src/test-utils/recordingFixtures.ts.
 * The P7 docx goldens are goldenParity.wasm.test.ts; the browser twin is
 * e2e/pandoc-formats.harness.spec.ts.
 *
 * Needs the built wasm pkg, pandoc.wasm and `cargo build -p quarto-output-extract`
 * (skips locally when missing, fails under CI).
 */
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { execute, type PandocRequest, type ShareTree } from '@quarto/pandoc-host';
import { WASM_PATH, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { extractText, findExtractor } from '../test-utils/goldenFixtures';
import { DOC_DIR, recordedDocs, referenceText } from '../test-utils/recordingFixtures';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_binary_file: (path: string, content: Uint8Array) => string;
  vfs_clear: () => string;
  render_pandoc_request: (path: string, format: string, source_date_epoch?: number) => Promise<{ error?: string; diagnostics: unknown[]; request?: unknown }>;
  get_pandoc_share_tree: () => ShareTree;
}

const CI = !!process.env.CI;
const SDE = 1_700_000_000;
const extractor = findExtractor();
const ready = pandocWasmAvailable() && !!extractor;

describe('environment', () => {
  it('has pandoc.wasm and the extractor (required in CI)', () => {
    if (!ready && CI) throw new Error(`format parity inputs missing (pandoc.wasm at ${WASM_PATH}, extractor ${extractor})`);
    if (!ready) console.warn('SKIPPING the typst/pptx/epub parity net: pandoc.wasm or quarto-output-extract missing');
  });
});

describe.skipIf(!ready)('recorded documents: Rust request -> pandoc.wasm == native pandoc', () => {
  let module: WebAssembly.Module;
  let shareTree: ShareTree;
  let wasm: WasmModule;
  const outDir = process.env.Q2_PARITY_OUT ?? mkdtempSync(path.join(os.tmpdir(), 'q2-formats-'));
  afterAll(() => {
    if (!process.env.Q2_PARITY_OUT) rmSync(outDir, { recursive: true, force: true });
  });

  beforeAll(async () => {
    mkdirSync(outDir, { recursive: true });
    module = await WebAssembly.compile(readFileSync(WASM_PATH));
    const here = path.dirname(fileURLToPath(import.meta.url));
    const bytes = readFileSync(path.join(here, '../../../crates/wasm-quarto-hub-client/pkg/wasm_quarto_hub_client_bg.wasm'));
    wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
    await wasm.default(bytes);
    shareTree = wasm.get_pandoc_share_tree();
  });

  for (const doc of recordedDocs()) {
    it(doc.name, async () => {
      wasm.vfs_clear();
      for (const f of doc.files) wasm.vfs_add_binary_file(`${DOC_DIR}/${f.path}`, f.bytes);
      const envelope = await wasm.render_pandoc_request(`${DOC_DIR}/${doc.qmd}`, doc.format, SDE);
      expect(envelope.error, JSON.stringify(envelope.diagnostics)).toBeUndefined();
      const request = envelope.request as unknown as PandocRequest;
      expect(request, JSON.stringify(envelope.diagnostics)).toBeDefined();

      const result = await execute(request, shareTree, { module });
      if (!result.ok) throw new Error(`${doc.name}: ${result.kind} status=${result.status}\n${result.stderr}`);

      const out = path.join(outDir, `${doc.name}.${doc.outputExt}`);
      writeFileSync(out, result.output);
      const actual = extractText(extractor!, out);
      const expected = referenceText(doc, (f) => extractText(extractor!, f));
      expect(actual, `differs from ${doc.referencePath} (output kept at ${out})`).toBe(expected);
    });
  }
});
