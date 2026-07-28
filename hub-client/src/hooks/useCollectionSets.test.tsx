/**
 * @vitest-environment jsdom
 */

import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../services/projectSetStorage', () => ({
  getCollectionPointers: vi.fn(),
  addCollectionPointer: vi.fn(),
  removeCollectionPointer: vi.fn(),
  setProjectSetPointer: vi.fn(),
}));

vi.mock('../services/projectStorage', () => ({
  listProjects: vi.fn(),
}));

vi.mock('../services/projectSetReconciler', () => ({
  reconcileIntoConnectedProjectSet: vi.fn(async () => 0),
}));

vi.mock('../services/projectSetService', () => ({
  setProjectSetHandlers: vi.fn(),
  connectCollections: vi.fn(),
  connectCollection: vi.fn(),
  createProjectSet: vi.fn(),
  createCollection: vi.fn(),
  listCollections: vi.fn(),
  listProjects: vi.fn(() => []),
  addProjectsBulk: vi.fn(),
  renameCollection: vi.fn(),
  addProject: vi.fn(),
  removeProjectFromCollection: vi.fn(),
  updateProjectDescriptionEverywhere: vi.fn(),
  updateProjectSummaryEverywhere: vi.fn(),
  touchProjectEverywhere: vi.fn(),
  getProjectSetDocId: vi.fn(),
  disconnectCollection: vi.fn(),
  addProjectToCollection: vi.fn(),
  moveProjectBetweenCollections: vi.fn(),
}));

import { useCollectionSets } from './useCollectionSets';
import * as projectSetStorage from '../services/projectSetStorage';
import * as projectStorage from '../services/projectStorage';
import * as projectSetService from '../services/projectSetService';

const mockGetCollectionPointers = vi.mocked(
  projectSetStorage.getCollectionPointers,
);
const mockAddCollectionPointer = vi.mocked(
  projectSetStorage.addCollectionPointer,
);
const mockSetProjectSetPointer = vi.mocked(
  projectSetStorage.setProjectSetPointer,
);
const mockListLegacyProjects = vi.mocked(projectStorage.listProjects);
const mockCreateProjectSet = vi.mocked(projectSetService.createProjectSet);
const mockCreateCollection = vi.mocked(projectSetService.createCollection);
const mockListCollections = vi.mocked(projectSetService.listCollections);

const rootSnapshot = {
  docId: 'offline-root-doc',
  syncServer: 'wss://offline.example/ws',
  name: 'My projects',
  entries: [],
  isRoot: true,
};

describe('useCollectionSets setup creation policy', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    const localValues = new Map<string, string>();
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: {
        get length() {
          return localValues.size;
        },
        clear: () => localValues.clear(),
        getItem: (key: string) => localValues.get(key) ?? null,
        key: (index: number) => [...localValues.keys()][index] ?? null,
        removeItem: (key: string) => {
          localValues.delete(key);
        },
        setItem: (key: string, value: string) => {
          localValues.set(key, value);
        },
      } satisfies Storage,
    });
    mockGetCollectionPointers.mockResolvedValue([]);
    mockListLegacyProjects.mockResolvedValue([]);
    mockCreateProjectSet.mockResolvedValue('offline-root-doc');
    mockListCollections.mockReturnValue([rootSnapshot]);
  });

  it('uses the offline-capable personal-root wrapper and stores both configured-server pointers', async () => {
    const { result } = renderHook(() => useCollectionSets());
    await waitFor(() => expect(result.current[0].status).toBe('needs-setup'));

    await act(async () => {
      await result.current[1].createProjectSet('wss://offline.example/ws');
    });

    expect(mockCreateProjectSet).toHaveBeenCalledWith(
      'wss://offline.example/ws',
      'My projects',
    );
    expect(mockCreateCollection).not.toHaveBeenCalled();
    expect(mockAddCollectionPointer).toHaveBeenCalledWith({
      projectSetDocId: 'offline-root-doc',
      syncServer: 'wss://offline.example/ws',
    });
    expect(mockSetProjectSetPointer).toHaveBeenCalledWith(
      'offline-root-doc',
      'wss://offline.example/ws',
    );
    expect(result.current[0].status).toBe('connected');
    expect(result.current[1].getSyncServer()).toBe('wss://offline.example/ws');
  });

  it('keeps migration on server-gated shared collection creation', async () => {
    const legacyProject = {
      id: 'legacy-1',
      indexDocId: 'legacy-index',
      syncServer: 'wss://projects.example/ws',
      description: 'Legacy project',
      createdAt: '2026-07-01T00:00:00.000Z',
      lastAccessed: '2026-07-01T00:00:00.000Z',
    };
    mockListLegacyProjects.mockResolvedValue([legacyProject]);
    mockCreateCollection.mockRejectedValue(new Error('server offline'));

    const { result } = renderHook(() => useCollectionSets());
    await waitFor(() =>
      expect(result.current[0].status).toBe('needs-migration'),
    );

    await act(async () => {
      await result.current[1].migrateProjects('wss://offline.example/ws');
    });

    expect(mockCreateCollection).toHaveBeenCalledWith(
      'wss://offline.example/ws',
      'My projects',
    );
    expect(mockCreateProjectSet).not.toHaveBeenCalled();
    expect(result.current[0].status).toBe('needs-migration');
    expect(result.current[0].error).toContain('server offline');
  });
});
