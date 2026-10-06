/**
 * Tests for ReplaySession
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@automerge/automerge', () => ({
  clone: vi.fn(),
  view: vi.fn(),
  free: vi.fn(),
  getAuthorForActor: vi.fn(),
}));

vi.mock('@automerge/automerge-repo', async (importOriginal) => {
  const original = await importOriginal<typeof import('@automerge/automerge-repo')>();
  return {
    ...original,
    decodeHeads: vi.fn(),
  };
});

import { clone, view, free, getAuthorForActor } from '@automerge/automerge';
import { decodeHeads } from '@automerge/automerge-repo';
import { createReplaySession, type ReplaySession } from './replay.js';

const mockClone = vi.mocked(clone);
const mockView = vi.mocked(view);
const mockFree = vi.mocked(free);
const mockDecodeHeads = vi.mocked(decodeHeads);
const mockGetAuthorForActor = vi.mocked(getAuthorForActor);

/** actor → author index entries the mocked `getAuthorForActor` serves. */
let authorByActor: Record<string, string | undefined> = {};

/**
 * Create a mock DocHandle with configurable history.
 * texts[i] is the content at history index i.
 * authors[i], when the array is provided, is the author footer on change i
 * (undefined = no footer, as on seq>1 and pre-transition changes).
 */
function createMockHandle(
  texts: string[],
  timestamps?: number[],
  actors?: string[],
  authors?: (string | undefined)[],
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
      return { time: ts, actor, author: authors?.[index] };
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
    authorByActor = {};
    mockGetAuthorForActor.mockImplementation(
      (_doc: unknown, actor: string) => authorByActor[actor],
    );
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
    authorByActor = {};
    mockGetAuthorForActor.mockImplementation(
      (_doc: unknown, actor: string) => authorByActor[actor],
    );
    updateContent = vi.fn();
  });

  function enterSession(
    texts: string[],
    timestamps?: number[],
    actors?: string[],
    authors?: (string | undefined)[],
  ) {
    const handle = createMockHandle(texts, timestamps, actors, authors);
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

    // ── Author-first resolution (author-ID transition) ─────────────
    //
    // getMetadataAt returns the *attribution key* for the step:
    //   change.author (seq-1 footer) ?? getAuthorForActor(actor) (seq>1)
    //   ?? change.actor (pre-transition history, no author anywhere).

    it('prefers the change author over the raw actor (seq-1 footer)', () => {
      enterSession(
        ['a', 'b'],
        undefined,
        ['random-actor-1', 'random-actor-1'],
        ['authorAlice', undefined],
      );

      expect(session.getMetadataAt(0).actor).toBe('authorAlice');
    });

    it('resolves a footerless seq>1 change through the actor→author index', () => {
      // The footer lives on the actor's seq-1 change only (Phase 0
      // finding 1); later changes resolve via getAuthorForActor.
      authorByActor = { 'random-actor-1': 'authorAlice' };
      enterSession(
        ['a', 'b'],
        undefined,
        ['random-actor-1', 'random-actor-1'],
        ['authorAlice', undefined],
      );

      expect(session.getMetadataAt(1).actor).toBe('authorAlice');
    });

    it('falls back to the bare actor for pre-transition changes (author: undefined)', () => {
      enterSession(['a', 'b'], undefined, ['stable-actor-1', 'stable-actor-1']);

      expect(session.getMetadataAt(1).actor).toBe('stable-actor-1');
    });

    it('keeps one continuous key across the transition boundary (D5)', () => {
      // Legacy steps (bare stable actor) and post-transition steps (author)
      // by the same user resolve to the SAME attribution key: the author ID
      // equals the legacy stable actor ID by construction.
      const KEY = 'a'.repeat(64);
      authorByActor = { 'random-actor-9': KEY };
      enterSession(
        ['a', 'b', 'c', 'd'],
        undefined,
        [KEY, KEY, 'random-actor-9', 'random-actor-9'],
        [undefined, undefined, KEY, undefined],
      );

      for (let i = 0; i < 4; i++) {
        expect(session.getMetadataAt(i).actor).toBe(KEY);
      }
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
