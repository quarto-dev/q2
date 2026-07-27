/**
 * Hook for the collections-of-project-sets lifecycle.
 *
 * Successor to useProjectSet (which managed exactly one set document).
 * On mount it reads the collections pointer array from IndexedDB
 * (self-healing from the legacy singleton pointer), connects every
 * collection document, and runs the one-time migration of legacy
 * localStorage collections (qh-collections-v1) into real synced
 * ProjectSetDocuments.
 *
 * The first pointer is the personal root collection: the superset of the
 * user's projects. Other collections reference (a subset of) the same
 * projects; the home view computes "Everything else" as root entries not
 * present in any other collection.
 *
 * See claude-notes/plans/2026-07-10-collections-as-project-sets.md.
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import type { ProjectSetEntry, ProjectSetEntrySummary } from '@quarto/quarto-automerge-schema';
import { projectSetKey } from '@quarto/quarto-automerge-schema';
import {
  getCollectionPointers,
  addCollectionPointer,
  removeCollectionPointer,
  setProjectSetPointer,
} from '../services/projectSetStorage';
import type { CollectionPointerEntry } from '../services/storage/types';
import * as projectSetService from '../services/projectSetService';
import type { CollectionSnapshot } from '../services/projectSetService';
import * as projectStorage from '../services/projectStorage';
import { reconcileIntoConnectedProjectSet } from '../services/projectSetReconciler';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';

// ============================================================================
// Types
// ============================================================================

export type CollectionsStatus =
  | 'loading'           // Reading pointers from IDB
  | 'needs-setup'       // No pointers, no old projects → fresh setup
  | 'needs-migration'   // No pointers, has old IDB projects → migration
  | 'connecting'        // Connecting to collection documents
  | 'connected'         // Root collection connected (others may have failed)
  | 'error';            // Root connection failed

export interface CollectionSetsState {
  status: CollectionsStatus;
  /** All connected collections, root first. */
  collections: CollectionSnapshot[];
  /** Root collection entries (compat with the classic single-set UI). */
  projects: ProjectSetEntry[];
  /** Collections whose documents could not be loaded this session. */
  unreachable: Array<{ pointer: CollectionPointerEntry; error: string }>;
  error: string | null;
  /** Old IDB projects that need migration (only during 'needs-migration'). */
  legacyProjects: ProjectEntry[];
}

export interface CollectionSetsActions {
  // ---- setup / linking (mirrors useProjectSet for the setup screens) ----
  createProjectSet: (syncServer: string) => Promise<void>;
  linkProjectSet: (projectSetDocId: string, syncServer: string) => Promise<void>;
  migrateProjects: (syncServer: string) => Promise<void>;
  mergeIntoProjectSet: (projectSetDocId: string, syncServer: string) => Promise<void>;

  // ---- root-set compat operations ----
  addProject: (entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => void;
  removeProject: (indexDocId: string) => void;
  updateProjectDescription: (indexDocId: string, description: string) => void;
  updateProjectSummary: (indexDocId: string, summary: ProjectSetEntrySummary) => void;
  touchProject: (indexDocId: string) => void;
  getProjectSetDocId: () => string | null;
  getSyncServer: () => string | null;

  // ---- collection operations ----
  /** Create a new (empty) collection on the root's sync server. */
  createCollection: (name: string) => Promise<string>;
  /** Subscribe to an existing collection document (join). */
  subscribeCollection: (projectSetDocId: string, syncServer: string) => Promise<void>;
  /** Unsubscribe (leave): drop the pointer; the document is untouched. */
  unsubscribeCollection: (collectionDocId: string) => Promise<void>;
  renameCollection: (collectionDocId: string, name: string) => void;
  addProjectToCollection: (collectionDocId: string, entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => void;
  removeProjectFromCollection: (collectionDocId: string, indexDocId: string) => void;
  moveProjectBetweenCollections: (fromDocId: string, toDocId: string, indexDocId: string) => void;
}

// ============================================================================
// localStorage collections migration (phase 3)
// ============================================================================

const LEGACY_LOCAL_KEY = 'qh-collections-v1';
const LEGACY_LOCAL_MIGRATED_KEY = 'qh-collections-v1-migrated';
const DEFAULT_ROOT_NAME = 'My projects';

interface LegacyLocalCollection {
  id: string;
  name: string;
  projectIds: string[];
}

/**
 * Convert legacy localStorage collections into real collection documents.
 * One-way and idempotent: the original JSON is preserved under a
 * `-migrated` key, never re-imported. Entries are matched against the
 * root set; ids that no longer resolve are skipped.
 */
async function migrateLocalCollections(rootServer: string): Promise<void> {
  const raw = localStorage.getItem(LEGACY_LOCAL_KEY);
  if (!raw) return;

  let parsed: LegacyLocalCollection[];
  try {
    parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) throw new Error('not an array');
  } catch {
    localStorage.setItem(LEGACY_LOCAL_MIGRATED_KEY, raw);
    localStorage.removeItem(LEGACY_LOCAL_KEY);
    return;
  }

  const rootEntries = projectSetService.listProjects();
  const byKey = new Map(rootEntries.map((e) => [projectSetKey(e.indexDocId), e]));

  for (const local of parsed) {
    if (typeof local?.name !== 'string' || !Array.isArray(local?.projectIds)) continue;
    const docId = await projectSetService.createCollection(rootServer, local.name);
    const entries = local.projectIds
      .map((id) => byKey.get(projectSetKey(id)))
      .filter((e): e is ProjectSetEntry => !!e);
    if (entries.length > 0) {
      projectSetService.addProjectsBulk(
        entries.map((e) => ({
          indexDocId: e.indexDocId,
          syncServer: e.syncServer,
          description: e.description,
          lastAccessed: e.lastAccessed,
        })),
        docId,
      );
    }
    await addCollectionPointer({ projectSetDocId: docId, syncServer: rootServer });
  }

  localStorage.setItem(LEGACY_LOCAL_MIGRATED_KEY, raw);
  localStorage.removeItem(LEGACY_LOCAL_KEY);
  console.log(`[collections] migrated ${parsed.length} local collection(s) to synced documents`);
}

// ============================================================================
// Hook
// ============================================================================

export function useCollectionSets(): [CollectionSetsState, CollectionSetsActions] {
  const [status, setStatus] = useState<CollectionsStatus>('loading');
  const [collections, setCollections] = useState<CollectionSnapshot[]>([]);
  const [unreachable, setUnreachable] = useState<CollectionSetsState['unreachable']>([]);
  const [error, setError] = useState<string | null>(null);
  const [legacyProjects, setLegacyProjects] = useState<ProjectEntry[]>([]);
  const syncServerRef = useRef<string | null>(null);
  const initRef = useRef(false);

  // Subscribe to service-side changes (local edits and remote sync)
  useEffect(() => {
    projectSetService.setProjectSetHandlers({
      onCollectionsChange: (snapshots) => setCollections(snapshots),
    });
  }, []);

  // Reconcile share-route IDB writes into the root set once connected
  useEffect(() => {
    if (status !== 'connected') return;
    let cancelled = false;
    (async () => {
      try {
        const added = await reconcileIntoConnectedProjectSet();
        if (!cancelled && added > 0) {
          setCollections(projectSetService.listCollections());
        }
      } catch (err) {
        console.error('[collections] reconciliation failed:', err);
      }
    })();
    return () => { cancelled = true; };
  }, [status]);

  /** Connect everything from the pointer array; run local migration. */
  const connectAll = useCallback(async (pointers: CollectionPointerEntry[]) => {
    setStatus('connecting');
    syncServerRef.current = pointers[0]?.syncServer ?? null;
    const { connected, failed } = await projectSetService.connectCollections(pointers);
    const rootFailed = failed.some((f) => f.pointer.projectSetDocId === pointers[0]?.projectSetDocId);
    if (rootFailed || connected.length === 0) {
      setError(failed[0]?.error ?? 'Failed to connect');
      setStatus('error');
      return;
    }
    setUnreachable(failed);

    // Name the root when it has none (pre-collections document)
    const root = connected[0];
    if (root && !root.name) {
      projectSetService.renameCollection(root.docId, DEFAULT_ROOT_NAME);
    }

    // One-time legacy localStorage collections migration
    try {
      await migrateLocalCollections(pointers[0].syncServer);
    } catch (err) {
      console.error('[collections] local migration failed (will retry next load):', err);
    }

    setCollections(projectSetService.listCollections());
    setStatus('connected');
  }, []);

  // Initialize on mount
  useEffect(() => {
    if (initRef.current) return;
    initRef.current = true;

    (async () => {
      try {
        const pointers = await getCollectionPointers();
        if (pointers.length > 0) {
          await connectAll(pointers);
          return;
        }
        const legacy = await projectStorage.listProjects();
        if (legacy.length > 0) {
          setLegacyProjects(legacy);
          setStatus('needs-migration');
        } else {
          setStatus('needs-setup');
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
        setStatus('error');
      }
    })();
  }, [connectAll]);

  // ---- setup actions ----

  /** Establish a brand-new root collection and record both pointers
   * (array + legacy singleton, the latter as a safety net). */
  const establishRoot = useCallback(async (docId: string, syncServer: string) => {
    await addCollectionPointer({ projectSetDocId: docId, syncServer });
    await setProjectSetPointer(docId, syncServer);
    syncServerRef.current = syncServer;
  }, []);

  const createProjectSet = useCallback(async (syncServer: string) => {
    setStatus('connecting');
    setError(null);
    try {
      const docId = await projectSetService.createCollection(syncServer, DEFAULT_ROOT_NAME);
      await establishRoot(docId, syncServer);
      await migrateLocalCollections(syncServer);
      setCollections(projectSetService.listCollections());
      setStatus('connected');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStatus('error');
    }
  }, [establishRoot]);

  const linkProjectSet = useCallback(async (projectSetDocId: string, syncServer: string) => {
    setStatus('connecting');
    setError(null);
    try {
      await projectSetService.connectCollection({ projectSetDocId, syncServer });
      await establishRoot(projectSetDocId, syncServer);
      await migrateLocalCollections(syncServer);
      setCollections(projectSetService.listCollections());
      setStatus('connected');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStatus('error');
    }
  }, [establishRoot]);

  const migrateProjects = useCallback(async (syncServer: string) => {
    setStatus('connecting');
    setError(null);
    try {
      const docId = await projectSetService.createCollection(syncServer, DEFAULT_ROOT_NAME);
      const legacy = await projectStorage.listProjects();
      if (legacy.length > 0) {
        const added = projectSetService.addProjectsBulk(
          legacy.map((p) => ({
            indexDocId: p.indexDocId,
            syncServer: p.syncServer,
            description: p.description,
            lastAccessed: p.lastAccessed,
          })),
          docId,
        );
        console.log(`Migrated ${added} project(s) to the root collection`);
      }
      await establishRoot(docId, syncServer);
      await migrateLocalCollections(syncServer);
      setCollections(projectSetService.listCollections());
      setLegacyProjects([]);
      setStatus('connected');
    } catch (err) {
      setError(
        'Could not reach sync server — your projects are safe, migration will retry automatically. ' +
        (err instanceof Error ? err.message : String(err)),
      );
      setStatus('needs-migration');
    }
  }, [establishRoot]);

  const mergeIntoProjectSet = useCallback(async (projectSetDocId: string, syncServer: string) => {
    setStatus('connecting');
    setError(null);
    try {
      await projectSetService.connectCollection({ projectSetDocId, syncServer });
      const legacy = await projectStorage.listProjects();
      if (legacy.length > 0) {
        const added = projectSetService.addProjectsBulk(
          legacy.map((p) => ({
            indexDocId: p.indexDocId,
            syncServer: p.syncServer,
            description: p.description,
            lastAccessed: p.lastAccessed,
          })),
          projectSetDocId,
        );
        console.log(`Merged ${added} project(s) into the root collection`);
      }
      await establishRoot(projectSetDocId, syncServer);
      await migrateLocalCollections(syncServer);
      setCollections(projectSetService.listCollections());
      setLegacyProjects([]);
      setStatus('connected');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStatus('needs-migration');
    }
  }, [establishRoot]);

  // ---- root-set compat operations ----

  const refresh = useCallback(() => {
    setCollections(projectSetService.listCollections());
  }, []);

  const addProject = useCallback((entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => {
    projectSetService.addProject(entry);
    refresh();
  }, [refresh]);

  const removeProject = useCallback((indexDocId: string) => {
    // Personal removal: root plus every subscribed collection this browser
    // can see. Other members of shared collections are unaffected (their
    // own root supersets still hold the project).
    for (const c of projectSetService.listCollections()) {
      projectSetService.removeProjectFromCollection(c.docId, indexDocId);
    }
    refresh();
  }, [refresh]);

  const updateProjectDescription = useCallback((indexDocId: string, description: string) => {
    projectSetService.updateProjectDescriptionEverywhere(indexDocId, description);
    refresh();
  }, [refresh]);

  const updateProjectSummary = useCallback((indexDocId: string, summary: ProjectSetEntrySummary) => {
    if (projectSetService.updateProjectSummaryEverywhere(indexDocId, summary)) {
      refresh();
    }
  }, [refresh]);

  const touchProject = useCallback((indexDocId: string) => {
    projectSetService.touchProjectEverywhere(indexDocId);
  }, []);

  const getProjectSetDocId = useCallback(() => projectSetService.getProjectSetDocId(), []);
  const getSyncServer = useCallback(() => syncServerRef.current, []);

  // ---- collection operations ----

  const createCollection = useCallback(async (name: string): Promise<string> => {
    const server = syncServerRef.current;
    if (!server) throw new Error('Not connected');
    const docId = await projectSetService.createCollection(server, name);
    await addCollectionPointer({ projectSetDocId: docId, syncServer: server });
    refresh();
    return docId;
  }, [refresh]);

  const subscribeCollection = useCallback(async (projectSetDocId: string, syncServer: string) => {
    await projectSetService.connectCollection({ projectSetDocId, syncServer });
    await addCollectionPointer({ projectSetDocId, syncServer });
    refresh();
  }, [refresh]);

  const unsubscribeCollection = useCallback(async (collectionDocId: string) => {
    projectSetService.disconnectCollection(collectionDocId);
    await removeCollectionPointer(collectionDocId);
    refresh();
  }, [refresh]);

  const renameCollection = useCallback((collectionDocId: string, name: string) => {
    projectSetService.renameCollection(collectionDocId, name);
    refresh();
  }, [refresh]);

  const addProjectToCollection = useCallback((collectionDocId: string, entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => {
    projectSetService.addProjectToCollection(collectionDocId, entry);
    refresh();
  }, [refresh]);

  const removeProjectFromCollection = useCallback((collectionDocId: string, indexDocId: string) => {
    projectSetService.removeProjectFromCollection(collectionDocId, indexDocId);
    refresh();
  }, [refresh]);

  const moveProjectBetweenCollections = useCallback((fromDocId: string, toDocId: string, indexDocId: string) => {
    projectSetService.moveProjectBetweenCollections(fromDocId, toDocId, indexDocId);
    refresh();
  }, [refresh]);

  const root = collections.find((c) => c.isRoot);
  const state: CollectionSetsState = {
    status,
    collections,
    projects: root?.entries ?? [],
    unreachable,
    error,
    legacyProjects,
  };

  const actions: CollectionSetsActions = {
    createProjectSet,
    linkProjectSet,
    migrateProjects,
    mergeIntoProjectSet,
    addProject,
    removeProject,
    updateProjectDescription,
    updateProjectSummary,
    touchProject,
    getProjectSetDocId,
    getSyncServer,
    createCollection,
    subscribeCollection,
    unsubscribeCollection,
    renameCollection,
    addProjectToCollection,
    removeProjectFromCollection,
    moveProjectBetweenCollections,
  };

  return [state, actions];
}
