/**
 * Project Set Service
 *
 * Manages the Automerge-backed project set document that stores a user's
 * project list. This replaces the per-browser IndexedDB project list,
 * enabling cross-browser sync of the project collection.
 *
 * The service manages its own Automerge Repo connection, independent from
 * the per-project SyncClient used for collaborative editing.
 */

import { Repo } from '@automerge/automerge-repo';
import type { DocHandle, DocumentId } from '@automerge/automerge-repo';
import { from as automergeFrom, save as automergeSerialize } from '@automerge/automerge';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import { resolveSyncServerUrl } from '../utils/routing';

import type {
  ProjectSetDocument,
  ProjectSetEntry,
} from '@quarto/quarto-automerge-schema';
import {
  CURRENT_PROJECT_SET_SCHEMA_VERSION,
  addProjectToSet,
  removeProjectFromSet,
  touchProjectInSet,
  projectSetKey,
} from '@quarto/quarto-automerge-schema';

// ============================================================================
// Types
// ============================================================================

/** Callback fired when the project list changes (local or remote). */
export type ProjectsChangeHandler = (projects: ProjectSetEntry[]) => void;

/** Callback fired when the connection state changes. */
export type ConnectionChangeHandler = (connected: boolean) => void;

// ============================================================================
// Internal State
// ============================================================================

let repo: Repo | null = null;
let wsAdapter: BrowserWebSocketClientAdapter | null = null;
let handle: DocHandle<ProjectSetDocument> | null = null;
let cleanupFn: (() => void) | null = null;

let onProjectsChange: ProjectsChangeHandler | null = null;
let onConnectionChange: ConnectionChangeHandler | null = null;

// ============================================================================
// Event Handlers
// ============================================================================

/**
 * Set callbacks for project set events.
 */
export function setProjectSetHandlers(handlers: {
  onProjectsChange?: ProjectsChangeHandler;
  onConnectionChange?: ConnectionChangeHandler;
}): void {
  if (handlers.onProjectsChange) onProjectsChange = handlers.onProjectsChange;
  if (handlers.onConnectionChange) onConnectionChange = handlers.onConnectionChange;
}

// ============================================================================
// Internal Helpers
// ============================================================================

function getProjectsList(doc: ProjectSetDocument): ProjectSetEntry[] {
  if (!doc.projects) return [];
  return Object.values(doc.projects).sort(
    (a, b) => b.lastAccessed.localeCompare(a.lastAccessed),
  );
}

function notifyProjectsChange(): void {
  if (!handle || !onProjectsChange) return;
  const doc = handle.doc();
  if (doc) {
    onProjectsChange(getProjectsList(doc));
  }
}

function waitForPeer(r: Repo, timeoutMs: number = 5000): Promise<void> {
  return new Promise((resolve, reject) => {
    const timeoutId = setTimeout(() => {
      cleanup();
      reject(new Error('Timeout waiting for peer connection'));
    }, timeoutMs);

    const onPeer = () => {
      cleanup();
      resolve();
    };

    const cleanup = () => {
      clearTimeout(timeoutId);
      r.networkSubsystem.off('peer', onPeer);
    };

    r.networkSubsystem.on('peer', onPeer);
  });
}

// ============================================================================
// Connection Management
// ============================================================================

/**
 * Connect to an existing project set document.
 *
 * Resolves immediately if the document is available in local storage,
 * then syncs with the server in the background.
 *
 * @returns The current list of projects, sorted by lastAccessed (most recent first).
 * @throws If the document cannot be loaded (e.g., first load on a new browser
 *         while offline — the document hasn't been cached locally yet).
 */
export async function connect(
  syncServerUrl: string,
  projectSetDocId: string,
): Promise<ProjectSetEntry[]> {
  await disconnect();

  wsAdapter = new BrowserWebSocketClientAdapter(resolveSyncServerUrl(syncServerUrl));
  repo = new Repo({
    network: [wsAdapter],
    storage: new IndexedDBStorageAdapter(),
  });

  const docId = projectSetDocId as DocumentId;
  const foundHandle = await repo.find<ProjectSetDocument>(docId);
  handle = foundHandle;

  // Check if we have a cached document locally
  const doc = handle.doc();
  if (doc) {
    // Have local data — resolve immediately
    // Subscribe to changes (from remote browsers)
    const onChange = () => notifyProjectsChange();
    handle.on('change', onChange);
    cleanupFn = () => handle?.off('change', onChange);

    // Start background sync (but don't wait for it)
    waitForPeer(repo, 5000)
      .then(() => onConnectionChange?.(true))
      .catch(() => onConnectionChange?.(false));

    return getProjectsList(doc);
  }

  // No local data — need to sync from server first
  let isOnline = false;
  try {
    await waitForPeer(repo, 5000);
    isOnline = true;
  } catch {
    isOnline = false;
  }

  onConnectionChange?.(isOnline);

  await foundHandle.whenReady();

  const syncedDoc = handle.doc();
  if (!syncedDoc) {
    throw new Error(
      isOnline
        ? 'Failed to load project set document'
        : 'Project set not found in local storage. Connect online first to sync.',
    );
  }

  // Subscribe to changes (from remote browsers)
  const onChange = () => notifyProjectsChange();
  handle.on('change', onChange);
  cleanupFn = () => handle?.off('change', onChange);

  return getProjectsList(syncedDoc);
}

/**
 * Create a new project set document on the sync server.
 *
 * @returns The document ID of the newly created ProjectSetDocument.
 * @throws If the sync server is unreachable.
 */
export async function createProjectSet(
  syncServerUrl: string,
): Promise<string> {
  await disconnect();

  wsAdapter = new BrowserWebSocketClientAdapter(resolveSyncServerUrl(syncServerUrl));
  repo = new Repo({
    network: [wsAdapter],
    storage: new IndexedDBStorageAdapter(),
  });

  // For creation, we need the server to be reachable so the document
  // gets synced. Without this, the document would only exist locally.
  try {
    await waitForPeer(repo, 10000);
  } catch {
    await disconnect();
    throw new Error(
      'Could not reach sync server. Please check your connection and try again.',
    );
  }

  onConnectionChange?.(true);

  // Create the document
  const initial = {
    projects: {},
    version: CURRENT_PROJECT_SET_SCHEMA_VERSION,
  } as Record<string, unknown>;
  const doc = automergeFrom(initial);
  handle = repo.import<ProjectSetDocument>(automergeSerialize(doc));

  // Subscribe to changes
  const onChange = () => notifyProjectsChange();
  handle.on('change', onChange);
  cleanupFn = () => handle?.off('change', onChange);

  return handle.documentId;
}

/**
 * Disconnect from the project set sync.
 */
export async function disconnect(): Promise<void> {
  if (cleanupFn) {
    cleanupFn();
    cleanupFn = null;
  }
  handle = null;
  if (wsAdapter) {
    wsAdapter.disconnect();
    wsAdapter = null;
  }
  repo = null;
}

/**
 * Check if currently connected to a project set.
 */
export function isConnected(): boolean {
  return handle !== null;
}

// ============================================================================
// Project CRUD Operations
// ============================================================================

/**
 * List all projects in the set, sorted by lastAccessed (most recent first).
 */
export function listProjects(): ProjectSetEntry[] {
  if (!handle) return [];
  const doc = handle.doc();
  if (!doc) return [];
  return getProjectsList(doc);
}

/**
 * Get a single project by its indexDocId.
 */
export function getProject(indexDocId: string): ProjectSetEntry | undefined {
  if (!handle) return undefined;
  const doc = handle.doc();
  if (!doc) return undefined;
  const key = projectSetKey(indexDocId);
  return doc.projects[key];
}

/**
 * Add a project to the set.
 */
export function addProject(
  entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>,
): void {
  if (!handle) throw new Error('Not connected to a project set');
  handle.change(doc => {
    addProjectToSet(doc, entry);
  });
  notifyProjectsChange();
}

/**
 * Remove a project from the set.
 */
export function removeProject(indexDocId: string): void {
  if (!handle) throw new Error('Not connected to a project set');
  handle.change(doc => {
    removeProjectFromSet(doc, indexDocId);
  });
  notifyProjectsChange();
}

/**
 * Update the description of a project.
 */
export function updateProjectDescription(
  indexDocId: string,
  description: string,
): void {
  if (!handle) throw new Error('Not connected to a project set');
  handle.change(doc => {
    const key = projectSetKey(indexDocId);
    const entry = doc.projects[key];
    if (entry) {
      entry.description = description;
    }
  });
  notifyProjectsChange();
}

/**
 * Update the lastAccessed timestamp for a project.
 */
export function touchProject(indexDocId: string): void {
  if (!handle) throw new Error('Not connected to a project set');
  handle.change(doc => {
    touchProjectInSet(doc, indexDocId);
  });
  // Don't notify for touch — it's a minor metadata update and the caller
  // already knows which project was selected.
}

/**
 * Get the document ID of the connected project set, or null if not connected.
 */
export function getProjectSetDocId(): string | null {
  return handle?.documentId ?? null;
}

// ============================================================================
// Bulk Operations (for migration)
// ============================================================================

/**
 * Add multiple projects to the set in a single Automerge change.
 * Used during migration from IndexedDB to avoid N individual changes.
 *
 * @returns The number of projects actually added (excludes duplicates).
 */
export function addProjectsBulk(
  entries: Array<{
    indexDocId: string;
    syncServer: string;
    description: string;
    lastAccessed?: string;
  }>,
): number {
  if (!handle) throw new Error('Not connected to a project set');

  let added = 0;
  handle.change(doc => {
    for (const entry of entries) {
      const key = projectSetKey(entry.indexDocId);
      if (!doc.projects[key]) {
        const now = new Date().toISOString();
        doc.projects[key] = {
          indexDocId: entry.indexDocId,
          syncServer: entry.syncServer,
          description: entry.description,
          addedAt: now,
          lastAccessed: entry.lastAccessed ?? now,
        };
        added++;
      }
    }
  });
  notifyProjectsChange();
  return added;
}

// ============================================================================
// Export (for JSON backup)
// ============================================================================

/**
 * Export the project set as a JSON-serializable array.
 * Used by the export/backup functionality.
 */
export function exportProjects(): ProjectSetEntry[] {
  return listProjects();
}

// ============================================================================
// Testing Utilities
// ============================================================================

/**
 * Reset all module-level state for testing.
 * @internal For testing only.
 */
export function _resetForTesting(): void {
  handle = null;
  wsAdapter = null;
  repo = null;
  cleanupFn = null;
  onProjectsChange = null;
  onConnectionChange = null;
}

/**
 * Get the internal handle for testing.
 * @internal For testing only.
 */
export function _getHandleForTesting(): DocHandle<ProjectSetDocument> | null {
  return handle;
}

/**
 * Set an internal handle for testing (mock injection).
 * @internal For testing only.
 */
export function _setHandleForTesting(
  mockHandle: DocHandle<ProjectSetDocument> | null,
): void {
  handle = mockHandle;
}
