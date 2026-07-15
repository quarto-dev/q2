/**
 * Local-first project-set bootstrap (A4, bd-u4p8xhdc).
 *
 * With no auth and no server, the app must open straight into a usable
 * selector: when there is no project-set pointer and no legacy projects,
 * useProjectSet auto-creates a *local* project set (no sync server) and
 * reaches the `connected` state — never `needs-setup`, which would have
 * forced a server-backed setup screen behind a login gate.
 *
 * Plan: claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor, cleanup } from '@testing-library/react';

const { pointerStore } = vi.hoisted(() => ({
  pointerStore: { current: null as { projectSetDocId: string; syncServer: string } | null },
}));

vi.mock('../services/projectSetStorage', () => ({
  getProjectSetPointer: vi.fn(async () => pointerStore.current),
  setProjectSetPointer: vi.fn(async (projectSetDocId: string, syncServer: string) => {
    pointerStore.current = { projectSetDocId, syncServer };
  }),
}));

vi.mock('../services/projectStorage', () => ({
  listProjects: vi.fn(async () => []),
}));

vi.mock('../services/projectSetReconciler', () => ({
  reconcileIntoConnectedProjectSet: vi.fn(async () => 0),
}));

const {
  createLocalProjectSetMock,
  connectMock,
  connectLocalMock,
  listProjectsMock,
} = vi.hoisted(() => ({
  createLocalProjectSetMock: vi.fn(async () => 'automerge:localSet'),
  connectMock: vi.fn(async () => []),
  connectLocalMock: vi.fn(async () => []),
  listProjectsMock: vi.fn(() => []),
}));

vi.mock('../services/projectSetService', () => ({
  setProjectSetHandlers: vi.fn(),
  connect: connectMock,
  connectLocal: connectLocalMock,
  createLocalProjectSet: createLocalProjectSetMock,
  createProjectSet: vi.fn(),
  listProjects: listProjectsMock,
  addProject: vi.fn(),
  removeProject: vi.fn(),
  updateProjectDescription: vi.fn(),
  touchProject: vi.fn(),
  addProjectsBulk: vi.fn(),
  getProjectSetDocId: vi.fn(() => 'automerge:localSet'),
  disconnect: vi.fn(async () => {}),
}));

import { useProjectSet } from './useProjectSet';
import * as projectSetStorage from '../services/projectSetStorage';

describe('useProjectSet local-first bootstrap', () => {
  beforeEach(() => {
    pointerStore.current = null;
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it('auto-creates a local project set when there is no pointer and no legacy', async () => {
    const { result } = renderHook(() => useProjectSet());

    await waitFor(() => expect(result.current[0].status).toBe('connected'));

    expect(createLocalProjectSetMock).toHaveBeenCalledTimes(1);
    // The local pointer is persisted with an empty (local) sync server.
    expect(projectSetStorage.setProjectSetPointer).toHaveBeenCalledWith('automerge:localSet', '');
    // No networked connect was attempted.
    expect(connectMock).not.toHaveBeenCalled();
    // The set reports as local (no sync server).
    expect(result.current[1].getSyncServer()).toBe('');
  });

  it('opens an existing local pointer via connectLocal, not the networked connect', async () => {
    pointerStore.current = { projectSetDocId: 'automerge:existingLocal', syncServer: '' };

    const { result } = renderHook(() => useProjectSet());
    await waitFor(() => expect(result.current[0].status).toBe('connected'));

    expect(connectLocalMock).toHaveBeenCalledWith('automerge:existingLocal');
    expect(connectMock).not.toHaveBeenCalled();
    expect(createLocalProjectSetMock).not.toHaveBeenCalled();
  });
});
