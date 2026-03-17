/**
 * Tests for ReplaySession
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@automerge/automerge', () => ({
  clone: vi.fn(),
  view: vi.fn(),
  free: vi.fn(),
}));

vi.mock('@automerge/automerge-repo', async (importOriginal) => {
  const original = await importOriginal<typeof import('@automerge/automerge-repo')>();
  return {
    ...original,
    decodeHeads: vi.fn(),
  };
});

import { clone, view, free } from '@automerge/automerge';
import { decodeHeads } from '@automerge/automerge-repo';
import { createReplaySession, type ReplaySession } from './replay.js';

const mockClone = vi.mocked(clone);
const mockView = vi.mocked(view);
const mockFree = vi.mocked(free);
const mockDecodeHeads = vi.mocked(decodeHeads);

/**
 * Create a mock DocHandle with configurable history.
 * texts[i] is the content at history index i.
 */
function createMockHandle(
  texts: string[],
  timestamps?: number[],
  actors?: string[],
) {
  const historyHeads = texts.map((_, i) => [`head-${i}`]);

  const handle = {
    history: vi.fn(() => historyHeads),
    metadata: vi.fn((changeHash?: string) => {
      if (!changeHash) return undefined;
      const index = historyHeads.findIndex(h => h[0] === changeHash);
      if (index < 0) return undefined;
      const ts = timestamps?.[index] ?? 1000000 + index * 1000;
      const actor = actors?.[index] ?? `actor${index}abcdef0123456789`;
      return { time: ts, actor };
    }),
    doc: vi.fn(() => ({ text: texts[texts.length - 1] })),
  };

  // clone returns a sentinel
  const cloneObj = { __clone: true };
  mockClone.mockReturnValue(cloneObj as never);

  // decodeHeads converts UrlHeads to binary heads (identity in tests)
  mockDecodeHeads.mockImplementation((heads) => heads as never);

  // view returns a doc-like object with the right text
  mockView.mockImplementation((_doc, heads) => {
    const headArr = heads as unknown as string[];
    const index = historyHeads.findIndex(h => h[0] === headArr[0]);
    return { text: texts[index] ?? '' } as never;
  });

  return handle;
}

describe('createReplaySession', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('returns null when history() returns undefined', () => {
    const handle = {
      history: vi.fn(() => undefined),
      metadata: vi.fn(),
      doc: vi.fn(),
    };
    const session = createReplaySession(handle as never, vi.fn());
    expect(session).toBeNull();
  });

  it('returns null when history is empty', () => {
    const handle = {
      history: vi.fn(() => []),
      metadata: vi.fn(),
      doc: vi.fn(),
    };
    mockClone.mockReturnValue({} as never);
    const session = createReplaySession(handle as never, vi.fn());
    expect(session).toBeNull();
  });

  it('returns a session with correct length', () => {
    const handle = createMockHandle(['a', 'ab', 'abc']);
    const session = createReplaySession(handle as never, vi.fn());
    expect(session).not.toBeNull();
    expect(session!.length).toBe(3);
  });
});

describe('ReplaySession', () => {
  let session: ReplaySession;
  let updateContent: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();
    updateContent = vi.fn();
  });

  function enterSession(texts: string[], timestamps?: number[], actors?: string[]) {
    const handle = createMockHandle(texts, timestamps, actors);
    const s = createReplaySession(handle as never, updateContent as (content: string) => void);
    expect(s).not.toBeNull();
    session = s!;
    return handle;
  }

  describe('getContentAt()', () => {
    it('returns correct text for each index', () => {
      enterSession(['first', 'second', 'third']);

      expect(session.getContentAt(0)).toBe('first');
      expect(session.getContentAt(1)).toBe('second');
      expect(session.getContentAt(2)).toBe('third');
    });

    it('caches results — second call does not re-invoke view', () => {
      enterSession(['hello', 'world']);

      session.getContentAt(0);
      session.getContentAt(0);

      // view should only be called once for index 0
      expect(mockView).toHaveBeenCalledTimes(1);
    });

    it('returns empty string for negative index', () => {
      enterSession(['a', 'b']);
      expect(session.getContentAt(-1)).toBe('');
    });

    it('returns empty string for out-of-bounds index', () => {
      enterSession(['a', 'b']);
      expect(session.getContentAt(100)).toBe('');
    });
  });

  describe('getMetadataAt()', () => {
    it('returns timestamp and actor', () => {
      enterSession(['a', 'b', 'c'], [1000, 2000, 3000], ['alice', 'bob', 'carol']);

      const meta = session.getMetadataAt(1);
      expect(meta.timestamp).toBe(2000);
      expect(meta.actor).toBe('bob');
    });

    it('returns nulls for out-of-bounds index', () => {
      enterSession(['a', 'b']);

      const meta = session.getMetadataAt(100);
      expect(meta.timestamp).toBeNull();
      expect(meta.actor).toBeNull();
    });

    it('returns nulls for negative index', () => {
      enterSession(['a', 'b']);

      const meta = session.getMetadataAt(-1);
      expect(meta.timestamp).toBeNull();
      expect(meta.actor).toBeNull();
    });
  });

  describe('applyContentAt()', () => {
    it('calls updateContent with correct text', () => {
      enterSession(['first', 'second', 'third']);

      session.applyContentAt(1);

      expect(updateContent).toHaveBeenCalledWith('second');
    });
  });

  describe('close()', () => {
    it('frees the cloned doc', () => {
      enterSession(['a', 'b']);

      session.close();

      expect(mockFree).toHaveBeenCalledTimes(1);
    });

    it('prevents use after close — getContentAt returns empty string', () => {
      enterSession(['a', 'b']);

      session.close();

      expect(session.getContentAt(0)).toBe('');
    });

    it('prevents use after close — getMetadataAt returns nulls', () => {
      enterSession(['a', 'b']);

      session.close();

      const meta = session.getMetadataAt(0);
      expect(meta.timestamp).toBeNull();
      expect(meta.actor).toBeNull();
    });

    it('is safe to call close() multiple times', () => {
      enterSession(['a', 'b']);

      session.close();
      session.close();

      // free should only be called once
      expect(mockFree).toHaveBeenCalledTimes(1);
    });
  });
});
