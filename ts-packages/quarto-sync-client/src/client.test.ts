/**
 * Tests for createSyncClient identity handling.
 *
 * Regression test: identity must be written to the index document
 * regardless of peer connection status. A prior bug gated identity
 * writes behind an `isOnline` check, which always failed because
 * the peer timeout was 1ms.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import type { DocumentId } from '@automerge/automerge-repo';

// Track calls to setIdentity
const setIdentitySpy = vi.fn();

// ── Mocks ──────────────────────────────────────────────────────────────

vi.mock('@automerge/automerge', () => ({
  clone: vi.fn((doc: unknown) => structuredClone(doc)),
  from: vi.fn((val: unknown) => structuredClone(val)),
  save: vi.fn(() => new Uint8Array(0)),
}));

vi.mock('@automerge/automerge-repo-network-websocket', () => ({
  BrowserWebSocketClientAdapter: vi.fn(),
}));

vi.mock('@automerge/automerge-repo-storage-indexeddb', () => ({
  IndexedDBStorageAdapter: vi.fn(),
}));

// Mock setIdentity so we can assert it was called
vi.mock('@quarto/quarto-automerge-schema', async (importOriginal) => {
  const original = await importOriginal<typeof import('@quarto/quarto-automerge-schema')>();
  return {
    ...original,
    setIdentity: (...args: unknown[]) => {
      setIdentitySpy(...args);
      return original.setIdentity(...(args as Parameters<typeof original.setIdentity>));
    },
  };
});

/**
 * Create a mock DocHandle that stores its document in-memory.
 */
function createMockHandle<T>(initialDoc: T): {
  handle: { doc: () => T; change: (fn: (d: T) => void) => void; on: () => void; off: () => void; documentId: string; whenReady: () => Promise<void>; update: (fn: (d: T) => T) => void };
  getDoc: () => T;
} {
  let current = structuredClone(initialDoc);
  const handle = {
    documentId: 'mock-doc-id' as DocumentId,
    doc: () => current,
    change: (fn: (d: T) => void) => {
      const draft = structuredClone(current);
      fn(draft);
      current = draft;
    },
    update: (fn: (d: T) => T) => {
      current = fn(structuredClone(current));
    },
    on: vi.fn(),
    off: vi.fn(),
    whenReady: () => Promise.resolve(),
  };
  return { handle, getDoc: () => current };
}

// Mock Repo — never emits 'peer', so waitForPeer always times out
vi.mock('@automerge/automerge-repo', async (importOriginal) => {
  const original = await importOriginal<typeof import('@automerge/automerge-repo')>();
  return {
    ...original,
    Repo: vi.fn(),
    // Keep real URL helpers
    generateAutomergeUrl: original.generateAutomergeUrl,
    parseAutomergeUrl: original.parseAutomergeUrl,
  };
});

import { Repo } from '@automerge/automerge-repo';
import { createSyncClient } from './client.js';
import type { SyncClientCallbacks } from './types.js';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';

/** Minimal no-op callbacks. */
function noopCallbacks(): SyncClientCallbacks {
  return {
    onFileAdded: vi.fn(),
    onFileChanged: vi.fn(),
    onBinaryChanged: vi.fn(),
    onFileRemoved: vi.fn(),
    onFilesChange: vi.fn(),
    onIdentitiesChange: vi.fn(),
    onConnectionChange: vi.fn(),
    onError: vi.fn(),
  };
}

/**
 * Install a mock Repo that never connects to a peer.
 * - `find()` returns `connectHandle` (for `connect` flow)
 * - `import()` returns `createHandle` (for `createNewProject` flow)
 * - `create()` returns `createHandle`
 * - networkSubsystem never emits 'peer', so `waitForPeer` always times out
 */
function installMockRepo<T>(
  connectHandle: ReturnType<typeof createMockHandle<T>>['handle'],
  createHandle: ReturnType<typeof createMockHandle<T>>['handle'],
) {
  const mockNetworkSubsystem = {
    on: vi.fn(),
    off: vi.fn(),
  };

  // Repo is called with `new`, so the mock must be a constructor
  vi.mocked(Repo).mockImplementation(function (this: unknown) {
    Object.assign(this as Record<string, unknown>, {
      find: vi.fn().mockResolvedValue(connectHandle),
      import: vi.fn().mockReturnValue(createHandle),
      create: vi.fn().mockReturnValue(createHandle),
      networkSubsystem: mockNetworkSubsystem,
    });
    return this as Repo;
  } as unknown as typeof Repo);
}

describe('createSyncClient identity', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('connect writes identity even when peer connection times out', async () => {
    const indexDoc: IndexDocument = { files: {}, version: 1, identities: {} };
    const { handle, getDoc } = createMockHandle(indexDoc);

    installMockRepo(handle, handle);

    const cbs = noopCallbacks();
    const client = createSyncClient(cbs);

    await client.connect(
      'ws://localhost:9999',
      'mock-doc-id',
      'actor-123',
      'Alice',
      '#FF0000',
    );

    // Identity must have been written
    expect(setIdentitySpy).toHaveBeenCalledWith(
      expect.anything(),
      'actor-123',
      'Alice',
      '#FF0000',
    );

    // Verify the document was actually mutated
    const doc = getDoc();
    expect(doc.identities?.['actor-123']).toEqual({
      name: 'Alice',
      color: '#FF0000',
    });
  });

  it('createNewProject writes identity even when peer connection times out', async () => {
    const indexDoc: IndexDocument = { files: {}, version: 1, identities: {} };
    const { handle, getDoc } = createMockHandle(indexDoc);

    installMockRepo(handle, handle);

    const cbs = noopCallbacks();
    const client = createSyncClient(cbs);

    // Use resolveActorId callback (mirrors App.tsx which passes actorId=undefined)
    const resolveActorId = vi.fn().mockResolvedValue('actor-456');

    await client.createNewProject(
      { syncServer: 'ws://localhost:9999', files: [] },
      undefined,    // actorId — App.tsx passes undefined
      'Bob',
      '#00FF00',
      resolveActorId,
    );

    // resolveActorId must be called regardless of connection status
    expect(resolveActorId).toHaveBeenCalled();

    // Identity must have been written
    expect(setIdentitySpy).toHaveBeenCalledWith(
      expect.anything(),
      'actor-456',
      'Bob',
      '#00FF00',
    );

    // Verify the document was actually mutated
    const doc = getDoc();
    expect(doc.identities?.['actor-456']).toEqual({
      name: 'Bob',
      color: '#00FF00',
    });
  });
});
