/**
 * Tests for the RLE (run-list) attribution producer.
 *
 * Covers patch application on runs directly, and cross-validates that
 * the runs path and the per-char path produce identical
 * `AttributionSource.queryByteRange` results for the same history.
 *
 * @vitest-environment node
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@automerge/automerge', async () => {
  const actual = await vi.importActual<typeof import('@automerge/automerge')>('@automerge/automerge');
  return { ...actual, diff: vi.fn() };
});
vi.mock('@automerge/automerge-repo', async () => {
  const actual = await vi.importActual<typeof import('@automerge/automerge-repo')>('@automerge/automerge-repo');
  return { ...actual, decodeHeads: vi.fn((heads: unknown) => heads) };
});

import { diff } from '@automerge/automerge';
import {
  buildAttributionMap,
  makeCharArraySource,
} from './attribution';
import {
  buildRunListAttribution,
  updateRunListAttribution,
  makeRunListSource,
  __internal,
  type AttributionRun,
  type RunListAttribution,
} from './attribution-runs';

const { runsInsert, runsDelete, applyPatchToRuns } = __internal;
const mockDiff = vi.mocked(diff);

// ---------------------------------------------------------------------------
// Mock-handle helpers (mirrored from attribution.test.ts)
// ---------------------------------------------------------------------------

interface MockPatch {
  action: 'splice' | 'del' | 'put';
  path: [string, number] | [string];
  value?: string;
  length?: number;
}

interface MockHistoryEntry {
  heads: string[];
  actor: string;
  time: number;
  patches: MockPatch[];
}

function createMockHandle(entries: MockHistoryEntry[]) {
  const history = entries.map(e => e.heads);
  const metadataMap = new Map<string, { time: number; actor: string }>();
  for (const e of entries) metadataMap.set(e.heads[0], { time: e.time, actor: e.actor });
  return {
    history: vi.fn(() => history),
    metadata: vi.fn((hash: string) => metadataMap.get(hash)),
    doc: vi.fn(() => ({ text: '' })),
  };
}

function createMockDiff(entries: MockHistoryEntry[]) {
  return vi.fn((_doc: unknown, _before: unknown, _after: unknown) => {
    const afterStr = JSON.stringify(_after);
    for (const entry of entries) {
      if (JSON.stringify(entry.heads) === afterStr) return entry.patches;
    }
    return [];
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  // @ts-expect-error -- mock global
  globalThis.requestIdleCallback = (cb: IdleRequestCallback) => {
    cb({ didTimeout: false, timeRemaining: () => 50 } as IdleDeadline);
    return 1;
  };
});

// ---------------------------------------------------------------------------
// runsInsert / runsDelete — direct unit tests
// ---------------------------------------------------------------------------

describe('runsInsert', () => {
  it('inserts into an empty run list', () => {
    const runs: AttributionRun[] = [];
    runsInsert(runs, 0, 5, { actor: 'a', time: 1 });
    expect(runs).toEqual([{ start: 0, end: 5, actor: 'a', time: 1 }]);
  });

  it('prepends and shifts the existing run right', () => {
    const runs: AttributionRun[] = [{ start: 0, end: 5, actor: 'a', time: 1 }];
    runsInsert(runs, 0, 3, { actor: 'b', time: 2 });
    expect(runs).toEqual([
      { start: 0, end: 3, actor: 'b', time: 2 },
      { start: 3, end: 8, actor: 'a', time: 1 },
    ]);
  });

  it('appends at the end', () => {
    const runs: AttributionRun[] = [{ start: 0, end: 5, actor: 'a', time: 1 }];
    runsInsert(runs, 5, 3, { actor: 'b', time: 2 });
    expect(runs).toEqual([
      { start: 0, end: 5, actor: 'a', time: 1 },
      { start: 5, end: 8, actor: 'b', time: 2 },
    ]);
  });

  it('splits a run when inserting strictly inside it', () => {
    const runs: AttributionRun[] = [{ start: 0, end: 10, actor: 'a', time: 1 }];
    runsInsert(runs, 4, 2, { actor: 'b', time: 2 });
    expect(runs).toEqual([
      { start: 0, end: 4, actor: 'a', time: 1 },
      { start: 4, end: 6, actor: 'b', time: 2 },
      { start: 6, end: 12, actor: 'a', time: 1 },
    ]);
  });

  it('is a no-op for zero-length insertion', () => {
    const runs: AttributionRun[] = [{ start: 0, end: 5, actor: 'a', time: 1 }];
    runsInsert(runs, 2, 0, { actor: 'b', time: 2 });
    expect(runs).toEqual([{ start: 0, end: 5, actor: 'a', time: 1 }]);
  });
});

describe('runsDelete', () => {
  it('deletes a whole run', () => {
    const runs: AttributionRun[] = [
      { start: 0, end: 3, actor: 'a', time: 1 },
      { start: 3, end: 6, actor: 'b', time: 2 },
    ];
    runsDelete(runs, 0, 3);
    expect(runs).toEqual([{ start: 0, end: 3, actor: 'b', time: 2 }]);
  });

  it('deletes in the middle of a run, shrinking it', () => {
    const runs: AttributionRun[] = [{ start: 0, end: 10, actor: 'a', time: 1 }];
    runsDelete(runs, 3, 4);
    expect(runs).toEqual([{ start: 0, end: 6, actor: 'a', time: 1 }]);
  });

  it('deletes across multiple runs', () => {
    const runs: AttributionRun[] = [
      { start: 0, end: 4, actor: 'a', time: 1 },
      { start: 4, end: 8, actor: 'b', time: 2 },
      { start: 8, end: 12, actor: 'c', time: 3 },
    ];
    runsDelete(runs, 2, 8); // deletes from char 2 through char 10
    expect(runs).toEqual([
      { start: 0, end: 2, actor: 'a', time: 1 },
      { start: 2, end: 4, actor: 'c', time: 3 },
    ]);
  });

  it('is a no-op for zero-length deletion', () => {
    const runs: AttributionRun[] = [{ start: 0, end: 5, actor: 'a', time: 1 }];
    runsDelete(runs, 2, 0);
    expect(runs).toEqual([{ start: 0, end: 5, actor: 'a', time: 1 }]);
  });
});

describe('applyPatchToRuns — put', () => {
  it('resets runs to a single run for a non-empty put', () => {
    const runs: AttributionRun[] = [{ start: 0, end: 99, actor: 'x', time: 0 }];
    applyPatchToRuns(
      runs,
      { action: 'put', path: ['text'], value: 'hello' },
      { actor: 'a', time: 1 },
    );
    expect(runs).toEqual([{ start: 0, end: 5, actor: 'a', time: 1 }]);
  });

  it('clears runs on an empty put', () => {
    const runs: AttributionRun[] = [{ start: 0, end: 99, actor: 'x', time: 0 }];
    applyPatchToRuns(
      runs,
      { action: 'put', path: ['text'], value: '' },
      { actor: 'a', time: 1 },
    );
    expect(runs).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// buildRunListAttribution — basic
// ---------------------------------------------------------------------------

describe('buildRunListAttribution', () => {
  it('builds runs matching the final text length for an append sequence', async () => {
    const entries: MockHistoryEntry[] = [
      {
        heads: ['h1'], actor: 'a1', time: 1000,
        patches: [{ action: 'splice', path: ['text', 0], value: 'he' }],
      },
      {
        heads: ['h2'], actor: 'a2', time: 2000,
        patches: [{ action: 'splice', path: ['text', 2], value: 'llo' }],
      },
    ];
    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    const result = await buildRunListAttribution(handle as never, 'text');

    expect(result).not.toBeNull();
    expect(result!.runs).toEqual([
      { start: 0, end: 2, actor: 'a1', time: 1000 },
      { start: 2, end: 5, actor: 'a2', time: 2000 },
    ]);
    expect(result!.processedHistoryIndex).toBe(2);
  });

  it('returns null when handle.history() is undefined', async () => {
    const handle = {
      history: vi.fn(() => undefined),
      metadata: vi.fn(),
      doc: vi.fn(() => ({ text: '' })),
    };
    const result = await buildRunListAttribution(handle as never, 'text');
    expect(result).toBeNull();
  });

  it('handles a single bulk insert well above the per-char splice chunk limit', async () => {
    const N = 200_000;
    const entries: MockHistoryEntry[] = [{
      heads: ['h1'], actor: 'a1', time: 1000,
      patches: [{ action: 'splice', path: ['text', 0], value: 'x'.repeat(N) }],
    }];
    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    const result = await buildRunListAttribution(handle as never, 'text');

    expect(result).not.toBeNull();
    // Run-based representation stays at exactly 1 run regardless of N.
    expect(result!.runs).toHaveLength(1);
    expect(result!.runs[0]).toEqual({ start: 0, end: N, actor: 'a1', time: 1000 });
  });

  it('yields to the event loop before touching history', async () => {
    // The hook renders the document without attribution during the build;
    // the build must yield first so the initial paint isn't blocked by
    // the first chunk's CPU work.
    const entries: MockHistoryEntry[] = [{
      heads: ['h1'], actor: 'a1', time: 1000,
      patches: [{ action: 'splice', path: ['text', 0], value: 'x' }],
    }];
    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    let idleCalls = 0;
    let metadataCalledBeforeIdle = false;
    const originalMetadata = handle.metadata;
    handle.metadata = vi.fn((hash: string) => {
      if (idleCalls === 0) metadataCalledBeforeIdle = true;
      return originalMetadata(hash);
    });

    // @ts-expect-error -- mock global
    globalThis.requestIdleCallback = (cb: IdleRequestCallback) => {
      idleCalls++;
      cb({ didTimeout: false, timeRemaining: () => 50 } as IdleDeadline);
      return idleCalls;
    };

    await buildRunListAttribution(handle as never, 'text');

    expect(idleCalls).toBeGreaterThan(0);
    expect(metadataCalledBeforeIdle).toBe(false);
  });
});

describe('updateRunListAttribution', () => {
  it('applies only the new history entries', () => {
    const existing: RunListAttribution = {
      runs: [{ start: 0, end: 2, actor: 'a1', time: 1000 }],
      processedHeads: ['h1'],
      processedHistoryIndex: 1,
    };
    const newEntry: MockHistoryEntry = {
      heads: ['h2'], actor: 'a2', time: 2000,
      patches: [{ action: 'splice', path: ['text', 2], value: 'XY' }],
    };
    const handle = createMockHandle([
      { heads: ['h1'], actor: 'a1', time: 1000, patches: [] },
      newEntry,
    ]);
    mockDiff.mockImplementation(createMockDiff([newEntry]));

    const result = updateRunListAttribution(existing, handle as never, 'text');

    expect(result.runs).toEqual([
      { start: 0, end: 2, actor: 'a1', time: 1000 },
      { start: 2, end: 4, actor: 'a2', time: 2000 },
    ]);
    expect(result.processedHistoryIndex).toBe(2);
  });
});

// ---------------------------------------------------------------------------
// makeRunListSource — query
// ---------------------------------------------------------------------------

describe('makeRunListSource', () => {
  const runs: AttributionRun[] = [
    { start: 0, end: 5, actor: 'a', time: 1000 },
    { start: 5, end: 10, actor: 'b', time: 2000 },
    { start: 10, end: 15, actor: 'c', time: 500 },
  ];
  // ASCII: byte offset = char index. Length 16 to cover end boundary.
  const byteToCharMap = Array.from({ length: 16 }, (_, i) => i);

  it('returns the single run covering a range', () => {
    const source = makeRunListSource(runs, byteToCharMap);
    expect(source.queryByteRange(0, 0, 5)).toEqual({ actor: 'a', time: 1000 });
    expect(source.queryByteRange(0, 5, 10)).toEqual({ actor: 'b', time: 2000 });
  });

  it('returns the most recent attribution across spanning runs', () => {
    const source = makeRunListSource(runs, byteToCharMap);
    // [0, 15) spans all three — most recent is b @ 2000
    expect(source.queryByteRange(0, 0, 15)).toEqual({ actor: 'b', time: 2000 });
  });

  it('returns null for an empty/inverted range', () => {
    const source = makeRunListSource(runs, byteToCharMap);
    expect(source.queryByteRange(0, 5, 5)).toBeNull();
    expect(source.queryByteRange(0, 7, 5)).toBeNull();
  });

  it('returns null when byte offsets map out of range', () => {
    const source = makeRunListSource(runs, byteToCharMap);
    expect(source.queryByteRange(0, 100, 200)).toBeNull();
  });

  it('returns null for an empty run list', () => {
    const source = makeRunListSource([], [0]);
    expect(source.queryByteRange(0, 0, 1)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Cross-validation: runs path must agree with per-char path on queries
// ---------------------------------------------------------------------------

describe('cross-validation: run-list vs per-char produce identical queries', () => {
  function genWorkload(pattern: 'append' | 'prepend' | 'random', N: number): MockHistoryEntry[] {
    let s = 0x12345678;
    const rand = () => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s / 0x7fffffff; };
    const out: MockHistoryEntry[] = [];
    let len = 0;
    for (let i = 0; i < N; i++) {
      const pos = pattern === 'append' ? len
        : pattern === 'prepend' ? 0
        : Math.floor(rand() * (len + 1));
      out.push({
        heads: [`${pattern}_h${i}`],
        actor: `a${i % 3}`,
        time: 1_700_000_000 + i,
        patches: [{ action: 'splice', path: ['text', pos], value: 'x' }],
      });
      len++;
    }
    return out;
  }

  for (const pattern of ['append', 'prepend', 'random'] as const) {
    it(`agrees on ${pattern} workload (N=200)`, async () => {
      const entries = genWorkload(pattern, 200);
      const handle = createMockHandle(entries);
      mockDiff.mockImplementation(createMockDiff(entries));

      const charMap = (await buildAttributionMap(handle as never, 'text'))!;
      const runList = (await buildRunListAttribution(handle as never, 'text'))!;
      expect(charMap.entries).toHaveLength(200);
      const byteToCharMap = Array.from({ length: 201 }, (_, i) => i);
      const charSource = makeCharArraySource(charMap.entries, byteToCharMap);
      const runSource = makeRunListSource(runList.runs, byteToCharMap);

      // Probe a variety of ranges; both sources must agree.
      const probes = [
        [0, 1], [0, 10], [0, 200], [50, 60], [100, 120], [150, 200], [75, 125],
      ];
      for (const [s, e] of probes) {
        expect(runSource.queryByteRange(0, s, e))
          .toEqual(charSource.queryByteRange(0, s, e));
      }
    });
  }
});
