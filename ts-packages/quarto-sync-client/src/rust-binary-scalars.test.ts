/**
 * Rust-authored binary documents store mimeType and hash as Automerge
 * scalar strings, which surface in JavaScript as ImmutableString objects.
 * The sync client's public callbacks and getters promise primitive strings.
 */

import { ImmutableString } from '@automerge/automerge';
import type { DocHandle, DocumentId } from '@automerge/automerge-repo';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createSyncClient, type SyncClient } from './client.js';
import { computeSHA256 } from './hash.js';
import { startTestHub, type TestHub } from './test-hub.js';

interface ScalarBinaryDocument {
  content: Uint8Array;
  mimeType: string | ImmutableString;
  hash: string | ImmutableString;
}

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 1, 2, 3, 4]);
const PNG_UPDATED = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 5, 6, 7, 8]);
const ONLINE = {
  storage: 'memory' as const,
  peerTimeoutMs: 10000,
  requireOnline: true,
};

let hub: TestHub;
const liveClients: SyncClient[] = [];

beforeEach(async () => {
  hub = await startTestHub();
});

afterEach(async () => {
  for (const client of liveClients.splice(0)) {
    await client.disconnect();
  }
  await hub.stop();
});

function client(callbacks?: {
  onFileAdded?: ReturnType<typeof vi.fn>;
  onBinaryChanged?: ReturnType<typeof vi.fn>;
}): SyncClient {
  const result = createSyncClient({
    onFileAdded: callbacks?.onFileAdded ?? (() => {}),
    onFileChanged: () => {},
    onBinaryChanged: callbacks?.onBinaryChanged ?? (() => {}),
    onFileRemoved: () => {},
  });
  liveClients.push(result);
  return result;
}

async function seedScalarBinary() {
  const writer = client();
  const project = await writer.createNewProject({
    syncServer: hub.url,
    files: [],
    ...ONLINE,
  });
  const created = await writer.createBinaryFile('img.png', PNG, 'image/png');
  expect(await hub.hubHasHeadsOf(writer, 8000)).toBe(true);

  const handle = await hub.repo.find<ScalarBinaryDocument>(created.docId as DocumentId);
  const hash = await computeSHA256(PNG);
  handle.change((doc) => {
    doc.mimeType = new ImmutableString('image/png');
    doc.hash = new ImmutableString(hash);
  });
  // This fixture must model the Rust scalar representation, not the plain
  // strings normally produced by TypeScript writes.
  expect(handle.doc()?.mimeType).toBeInstanceOf(ImmutableString);
  expect(handle.doc()?.hash).toBeInstanceOf(ImmutableString);

  return { project, created, handle };
}

async function connectScalarReader() {
  const seeded = await seedScalarBinary();
  const onFileAdded = vi.fn();
  const onBinaryChanged = vi.fn();
  const reader = client({ onFileAdded, onBinaryChanged });
  await reader.connect(
    hub.url,
    seeded.project.indexDocId,
    undefined,
    undefined,
    undefined,
    ONLINE,
  );
  return { ...seeded, reader, onFileAdded, onBinaryChanged };
}

describe('Rust binary scalar normalization', () => {
  it('emits and returns primitive MIME strings on cold connect', async () => {
    const { reader, onFileAdded } = await connectScalarReader();

    const added = onFileAdded.mock.calls.find(([path]) => path === 'img.png');
    const addedMime = added?.[1].type === 'binary' ? added[1].mimeType : undefined;
    expect.soft(addedMime).toBe('image/png');
    expect.soft(typeof addedMime).toBe('string');

    const read = reader.getBinaryFileContent('img.png');
    expect.soft(read?.mimeType).toBe('image/png');
    expect.soft(typeof read?.mimeType).toBe('string');
  });

  it('returns a primitive MIME string from binary document lookup by id', async () => {
    const { reader, created } = await connectScalarReader();

    const read = await reader.getBinaryDocById(created.docId);

    expect(read?.mimeType).toBe('image/png');
    expect(typeof read?.mimeType).toBe('string');
  });

  it('deduplicates identical content against a scalar stored hash', async () => {
    const { reader, created } = await connectScalarReader();

    const duplicate = await reader.createBinaryFile('img.png', PNG, 'image/png');

    expect(duplicate).toEqual({
      docId: created.docId,
      path: 'img.png',
      deduplicated: true,
    });
  });

  it('emits a primitive MIME string for a remote binary change', async () => {
    const { handle, onBinaryChanged } = await connectScalarReader();
    const updatedHash = await computeSHA256(PNG_UPDATED);
    handle.change((doc) => {
      doc.content = PNG_UPDATED;
      doc.mimeType = new ImmutableString('image/png');
      doc.hash = new ImmutableString(updatedHash);
    });

    await vi.waitFor(() => expect(onBinaryChanged).toHaveBeenCalled());
    const [path, bytes, mimeType] = onBinaryChanged.mock.calls.at(-1)!;
    expect(path).toBe('img.png');
    expect(bytes).toEqual(PNG_UPDATED);
    expect(mimeType).toBe('image/png');
    expect(typeof mimeType).toBe('string');
  });

  it('emits a primitive MIME string for a change on a locally created file', async () => {
    const onBinaryChanged = vi.fn();
    const writer = client({ onBinaryChanged });
    await writer.createNewProject({ syncServer: hub.url, files: [], ...ONLINE });
    await writer.createBinaryFile('img.png', PNG, 'image/png');
    onBinaryChanged.mockClear();

    const handle = writer.getFileHandle('img.png') as unknown as DocHandle<ScalarBinaryDocument>;
    const updatedHash = await computeSHA256(PNG_UPDATED);
    handle.change((doc) => {
      doc.content = PNG_UPDATED;
      doc.mimeType = new ImmutableString('image/png');
      doc.hash = new ImmutableString(updatedHash);
    });

    await vi.waitFor(() => expect(onBinaryChanged).toHaveBeenCalled());
    const mimeType = onBinaryChanged.mock.calls.at(-1)?.[2];
    expect(mimeType).toBe('image/png');
    expect(typeof mimeType).toBe('string');
  });
});
