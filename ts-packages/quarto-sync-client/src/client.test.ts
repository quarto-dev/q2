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
    onCapturesChange: vi.fn(),
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
      flush: vi.fn().mockResolvedValue(undefined),
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

describe('createSyncClient captures (Phase C.3)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('connect fires onCapturesChange with the empty sidecar when the index doc has no captures', async () => {
    const indexDoc: IndexDocument = { files: {}, version: 2, identities: {} };
    const { handle } = createMockHandle(indexDoc);

    installMockRepo(handle, handle);

    const cbs = noopCallbacks();
    const client = createSyncClient(cbs);

    await client.connect('ws://localhost:9999', 'mock-doc-id', 'actor-1', 'Alice', '#FF0000');

    expect(cbs.onCapturesChange).toHaveBeenCalledWith({});
  });

  it('connect fires onCapturesChange with the populated sidecar when the index doc carries captures', async () => {
    const indexDoc: IndexDocument = {
      files: { 'index.qmd': 'doc1' },
      version: 2,
      identities: {},
      captures: {
        'index.qmd': { captureDocId: 'cap-1', state: 'idle' },
      },
    };
    const { handle } = createMockHandle(indexDoc);

    installMockRepo(handle, handle);

    const cbs = noopCallbacks();
    const client = createSyncClient(cbs);

    await client.connect('ws://localhost:9999', 'mock-doc-id', 'actor-1', 'Alice', '#FF0000');

    expect(cbs.onCapturesChange).toHaveBeenCalledWith({
      'index.qmd': { captureDocId: 'cap-1', state: 'idle' },
    });
  });
});

describe('createSyncClient getIndexHandle (bd-sfet3264 Phase 2A)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('returns null before connect and the index DocHandle after connect', async () => {
    const indexDoc: IndexDocument = { files: {}, version: 2, identities: {} };
    const { handle } = createMockHandle(indexDoc);
    installMockRepo(handle, handle);

    const client = createSyncClient(noopCallbacks());
    // Before connect there is no index handle to broadcast on.
    expect(client.getIndexHandle()).toBeNull();

    await client.connect('ws://localhost:9999', 'mock-doc-id', 'actor-1', 'Alice', '#FF0000');

    // After connect the index handle is exposed (the ephemeral channel
    // carrier for the execution beacon/request protocol).
    expect(client.getIndexHandle()).toBe(handle);
  });
});

describe('createSyncClient clearCapture (D6 / bd-sfet3264)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('clearCapture removes the CaptureRef sidecar entry for the path, leaving siblings intact', async () => {
    const indexDoc: IndexDocument = {
      files: { 'index.qmd': 'doc1', 'other.qmd': 'doc2' },
      version: 2,
      identities: {},
      captures: {
        'index.qmd': { captureDocId: 'cap-1', state: 'idle' },
        'other.qmd': { captureDocId: 'cap-2', state: 'idle' },
      },
    };
    const { handle, getDoc } = createMockHandle(indexDoc);
    installMockRepo(handle, handle);

    const client = createSyncClient(noopCallbacks());
    await client.connect('ws://localhost:9999', 'mock-doc-id', 'actor-1', 'Alice', '#FF0000');

    client.clearCapture('index.qmd');

    const doc = getDoc();
    expect(doc.captures?.['index.qmd']).toBeUndefined();
    // The sibling's capture must be untouched — clear is per-document.
    expect(doc.captures?.['other.qmd']).toEqual({ captureDocId: 'cap-2', state: 'idle' });
  });

  it('clearCapture is a no-op (no throw, no sibling change) when the path has no capture', async () => {
    const indexDoc: IndexDocument = {
      files: { 'index.qmd': 'doc1' },
      version: 2,
      identities: {},
      captures: {
        'index.qmd': { captureDocId: 'cap-1', state: 'idle' },
      },
    };
    const { handle, getDoc } = createMockHandle(indexDoc);
    installMockRepo(handle, handle);

    const client = createSyncClient(noopCallbacks());
    await client.connect('ws://localhost:9999', 'mock-doc-id', 'actor-1', 'Alice', '#FF0000');

    expect(() => client.clearCapture('nonexistent.qmd')).not.toThrow();
    // The existing capture is left intact.
    expect(getDoc().captures?.['index.qmd']).toEqual({ captureDocId: 'cap-1', state: 'idle' });
  });
});

// ─── bd-4uvv: getBinaryDocById prefix normalization ──────────────────
//
// samod's TS `repo.find()` rejects bare docIds with
// `Error: Invalid AutomergeUrl: <id>`. The IndexDocument's capture
// sidecar (Phase C.3) stores bare docIds — same as `files` — so the
// binary-doc-by-id call path must prepend the `automerge:` scheme just
// like the text-doc loader (`loadFileDocuments`, lines 305-307 in
// client.ts). Without the prefix, every project-mode q2 preview render
// falls back to the default registry because the capture binary doc
// never arrives, and code cells render as inert source even when the
// server's eager capture driver wrote a perfectly valid capture.

describe('createSyncClient.getBinaryDocById URL normalization (bd-4uvv)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  /**
   * Spy-wrapper around `installMockRepo`. Captures the actual `find()`
   * arguments so the test can assert the prefix-normalization rule
   * without invoking samod's real URL parser.
   */
  function installSpyRepo<T>(
    binaryHandle: ReturnType<typeof createMockHandle<T>>['handle'],
  ): { findCalls: unknown[] } {
    const findCalls: unknown[] = [];
    const mockNetworkSubsystem = { on: vi.fn(), off: vi.fn() };
    vi.mocked(Repo).mockImplementation(function (this: unknown) {
      Object.assign(this as Record<string, unknown>, {
        find: vi.fn((id: unknown) => {
          findCalls.push(id);
          return Promise.resolve(binaryHandle);
        }),
        import: vi.fn().mockReturnValue(binaryHandle),
        create: vi.fn().mockReturnValue(binaryHandle),
        flush: vi.fn().mockResolvedValue(undefined),
        networkSubsystem: mockNetworkSubsystem,
      });
      return this as Repo;
    } as unknown as typeof Repo);
    return { findCalls };
  }

  it('prepends the automerge: scheme when callers pass a bare docId', async () => {
    // Index doc handle just so `connect` succeeds. The real assertion
    // is on the *second* find() call (the one inside getBinaryDocById).
    const indexDoc: IndexDocument = { files: {}, version: 2, identities: {} };
    const { handle: indexHandle } = createMockHandle(indexDoc);
    // Binary doc handle returned for the capture lookup. Shape mirrors
    // what `quarto_hub::resource::create_binary_document` writes.
    const binaryContent = new Uint8Array([1, 2, 3]);
    const { handle: binaryHandle } = createMockHandle({
      content: binaryContent,
      mimeType: 'application/x-engine-capture+gzip',
    });

    // First `find` call resolves the index doc; subsequent calls land
    // on `binaryHandle`. The single-handle installSpyRepo collapses
    // both; for this test we only care about the bare-id round trip.
    const { findCalls } = installSpyRepo(binaryHandle);
    // Override: the very first find (during `connect`) needs the
    // index-doc handle, not the binary handle.
    vi.mocked(Repo).mockImplementationOnce(function (this: unknown) {
      Object.assign(this as Record<string, unknown>, {
        find: vi.fn((id: unknown) => {
          findCalls.push(id);
          // Return indexHandle for the connect path, binaryHandle for
          // subsequent calls. Driven by call order, not id-shape.
          return Promise.resolve(findCalls.length === 1 ? indexHandle : binaryHandle);
        }),
        import: vi.fn().mockReturnValue(indexHandle),
        create: vi.fn().mockReturnValue(indexHandle),
        flush: vi.fn().mockResolvedValue(undefined),
        networkSubsystem: { on: vi.fn(), off: vi.fn() },
      });
      return this as Repo;
    } as unknown as typeof Repo);

    const cbs = noopCallbacks();
    const client = createSyncClient(cbs);
    await client.connect('ws://localhost:9999', 'mock-index-id', 'actor-1', 'Alice', '#FF0000');

    const result = await client.getBinaryDocById('bare-capture-id');

    // Second find() call (after connect's index-doc lookup) is the
    // bd-4uvv assertion: bare id must be prefixed.
    expect(findCalls.length).toBeGreaterThanOrEqual(2);
    expect(findCalls[findCalls.length - 1]).toBe('automerge:bare-capture-id');
    // And the returned bytes must round-trip back to the caller.
    // `toStrictEqual` because the mock handle stores a structuredClone,
    // breaking the original Uint8Array's object identity.
    expect(result).not.toBeNull();
    expect(result?.content).toStrictEqual(binaryContent);
    expect(result?.mimeType).toBe('application/x-engine-capture+gzip');
  });

  it('does not double-prefix when the docId already has the scheme', async () => {
    const indexDoc: IndexDocument = { files: {}, version: 2, identities: {} };
    const { handle: indexHandle } = createMockHandle(indexDoc);
    const { handle: binaryHandle } = createMockHandle({
      content: new Uint8Array([0]),
      mimeType: 'application/x-engine-capture+gzip',
    });

    const findCalls: unknown[] = [];
    vi.mocked(Repo).mockImplementation(function (this: unknown) {
      Object.assign(this as Record<string, unknown>, {
        find: vi.fn((id: unknown) => {
          findCalls.push(id);
          return Promise.resolve(findCalls.length === 1 ? indexHandle : binaryHandle);
        }),
        import: vi.fn().mockReturnValue(indexHandle),
        create: vi.fn().mockReturnValue(indexHandle),
        flush: vi.fn().mockResolvedValue(undefined),
        networkSubsystem: { on: vi.fn(), off: vi.fn() },
      });
      return this as Repo;
    } as unknown as typeof Repo);

    const cbs = noopCallbacks();
    const client = createSyncClient(cbs);
    await client.connect('ws://localhost:9999', 'mock-index-id', 'actor-1', 'Alice', '#FF0000');

    await client.getBinaryDocById('automerge:already-prefixed');

    // Idempotent: no `automerge:automerge:...` double-prefix.
    expect(findCalls[findCalls.length - 1]).toBe('automerge:already-prefixed');
  });
});
