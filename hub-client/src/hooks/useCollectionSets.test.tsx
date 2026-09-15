/**
 * @vitest-environment jsdom
 *
 * useCollectionSets.retry(): after a failed root connect (status 'error'),
 * re-read the pointers and re-enter the state machine, so the retry card
 * that replaced the setup form (bd-4h1hv60p) has something to call.
 *
 * Plan: claude-notes/plans/2026-09-15-auto-create-project-set-on-first-run.md
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, cleanup, waitFor } from '@testing-library/react';

vi.mock('../services/projectSetStorage', () => ({
  getCollectionPointers: vi.fn(),
  addCollectionPointer: vi.fn(async () => {}),
  removeCollectionPointer: vi.fn(async () => {}),
  setProjectSetPointer: vi.fn(async () => {}),
}));
vi.mock('../services/projectSetService', () => ({
  setProjectSetHandlers: vi.fn(),
  connectCollections: vi.fn(),
  connectCollection: vi.fn(),
  createCollection: vi.fn(),
  listCollections: vi.fn(() => []),
  listProjects: vi.fn(() => []),
  renameCollection: vi.fn(),
  getProjectSetDocId: vi.fn(() => null),
  addProjectsBulk: vi.fn(() => 0),
}));
vi.mock('../services/projectStorage', () => ({
  listProjects: vi.fn(async () => []),
}));
vi.mock('../services/projectSetReconciler', () => ({
  reconcileIntoConnectedProjectSet: vi.fn(async () => 0),
  importProjectsAndReconcile: vi.fn(),
}));

import { useCollectionSets } from './useCollectionSets';
import * as projectSetStorage from '../services/projectSetStorage';
import * as projectSetService from '../services/projectSetService';

const ROOT = { projectSetDocId: 'automerge:root', syncServer: 'ws://hub/ws' };
const rootSnapshot = {
  docId: ROOT.projectSetDocId,
  syncServer: ROOT.syncServer,
  name: 'My projects',
  entries: [],
  isRoot: true,
};

beforeEach(() => {
  vi.mocked(projectSetStorage.getCollectionPointers).mockResolvedValue([ROOT]);
  vi.mocked(projectSetService.listCollections).mockReturnValue([rootSnapshot]);
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('useCollectionSets.retry', () => {
  it('re-reads the pointers and connects after a failed root connect', async () => {
    vi.mocked(projectSetService.connectCollections)
      .mockResolvedValueOnce({
        connected: [],
        failed: [{ pointer: ROOT, error: 'sync server unreachable' }],
      })
      .mockResolvedValueOnce({ connected: [rootSnapshot], failed: [] });

    const { result } = renderHook(() => useCollectionSets());

    await waitFor(() => expect(result.current[0].status).toBe('error'));
    expect(result.current[0].error).toBe('sync server unreachable');

    await act(async () => {
      await result.current[1].retry();
    });

    await waitFor(() => expect(result.current[0].status).toBe('connected'));
    expect(result.current[0].error).toBeNull();
    expect(projectSetStorage.getCollectionPointers).toHaveBeenCalledTimes(2);
    expect(projectSetService.connectCollections).toHaveBeenCalledTimes(2);
  });

  it('passes through loading so a fresh browser re-enters needs-setup', async () => {
    // A failed silent create leaves status 'error' with no pointer written;
    // retry must land back on needs-setup so the auto-establish hook can
    // fire again (it re-arms whenever status passes through 'loading').
    vi.mocked(projectSetStorage.getCollectionPointers).mockResolvedValue([]);

    const { result } = renderHook(() => useCollectionSets());
    await waitFor(() => expect(result.current[0].status).toBe('needs-setup'));

    vi.mocked(projectSetService.createCollection).mockRejectedValueOnce(new Error('offline'));
    await act(async () => {
      await result.current[1].createProjectSet('ws://hub/ws');
    });
    expect(result.current[0].status).toBe('error');

    // Kick off the retry without flushing its continuation: the synchronous
    // part must already have published 'loading', which is what re-arms
    // useAutoEstablishRoot.
    let pending!: Promise<void>;
    act(() => {
      pending = result.current[1].retry();
    });
    expect(result.current[0].status).toBe('loading');

    await act(async () => {
      await pending;
    });
    await waitFor(() => expect(result.current[0].status).toBe('needs-setup'));
    expect(result.current[0].error).toBeNull();
  });
});
