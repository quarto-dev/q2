/**
 * The pandoc.wasm parity net in Node (pandoc-host H3): each P7 docx golden fixture goes
 * through the real chain (the Rust `render_pandoc_request` in the built wasm, from the VFS,
 * then the host core on the pinned pandoc.wasm) and R0's extractor CLI compares the result
 * with the Q1 golden. The comparator is Rust-only, so the outputs are written to disk
 * (`Q2_PARITY_OUT`, default a temp dir) and `quarto-output-extract extract` reads them.
 *
 * Needs: the built wasm pkg (`npm run build:wasm`), pandoc.wasm (`node scripts/fetch-pandoc-wasm.mjs`)
 * and `cargo build -p quarto-output-extract`. Missing pieces skip locally and fail under CI.
 * The browser twin is e2e/pandoc-parity.harness.spec.ts.
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { execute, type PandocRequest, type ShareTree } from '@quarto/pandoc-host';
import { fileURLToPath } from 'node:url';
import { WASM_PATH, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { findExtractor, fixtureFiles, goldenFixtures, referenceFor, snapshotName } from '../test-utils/goldenFixtures';

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
const haveWasm = pandocWasmAvailable();
const ready = haveWasm && !!extractor;

describe('environment', () => {
  it('has pandoc.wasm and the extractor (required in CI)', () => {
    if (!ready && CI) throw new Error(`parity net inputs missing (pandoc.wasm at ${WASM_PATH}: ${haveWasm}; extractor: ${extractor})`);
    if (!ready) console.warn('SKIPPING the wasm parity net: pandoc.wasm or quarto-output-extract missing');
  });
});

describe.skipIf(!ready)('docx golden fixtures: Rust request -> pandoc.wasm == the Q1 golden', () => {
  let module: WebAssembly.Module;
  let shareTree: ShareTree;
  let wasm: WasmModule;
  const outDir = process.env.Q2_PARITY_OUT ?? mkdtempSync(path.join(os.tmpdir(), 'q2-parity-'));
  afterAll(() => {
    if (!process.env.Q2_PARITY_OUT) rmSync(outDir, { recursive: true, force: true });
  });

  beforeAll(async () => {
    mkdirSync(outDir, { recursive: true });
    module = await WebAssembly.compile(readFileSync(WASM_PATH));
    // Not `initWasm()`: it fetches the wasm by URL, which Node cannot.
    const here = path.dirname(fileURLToPath(import.meta.url));
    const bytes = readFileSync(path.join(here, '../../../crates/wasm-quarto-hub-client/pkg/wasm_quarto_hub_client_bg.wasm'));
    wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
    await wasm.default(bytes);
    shareTree = wasm.get_pandoc_share_tree();
  });

  for (const fixture of goldenFixtures()) {
    it(fixture.qmd, async () => {
      wasm.vfs_clear();
      for (const f of fixtureFiles(fixture)) wasm.vfs_add_binary_file(`/project/${f.path}`, f.bytes);
      const envelope = await wasm.render_pandoc_request(`/project/${fixture.qmd}`, 'docx', SDE);
      expect(envelope.error, JSON.stringify(envelope.diagnostics)).toBeUndefined();
      const request = envelope.request as unknown as PandocRequest;
      expect(request, JSON.stringify(envelope.diagnostics)).toBeDefined();

      const result = await execute(request, shareTree, { module });
      if (!result.ok) throw new Error(`${fixture.qmd}: ${result.kind} status=${result.status}\n${result.stderr}`);

      const out = path.join(outDir, `${snapshotName(fixture.qmd, 'docx')}.docx`);
      writeFileSync(out, result.output);
      const actual = execFileSync(extractor!, ['extract', out], { maxBuffer: 1 << 28 }).toString('utf8').trimEnd();
      const reference = referenceFor(fixture);
      expect(actual, `differs from ${reference.source} (output kept at ${out})`).toBe(reference.text);
    });
  }
});
