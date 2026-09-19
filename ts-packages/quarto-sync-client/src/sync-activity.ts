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

// ---------------------------------------------------------------------------
// Cross-session persistence: the three per-doc timestamps shown in the
// connection-status dialog survive a page reload via localStorage. Only
// getDocSyncActivityWithPersisted() reads them back — getDocSyncActivity()
// stays session-only on purpose, because SyncStatusBadge's green "just now"
// state and client.ts's delivery detection both mean "observed *this*
// session".
// ---------------------------------------------------------------------------

const STORAGE_KEY = 'q2hub.syncActivity';
const MAX_PERSISTED_DOCS = 50;
const SAVE_THROTTLE_MS = 1000;

type PersistedDocActivity = Pick<
  DocSyncActivity,
  'lastEphemeralMessageAt' | 'lastRemoteChangeAt' | 'lastLocalDeliveredAt'
>;

/** localStorage when present and accessible (browser); null in Node. */
function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

let persistedLoaded = false;
const persisted = new Map<string, PersistedDocActivity>();

function asTimestamp(v: unknown): number | null {
  return typeof v === 'number' && Number.isFinite(v) && v > 0 ? v : null;
}

function loadPersisted(): void {
  if (persistedLoaded) return;
  persistedLoaded = true;
  try {
    const raw = storage()?.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed = JSON.parse(raw) as Record<string, Partial<PersistedDocActivity>>;
    for (const [docId, v] of Object.entries(parsed)) {
      persisted.set(docId, {
        lastEphemeralMessageAt: asTimestamp(v?.lastEphemeralMessageAt),
        lastRemoteChangeAt: asTimestamp(v?.lastRemoteChangeAt),
        lastLocalDeliveredAt: asTimestamp(v?.lastLocalDeliveredAt),
      });
    }
  } catch {
    // corrupt or inaccessible storage — start fresh
  }
}

function newestTimestamp(v: PersistedDocActivity): number {
  return Math.max(
    v.lastEphemeralMessageAt ?? 0,
    v.lastRemoteChangeAt ?? 0,
    v.lastLocalDeliveredAt ?? 0,
  );
}

function savePersisted(): void {
  const s = storage();
  if (!s) return;
  let entries = [...persisted.entries()];
  if (entries.length > MAX_PERSISTED_DOCS) {
    entries.sort((a, b) => newestTimestamp(b[1]) - newestTimestamp(a[1]));
    entries = entries.slice(0, MAX_PERSISTED_DOCS);
    for (const key of [...persisted.keys()]) {
      if (!entries.some(([k]) => k === key)) persisted.delete(key);
    }
  }
  try {
    s.setItem(STORAGE_KEY, JSON.stringify(Object.fromEntries(entries)));
  } catch {
    // storage full/unavailable — timestamps just won't survive reload
  }
}

let lastSaveAt = 0;
let saveTimer: ReturnType<typeof setTimeout> | null = null;

/** Write-through at most once per second: immediate when idle, trailing
 * timer during bursts (ephemeral messages can arrive many times per second). */
function scheduleSave(): void {
  const now = Date.now();
  if (now - lastSaveAt >= SAVE_THROTTLE_MS) {
    lastSaveAt = now;
    savePersisted();
    return;
  }
  if (saveTimer !== null) return;
  saveTimer = setTimeout(
    () => {
      saveTimer = null;
      lastSaveAt = Date.now();
      savePersisted();
    },
    SAVE_THROTTLE_MS - (now - lastSaveAt),
  );
}

function maxTimestamp(a: number | null, b: number | null): number | null {
  return Math.max(a ?? 0, b ?? 0) || null;
}

/** Fold the session entry into the persisted one (max per field, so a fresh
 * session's first event doesn't erase last session's other timestamps). */
function persistDoc(documentId: string, entry: DocSyncActivity): void {
  loadPersisted();
  const prev = persisted.get(documentId);
  persisted.set(documentId, {
    lastEphemeralMessageAt: maxTimestamp(entry.lastEphemeralMessageAt, prev?.lastEphemeralMessageAt ?? null),
    lastRemoteChangeAt: maxTimestamp(entry.lastRemoteChangeAt, prev?.lastRemoteChangeAt ?? null),
    lastLocalDeliveredAt: maxTimestamp(entry.lastLocalDeliveredAt, prev?.lastLocalDeliveredAt ?? null),
  });
  scheduleSave();
}

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
    const entry = docEntry(documentId);
    entry.lastEphemeralMessageAt = lastEphemeralMessageAt;
    persistDoc(documentId, entry);
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
  const entry = docEntry(documentId);
  entry.lastRemoteChangeAt = lastRemoteChange.at;
  perDocRemoteChange.set(documentId, lastRemoteChange);
  persistDoc(documentId, entry);
}

export function recordLocalChange(documentId: string): void {
  docEntry(documentId).lastLocalChangeAt = Date.now();
}

export function recordLocalDelivery(documentId: string): void {
  const entry = docEntry(documentId);
  entry.lastLocalDeliveredAt = Date.now();
  persistDoc(documentId, entry);
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

/**
 * Like getDocSyncActivity, but the three persisted timestamps (ephemeral,
 * remote change, read receipt) also survive page reloads — per field, the
 * newer of this session's value and the localStorage-persisted one.
 * `lastSyncMessageAt` / `lastLocalChangeAt` remain session-only.
 */
export function getDocSyncActivityWithPersisted(documentId: string): DocSyncActivity {
  const live = getDocSyncActivity(documentId);
  loadPersisted();
  const p = persisted.get(documentId);
  if (!p) return live;
  return {
    ...live,
    lastEphemeralMessageAt: maxTimestamp(live.lastEphemeralMessageAt, p.lastEphemeralMessageAt),
    lastRemoteChangeAt: maxTimestamp(live.lastRemoteChangeAt, p.lastRemoteChangeAt),
    lastLocalDeliveredAt: maxTimestamp(live.lastLocalDeliveredAt, p.lastLocalDeliveredAt),
  };
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
  persisted.clear();
  persistedLoaded = true; // don't resurrect the cleared state from storage
  if (saveTimer !== null) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  lastSaveAt = 0;
  try {
    storage()?.removeItem(STORAGE_KEY);
  } catch {
    // storage unavailable — nothing persisted to clear
  }
}
