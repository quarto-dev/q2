/**
 * `WarmSession` (pandoc-host H10a Task 5): poison detection and replacement, the memory retire flag, the
 * consecutive-error recycle and per-run fault instances, against real pandoc.wasm. A fake worker cannot see
 * poisoning, which is why these are wasm tests (`test:wasm`), not `ts-packages/pandoc-host` unit tests.
 */
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, describe, expect, it } from 'vitest';
import { WarmPandoc, WarmSession, execute, type ExecuteResult, type PandocRequest, type ShareTree, type WarmInstance } from '@quarto/pandoc-host';
import { WASM_PATH, pandocWasmAvailable } from '../test-utils/pandocRecordings';
import { DOC_DIR, recordedDocs } from '../test-utils/recordingFixtures';

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_binary_file: (path: string, content: Uint8Array) => string;
  vfs_clear: () => string;
  render_pandoc_request: (path: string, format: string, source_date_epoch?: number) => Promise<{ error?: string; diagnostics: unknown[]; request?: unknown }>;
  get_pandoc_share_tree: () => ShareTree;
}

const here = path.dirname(fileURLToPath(import.meta.url));
const PKG_WASM = path.join(here, '../../../crates/wasm-quarto-hub-client/pkg/wasm_quarto_hub_client_bg.wasm');
const CI = !!process.env.CI;
const ready = pandocWasmAvailable() && existsSync(PKG_WASM);
const enc = new TextEncoder();

describe('environment', () => {
  it('has pandoc.wasm and the built hub wasm (required in CI)', () => {
    if (!ready && CI) throw new Error(`warm session tests need pandoc.wasm (${WASM_PATH}) and ${PKG_WASM}`);
    if (!ready) console.warn('SKIPPING the warm session tests: pandoc.wasm or the built hub wasm is missing');
  });
});

describe.skipIf(!ready)('WarmSession', () => {
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
    const envelope = await wasm.render_pandoc_request(`${DOC_DIR}/${doc.qmd}`, 'typst', 1_700_000_000);
    good = envelope.request as PandocRequest;
    const fresh = await execute(good, shareTree, { module });
    if (!fresh.ok) throw new Error(fresh.stderr);
    freshOutput = fresh.output;
  });

  const withInput = (req: PandocRequest, bytes: Uint8Array): PandocRequest => ({ ...req, files: req.files.map((f) => (f.path === req.json_path ? { ...f, bytes } : f)) });
  /** The callouts document with its blocks repeated: big enough to exhaust a 100 MB heap. */
  const bigRequest = (): PandocRequest => {
    const doc = JSON.parse(new TextDecoder().decode(good.files.find((f) => f.path === good.json_path)!.bytes));
    doc.blocks = Array.from({ length: 400 }, () => doc.blocks).flat();
    return withInput(good, enc.encode(JSON.stringify(doc)));
  };
  const broken = () => withInput(good, enc.encode('{not json'));
  const expectGood = (r: ExecuteResult) => {
    if (!r.ok) throw new Error(`${r.kind}: ${r.stderr}`);
    expect(Buffer.from(r.output).equals(Buffer.from(freshOutput))).toBe(true);
  };
  /** A request whose filter is `lua` over a one-paragraph document (the probe shape of warmPandoc.wasm.test.ts). */
  function filterRequest(lua: string): PandocRequest {
    const dir = good.json_path.slice(0, good.json_path.lastIndexOf('/'));
    const doc = '{"pandoc-api-version":[1,23,1],"meta":{},"blocks":[{"t":"Para","c":[{"t":"Str","c":"x"}]}]}';
    return {
      ...good,
      argv: ['pandoc', '-f', 'json', '-t', 'plain', '--data-dir', `${good.share_tree_path}/pandoc/datadir`, '-L', `${dir}/f.lua`, '-o', good.output_path, good.json_path],
      files: [...good.files.filter((f) => f.path !== good.json_path), { path: good.json_path, bytes: enc.encode(doc) }, { path: `${dir}/f.lua`, bytes: enc.encode(lua) }],
    };
  }

  /** Wraps a real instance and overrides what it reports about its health. */
  const forcing = (signals: Partial<WarmInstance['signals']>, canary?: boolean) => async (m: WebAssembly.Module, o: Parameters<typeof WarmPandoc.create>[1]): Promise<WarmInstance> => {
    const real = await WarmPandoc.create(m, o);
    let forced = false;
    return {
      run: async (req, st, hooks) => {
        const r = await real.run(req, st, hooks);
        forced = true;
        return r;
      },
      canary: () => canary ?? real.canary(),
      memoryBytes: () => real.memoryBytes(),
      get signals() {
        return forced ? { trapped: false, thrownExit: null, oomMessage: false, ...signals } : real.signals;
      },
    };
  };

  it('serves renders from one instance, byte-equal to fresh', async () => {
    const session = await WarmSession.create(module);
    expectGood(await session.run(good, shareTree));
    expectGood(await session.run(good, shareTree));
    expect(session.created).toBe(1);
  });

  it('os.exit(3) in a filter is a pandoc-exit and does not poison: the canary passes', async () => {
    const session = await WarmSession.create(module);
    const r = await session.run(filterRequest('os.exit(3)\n'), shareTree);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r).toMatchObject({ kind: 'pandoc-exit', status: 3 });
    expect(session.created).toBe(1);
    expectGood(await session.run(good, shareTree));
    expect(session.created).toBe(1);
  });

  it('heap exhaustion poisons the instance; it is replaced and the next render is byte-equal to fresh', async () => {
    const session = await WarmSession.create(module, { rts: ['+RTS', '-M100m', '-RTS'] });
    expectGood(await session.run(good, shareTree));
    const r = await session.run(bigRequest(), shareTree);
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.kind).toBe('oom');
    expect(r.stats).toBeDefined();
    expect(session.created).toBe(2); // the replacement started at once
    expectGood(await session.run(good, shareTree));
    expect(session.created).toBe(2);
  });

  it('a crash fault runs on a dedicated instance and leaves the session instance serving', async () => {
    const session = await WarmSession.create(module);
    const r = await session.run(good, shareTree, { fault: { kind: 'crash', message: 'injected' } });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.kind).toBe('crash');
    expectGood(await session.run(good, shareTree));
    expect(session.created).toBe(2); // the first instance plus the dedicated one; the first was not replaced
  });

  it('an oom fault is an RTS option of a dedicated instance', async () => {
    const session = await WarmSession.create(module);
    const r = await session.run(good, shareTree, { fault: { kind: 'oom', limit: '5m' } });
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.kind).toBe('oom');
    expectGood(await session.run(good, shareTree));
  });

  it('after errorRecycleN consecutive errored renders the instance is recreated; a success resets the count', async () => {
    const session = await WarmSession.create(module, { errorRecycleN: 3 });
    await session.run(broken(), shareTree);
    await session.run(broken(), shareTree);
    expectGood(await session.run(good, shareTree));
    await session.run(broken(), shareTree);
    await session.run(broken(), shareTree);
    expect(session.created).toBe(1);
    await session.run(broken(), shareTree);
    expect(session.created).toBe(2);
    expectGood(await session.run(good, shareTree));
  });

  it('a render above retireBytes reports retire, failures included', async () => {
    const session = await WarmSession.create(module, { retireBytes: 1 });
    const ok = await session.run(good, shareTree);
    expectGood(ok);
    expect(ok.stats?.retire).toBe(true);
    const bad = await session.run(broken(), shareTree);
    expect(bad.ok).toBe(false);
    expect(bad.stats?.retire).toBe(true);
    const normal = await WarmSession.create(module);
    expect((await normal.run(good, shareTree)).stats?.retire).toBeUndefined();
  });

  it('an out-of-memory message on fd 2 poisons the instance', async () => {
    const session = await WarmSession.create(module, { createInstance: forcing({ oomMessage: true }) });
    await session.run(good, shareTree);
    expect(session.created).toBe(2);
  });

  it('a thrown exit poisons the instance only when the canary then fails', async () => {
    const poisoned = await WarmSession.create(module, { createInstance: forcing({ thrownExit: 1 }, false) });
    await poisoned.run(good, shareTree);
    expect(poisoned.created).toBe(2);
    const healthy = await WarmSession.create(module, { createInstance: forcing({ thrownExit: 3 }, true) });
    await healthy.run(good, shareTree);
    expect(healthy.created).toBe(1);
  });

  it('a wasm trap poisons the instance', async () => {
    const session = await WarmSession.create(module, { createInstance: forcing({ trapped: true }) });
    await session.run(good, shareTree);
    expect(session.created).toBe(2);
  });
});
