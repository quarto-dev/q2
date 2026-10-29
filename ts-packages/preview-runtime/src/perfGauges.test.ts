/**
 * Enabled/disabled parity for the render-path gauges (bd-k8ptcrd9).
 * With recording ON, the instrumented wrappers record their gauges; with
 * it OFF they return identical results and record nothing. This guards
 * the early-return fast paths against two failure modes: a fast path
 * that diverges from the measured path's results, and a fast path that
 * still pays for the gauges.
 */

import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest';
import { _setWasmModuleForTest, vfsAddFile, renderQmd } from './wasmRenderer';
import { perfDump, perfReset, setPerfOverrideForTests } from './perfCounters';

const wasmStub = {
  vfs_add_file: vi.fn(() => JSON.stringify({ success: true })),
  render_qmd: vi.fn(async (_path: string) => JSON.stringify({ success: true, html: '<p>x</p>' })),
};

beforeAll(() => {
  _setWasmModuleForTest(wasmStub);
});

afterEach(() => {
  setPerfOverrideForTests(null);
  perfReset();
  vi.restoreAllMocks();
});

function dumpLines(): string[] {
  const log = vi.spyOn(console, 'log').mockImplementation(() => {});
  perfDump();
  return log.mock.calls.map((c) => c[0] as string);
}

describe('gauge fast paths', () => {
  it('records gauges when enabled', async () => {
    setPerfOverrideForTests(true);
    vfsAddFile('a.qmd', 'hello');
    await renderQmd('a.qmd');
    const lines = dumpLines().join('\n');
    expect(lines).toContain('perf.vfs.write runs=1 total_ms=');
    expect(lines).toContain('bytes=5');
    expect(lines).toContain('perf.render.started runs=1');
    expect(lines).toContain('perf.render.completed runs=1');
    expect(lines).toContain('perf.wasm-envelope.parse runs=2');
  });

  it('returns identical results and records nothing when disabled', async () => {
    setPerfOverrideForTests(false);
    const write = vfsAddFile('a.qmd', 'hello');
    const render = await renderQmd('a.qmd');
    expect(write).toEqual({ success: true });
    expect(render).toEqual({ success: true, html: '<p>x</p>' });
    expect(dumpLines()).toEqual([]);
  });
});
