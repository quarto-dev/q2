/**
 * getRepo() / getDocInventory() — Automerge-layer introspection for the
 * hub-client in-context debug API (bd-q93tkglb; plan:
 * claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md).
 *
 * Contract: getDocInventory() reports every document this client knows
 * about — the index doc first, then per-path entries sorted by path —
 * with the bare doc id, role (index / file / binary-file), the live
 * automerge-repo DocHandle state, the doc's heads when ready, and the
 * client's own unavailable marker for dangling index entries.
 * getRepo() exposes the live Repo so same-heap debug tooling (the
 * `quartoDebug.am` console API) can reach handles the wrapper doesn't
 * surface. Observation only — nothing here mutates.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import {
  generateAutomergeUrl,
  parseAutomergeUrl,
  type DocumentId,
} from '@automerge/automerge-repo';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';

let hub: TestHub;
const liveClients: SyncClient[] = [];

beforeEach(async () => {
  hub = await startTestHub();
});

afterEach(async () => {
  for (const c of liveClients.splice(0)) {
    await c.disconnect();
  }
  await hub.stop();
});

function client(): SyncClient {
  const c = createSyncClient({
    onFileAdded: () => {},
    onFileChanged: () => {},
    onFileRemoved: () => {},
  });
  liveClients.push(c);
  return c;
}

/** 1x1 transparent PNG, base64 (createNewProject takes binary as base64). */
const TINY_PNG_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk' +
  'YPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';

/**
 * Create a project on the hub with one text and one binary file, wait
 * for the hub to hold every doc, then disconnect the creator. Returns
 * the index doc id and the created files' doc ids by path.
 */
async function createProjectOnHub(): Promise<{
  indexDocId: string;
  docIdByPath: Map<string, string>;
}> {
  const creator = client();
  const result = await creator.createNewProject({
    syncServer: hub.url,
    files: [
      { path: 'main.qmd', content: 'hello inventory\n', contentType: 'text' },
      {
        path: 'logo.png',
        content: TINY_PNG_B64,
        contentType: 'binary',
        mimeType: 'image/png',
      },
    ],
    storage: 'memory',
    peerTimeoutMs: 10000,
    requireOnline: true,
  });
  expect(await hub.hubHasDoc(result.indexDocId, 8000)).toBe(true);
  for (const f of result.files) {
    expect(await hub.hubHasDoc(f.docId, 8000)).toBe(true);
  }
  await creator.disconnect();
  return {
    indexDocId: result.indexDocId,
    docIdByPath: new Map(result.files.map((f) => [f.path, f.docId])),
  };
}

async function connectReader(indexDocId: string): Promise<SyncClient> {
  const reader = client();
  await reader.connect(hub.url, indexDocId, undefined, undefined, undefined, {
    storage: 'memory',
    peerTimeoutMs: 10000,
    requireOnline: true,
    findDocRetry: { attempts: 1, baseDelayMs: 10 },
  });
  return reader;
}

describe('getRepo', () => {
  it('is null before connect and after disconnect', async () => {
    const c = client();
    expect(c.getRepo()).toBeNull();

    const { indexDocId } = await createProjectOnHub();
    const reader = await connectReader(indexDocId);
    expect(reader.getRepo()).not.toBeNull();

    await reader.disconnect();
    expect(reader.getRepo()).toBeNull();
  });

  it('returns the live Repo that holds this connection’s handles', async () => {
    const { indexDocId } = await createProjectOnHub();
    const reader = await connectReader(indexDocId);

    const repo = reader.getRepo();
    expect(repo).not.toBeNull();
    // The repo's handle cache is keyed by bare document id; the index
    // doc must be in it — proof this is the connection's own Repo, not
    // a fresh one.
    expect(Object.keys(repo!.handles)).toContain(indexDocId);
  });
});

describe('getDocInventory', () => {
  it('is empty before connect', () => {
    expect(client().getDocInventory()).toEqual([]);
  });

  it('reports index, text, and binary docs with states and heads', async () => {
    const { indexDocId, docIdByPath } = await createProjectOnHub();
    const reader = await connectReader(indexDocId);

    const inventory = reader.getDocInventory();

    // Index doc first, then per-path entries sorted by path.
    expect(inventory.map((e) => [e.role, e.path])).toEqual([
      ['index', null],
      ['binary-file', 'logo.png'],
      ['file', 'main.qmd'],
    ]);

    const [index, logo, main] = inventory;

    expect(index.docId).toBe(indexDocId);
    expect(index.handleState).toBe('ready');
    expect(index.unavailableMarker).toBe(false);
    expect(index.heads).not.toBeNull();
    expect(index.heads!.length).toBeGreaterThan(0);
    for (const h of index.heads!) expect(typeof h).toBe('string');

    expect(main.docId).toBe(docIdByPath.get('main.qmd'));
    expect(main.handleState).toBe('ready');
    expect(main.unavailableMarker).toBe(false);
    expect(main.heads!.length).toBeGreaterThan(0);

    expect(logo.docId).toBe(docIdByPath.get('logo.png'));
    expect(logo.handleState).toBe('ready');
  });

  it('advances a file’s heads when its content changes', async () => {
    const { indexDocId } = await createProjectOnHub();
    const reader = await connectReader(indexDocId);

    const headsOf = () =>
      reader.getDocInventory().find((e) => e.path === 'main.qmd')!.heads;

    const before = headsOf();
    reader.updateFileContent('main.qmd', 'edited content\n');
    const after = headsOf();

    expect(before).not.toBeNull();
    expect(after).not.toBeNull();
    expect(after).not.toEqual(before);
  });

  it('reports a dangling index entry with the unavailable marker and null heads', async () => {
    const { indexDocId } = await createProjectOnHub();

    // Mint an index entry pointing at a doc no repo holds.
    const ghostId = parseAutomergeUrl(generateAutomergeUrl()).documentId;
    const indexOnHub = await hub.repo.find<{ files: Record<string, string> }>(
      indexDocId as DocumentId,
    );
    indexOnHub.change((d) => {
      d.files['ghost.qmd'] = ghostId;
    });

    const reader = await connectReader(indexDocId);
    const ghost = reader
      .getDocInventory()
      .find((e) => e.path === 'ghost.qmd');

    expect(ghost).toBeDefined();
    expect(ghost!.docId).toBe(ghostId);
    expect(ghost!.unavailableMarker).toBe(true);
    expect(ghost!.heads).toBeNull();
    // Same setup as sync-diagnostics.test.ts: with a connected hub that
    // reports the doc missing, the repo's settled verdict is visible.
    expect(ghost!.handleState).toBe('unavailable');
    // Binary-ness of a never-loaded doc is unknowable; dangling entries
    // report the generic file role.
    expect(ghost!.role).toBe('file');
  });
});
