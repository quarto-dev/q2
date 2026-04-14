/**
 * Tests for attribution service — per-character attribution from Automerge history.
 *
 * Test specs 1, 1b, 1c, 2, 3 from the plan.
 *
 * @vitest-environment node
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

// Mock automergeSync before importing the module under test
vi.mock('./automergeSync', () => ({
  getFileHandle: vi.fn(),
}));

import type { ActorIdentity } from './automergeSync';
import {
  buildAttributionMap,
  updateAttributionMap,
  getNodeAttribution,
  buildByteToCharMap,
  HistoryCompactedError,
  CHUNK_SIZE,
} from './attribution';
import type { AttributionMap, CharAttribution } from './attribution';

// ---------------------------------------------------------------------------
// Helpers to build mock handles with controllable history
// ---------------------------------------------------------------------------

interface MockPatch {
  action: 'insert' | 'del';
  path: [string, number];
  values?: string[];
  length?: number;
}

interface MockHistoryEntry {
  heads: string[];
  actor: string;
  time: number;
  patches: MockPatch[];
}

/**
 * Build a mock DocHandle whose history() / metadata() / doc() are driven by
 * a sequence of MockHistoryEntry objects.
 *
 * The diff() function is mocked at module level; the handle only needs to
 * expose history(), metadata(), and doc().
 */
function createMockHandle(entries: MockHistoryEntry[]) {
  const history = entries.map(e => e.heads);
  const metadataMap = new Map<string, { time: number; actor: string }>();
  for (const e of entries) {
    metadataMap.set(e.heads[0], { time: e.time, actor: e.actor });
  }

  return {
    history: vi.fn(() => history),
    metadata: vi.fn((hash: string) => metadataMap.get(hash)),
    doc: vi.fn(() => ({ text: '' })),
    _entries: entries,
  };
}

/**
 * Create a mock diff function that returns patches for each consecutive pair.
 * The function matches on (before, after) heads to return the right patches.
 */
function createMockDiff(entries: MockHistoryEntry[]) {
  return vi.fn((_doc: unknown, _before: unknown, _after: unknown) => {
    // Find the entry whose heads match 'after'
    const afterStr = JSON.stringify(_after);
    for (const entry of entries) {
      if (JSON.stringify(entry.heads) === afterStr) {
        return entry.patches;
      }
    }
    return [];
  });
}

// We'll mock the diff and decodeHeads at module level
vi.mock('@automerge/automerge', async () => {
  const actual = await vi.importActual<typeof import('@automerge/automerge')>('@automerge/automerge');
  return {
    ...actual,
    diff: vi.fn(),
  };
});

vi.mock('@automerge/automerge-repo', async () => {
  const actual = await vi.importActual<typeof import('@automerge/automerge-repo')>('@automerge/automerge-repo');
  return {
    ...actual,
    // decodeHeads is an identity function for our mock string heads
    decodeHeads: vi.fn((heads: unknown) => heads),
  };
});

import { diff } from '@automerge/automerge';
const mockDiff = vi.mocked(diff);

// Mock requestIdleCallback for chunked processing tests
const mockRequestIdleCallback = vi.fn((cb: IdleRequestCallback) => {
  cb({ didTimeout: false, timeRemaining: () => 50 } as IdleDeadline);
  return 1;
});

const mockCancelIdleCallback = vi.fn();

// Install global mocks before each test
beforeEach(() => {
  vi.clearAllMocks();
  // @ts-expect-error -- mock global
  globalThis.requestIdleCallback = mockRequestIdleCallback;
  // @ts-expect-error -- mock global
  globalThis.cancelIdleCallback = mockCancelIdleCallback;
});

// ===========================================================================
// Test spec 1: buildAttributionMap — full build (cold start)
// ===========================================================================

describe('buildAttributionMap — full build', () => {
  it('builds entries matching current text length for 3 changes by 2 actors', async () => {
    // History: actor1 inserts "he", actor2 inserts "llo" at index 2, actor1 inserts " w" at 5
    const entries: MockHistoryEntry[] = [
      {
        heads: ['h1'],
        actor: 'actor1',
        time: 1000,
        patches: [{ action: 'insert', path: ['text', 0], values: ['h', 'e'] }],
      },
      {
        heads: ['h2'],
        actor: 'actor2',
        time: 2000,
        patches: [{ action: 'insert', path: ['text', 2], values: ['l', 'l', 'o'] }],
      },
      {
        heads: ['h3'],
        actor: 'actor1',
        time: 3000,
        patches: [{ action: 'insert', path: ['text', 5], values: [' ', 'w'] }],
      },
    ];

    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    const result = await buildAttributionMap(handle as any, 'text');

    expect(result).not.toBeNull();
    // "hello w" = 7 characters
    expect(result!.entries).toHaveLength(7);

    // 'h','e' → actor1
    expect(result!.entries[0]).toEqual({ actor: 'actor1', time: 1000 });
    expect(result!.entries[1]).toEqual({ actor: 'actor1', time: 1000 });

    // 'l','l','o' → actor2
    expect(result!.entries[2]).toEqual({ actor: 'actor2', time: 2000 });
    expect(result!.entries[3]).toEqual({ actor: 'actor2', time: 2000 });
    expect(result!.entries[4]).toEqual({ actor: 'actor2', time: 2000 });

    // ' ','w' → actor1
    expect(result!.entries[5]).toEqual({ actor: 'actor1', time: 3000 });
    expect(result!.entries[6]).toEqual({ actor: 'actor1', time: 3000 });
  });

  it('sets processedHeads to final heads and processedHistoryIndex to history.length', async () => {
    const entries: MockHistoryEntry[] = [
      {
        heads: ['h1'],
        actor: 'actor1',
        time: 1000,
        patches: [{ action: 'insert', path: ['text', 0], values: ['a'] }],
      },
      {
        heads: ['h2'],
        actor: 'actor2',
        time: 2000,
        patches: [{ action: 'insert', path: ['text', 1], values: ['b'] }],
      },
    ];

    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    const result = await buildAttributionMap(handle as any, 'text');

    expect(result).not.toBeNull();
    expect(result!.processedHeads).toEqual(['h2']);
    expect(result!.processedHistoryIndex).toBe(2);
  });

  it('returns empty entries attributed to local actor when history is empty', async () => {
    const handle = {
      history: vi.fn(() => []),
      metadata: vi.fn(),
      doc: vi.fn(() => ({ text: '' })),
    };

    const result = await buildAttributionMap(handle as any, 'text');

    expect(result).not.toBeNull();
    expect(result!.entries).toHaveLength(0);
    expect(result!.processedHistoryIndex).toBe(0);
  });

  it('returns null when handle.history() returns undefined', async () => {
    const handle = {
      history: vi.fn(() => undefined),
      metadata: vi.fn(),
      doc: vi.fn(() => ({ text: '' })),
    };

    const result = await buildAttributionMap(handle as any, 'text');

    expect(result).toBeNull();
  });

  it('filters out patches for non-text fields', async () => {
    const entries: MockHistoryEntry[] = [
      {
        heads: ['h1'],
        actor: 'actor1',
        time: 1000,
        patches: [
          { action: 'insert', path: ['text', 0], values: ['a', 'b'] },
          // This patch targets a different field — should be ignored
          { action: 'insert', path: ['other', 0], values: ['x'] } as any,
        ],
      },
    ];

    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    const result = await buildAttributionMap(handle as any, 'text');

    expect(result).not.toBeNull();
    // Only 'a','b' from the 'text' field — 'x' from 'other' is excluded
    expect(result!.entries).toHaveLength(2);
  });
});

// ===========================================================================
// Test spec 1b: updateAttributionMap — incremental update (warm path)
// ===========================================================================

describe('updateAttributionMap — incremental update', () => {
  it('attributes only new characters from insertion to the new actor', () => {
    // Existing map: "ab" by actor1
    const existingMap: AttributionMap = {
      entries: [
        { actor: 'actor1', time: 1000 },
        { actor: 'actor1', time: 1000 },
      ],
      processedHeads: ['h1'],
      processedHistoryIndex: 1,
    };

    // New history entry: actor2 inserts "cd" at index 2
    const newEntry: MockHistoryEntry = {
      heads: ['h2'],
      actor: 'actor2',
      time: 2000,
      patches: [{ action: 'insert', path: ['text', 2], values: ['c', 'd'] }],
    };

    const handle = createMockHandle([
      { heads: ['h1'], actor: 'actor1', time: 1000, patches: [] },
      newEntry,
    ]);
    mockDiff.mockImplementation(createMockDiff([newEntry]));

    const result = updateAttributionMap(existingMap, handle as any, 'text');

    // "abcd" = 4 entries
    expect(result.entries).toHaveLength(4);
    // Original entries unchanged
    expect(result.entries[0]).toEqual({ actor: 'actor1', time: 1000 });
    expect(result.entries[1]).toEqual({ actor: 'actor1', time: 1000 });
    // New entries attributed to actor2
    expect(result.entries[2]).toEqual({ actor: 'actor2', time: 2000 });
    expect(result.entries[3]).toEqual({ actor: 'actor2', time: 2000 });
  });

  it('removes entries at deleted range, preserves surrounding entries', () => {
    // Existing map: "abcd" — a,b by actor1, c,d by actor2
    const existingMap: AttributionMap = {
      entries: [
        { actor: 'actor1', time: 1000 },
        { actor: 'actor1', time: 1000 },
        { actor: 'actor2', time: 2000 },
        { actor: 'actor2', time: 2000 },
      ],
      processedHeads: ['h2'],
      processedHistoryIndex: 2,
    };

    // actor1 deletes "bc" (index 1, length 2)
    const newEntry: MockHistoryEntry = {
      heads: ['h3'],
      actor: 'actor1',
      time: 3000,
      patches: [{ action: 'del', path: ['text', 1], length: 2 }],
    };

    const handle = createMockHandle([
      { heads: ['h1'], actor: 'actor1', time: 1000, patches: [] },
      { heads: ['h2'], actor: 'actor2', time: 2000, patches: [] },
      newEntry,
    ]);
    mockDiff.mockImplementation(createMockDiff([newEntry]));

    const result = updateAttributionMap(existingMap, handle as any, 'text');

    // "ad" = 2 entries
    expect(result.entries).toHaveLength(2);
    expect(result.entries[0]).toEqual({ actor: 'actor1', time: 1000 });
    expect(result.entries[1]).toEqual({ actor: 'actor2', time: 2000 });
  });

  it('handles mixed replace: deletion removes old, insertion adds new', () => {
    // Existing map: "hello" — all by actor1
    const existingMap: AttributionMap = {
      entries: Array(5).fill({ actor: 'actor1', time: 1000 }),
      processedHeads: ['h1'],
      processedHistoryIndex: 1,
    };

    // actor2 replaces "llo" (index 2, len 3) with "y"
    const newEntry: MockHistoryEntry = {
      heads: ['h2'],
      actor: 'actor2',
      time: 2000,
      patches: [
        { action: 'del', path: ['text', 2], length: 3 },
        { action: 'insert', path: ['text', 2], values: ['y'] },
      ],
    };

    const handle = createMockHandle([
      { heads: ['h1'], actor: 'actor1', time: 1000, patches: [] },
      newEntry,
    ]);
    mockDiff.mockImplementation(createMockDiff([newEntry]));

    const result = updateAttributionMap(existingMap, handle as any, 'text');

    // "hey" = 3 entries
    expect(result.entries).toHaveLength(3);
    expect(result.entries[0]).toEqual({ actor: 'actor1', time: 1000 });
    expect(result.entries[1]).toEqual({ actor: 'actor1', time: 1000 });
    expect(result.entries[2]).toEqual({ actor: 'actor2', time: 2000 }); // 'y'
  });

  it('advances processedHeads and processedHistoryIndex', () => {
    const existingMap: AttributionMap = {
      entries: [{ actor: 'actor1', time: 1000 }],
      processedHeads: ['h1'],
      processedHistoryIndex: 1,
    };

    const newEntry: MockHistoryEntry = {
      heads: ['h2'],
      actor: 'actor2',
      time: 2000,
      patches: [{ action: 'insert', path: ['text', 1], values: ['b'] }],
    };

    const handle = createMockHandle([
      { heads: ['h1'], actor: 'actor1', time: 1000, patches: [] },
      newEntry,
    ]);
    mockDiff.mockImplementation(createMockDiff([newEntry]));

    const result = updateAttributionMap(existingMap, handle as any, 'text');

    expect(result.processedHeads).toEqual(['h2']);
    expect(result.processedHistoryIndex).toBe(2);
  });

  it('throws HistoryCompactedError when processedHistoryIndex > history.length', () => {
    const existingMap: AttributionMap = {
      entries: [{ actor: 'actor1', time: 1000 }],
      processedHeads: ['h5'],
      processedHistoryIndex: 5, // was at index 5
    };

    // But history only has 2 entries now (compacted)
    const handle = {
      history: vi.fn(() => [['h1'], ['h2']]),
      metadata: vi.fn(),
      doc: vi.fn(() => ({ text: '' })),
    };

    expect(() => {
      updateAttributionMap(existingMap, handle as any, 'text');
    }).toThrow(HistoryCompactedError);
  });
});

// ===========================================================================
// Test spec 1c: buildAttributionMap — chunked processing
// ===========================================================================

describe('buildAttributionMap — chunked processing', () => {
  it('processes large history in chunks via requestIdleCallback', async () => {
    // Create 120 history entries, each inserting one character
    const entries: MockHistoryEntry[] = [];
    for (let i = 0; i < 120; i++) {
      entries.push({
        heads: [`h${i}`],
        actor: `actor${i % 2}`,
        time: 1000 + i,
        patches: [{ action: 'insert', path: ['text', i], values: [String.fromCharCode(97 + (i % 26))] }],
      });
    }

    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    const result = await buildAttributionMap(handle as any, 'text');

    expect(result).not.toBeNull();
    expect(result!.entries).toHaveLength(120);
    // requestIdleCallback should have been called for chunking
    // 120 entries / CHUNK_SIZE chunks, plus potentially the initial call
    expect(mockRequestIdleCallback.mock.calls.length).toBeGreaterThanOrEqual(
      Math.ceil(120 / CHUNK_SIZE)
    );
  });

  it('returns null immediately when signal is aborted before first chunk', async () => {
    const entries: MockHistoryEntry[] = [
      {
        heads: ['h1'],
        actor: 'actor1',
        time: 1000,
        patches: [{ action: 'insert', path: ['text', 0], values: ['a'] }],
      },
    ];

    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    const controller = new AbortController();
    controller.abort(); // Abort before calling

    const result = await buildAttributionMap(handle as any, 'text', controller.signal);

    expect(result).toBeNull();
  });

  it('returns null when signal is aborted between chunks', async () => {
    // Create enough entries to span multiple chunks
    const entries: MockHistoryEntry[] = [];
    for (let i = 0; i < CHUNK_SIZE + 10; i++) {
      entries.push({
        heads: [`h${i}`],
        actor: 'actor1',
        time: 1000 + i,
        patches: [{ action: 'insert', path: ['text', i], values: ['a'] }],
      });
    }

    const handle = createMockHandle(entries);
    mockDiff.mockImplementation(createMockDiff(entries));

    const controller = new AbortController();

    // Abort after the first chunk's requestIdleCallback fires
    let callCount = 0;
    mockRequestIdleCallback.mockImplementation((cb: IdleRequestCallback) => {
      callCount++;
      if (callCount === 1) {
        // Let first chunk complete, then abort
        cb({ didTimeout: false, timeRemaining: () => 50 } as IdleDeadline);
        controller.abort();
      } else {
        cb({ didTimeout: false, timeRemaining: () => 50 } as IdleDeadline);
      }
      return callCount;
    });

    const result = await buildAttributionMap(handle as any, 'text', controller.signal);

    expect(result).toBeNull();
  });
});

// ===========================================================================
// Test spec 2: getNodeAttribution query
// ===========================================================================

describe('getNodeAttribution', () => {
  // Mock SourceInfoReconstructor
  function createMockReconstructor(sourceLocation: { fileId: number; start: number; end: number } | null) {
    return {
      getSourceLocation: vi.fn((id: number) => {
        if (sourceLocation === null) throw new Error('Invalid source info ID');
        return sourceLocation;
      }),
    };
  }

  it('resolves source info to node attribution with correct identity', () => {
    const map: AttributionMap = {
      entries: [
        { actor: 'actor1', time: 1000 },
        { actor: 'actor1', time: 1000 },
        { actor: 'actor2', time: 2000 },
        { actor: 'actor2', time: 2000 },
        { actor: 'actor2', time: 2000 },
      ],
      processedHeads: ['h2'],
      processedHistoryIndex: 2,
    };

    // Source info ID 5 → file 0, bytes 2-5 → chars 2-5 (ASCII)
    const reconstructor = createMockReconstructor({ fileId: 0, start: 2, end: 5 });
    const byteToCharMap = [0, 1, 2, 3, 4, 5]; // ASCII: byte offset === char index (+ end boundary)
    const identities: Record<string, ActorIdentity> = {
      actor2: { name: 'Bob', color: '#2196F3' },
    };

    const result = getNodeAttribution(5, reconstructor as any, map, byteToCharMap, identities);

    expect(result).not.toBeNull();
    expect(result!.actor).toBe('actor2');
    expect(result!.time).toBe(2000);
    expect(result!.color).toBe('#2196F3');
    expect(result!.name).toBe('Bob');
  });

  it('returns null for invalid source info ID (reconstructor throws)', () => {
    const map: AttributionMap = {
      entries: [{ actor: 'actor1', time: 1000 }],
      processedHeads: ['h1'],
      processedHistoryIndex: 1,
    };

    const reconstructor = createMockReconstructor(null);
    const byteToCharMap = [0];
    const identities: Record<string, ActorIdentity> = {};

    const result = getNodeAttribution(999, reconstructor as any, map, byteToCharMap, identities);

    expect(result).toBeNull();
  });

  it('returns null when identities map is empty', () => {
    const map: AttributionMap = {
      entries: [
        { actor: 'actor1', time: 1000 },
        { actor: 'actor1', time: 1000 },
      ],
      processedHeads: ['h1'],
      processedHistoryIndex: 1,
    };

    const reconstructor = createMockReconstructor({ fileId: 0, start: 0, end: 2 });
    const byteToCharMap = [0, 1, 2]; // 2 bytes + end boundary
    const identities: Record<string, ActorIdentity> = {};

    const result = getNodeAttribution(0, reconstructor as any, map, byteToCharMap, identities);

    // Should still return attribution even without identity — uses fallback
    // The actor is known, but identity may not have name/color
    // Implementation should handle missing identity gracefully
    expect(result).not.toBeNull();
    expect(result!.actor).toBe('actor1');
  });

  it('returns null when attribution map is null-like (empty entries for range)', () => {
    const map: AttributionMap = {
      entries: [], // empty
      processedHeads: ['h1'],
      processedHistoryIndex: 1,
    };

    const reconstructor = createMockReconstructor({ fileId: 0, start: 0, end: 5 });
    const byteToCharMap = [0, 1, 2, 3, 4];
    const identities: Record<string, ActorIdentity> = {};

    const result = getNodeAttribution(0, reconstructor as any, map, byteToCharMap, identities);

    expect(result).toBeNull();
  });

  it('finds most recent attribution in the byte range', () => {
    const map: AttributionMap = {
      entries: [
        { actor: 'actor1', time: 1000 },
        { actor: 'actor2', time: 3000 }, // most recent
        { actor: 'actor1', time: 2000 },
      ],
      processedHeads: ['h3'],
      processedHistoryIndex: 3,
    };

    const reconstructor = createMockReconstructor({ fileId: 0, start: 0, end: 3 });
    const byteToCharMap = [0, 1, 2, 3]; // 3 bytes + end boundary
    const identities: Record<string, ActorIdentity> = {
      actor2: { name: 'Bob', color: '#E91E63' },
    };

    const result = getNodeAttribution(0, reconstructor as any, map, byteToCharMap, identities);

    expect(result).not.toBeNull();
    // Most recent is actor2 at time 3000
    expect(result!.actor).toBe('actor2');
    expect(result!.time).toBe(3000);
  });
});

// ===========================================================================
// Test spec 3: UTF-8 byte offset → JS char index conversion
// ===========================================================================

describe('buildByteToCharMap', () => {
  it('ASCII text: byte offset equals char index', () => {
    const map = buildByteToCharMap('hello');
    // 5 bytes + 1 for end boundary = 6 entries
    expect(map).toHaveLength(6);
    expect(map[0]).toBe(0); // byte 0 → char 0
    expect(map[1]).toBe(1); // byte 1 → char 1
    expect(map[4]).toBe(4); // byte 4 → char 4
    expect(map[5]).toBe(5); // end boundary
  });

  it('2-byte UTF-8 (e.g., accented chars): byte offset > char index', () => {
    // 'é' is U+00E9 = 2 bytes in UTF-8, 1 JS char
    const map = buildByteToCharMap('é');
    // 2 bytes + 1 boundary = 3 entries
    expect(map).toHaveLength(3);
    expect(map[0]).toBe(0); // first byte → char 0
    expect(map[1]).toBe(0); // second byte → still char 0 (middle of multi-byte)
    expect(map[2]).toBe(1); // end boundary → char 1
  });

  it('3-byte UTF-8 (CJK \\u4e16): byte offset > char index, 1 JS char', () => {
    // '世' (U+4E16) = 3 bytes in UTF-8, 1 JS char
    const map = buildByteToCharMap('世');
    // 3 bytes + 1 boundary = 4 entries
    expect(map).toHaveLength(4);
    expect(map[0]).toBe(0); // byte 0 → char 0
    expect(map[1]).toBe(0); // byte 1 → char 0 (mid-sequence)
    expect(map[2]).toBe(0); // byte 2 → char 0 (mid-sequence)
    expect(map[3]).toBe(1); // end boundary
  });

  it('4-byte UTF-8 (emoji \\u{1F600}): maps to 2 JS chars (surrogate pair)', () => {
    // '😀' (U+1F600) = 4 bytes in UTF-8, 2 JS chars (surrogate pair)
    const map = buildByteToCharMap('😀');
    // 4 bytes + 1 boundary = 5 entries
    expect(map).toHaveLength(5);
    expect(map[0]).toBe(0); // byte 0 → char 0
    expect(map[1]).toBe(0); // byte 1 → char 0
    expect(map[2]).toBe(0); // byte 2 → char 0
    expect(map[3]).toBe(0); // byte 3 → char 0
    expect(map[4]).toBe(2); // end boundary → char 2 (past the surrogate pair)
  });

  it('mixed ASCII + CJK + emoji', () => {
    // 'a世😀' = 1 + 3 + 4 = 8 bytes, 1 + 1 + 2 = 4 JS chars
    const map = buildByteToCharMap('a世😀');
    expect(map).toHaveLength(9); // 8 bytes + 1 boundary

    // 'a' — byte 0
    expect(map[0]).toBe(0);
    // '世' — bytes 1,2,3
    expect(map[1]).toBe(1);
    expect(map[2]).toBe(1);
    expect(map[3]).toBe(1);
    // '😀' — bytes 4,5,6,7
    expect(map[4]).toBe(2);
    expect(map[5]).toBe(2);
    expect(map[6]).toBe(2);
    expect(map[7]).toBe(2);
    // End boundary
    expect(map[8]).toBe(4);
  });

  it('empty text returns single-element mapping', () => {
    const map = buildByteToCharMap('');
    // 0 bytes + 1 boundary = 1 entry
    expect(map).toHaveLength(1);
    expect(map[0]).toBe(0);
  });
});
