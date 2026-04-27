/**
 * Tests for projectSetReconciler.
 *
 * The reconciler computes which IDB project entries are missing from the
 * synced project set, keyed by the `automerge:`-stripped indexDocId. It is
 * pure — no IDB, no Automerge, no network — so every test here is a plain
 * data-in/data-out assertion.
 *
 * Context: see claude-notes/plans/2026-04-16-share-link-project-not-added.md.
 */

import { describe, it, expect } from 'vitest';
import { computeReconcileAdds, type ReconcilableEntry } from './projectSetReconciler';

function idb(partial: Partial<ReconcilableEntry> & Pick<ReconcilableEntry, 'indexDocId'>): ReconcilableEntry {
  return {
    syncServer: 'ws://localhost:3030',
    description: 'Project',
    lastAccessed: '2026-04-16T00:00:00.000Z',
    ...partial,
  };
}

describe('computeReconcileAdds', () => {
  it('returns nothing when both sides are empty', () => {
    expect(computeReconcileAdds([], [])).toEqual([]);
  });

  it('returns nothing when IDB is empty', () => {
    expect(
      computeReconcileAdds([], [{ indexDocId: 'automerge:abc' }]),
    ).toEqual([]);
  });

  it('returns all IDB entries when the set is empty', () => {
    const a = idb({ indexDocId: 'automerge:abc', description: 'A' });
    const b = idb({ indexDocId: 'automerge:def', description: 'B' });
    expect(computeReconcileAdds([a, b], [])).toEqual([a, b]);
  });

  it('excludes an IDB entry already present in the set (both prefixed)', () => {
    const a = idb({ indexDocId: 'automerge:abc' });
    const b = idb({ indexDocId: 'automerge:def' });
    const result = computeReconcileAdds(
      [a, b],
      [{ indexDocId: 'automerge:abc' }],
    );
    expect(result).toEqual([b]);
  });

  it('normalises the automerge: prefix when comparing — IDB prefixed, set unprefixed', () => {
    // Mirrors the reporting user's profile doc, where some entries were stored
    // with the prefix and some without (see the bug report's debug dump).
    const a = idb({ indexDocId: 'automerge:abc' });
    const result = computeReconcileAdds([a], [{ indexDocId: 'abc' }]);
    expect(result).toEqual([]);
  });

  it('normalises the automerge: prefix when comparing — IDB unprefixed, set prefixed', () => {
    const a = idb({ indexDocId: 'abc' });
    const result = computeReconcileAdds([a], [{ indexDocId: 'automerge:abc' }]);
    expect(result).toEqual([]);
  });

  it('deduplicates IDB rows that resolve to the same key, preferring the most recently accessed', () => {
    // Two IDB rows that canonicalise to the same key — this can happen if
    // different code paths wrote the indexDocId with and without the prefix.
    const older = idb({
      indexDocId: 'abc',
      description: 'older',
      lastAccessed: '2026-04-10T00:00:00.000Z',
    });
    const newer = idb({
      indexDocId: 'automerge:abc',
      description: 'newer',
      lastAccessed: '2026-04-15T00:00:00.000Z',
    });
    const result = computeReconcileAdds([older, newer], []);
    expect(result).toEqual([newer]);
  });

  it('ignores set entries that are not in IDB (no deletions proposed)', () => {
    // The reconciler adds; it never removes. A user who deleted a project
    // locally must not see it resurrected on another browser via reconcile.
    const result = computeReconcileAdds(
      [idb({ indexDocId: 'automerge:onlyInIdb' })],
      [{ indexDocId: 'automerge:onlyInSet' }],
    );
    expect(result).toEqual([idb({ indexDocId: 'automerge:onlyInIdb' })]);
  });

  it('passes through syncServer / description / lastAccessed verbatim', () => {
    const a = idb({
      indexDocId: 'automerge:abc',
      syncServer: 'wss://example.com/ws',
      description: 'My Project',
      lastAccessed: '2026-04-14T12:34:56.000Z',
    });
    const [out] = computeReconcileAdds([a], []);
    expect(out).toEqual(a);
  });
});
