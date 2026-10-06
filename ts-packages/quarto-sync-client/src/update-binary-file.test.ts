/**
 * `updateBinaryFileContent` — in-place replacement of a binary file's
 * bytes (quarto-hub-mcp CAP-5: `write_file` with `encoding: "base64"` on
 * an existing binary). Mirrors `updateFileContent` for text: the path
 * keeps its document (index entries and collaborators' views are
 * untouched), and the BinaryDocumentContent invariant — `hash` tracks
 * `content` — is maintained by the client, not by callers.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';

import { createSyncClient, type SyncClient } from './client.js';
import { computeSHA256 } from './hash.js';
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

const PNG_1 = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4]);
const PNG_2 = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 9, 9, 9, 9, 8, 7]);

describe('updateBinaryFileContent', () => {
  it('replaces the bytes in place and keeps the document hash in sync', async () => {
    const c = client();
    const created = await c.createNewProject({
      syncServer: hub.url,
      files: [],
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
    });
    const first = await c.createBinaryFile('img.png', PNG_1, 'image/png');
    expect(first.deduplicated).toBe(false);

    await c.updateBinaryFileContent('img.png', PNG_2, 'image/png');

    const read = c.getBinaryFileContent('img.png');
    expect(read).not.toBeNull();
    expect(Array.from(read!.content)).toEqual(Array.from(PNG_2));
    expect(read!.mimeType).toBe('image/png');

    // The BinaryDocumentContent invariant: hash tracks content.
    const handle = c.getFileHandle('img.png');
    expect(handle).toBeDefined();
    const doc = handle!.doc() as { hash?: string } | undefined;
    expect(doc?.hash).toBe(await computeSHA256(PNG_2));

    // In place: the index still points at the same document.
    const inventory = c.getDocInventory();
    const entry = inventory.find((e) => e.path === 'img.png');
    expect(entry?.docId).toBe(first.docId);
  });

  it('rejects a text file path', async () => {
    const c = client();
    await c.createNewProject({
      syncServer: hub.url,
      files: [{ path: 'index.qmd', content: 'hello\n', contentType: 'text' as const }],
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
    });
    await expect(c.updateBinaryFileContent('index.qmd', PNG_1, 'image/png')).rejects.toThrow(
      /not binary/i,
    );
  });

  it('rejects an unknown path', async () => {
    const c = client();
    await c.createNewProject({
      syncServer: hub.url,
      files: [],
      storage: 'memory',
      peerTimeoutMs: 10000,
      requireOnline: true,
    });
    await expect(c.updateBinaryFileContent('nope.png', PNG_1, 'image/png')).rejects.toThrow(
      /no (handle|file)/i,
    );
  });
});
