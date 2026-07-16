/**
 * Reusable fixtures for the offline-cached-hub-project timeline
 * (epic bd-xxjy9yfp, B0 bd-ysusqcb3).
 *
 * Models the full online → offline → reconnect story a hub project goes
 * through when the user loses connectivity and keeps editing:
 *
 *   1. {@link createSyncedHubProject} — create a project online against a
 *      {@link TestHub} and confirm the hub holds both the index and file
 *      docs, so the project is genuinely cached client-side (IndexedDB) AND
 *      server-side.
 *   2. {@link goOffline} — drop the live sockets and hold new upgrades, so
 *      the *same URL* is now unreachable (a mid-session network loss).
 *   3. {@link reopenCachedOffline} — a fresh sync client (a browser
 *      "reload") reopens the cached project; the peer wait times out and
 *      the client degrades to offline-from-cache.
 *   4. {@link goOnline} — accept upgrades again; the client's adapter
 *      reconnects and offline edits sync up.
 *
 * Built on `startTestHub` (its `setHolding`/`dropConnections` give a
 * stable-URL offline window) + `fake-indexeddb` (a fresh `SyncClient`
 * re-reads the same cache). Tests own the fake-indexeddb reset.
 */

import type { DocumentId } from '@automerge/automerge-repo';

import { createSyncClient, type SyncClient } from './client.js';
import type { SyncClientCallbacks } from './types.js';
import type { TestHub } from './test-hub.js';

/** A sync client plus the file-added / identity notifications it received. */
export interface RecordingClient {
  client: SyncClient;
  /** Paths passed to `onFileAdded`, in order. */
  filesAdded: string[];
}

/** A `SyncClient` with no-op callbacks that records added file paths. */
export function makeRecordingClient(): RecordingClient {
  const filesAdded: string[] = [];
  const callbacks: SyncClientCallbacks = {
    onFileAdded: (path) => {
      filesAdded.push(path);
    },
    onFileChanged: () => {},
    onBinaryChanged: () => {},
    onFileRemoved: () => {},
  };
  return { client: createSyncClient(callbacks), filesAdded };
}

/** The `identities` map (actorId → {name,color}) from a client's index doc. */
export function readIdentities(
  client: SyncClient,
): Record<string, { name: string; color: string }> {
  const doc = client.getIndexHandle()?.doc();
  return (doc?.identities ?? {}) as Record<string, { name: string; color: string }>;
}

export interface CachedHubProject {
  indexDocId: string;
  fileDocId: string;
  path: string;
  content: string;
}

/**
 * Create a project online against `hub` and wait until the hub actually
 * holds both docs — the project is then cached on both ends. The creator
 * client is disconnected before returning; callers reopen with a fresh
 * client to model a reload.
 */
export async function createSyncedHubProject(
  hub: TestHub,
  opts: {
    path?: string;
    content?: string;
    actor?: string;
    screenName?: string;
    color?: string;
  } = {},
): Promise<CachedHubProject> {
  const path = opts.path ?? 'doc.qmd';
  const content = opts.content ?? 'online body\n';
  const { client } = makeRecordingClient();
  const created = await client.createNewProject(
    {
      syncServer: hub.url,
      files: [{ path, content, contentType: 'text' }],
      storage: 'indexeddb',
      // Create while the peer is connected so the docs flush to the hub
      // immediately (no background-sync race).
      peerTimeoutMs: 10000,
      requireOnline: true,
    },
    opts.actor,
    opts.screenName,
    opts.color,
  );
  const fileDocId = created.files[0]!.docId;
  await client.flush();
  const gotIndex = await hub.hubHasDoc(created.indexDocId, 8000);
  const gotFile = await hub.hubHasDoc(fileDocId, 8000);
  if (!gotIndex || !gotFile) {
    throw new Error('createSyncedHubProject: docs did not reach the hub');
  }
  await client.disconnect();
  return { indexDocId: created.indexDocId, fileDocId, path, content };
}

/** Model going offline on a stable URL: drop live sockets + hold new upgrades. */
export function goOffline(hub: TestHub): void {
  hub.setHolding(true);
  hub.dropConnections();
}

/** Model reconnect: accept queued and future upgrades again. */
export function goOnline(hub: TestHub): void {
  hub.setHolding(false);
}

/**
 * Reopen a cached hub project while `hub` is offline (holding upgrades):
 * the peer wait times out fast and the client degrades to
 * offline-from-cache. Returns the client plus the file listing.
 */
export async function reopenCachedOffline(
  hub: TestHub,
  project: CachedHubProject,
  opts: { actor?: string; screenName?: string; color?: string } = {},
): Promise<RecordingClient & { files: Awaited<ReturnType<SyncClient['connect']>> }> {
  const rec = makeRecordingClient();
  const files = await rec.client.connect(
    hub.url,
    project.indexDocId,
    opts.actor,
    opts.screenName,
    opts.color,
    { storage: 'indexeddb', peerTimeoutMs: 300 },
  );
  return { ...rec, files };
}

/**
 * Ground truth on the hub side: poll the hub's own repo until the file
 * doc's text equals `expected` (or the deadline passes). This is what a
 * collaborator on the hub would eventually see.
 */
export async function waitForHubFileText(
  hub: TestHub,
  fileDocId: string,
  expected: string,
  timeoutMs = 8000,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const handle = await hub.repo.find(fileDocId as DocumentId);
      const doc = handle.doc() as { text?: string } | undefined;
      if (doc?.text === expected) return true;
    } catch {
      // unavailable — keep polling until the deadline
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  return false;
}
