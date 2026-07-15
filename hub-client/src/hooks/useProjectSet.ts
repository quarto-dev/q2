/**
 * Hook for managing the project set connection lifecycle.
 *
 * On mount, checks IndexedDB for a project set pointer:
 * - If found: connects to the Automerge-backed project set document
 * - If not found: signals that setup is needed (first-time or migration)
 *
 * Exposes the project set state and operations to the rest of the app.
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import type { ProjectSetEntry } from '@quarto/quarto-automerge-schema';
import {
  getProjectSetPointer,
  setProjectSetPointer,
} from '../services/projectSetStorage';
import * as projectSetService from '../services/projectSetService';
import * as projectStorage from '../services/projectStorage';
import { reconcileIntoConnectedProjectSet } from '../services/projectSetReconciler';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';

// ============================================================================
// Types
// ============================================================================

export type ProjectSetStatus =
  | 'loading'           // Checking IDB for pointer
  | 'needs-setup'       // No pointer, no old projects → fresh setup
  | 'needs-migration'   // No pointer, has old projects → migration
  | 'connecting'        // Pointer found, connecting to Automerge
  | 'connected'         // Connected to project set
  | 'error';            // Connection failed

export interface ProjectSetState {
  status: ProjectSetStatus;
  projects: ProjectSetEntry[];
  error: string | null;
  /** Old IDB projects that need migration (only set during 'needs-migration'). */
  legacyProjects: ProjectEntry[];
}

export interface ProjectSetActions {
  /** Create a new project set and store the pointer. */
  createProjectSet: (syncServer: string) => Promise<void>;
  /** Link to an existing project set (from another browser). */
  linkProjectSet: (projectSetDocId: string, syncServer: string) => Promise<void>;
  /** Migrate old IDB projects into a new project set. */
  migrateProjects: (syncServer: string) => Promise<void>;
  /** Merge old IDB projects into an existing project set. */
  mergeIntoProjectSet: (projectSetDocId: string, syncServer: string) => Promise<void>;
  /** Add a project to the connected set. */
  addProject: (entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => void;
  /** Remove a project from the set. */
  removeProject: (indexDocId: string) => void;
  /** Update a project's description. */
  updateProjectDescription: (indexDocId: string, description: string) => void;
  /** Touch a project (update lastAccessed). */
  touchProject: (indexDocId: string) => void;
  /** Get the connected project set document ID. */
  getProjectSetDocId: () => string | null;
  /** Get the sync server URL for the project set. */
  getSyncServer: () => string | null;
}

// ============================================================================
// Hook
// ============================================================================

export function useProjectSet(): [ProjectSetState, ProjectSetActions] {
  const [status, setStatus] = useState<ProjectSetStatus>('loading');
  const [projects, setProjects] = useState<ProjectSetEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [legacyProjects, setLegacyProjects] = useState<ProjectEntry[]>([]);
  const syncServerRef = useRef<string | null>(null);
  const initRef = useRef(false);

  // Subscribe to remote changes
  useEffect(() => {
    projectSetService.setProjectSetHandlers({
      onProjectsChange: (newProjects) => {
        setProjects(newProjects);
      },
    });
  }, []);

  // Reconcile local IDB entries into the synced set whenever we become
  // connected. This closes the race window in the share-route handler: if a
  // share link wrote a project to IDB before the set was connected, the
  // reconciler picks it up here. Idempotent and safe to re-run.
  useEffect(() => {
    if (status !== 'connected') return;
    let cancelled = false;
    (async () => {
      try {
        const added = await reconcileIntoConnectedProjectSet();
        if (!cancelled && added > 0) {
          setProjects(projectSetService.listProjects());
        }
      } catch (err) {
        // Non-fatal: reconciliation is a best-effort self-heal.
        console.error('[project-set] reconciliation failed:', err);
      }
    })();
    return () => { cancelled = true; };
  }, [status]);

  // Initialize on mount
  useEffect(() => {
    if (initRef.current) return;
    initRef.current = true;

    const init = async () => {
      try {
        const pointer = await getProjectSetPointer();

        if (pointer) {
          // Have a pointer — connect to the project set. An empty syncServer
          // marks a local-only set: open it from the cache with no network.
          setStatus('connecting');
          syncServerRef.current = pointer.syncServer;
          const entries = pointer.syncServer
            ? await projectSetService.connect(
                pointer.syncServer,
                pointer.projectSetDocId,
              )
            : await projectSetService.connectLocal(pointer.projectSetDocId);
          setProjects(entries);
          setStatus('connected');
        } else {
          // No pointer — check for legacy projects
          const legacy = await projectStorage.listProjects();
          if (legacy.length > 0) {
            setLegacyProjects(legacy);
            setStatus('needs-migration');
          } else {
            // Local-first (bd-u4p8xhdc): no pointer and nothing to migrate →
            // auto-create a local project set so the app opens straight into
            // a usable selector with no login and no server. The set is
            // minted client-side and lives in the local cache; its pointer
            // records an empty syncServer to mark it local.
            const docId = await projectSetService.createLocalProjectSet();
            await setProjectSetPointer(docId, '');
            syncServerRef.current = '';
            setProjects([]);
            setStatus('connected');
          }
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
        setStatus('error');
      }
    };

    init();
  }, []);

  // ---- Actions ----

  const createProjectSet = useCallback(async (syncServer: string) => {
    setStatus('connecting');
    setError(null);
    try {
      const docId = await projectSetService.createProjectSet(syncServer);
      await setProjectSetPointer(docId, syncServer);
      syncServerRef.current = syncServer;
      setProjects([]);
      setStatus('connected');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStatus('error');
    }
  }, []);

  const linkProjectSet = useCallback(async (projectSetDocId: string, syncServer: string) => {
    setStatus('connecting');
    setError(null);
    try {
      const entries = await projectSetService.connect(syncServer, projectSetDocId);
      await setProjectSetPointer(projectSetDocId, syncServer);
      syncServerRef.current = syncServer;
      setProjects(entries);
      setStatus('connected');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStatus('error');
    }
  }, []);

  const migrateProjects = useCallback(async (syncServer: string) => {
    setStatus('connecting');
    setError(null);
    try {
      // Step 1: Create a new project set document on the sync server
      const docId = await projectSetService.createProjectSet(syncServer);

      // Step 2: Populate it with entries from the old IDB store
      const legacy = await projectStorage.listProjects();
      if (legacy.length > 0) {
        const added = projectSetService.addProjectsBulk(
          legacy.map((p) => ({
            indexDocId: p.indexDocId,
            syncServer: p.syncServer,
            description: p.description,
            lastAccessed: p.lastAccessed,
          })),
        );
        console.log(`Migrated ${added} project(s) to synced project set`);
      }

      // Step 3: Store the pointer — this is the commit point.
      // Only after this succeeds is the migration complete.
      await setProjectSetPointer(docId, syncServer);
      syncServerRef.current = syncServer;

      // Step 4: Update local state
      setProjects(projectSetService.listProjects());
      setLegacyProjects([]);
      setStatus('connected');
    } catch (err) {
      // Migration failed — IDB is unchanged (pointer was not written)
      setError(
        'Could not reach sync server — your projects are safe, migration will retry automatically. ' +
        (err instanceof Error ? err.message : String(err))
      );
      setStatus('needs-migration');
    }
  }, []);

  const mergeIntoProjectSet = useCallback(async (projectSetDocId: string, syncServer: string) => {
    setStatus('connecting');
    setError(null);
    try {
      // Step 1: Connect to the existing project set
      await projectSetService.connect(syncServer, projectSetDocId);

      // Step 2: Merge local IDB projects into it
      const legacy = await projectStorage.listProjects();
      if (legacy.length > 0) {
        const added = projectSetService.addProjectsBulk(
          legacy.map((p) => ({
            indexDocId: p.indexDocId,
            syncServer: p.syncServer,
            description: p.description,
            lastAccessed: p.lastAccessed,
          })),
        );
        console.log(`Merged ${added} project(s) into existing project set`);
      }

      // Step 3: Store the pointer
      await setProjectSetPointer(projectSetDocId, syncServer);
      syncServerRef.current = syncServer;

      // Step 4: Update local state
      setProjects(projectSetService.listProjects());
      setLegacyProjects([]);
      setStatus('connected');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStatus('needs-migration');
    }
  }, []);

  const addProject = useCallback((entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => {
    projectSetService.addProject(entry);
    setProjects(projectSetService.listProjects());
  }, []);

  const removeProject = useCallback((indexDocId: string) => {
    projectSetService.removeProject(indexDocId);
    setProjects(projectSetService.listProjects());
  }, []);

  const updateProjectDescription = useCallback((indexDocId: string, description: string) => {
    projectSetService.updateProjectDescription(indexDocId, description);
    setProjects(projectSetService.listProjects());
  }, []);

  const touchProject = useCallback((indexDocId: string) => {
    projectSetService.touchProject(indexDocId);
    // Don't update projects list for touch — it's a minor metadata update
  }, []);

  const getProjectSetDocId = useCallback(() => {
    return projectSetService.getProjectSetDocId();
  }, []);

  const getSyncServer = useCallback(() => {
    return syncServerRef.current;
  }, []);

  const state: ProjectSetState = { status, projects, error, legacyProjects };
  const actions: ProjectSetActions = {
    createProjectSet,
    linkProjectSet,
    migrateProjects,
    mergeIntoProjectSet,
    addProject,
    removeProject,
    updateProjectDescription,
    touchProject,
    getProjectSetDocId,
    getSyncServer,
  };

  return [state, actions];
}
