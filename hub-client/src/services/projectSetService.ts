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
import { fetchAuthMe } from './authService';
import { CollectionConnectError } from './collectionConnectError';

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
  /**
   * Number of currently-connected sync peers. Live, unlike a latched
   * first-connect promise: it goes back to 0 when the websocket drops
   * (e.g. the session expired and the reconnect loop is being rejected),
   * so "is the sync connection alive right now?" has a truthful answer.
   */
  connectedPeers: number;
  /**
   * Resolve `true` as soon as a sync peer is connected (immediately if
   * one already is), or `false` after `timeoutMs` with none. Without a
   * timeout, wait for the next connection indefinitely.
   */
  whenConnected(timeoutMs?: number): Promise<boolean>;
}

type CollectionCreationPolicy = 'server-required' | 'local-first';

/**
 * Timeouts for the connectCollection find/classify path. Tests inject
 * small values; production uses the defaults.
 */
export interface ConnectCollectionTuning {
  /**
   * Per-attempt bound on `repo.find()`. Without it a slow-to-serve doc
   * waits out automerge-repo's ~60 s internal unavailable timeout (same
   * rationale as quarto-sync-client's FIND_DOC_ATTEMPT_TIMEOUT_MS).
   */
  attemptTimeoutMs?: number;
  /** How long to wait for a sync peer before classifying the failure. */
  connectWaitMs?: number;
  /** After a peer is present, how long to wait for the document to
   * arrive before declaring it not-found on the server. */
  docWaitMs?: number;
}

const DEFAULT_CONNECT_TUNING: Required<ConnectCollectionTuning> = {
  attemptTimeoutMs: 5000,
  connectWaitMs: 5000,
  docWaitMs: 5000,
};

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
// Debug snapshot (quartoDebug.am, bd-q93tkglb)
// ============================================================================

/**
 * Structural view of a Repo as the debug snapshot needs it. The real
 * `Repo` satisfies this (PeerId is a branded string); tests fabricate it.
 */
export interface DebugRepoLike {
  peerId: string;
  peers: readonly string[];
}

/** Structural view of a collection connection for the debug snapshot. */
export interface DebugCollectionConnectionLike {
  docId: string;
  syncServer: string;
  handle: {
    state: string;
    heads(): readonly string[];
    doc(): unknown;
  };
}

export interface ProjectSetServerDebug {
  url: string;
  peerId: string;
  connectedPeers: string[];
  refCount: number;
}

export interface ProjectSetCollectionDebug {
  docId: string;
  syncServer: string;
  name: string | undefined;
  isRoot: boolean;
  entryCount: number;
  handleState: string;
  heads: string[] | null;
}

/** Read-only, JSON-serializable view of this service's connections. */
export interface ProjectSetDebugSnapshot {
  servers: ProjectSetServerDebug[];
  collections: ProjectSetCollectionDebug[];
}

/**
 * Pure mapping from the service's internal maps to the debug snapshot.
 * Exported so tests can drive it with fabricated repos/handles; the
 * stateful shell is {@link getProjectSetDebugSnapshot}.
 */
export function buildProjectSetDebugSnapshot(
  serverMap: ReadonlyMap<string, { repo: DebugRepoLike; refCount: number }>,
  connectionMap: ReadonlyMap<string, DebugCollectionConnectionLike>,
  rootDocId: string | null,
): ProjectSetDebugSnapshot {
  const serverList: ProjectSetServerDebug[] = [];
  for (const [url, server] of serverMap) {
    serverList.push({
      url,
      peerId: server.repo.peerId,
      connectedPeers: [...server.repo.peers],
      refCount: server.refCount,
    });
  }

  const collectionList: ProjectSetCollectionDebug[] = [];
  for (const conn of connectionMap.values()) {
    const ready = conn.handle.state === 'ready';
    const doc = ready
      ? (conn.handle.doc() as { name?: string; projects?: Record<string, unknown> } | undefined)
      : undefined;
    collectionList.push({
      docId: conn.docId,
      syncServer: conn.syncServer,
      name: doc?.name,
      isRoot: conn.docId === rootDocId,
      entryCount: doc?.projects ? Object.keys(doc.projects).length : 0,
      handleState: conn.handle.state,
      heads: ready ? [...conn.handle.heads()] : null,
    });
  }

  return { servers: serverList, collections: collectionList };
}

/**
 * Live DocHandle for a connected collection (keyed by bare doc id), or
 * null when that collection is not connected. Debug accessor for
 * `quartoDebug.am` (bd-q93tkglb) — observation only.
 */
export function getCollectionHandle(
  collectionDocId: string,
): DocHandle<ProjectSetDocument> | null {
  return connections.get(collectionDocId)?.handle ?? null;
}

/**
 * Read-only, JSON-serializable snapshot of the project-set service's
 * live server + collection connections, for the in-context debug API
 * `quartoDebug.am` (bd-q93tkglb). Observation only.
 */
export function getProjectSetDebugSnapshot(): ProjectSetDebugSnapshot {
  return buildProjectSetDebugSnapshot(
    servers,
    connections,
    rootConnection()?.docId ?? null,
  );
}

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

/**
 * Get or create the shared Repo for a sync server. The connection keeps
 * a live count of connected sync peers ('peer' / 'peer-disconnected'
 * re-fire across websocket reconnects), so every collection on the
 * server can wait for — or truthfully check — connectivity regardless
 * of connection order.
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
    const conn: ServerConnection = {
      repo,
      wsAdapter,
      refCount: 0,
      connectedPeers: 0,
      whenConnected(timeoutMs?: number): Promise<boolean> {
        if (conn.connectedPeers > 0) return Promise.resolve(true);
        return new Promise((resolve) => {
          let timer: ReturnType<typeof setTimeout> | undefined;
          const onPeer = () => {
            if (timer !== undefined) clearTimeout(timer);
            repo.networkSubsystem.off('peer', onPeer);
            resolve(true);
          };
          if (timeoutMs !== undefined) {
            timer = setTimeout(() => {
              repo.networkSubsystem.off('peer', onPeer);
              resolve(false);
            }, timeoutMs);
          }
          repo.networkSubsystem.on('peer', onPeer);
        });
      },
    };
    repo.networkSubsystem.on('peer', () => {
      conn.connectedPeers += 1;
    });
    repo.networkSubsystem.on('peer-disconnected', () => {
      conn.connectedPeers = Math.max(0, conn.connectedPeers - 1);
    });
    server = conn;
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
 * True for the retriable failure shapes of `repo.find()`: the bare
 * "Document <id> is unavailable" rejection and our own AbortSignal
 * timeout. Same predicate family as quarto-sync-client's findDoc
 * (bd-jit6pdwq).
 */
function isRetriableFindError(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err);
  if (/unavailable/i.test(message)) return true;
  if (!(err instanceof Error)) return false;
  return (
    err.name === 'TimeoutError' ||
    err.name === 'AbortError' ||
    /\baborted?\b|timed?\s*out/i.test(err.message)
  );
}

/** Race a handle's READY transition against a timeout. */
function raceHandleReady(
  handle: DocHandle<ProjectSetDocument>,
  timeoutMs: number,
): Promise<boolean> {
  return Promise.race([
    handle
      .whenReady()
      .then(() => true)
      .catch(() => false),
    new Promise<boolean>((resolve) => setTimeout(() => resolve(false), timeoutMs)),
  ]);
}

/**
 * Classify a connect failure when no sync peer could be established.
 * Browsers hide the HTTP status of a failed websocket upgrade (a 401
 * from an expired session looks identical to a dropped network), so we
 * disambiguate out-of-band via GET /auth/me — the same trick as
 * useAuthProbe (bd-3o8zmz46). In auth-disabled builds (no
 * VITE_GOOGLE_CLIENT_ID) /auth/me always 401s, so the auth-expired
 * verdict is only available when auth is actually on.
 */
async function classifyDisconnected(
  docId: string,
  cause: unknown,
): Promise<CollectionConnectError> {
  if (typeof navigator !== 'undefined' && navigator.onLine === false) {
    return new CollectionConnectError('offline', docId, cause);
  }
  const authEnabled = Boolean(import.meta.env.VITE_GOOGLE_CLIENT_ID);
  try {
    const me = await fetchAuthMe();
    if (me === null && authEnabled) {
      return new CollectionConnectError('auth-expired', docId, cause);
    }
    return new CollectionConnectError('sync-unreachable', docId, cause);
  } catch {
    return new CollectionConnectError('offline', docId, cause);
  }
}

/**
 * Find a collection document, classifying failures (bd-tux4m6od).
 *
 * `repo.find()` resolves from local storage without the network; when
 * the doc isn't cached it asks connected sync peers — and rejects with
 * the bare "unavailable" both when the server lacks the doc AND when no
 * peer was connected yet. The websocket adapter force-marks itself
 * ready 1 s after it starts connecting, so any handshake slower than
 * that (or one rejected with 401) turned into an instant, misleading
 * "Document <id> is unavailable".
 *
 * Strategy: try once (bounded); on a retriable failure wait for a live
 * peer (bounded) — automerge-repo re-requests requested/unavailable
 * docs from newly-added peers (CollectionSynchronizer.addPeer →
 * beginSync), so a late-arriving doc still lands — then classify what
 * remains: no peer → auth-expired / offline / sync-unreachable via the
 * /auth/me probe; live peer but no doc → not-found.
 */
async function findCollectionDoc(
  server: ServerConnection,
  docId: DocumentId,
  tuning: Required<ConnectCollectionTuning>,
): Promise<DocHandle<ProjectSetDocument>> {
  let firstError: unknown;
  try {
    return await server.repo.find<ProjectSetDocument>(docId, {
      signal: AbortSignal.timeout(tuning.attemptTimeoutMs),
    });
  } catch (err) {
    firstError = err;
  }
  if (!isRetriableFindError(firstError)) {
    throw new CollectionConnectError('unknown', docId, firstError);
  }

  if (server.connectedPeers === 0) {
    const connected = await server.whenConnected(tuning.connectWaitMs);
    if (!connected) {
      onConnectionChange?.(false);
      throw await classifyDisconnected(docId, firstError);
    }
  }

  // A sync peer is connected; give the re-request a bounded window.
  let handle: DocHandle<ProjectSetDocument>;
  try {
    handle = await server.repo.find<ProjectSetDocument>(docId, {
      allowableStates: ['ready', 'unavailable'],
      signal: AbortSignal.timeout(tuning.docWaitMs),
    });
  } catch (err) {
    // Stuck in 'requesting' past the deadline with a live peer: the
    // server is connected but not answering.
    throw new CollectionConnectError(
      isRetriableFindError(err) ? 'sync-unreachable' : 'unknown',
      docId,
      err,
    );
  }
  if (handle.state === 'ready') return handle;
  if (await raceHandleReady(handle, tuning.docWaitMs)) return handle;
  throw new CollectionConnectError('not-found', docId, firstError);
}

/**
 * Connect to one collection document. Resolves from local cache when
 * available (background-syncing after), otherwise waits for the server.
 * Idempotent per doc id. Failures reject with CollectionConnectError
 * so callers can present an actionable message (bd-tux4m6od).
 */
export async function connectCollection(
  pointer: CollectionPointerEntry,
  tuning?: ConnectCollectionTuning,
): Promise<CollectionSnapshot> {
  const existing = connections.get(pointer.projectSetDocId);
  if (existing) return snapshotOf(existing);

  const resolvedTuning = { ...DEFAULT_CONNECT_TUNING, ...tuning };
  const server = acquireServer(pointer.syncServer);
  try {
    const docId = pointer.projectSetDocId as DocumentId;
    const handle = await findCollectionDoc(server, docId, resolvedTuning);

    // Report connection state in the background: cache hits resolve
    // before the websocket has finished connecting.
    void server
      .whenConnected(resolvedTuning.connectWaitMs)
      .then((online) => onConnectionChange?.(online));

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

async function createCollectionDocument(
  syncServerUrl: string,
  name: string | undefined,
  policy: CollectionCreationPolicy,
): Promise<string> {
  const server = acquireServer(syncServerUrl);
  try {
    if (policy === 'server-required') {
      // Shared collections require a peer so callers know the document has
      // reached the configured server before they publish its pointer.
      if (!(await server.whenConnected(10000))) {
        throw new Error(
          'Could not reach sync server. Please check your connection and try again.',
        );
      }
      onConnectionChange?.(true);
    } else {
      // A fresh personal root is useful while offline. The websocket adapter
      // keeps reconnecting, and the Repo will announce this handle when its
      // first peer eventually arrives.
      onConnectionChange?.(false);
      void server.whenConnected().then(() => onConnectionChange?.(true));
    }

    const initial = {
      projects: {},
      version: CURRENT_PROJECT_SET_SCHEMA_VERSION,
      ...(name !== undefined ? { name } : {}),
    } as Record<string, unknown>;
    const doc = automergeFrom(initial);
    const handle = server.repo.import<ProjectSetDocument>(automergeSerialize(doc));

    if (policy === 'local-first') {
      // Repo saves are normally debounced. Setup must not publish pointers
      // until the empty root is durably available for an offline reload.
      await server.repo.flush([handle.documentId]);
    }

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
  } catch (err) {
    releaseServer(syncServerUrl);
    throw err;
  }
}

/**
 * Create a new shared collection document on a sync server.
 *
 * @returns The document ID of the new ProjectSetDocument.
 * @throws If the sync server is unreachable.
 */
export function createCollection(
  syncServerUrl: string,
  name?: string,
): Promise<string> {
  return createCollectionDocument(syncServerUrl, name, 'server-required');
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
export async function createProjectSet(
  syncServerUrl: string,
  name?: string,
): Promise<string> {
  await disconnect();
  return createCollectionDocument(syncServerUrl, name, 'local-first');
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
