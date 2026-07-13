/**
 * Project Set / Collections Service
 *
 * Manages the Automerge-backed ProjectSetDocuments that store a user's
 * project lists. Historically this service managed exactly one document
 * (the project set); with collections, each collection IS a
 * ProjectSetDocument and this service manages a MAP of connections —
 * one per collection the browser is subscribed to.
 *
 * The first connection (insertion order) is the personal root collection:
 * the migrated legacy project set that acts as the superset of the user's
 * projects. The legacy singleton API (connect, listProjects, addProject,
 * …) delegates to it, so pre-collections callers keep working unchanged.
 *
 * Repos are shared per sync server: many collection docs on one server
 * ride a single websocket connection.
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
  ProjectSetEntrySummary,
} from '@quarto/quarto-automerge-schema';
import {
  CURRENT_PROJECT_SET_SCHEMA_VERSION,
  addProjectToSet,
  removeProjectFromSet,
  touchProjectInSet,
  updateProjectSummaryInSet,
  setProjectSetName,
  projectSetKey,
} from '@quarto/quarto-automerge-schema';
import type { CollectionPointerEntry } from './storage/types';

// ============================================================================
// Types
// ============================================================================

/** Callback fired when the ROOT collection's project list changes. */
export type ProjectsChangeHandler = (projects: ProjectSetEntry[]) => void;

/** Callback fired when any collection's contents (or the set of
 * collections) changes. */
export type CollectionsChangeHandler = (collections: CollectionSnapshot[]) => void;

/** Callback fired when the connection state changes. */
export type ConnectionChangeHandler = (connected: boolean) => void;

/** Immutable view of one connected collection. */
export interface CollectionSnapshot {
  /** Automerge document id of the collection's ProjectSetDocument. */
  docId: string;
  /** Sync server the collection lives on. */
  syncServer: string;
  /** Collection display name (absent on pre-collections documents). */
  name?: string;
  /** Entries sorted by lastAccessed, most recent first. */
  entries: ProjectSetEntry[];
  /** True for the personal root collection (first connection). */
  isRoot: boolean;
}

interface CollectionConnection {
  docId: string;
  syncServer: string;
  handle: DocHandle<ProjectSetDocument>;
  cleanup: () => void;
}

interface ServerConnection {
  repo: Repo;
  wsAdapter: BrowserWebSocketClientAdapter;
  /** Number of collection connections using this server. */
  refCount: number;
}

// ============================================================================
// Internal State
// ============================================================================

/** Server connections keyed by resolved websocket URL. */
const servers = new Map<string, ServerConnection>();

/** Collection connections keyed by doc id; insertion order matters —
 * the first entry is the personal root collection. */
const connections = new Map<string, CollectionConnection>();

let onProjectsChange: ProjectsChangeHandler | null = null;
let onCollectionsChange: CollectionsChangeHandler | null = null;
let onConnectionChange: ConnectionChangeHandler | null = null;

// ============================================================================
// Event Handlers
// ============================================================================

/**
 * Set callbacks for project set / collection events.
 */
export function setProjectSetHandlers(handlers: {
  onProjectsChange?: ProjectsChangeHandler;
  onCollectionsChange?: CollectionsChangeHandler;
  onConnectionChange?: ConnectionChangeHandler;
}): void {
  if (handlers.onProjectsChange) onProjectsChange = handlers.onProjectsChange;
  if (handlers.onCollectionsChange) onCollectionsChange = handlers.onCollectionsChange;
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

function rootConnection(): CollectionConnection | null {
  const first = connections.values().next();
  return first.done ? null : first.value;
}

function connectionOrThrow(collectionDocId?: string): CollectionConnection {
  const conn = collectionDocId ? connections.get(collectionDocId) : rootConnection();
  if (!conn) {
    throw new Error(
      collectionDocId
        ? `Not connected to collection ${collectionDocId}`
        : 'Not connected to a project set',
    );
  }
  return conn;
}

function snapshotOf(conn: CollectionConnection): CollectionSnapshot {
  const doc = conn.handle.doc();
  return {
    docId: conn.docId,
    syncServer: conn.syncServer,
    name: doc?.name,
    entries: doc ? getProjectsList(doc) : [],
    isRoot: rootConnection()?.docId === conn.docId,
  };
}

function notifyChange(): void {
  if (onProjectsChange) {
    const root = rootConnection();
    const doc = root?.handle.doc();
    if (doc) onProjectsChange(getProjectsList(doc));
  }
  if (onCollectionsChange) {
    onCollectionsChange(listCollections());
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

/**
 * Get or create the shared Repo for a sync server.
 * When created, kicks off a peer wait purely for the connection indicator.
 */
function acquireServer(syncServerUrl: string): ServerConnection {
  const resolved = resolveSyncServerUrl(syncServerUrl);
  let server = servers.get(resolved);
  if (!server) {
    const wsAdapter = new BrowserWebSocketClientAdapter(resolved);
    const repo = new Repo({
      network: [wsAdapter],
      storage: new IndexedDBStorageAdapter(),
    });
    server = { repo, wsAdapter, refCount: 0 };
    servers.set(resolved, server);
  }
  server.refCount++;
  return server;
}

function releaseServer(syncServerUrl: string): void {
  const resolved = resolveSyncServerUrl(syncServerUrl);
  const server = servers.get(resolved);
  if (!server) return;
  server.refCount--;
  if (server.refCount <= 0) {
    server.wsAdapter.disconnect();
    servers.delete(resolved);
  }
}

// ============================================================================
// Connection Management
// ============================================================================

/**
 * Connect to one collection document. Resolves from local cache when
 * available (background-syncing after), otherwise waits for the server.
 * Idempotent per doc id.
 */
export async function connectCollection(
  pointer: CollectionPointerEntry,
): Promise<CollectionSnapshot> {
  const existing = connections.get(pointer.projectSetDocId);
  if (existing) return snapshotOf(existing);

  const server = acquireServer(pointer.syncServer);
  try {
    const docId = pointer.projectSetDocId as DocumentId;
    const handle = await server.repo.find<ProjectSetDocument>(docId);

    if (!handle.doc()) {
      // No local cache — wait for the network before declaring the doc.
      let isOnline = false;
      try {
        await waitForPeer(server.repo, 5000);
        isOnline = true;
      } catch {
        isOnline = false;
      }
      onConnectionChange?.(isOnline);
      await handle.whenReady();
      if (!handle.doc()) {
        throw new Error(
          isOnline
            ? 'Failed to load collection document'
            : 'Collection not found in local storage. Connect online first to sync.',
        );
      }
    } else {
      waitForPeer(server.repo, 5000)
        .then(() => onConnectionChange?.(true))
        .catch(() => onConnectionChange?.(false));
    }

    const onChange = () => notifyChange();
    handle.on('change', onChange);
    const conn: CollectionConnection = {
      docId: pointer.projectSetDocId,
      syncServer: pointer.syncServer,
      handle,
      cleanup: () => handle.off('change', onChange),
    };
    connections.set(conn.docId, conn);
    return snapshotOf(conn);
  } catch (err) {
    releaseServer(pointer.syncServer);
    throw err;
  }
}

/**
 * Connect to many collections. Individual failures don't abort the rest;
 * they're reported in the result so the UI can surface unreachable
 * collections without losing the reachable ones.
 */
export async function connectCollections(
  pointers: CollectionPointerEntry[],
): Promise<{ connected: CollectionSnapshot[]; failed: Array<{ pointer: CollectionPointerEntry; error: string }> }> {
  const connected: CollectionSnapshot[] = [];
  const failed: Array<{ pointer: CollectionPointerEntry; error: string }> = [];
  for (const pointer of pointers) {
    try {
      connected.push(await connectCollection(pointer));
    } catch (err) {
      failed.push({ pointer, error: err instanceof Error ? err.message : String(err) });
    }
  }
  notifyChange();
  return { connected, failed };
}

/**
 * Create a new collection document on a sync server.
 *
 * @returns The document ID of the new ProjectSetDocument.
 * @throws If the sync server is unreachable.
 */
export async function createCollection(
  syncServerUrl: string,
  name?: string,
): Promise<string> {
  const server = acquireServer(syncServerUrl);
  try {
    // Creation requires the server so the document actually syncs.
    await waitForPeer(server.repo, 10000);
  } catch {
    releaseServer(syncServerUrl);
    throw new Error(
      'Could not reach sync server. Please check your connection and try again.',
    );
  }
  onConnectionChange?.(true);

  const initial = {
    projects: {},
    version: CURRENT_PROJECT_SET_SCHEMA_VERSION,
    ...(name !== undefined ? { name } : {}),
  } as Record<string, unknown>;
  const doc = automergeFrom(initial);
  const handle = server.repo.import<ProjectSetDocument>(automergeSerialize(doc));

  const onChange = () => notifyChange();
  handle.on('change', onChange);
  const conn: CollectionConnection = {
    docId: handle.documentId,
    syncServer: syncServerUrl,
    handle,
    cleanup: () => handle.off('change', onChange),
  };
  connections.set(conn.docId, conn);
  notifyChange();
  return handle.documentId;
}

/**
 * Disconnect one collection (unsubscribe). The document is untouched.
 */
export function disconnectCollection(collectionDocId: string): void {
  const conn = connections.get(collectionDocId);
  if (!conn) return;
  conn.cleanup();
  connections.delete(collectionDocId);
  releaseServer(conn.syncServer);
  notifyChange();
}

/**
 * Disconnect everything (all collections, all servers).
 */
export async function disconnect(): Promise<void> {
  for (const conn of connections.values()) {
    conn.cleanup();
  }
  connections.clear();
  for (const server of servers.values()) {
    server.wsAdapter.disconnect();
  }
  servers.clear();
}

/**
 * Check if connected to at least the root collection.
 */
export function isConnected(): boolean {
  return rootConnection() !== null;
}

// ============================================================================
// Collection Queries and Operations
// ============================================================================

/** Snapshots of all connected collections, root first. */
export function listCollections(): CollectionSnapshot[] {
  return [...connections.values()].map(snapshotOf);
}

/** Snapshot of one collection, or undefined when not connected. */
export function getCollection(collectionDocId: string): CollectionSnapshot | undefined {
  const conn = connections.get(collectionDocId);
  return conn ? snapshotOf(conn) : undefined;
}

/** Rename a collection (for everyone subscribed — it's the shared doc). */
export function renameCollection(collectionDocId: string, name: string): void {
  const conn = connectionOrThrow(collectionDocId);
  conn.handle.change(doc => {
    setProjectSetName(doc, name);
  });
  notifyChange();
}

/** Add a project entry to a collection (deduped by indexDocId). */
export function addProjectToCollection(
  collectionDocId: string,
  entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>,
): void {
  const conn = connectionOrThrow(collectionDocId);
  conn.handle.change(doc => {
    addProjectToSet(doc, entry);
  });
  notifyChange();
}

/** Remove a project entry from a collection. */
export function removeProjectFromCollection(
  collectionDocId: string,
  indexDocId: string,
): void {
  const conn = connectionOrThrow(collectionDocId);
  conn.handle.change(doc => {
    removeProjectFromSet(doc, indexDocId);
  });
  notifyChange();
}

/**
 * Move a project between collections: copy the entry (preserving its
 * metadata) into the target, then remove it from the source.
 */
export function moveProjectBetweenCollections(
  fromDocId: string,
  toDocId: string,
  indexDocId: string,
): void {
  if (fromDocId === toDocId) return;
  const from = connectionOrThrow(fromDocId);
  const to = connectionOrThrow(toDocId);
  const sourceDoc = from.handle.doc();
  const entry = sourceDoc?.projects[projectSetKey(indexDocId)];
  if (!entry) return;
  const copy: ProjectSetEntry = JSON.parse(JSON.stringify(entry));
  to.handle.change(doc => {
    const key = projectSetKey(indexDocId);
    if (!doc.projects[key]) {
      doc.projects[key] = copy;
    }
  });
  from.handle.change(doc => {
    removeProjectFromSet(doc, indexDocId);
  });
  notifyChange();
}

/** Update a project's description wherever the entry appears. */
export function updateProjectDescriptionEverywhere(
  indexDocId: string,
  description: string,
): void {
  const key = projectSetKey(indexDocId);
  for (const conn of connections.values()) {
    if (conn.handle.doc()?.projects[key]) {
      conn.handle.change(doc => {
        const entry = doc.projects[key];
        if (entry) entry.description = description;
      });
    }
  }
  notifyChange();
}

/** Touch lastAccessed wherever the entry appears. */
export function touchProjectEverywhere(indexDocId: string): void {
  const key = projectSetKey(indexDocId);
  for (const conn of connections.values()) {
    if (conn.handle.doc()?.projects[key]) {
      conn.handle.change(doc => {
        touchProjectInSet(doc, indexDocId);
      });
    }
  }
  // No notify: minor metadata update, caller knows what it selected.
}

/** Update the cached peek summary wherever the entry appears. */
export function updateProjectSummaryEverywhere(
  indexDocId: string,
  summary: ProjectSetEntrySummary,
): boolean {
  const key = projectSetKey(indexDocId);
  let updated = false;
  for (const conn of connections.values()) {
    if (conn.handle.doc()?.projects[key]) {
      conn.handle.change(doc => {
        updated = updateProjectSummaryInSet(doc, indexDocId, summary) || updated;
      });
    }
  }
  if (updated) notifyChange();
  return updated;
}

// ============================================================================
// Legacy singleton API (delegates to the personal root collection)
// ============================================================================

/**
 * Connect to an existing project set document as the personal root
 * collection, tearing down any previous connections.
 *
 * @returns The current list of projects, most recently accessed first.
 */
export async function connect(
  syncServerUrl: string,
  projectSetDocId: string,
): Promise<ProjectSetEntry[]> {
  await disconnect();
  const snapshot = await connectCollection({ projectSetDocId, syncServer: syncServerUrl });
  return snapshot.entries;
}

/**
 * Create a new project set document as the personal root collection.
 *
 * @returns The document ID of the newly created ProjectSetDocument.
 */
export async function createProjectSet(syncServerUrl: string): Promise<string> {
  await disconnect();
  return createCollection(syncServerUrl);
}

/**
 * List all projects in the root set, sorted by lastAccessed.
 */
export function listProjects(): ProjectSetEntry[] {
  const root = rootConnection();
  const doc = root?.handle.doc();
  return doc ? getProjectsList(doc) : [];
}

/**
 * Get a single project from the root set by its indexDocId.
 */
export function getProject(indexDocId: string): ProjectSetEntry | undefined {
  const doc = rootConnection()?.handle.doc();
  return doc?.projects[projectSetKey(indexDocId)];
}

/**
 * Add a project to the root set.
 */
export function addProject(
  entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>,
): void {
  addProjectToCollection(connectionOrThrow().docId, entry);
}

/**
 * Remove a project from the root set.
 */
export function removeProject(indexDocId: string): void {
  removeProjectFromCollection(connectionOrThrow().docId, indexDocId);
}

/**
 * Update the description of a project (in every collection it appears in).
 */
export function updateProjectDescription(
  indexDocId: string,
  description: string,
): void {
  connectionOrThrow();
  updateProjectDescriptionEverywhere(indexDocId, description);
}

/**
 * Update the lastAccessed timestamp (in every collection it appears in).
 */
export function touchProject(indexDocId: string): void {
  connectionOrThrow();
  touchProjectEverywhere(indexDocId);
}

/**
 * Replace the cached peek summary (in every collection it appears in).
 * No-op (returns false) when not connected or the entry is missing.
 */
export function updateProjectSummary(
  indexDocId: string,
  summary: ProjectSetEntrySummary,
): boolean {
  if (!rootConnection()) return false;
  return updateProjectSummaryEverywhere(indexDocId, summary);
}

/**
 * Get the document ID of the root project set, or null if not connected.
 */
export function getProjectSetDocId(): string | null {
  return rootConnection()?.docId ?? null;
}

// ============================================================================
// Bulk Operations (for migration)
// ============================================================================

/**
 * Add multiple projects to a collection in a single Automerge change.
 * Targets the root set when no collection id is given.
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
  collectionDocId?: string,
): number {
  const conn = connectionOrThrow(collectionDocId);

  let added = 0;
  conn.handle.change(doc => {
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
  notifyChange();
  return added;
}

// ============================================================================
// Export (for JSON backup)
// ============================================================================

/**
 * Export the root project set as a JSON-serializable array.
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
  for (const conn of connections.values()) conn.cleanup();
  connections.clear();
  servers.clear();
  onProjectsChange = null;
  onCollectionsChange = null;
  onConnectionChange = null;
}

/**
 * Get the root handle for testing.
 * @internal For testing only.
 */
export function _getHandleForTesting(): DocHandle<ProjectSetDocument> | null {
  return rootConnection()?.handle ?? null;
}

/**
 * Inject a root handle for testing (mock injection).
 * @internal For testing only.
 */
export function _setHandleForTesting(
  mockHandle: DocHandle<ProjectSetDocument> | null,
): void {
  connections.clear();
  if (mockHandle) {
    connections.set('_test-root', {
      docId: '_test-root',
      syncServer: 'wss://test.invalid',
      handle: mockHandle,
      cleanup: () => {},
    });
  }
}
