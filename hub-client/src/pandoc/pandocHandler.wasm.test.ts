/**
 * The worker handler's warm mode (pandoc-host H10b Task 1a), against real pandoc.wasm: `init.warm` creates
 * the `WarmSession` before `ready`, and results carry `RunStats` on success and on failure.
 */
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, describe, expect, it } from 'vitest';
import { createHandler, execute, type PandocRequest, type ShareTree, type WorkerResponse } from '@quarto/pandoc-host';
import { WASM_PATH, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { DOC_DIR, recordedDocs } from '../test-utils/recordingFixtures';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_binary_file: (path: string, content: Uint8Array) => string;
  vfs_clear: () => string;
  render_pandoc_request: (path: string, format: string, source_date_epoch?: number) => Promise<{ request?: unknown }>;
  get_pandoc_share_tree: () => ShareTree;
}

const here = path.dirname(fileURLToPath(import.meta.url));
const PKG_WASM = path.join(here, '../../../crates/wasm-quarto-hub-client/pkg/wasm_quarto_hub_client_bg.wasm');
const ready = pandocWasmAvailable() && existsSync(PKG_WASM);

describe('environment', () => {
  it('has pandoc.wasm and the built hub wasm (required in CI)', () => {
    if (!ready && process.env.CI) throw new Error(`handler tests need pandoc.wasm (${WASM_PATH}) and ${PKG_WASM}`);
    if (!ready) console.warn('SKIPPING the handler tests: pandoc.wasm or the built hub wasm is missing');
  });
});

describe.skipIf(!ready)('createHandler', () => {
  let module: WebAssembly.Module;
  let shareTree: ShareTree;
  let good: PandocRequest;
  let freshOutput: Uint8Array;

  beforeAll(async () => {
    module = await WebAssembly.compile(readFileSync(WASM_PATH));
    const wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
    await wasm.default(readFileSync(PKG_WASM));
    shareTree = wasm.get_pandoc_share_tree();
    const doc = recordedDocs().find((d) => d.name === 'callouts-typst')!;
    wasm.vfs_clear();
    for (const f of doc.files) wasm.vfs_add_binary_file(`${DOC_DIR}/${f.path}`, f.bytes);
    good = (await wasm.render_pandoc_request(`${DOC_DIR}/${doc.qmd}`, 'typst', 1_700_000_000)).request as PandocRequest;
    const fresh = await execute(good, shareTree, { module });
    if (!fresh.ok) throw new Error(fresh.stderr);
    freshOutput = fresh.output;
  });

  const harness = () => {
    const out: WorkerResponse[] = [];
    return { out, handle: createHandler((m) => void out.push(m)) };
  };
  const brokenOf = (req: PandocRequest): PandocRequest => ({
    ...req,
    files: req.files.map((f) => (f.path === req.json_path ? { ...f, bytes: new TextEncoder().encode('{not json') } : f)),
  });
  const results = (out: WorkerResponse[]) => out.filter((m): m is Extract<WorkerResponse, { type: 'result' }> => m.type === 'result');

  it('warm init creates the instance before ready; runs are served warm, byte-equal to fresh', async () => {
    const { out, handle } = harness();
    await handle({ type: 'init', module, warm: true });
    expect(out.map((m) => m.type)).toEqual(['ready']);
    await handle({ type: 'run', id: 1, request: good, shareTree });
    await handle({ type: 'run', id: 2, request: good, shareTree });
    const [a, b] = results(out);
    for (const r of [a, b]) {
      if (!r.result.ok) throw new Error(r.result.stderr);
      expect(r.result.stats.warm).toBe(true);
      expect(Buffer.from(r.result.output).equals(Buffer.from(freshOutput))).toBe(true);
    }
    expect([a.id, b.id]).toEqual([1, 2]);
    expect(out.filter((m) => m.type === 'progress').length).toBeGreaterThan(0);
  });

  it('a failed warm render carries stats', async () => {
    const { out, handle } = harness();
    await handle({ type: 'init', module, warm: true });
    await handle({ type: 'run', id: 1, request: brokenOf(good), shareTree });
    const r = results(out)[0].result;
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.kind).toBe('pandoc-exit');
      expect(r.stats?.warm).toBe(true);
      expect(r.stats?.memoryBytes).toBeGreaterThan(0);
    }
  });

  it('without warm the handler runs the fresh path: no warm stat', async () => {
    const { out, handle } = harness();
    await handle({ type: 'init', module });
    await handle({ type: 'run', id: 1, request: good, shareTree });
    const r = results(out)[0].result;
    if (!r.ok) throw new Error(r.stderr);
    expect(r.stats.warm).toBeUndefined();
  });

  it('a fault is forwarded per run in warm mode and the next run is unaffected', async () => {
    const { out, handle } = harness();
    await handle({ type: 'init', module, warm: true });
    await handle({ type: 'run', id: 1, request: good, shareTree, fault: { kind: 'crash' } });
    await handle({ type: 'run', id: 2, request: good, shareTree });
    const [a, b] = results(out);
    expect(a.result.ok).toBe(false);
    if (!a.result.ok) expect(a.result.kind).toBe('crash');
    expect(b.result.ok).toBe(true);
  });
});
