/**
 * Coarse "when did we last see sync activity" timestamps, read by the
 * hub-client connection-status dialog. Module-level state on purpose:
 * hub-client resolves this package to a single source module, so the
 * recorder (client.ts) and the reader (UI) share one copy. Alongside the global timestamps, a per-documentId map
 * tracks the same stats so the UI can split them by document (current
 * file vs. project index doc).
 */

import type { Patch } from '@automerge/automerge-repo';

/**
 * Metadata of the last sync message. The payload itself is binary
 * Automerge change data, so metadata is all there is to show.
 */
export interface SyncMessageSummary {
  type: string;
  documentId?: string;
  senderId?: string;
  byteLength?: number;
}

/**
 * The diff (Automerge patches) applied by the most recent remote
 * change, i.e. a doc change whose applied changes carry a non-local
 * actor.
 */
export interface RemoteChangeSummary {
  at: number;
  documentId: string;
  /** First MAX_REMOTE_PATCHES patches of the change. */
  patches: Patch[];
  /** Total patch count (may exceed patches.length). */
  patchCount: number;
  /** Full text of the doc before/after the change (text docs only). */
  beforeText?: string;
  afterText?: string;
}

export interface SyncActivity {
  /** Last sync-protocol message received over the websocket (ms epoch). */
  lastSyncMessageAt: number | null;
  /** Metadata of that last sync message. */
  lastSyncMessageSummary: SyncMessageSummary | null;
  /** Last ephemeral (presence/execution) message received (ms epoch). */
  lastEphemeralMessageAt: number | null;
  /** Diff of the last remotely-caused document change. */
  lastRemoteChange: RemoteChangeSummary | null;
}

/** The same stats, scoped to one document. */
export interface DocSyncActivity {
  lastSyncMessageAt: number | null;
  lastEphemeralMessageAt: number | null;
  lastRemoteChangeAt: number | null;
  /** Last doc change made by the local actor (ms epoch). */
  lastLocalChangeAt: number | null;
  /**
   * Last time a storage-backed peer (the hub) confirmed heads that
   * include a local change from this session — i.e. "your change got
   * synced" (ms epoch).
   */
  lastLocalDeliveredAt: number | null;
}

/** One timestamped connection-lifecycle event for the debug log. */
export interface ConnectionEvent {
  at: number;
  kind: string;
  detail?: string;
}

const MAX_REMOTE_PATCHES = 20;
const MAX_CONNECTION_EVENTS = 50;

const connectionLog: ConnectionEvent[] = [];

export function recordConnectionEvent(kind: string, detail?: string): void {
  connectionLog.push({ at: Date.now(), kind, detail });
  if (connectionLog.length > MAX_CONNECTION_EVENTS) {
    connectionLog.splice(0, connectionLog.length - MAX_CONNECTION_EVENTS);
  }
}

/** Connection events, newest first. */
export function getConnectionLog(): ConnectionEvent[] {
  return [...connectionLog].reverse();
}

let lastSyncMessageAt: number | null = null;
let lastSyncMessageSummary: SyncMessageSummary | null = null;
let lastEphemeralMessageAt: number | null = null;
let lastRemoteChange: RemoteChangeSummary | null = null;

const perDoc = new Map<string, DocSyncActivity>();
const perDocRemoteChange = new Map<string, RemoteChangeSummary>();

function emptyDocActivity(): DocSyncActivity {
  return {
    lastSyncMessageAt: null,
    lastEphemeralMessageAt: null,
    lastRemoteChangeAt: null,
    lastLocalChangeAt: null,
    lastLocalDeliveredAt: null,
  };
}

function docEntry(documentId: string): DocSyncActivity {
  let entry = perDoc.get(documentId);
  if (!entry) {
    entry = emptyDocActivity();
    perDoc.set(documentId, entry);
  }
  return entry;
}

export function recordSyncMessage(summary?: SyncMessageSummary): void {
  lastSyncMessageAt = Date.now();
  lastSyncMessageSummary = summary ?? null;
  if (summary?.documentId) {
    docEntry(summary.documentId).lastSyncMessageAt = lastSyncMessageAt;
  }
}

export function recordEphemeralMessage(documentId?: string): void {
  lastEphemeralMessageAt = Date.now();
  if (documentId) {
    docEntry(documentId).lastEphemeralMessageAt = lastEphemeralMessageAt;
  }
}

export function recordRemoteChange(
  documentId: string,
  patches: Patch[],
  texts?: { beforeText?: string; afterText?: string },
): void {
  lastRemoteChange = {
    at: Date.now(),
    documentId,
    patches: patches.slice(0, MAX_REMOTE_PATCHES),
    patchCount: patches.length,
    beforeText: texts?.beforeText,
    afterText: texts?.afterText,
  };
  docEntry(documentId).lastRemoteChangeAt = lastRemoteChange.at;
  perDocRemoteChange.set(documentId, lastRemoteChange);
}

export function recordLocalChange(documentId: string): void {
  docEntry(documentId).lastLocalChangeAt = Date.now();
}

export function recordLocalDelivery(documentId: string): void {
  docEntry(documentId).lastLocalDeliveredAt = Date.now();
}

export function getSyncActivity(): SyncActivity {
  return {
    lastSyncMessageAt,
    lastSyncMessageSummary,
    lastEphemeralMessageAt,
    lastRemoteChange,
  };
}

/** Stats for one document; all-null when nothing was recorded for it. */
export function getDocSyncActivity(documentId: string): DocSyncActivity {
  return { ...(perDoc.get(documentId) ?? emptyDocActivity()) };
}

/** The last remote change applied to one document, if any. */
export function getDocRemoteChange(documentId: string): RemoteChangeSummary | null {
  return perDocRemoteChange.get(documentId) ?? null;
}

export function resetSyncActivity(): void {
  lastSyncMessageAt = null;
  lastSyncMessageSummary = null;
  lastEphemeralMessageAt = null;
  lastRemoteChange = null;
  perDoc.clear();
  perDocRemoteChange.clear();
  connectionLog.length = 0;
}
