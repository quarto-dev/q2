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

import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('./projectStorage', () => ({
  importData: vi.fn(),
  listProjects: vi.fn(),
  deleteProjectByIndexDocId: vi.fn(),
}));
vi.mock('./projectSetService', () => ({
  isConnected: vi.fn(),
  listProjects: vi.fn(),
  addProjectsBulk: vi.fn(),
  getRootTombstones: vi.fn(() => ({})),
}));

import {
  computeReconcileAdds,
  computeReconcilePurges,
  reconcileIntoConnectedProjectSet,
  importProjectsAndReconcile,
  type ReconcilableEntry,
} from './projectSetReconciler';
import * as projectStorage from './projectStorage';
import * as projectSetService from './projectSetService';

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

describe('importProjectsAndReconcile', () => {
  // Regression for the 2026-08-04 bug: "Imported 30 project(s)" but the home
  // displayed none. importData writes only the legacy IDB store; the set-mode
  // UI renders from the root ProjectSetDocument, and the reconcile sweep only
  // ran on page load. Import must reconcile immediately so imported projects
  // are visible without a reload.
  const mockImportData = vi.mocked(projectStorage.importData);
  const mockListIdb = vi.mocked(projectStorage.listProjects);
  const mockIsConnected = vi.mocked(projectSetService.isConnected);
  const mockListSet = vi.mocked(projectSetService.listProjects);
  const mockAddBulk = vi.mocked(projectSetService.addProjectsBulk);

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('imports into IDB then reconciles the new entries into the connected set', async () => {
    mockImportData.mockResolvedValue(3);
    mockIsConnected.mockReturnValue(true);
    mockListIdb.mockResolvedValue([
      idb({ indexDocId: 'automerge:a' }),
      idb({ indexDocId: 'automerge:b' }),
      idb({ indexDocId: 'automerge:c' }),
    ] as never);
    mockListSet.mockReturnValue([]);
    mockAddBulk.mockReturnValue(3);

    const result = await importProjectsAndReconcile('{"json":true}');

    expect(mockImportData).toHaveBeenCalledWith('{"json":true}');
    expect(mockAddBulk).toHaveBeenCalledTimes(1);
    expect(result).toEqual({ imported: 3, reconciled: 3, connected: true });
  });

  it('reports connected: false (and reconciles nothing) when the set is offline', async () => {
    // Entries land in IDB and will be swept by the on-load reconciler later;
    // the caller needs `connected` to word its message honestly.
    mockImportData.mockResolvedValue(5);
    mockIsConnected.mockReturnValue(false);

    const result = await importProjectsAndReconcile('{}');

    expect(mockAddBulk).not.toHaveBeenCalled();
    expect(result).toEqual({ imported: 5, reconciled: 0, connected: false });
  });

  it('reconciles entries already stranded in IDB even when the file adds nothing new', async () => {
    // Re-importing the same file after the bug: importData dedupes (0 new IDB
    // rows) but the set is still missing the entries — they must be added.
    mockImportData.mockResolvedValue(0);
    mockIsConnected.mockReturnValue(true);
    mockListIdb.mockResolvedValue([idb({ indexDocId: 'automerge:a' })] as never);
    mockListSet.mockReturnValue([]);
    mockAddBulk.mockReturnValue(1);

    const result = await importProjectsAndReconcile('{}');

    expect(result).toEqual({ imported: 0, reconciled: 1, connected: true });
  });

  it('propagates importData errors without attempting a reconcile', async () => {
    mockImportData.mockRejectedValue(new Error('Invalid import format'));

    await expect(importProjectsAndReconcile('not json')).rejects.toThrow('Invalid import format');
    expect(mockIsConnected).not.toHaveBeenCalled();
    expect(mockAddBulk).not.toHaveBeenCalled();
  });
});

describe('tombstones (latest-wins)', () => {
  it('suppresses an IDB row whose deletion tombstone is newer', () => {
    // Regression: deleting a project from the set used to leave the IDB row
    // behind, and the on-load reconcile resurrected it on the next load.
    const stale = idb({
      indexDocId: 'automerge:abc',
      lastAccessed: '2026-04-10T00:00:00.000Z',
    });
    expect(
      computeReconcileAdds([stale], [], { abc: '2026-04-16T00:00:00.000Z' }),
    ).toEqual([]);
  });

  it('adds an IDB row newer than its tombstone — the later access wins', () => {
    const fresh = idb({
      indexDocId: 'automerge:abc',
      lastAccessed: '2026-04-17T00:00:00.000Z',
    });
    expect(
      computeReconcileAdds([fresh], [], { abc: '2026-04-16T00:00:00.000Z' }),
    ).toEqual([fresh]);
  });

  it('delete wins ties', () => {
    const tie = idb({
      indexDocId: 'automerge:abc',
      lastAccessed: '2026-04-16T00:00:00.000Z',
    });
    expect(
      computeReconcileAdds([tie], [], { abc: '2026-04-16T00:00:00.000Z' }),
    ).toEqual([]);
  });

  it('matches tombstones by canonical key regardless of prefix', () => {
    const row = idb({
      indexDocId: 'abc', // unprefixed historical row
      lastAccessed: '2026-04-10T00:00:00.000Z',
    });
    expect(
      computeReconcileAdds([row], [], { abc: '2026-04-16T00:00:00.000Z' }),
    ).toEqual([]);
  });

  it('computeReconcilePurges returns exactly the tombstone-losing rows', () => {
    const stale = idb({
      indexDocId: 'automerge:abc',
      lastAccessed: '2026-04-10T00:00:00.000Z',
    });
    const fresh = idb({
      indexDocId: 'automerge:def',
      lastAccessed: '2026-04-17T00:00:00.000Z',
    });
    const tombstones = {
      abc: '2026-04-16T00:00:00.000Z',
      def: '2026-04-16T00:00:00.000Z',
    };
    expect(computeReconcilePurges([stale, fresh], [], tombstones)).toEqual([stale]);
  });

  it('computeReconcilePurges never proposes rows still present in the set', () => {
    // Torn state (entry present AND tombstone present, possible after a
    // concurrent add/delete merge): the live entry wins, nothing to purge.
    const row = idb({
      indexDocId: 'automerge:abc',
      lastAccessed: '2026-04-10T00:00:00.000Z',
    });
    expect(
      computeReconcilePurges(
        [row],
        [{ indexDocId: 'automerge:abc' }],
        { abc: '2026-04-16T00:00:00.000Z' },
      ),
    ).toEqual([]);
  });
});

describe('reconcileIntoConnectedProjectSet', () => {
  const mockListIdb = vi.mocked(projectStorage.listProjects);
  const mockIsConnected = vi.mocked(projectSetService.isConnected);
  const mockListSet = vi.mocked(projectSetService.listProjects);
  const mockAddBulk = vi.mocked(projectSetService.addProjectsBulk);
  const mockGetTombstones = vi.mocked(projectSetService.getRootTombstones);
  const mockDelete = vi.mocked(projectStorage.deleteProjectByIndexDocId);

  beforeEach(() => {
    vi.clearAllMocks();
    mockGetTombstones.mockReturnValue({});
  });

  it('purges tombstone-losing IDB rows and reconciles only the winners', async () => {
    mockIsConnected.mockReturnValue(true);
    mockListIdb.mockResolvedValue([
      idb({ indexDocId: 'automerge:stale', lastAccessed: '2026-04-10T00:00:00.000Z' }),
      idb({ indexDocId: 'automerge:fresh', lastAccessed: '2026-04-17T00:00:00.000Z' }),
    ] as never);
    mockListSet.mockReturnValue([]);
    mockGetTombstones.mockReturnValue({ stale: '2026-04-16T00:00:00.000Z' });
    mockAddBulk.mockReturnValue(1);

    const added = await reconcileIntoConnectedProjectSet();

    expect(mockDelete).toHaveBeenCalledTimes(1);
    expect(mockDelete).toHaveBeenCalledWith('automerge:stale');
    expect(mockAddBulk).toHaveBeenCalledWith([
      expect.objectContaining({ indexDocId: 'automerge:fresh' }),
    ]);
    expect(added).toBe(1);
  });

  it('does not purge when the set is not connected', async () => {
    mockIsConnected.mockReturnValue(false);

    const added = await reconcileIntoConnectedProjectSet();

    expect(added).toBe(0);
    expect(mockDelete).not.toHaveBeenCalled();
    expect(mockAddBulk).not.toHaveBeenCalled();
  });
});
