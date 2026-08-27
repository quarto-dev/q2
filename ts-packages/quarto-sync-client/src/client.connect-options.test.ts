/**
 * Tests for `connect()`'s options bag (bd-jit6pdwq).
 *
 * Plan: claude-notes/plans/2026-06-11-firefox-ws-peer-timeout-fix.md
 * Research: claude-notes/research/2026-06-11-firefox-ws-handshake-serialization.md
 *
 * The q2-preview SPA needs connection semantics that hub-client does
 * not: an indefinite peer wait (Firefox serializes WS handshakes per
 * IP address, so a hung handshake in *another tab* can stall ours far
 * past any sane finite budget), memory storage (preview docs are
 * ephemeral; IndexedDB can never hit and its open gates the WS join),
 * and recovery when `repo.find()` loses the cold-start sync race.
 * All of it is opt-in via a trailing options bag; defaults preserve
 * hub-client's offline-first behavior exactly.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { DocumentId } from '@automerge/automerge-repo';

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

vi.mock('@automerge/automerge-repo', async (importOriginal) => {
  const original = await importOriginal<typeof import('@automerge/automerge-repo')>();
  return {
    ...original,
    Repo: vi.fn(),
    generateAutomergeUrl: original.generateAutomergeUrl,
    parseAutomergeUrl: original.parseAutomergeUrl,
  };
});

import { Repo } from '@automerge/automerge-repo';
import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb';
import { createSyncClient } from './client.js';
import { MemoryStorageAdapter } from './storage-adapter.js';
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

/** In-memory DocHandle mock (same shape as client.test.ts). */
function createMockHandle<T>(initialDoc: T) {
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

/**
 * Stateful network-subsystem mock that can actually emit events —
 * unlike client.test.ts's inert `{on, off}`, these tests need to
 * deliver `peer` at controlled moments.
 */
function createEmittingNetworkSubsystem() {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  return {
    on: vi.fn((event: string, cb: (payload: unknown) => void) => {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(cb);
    }),
    off: vi.fn((event: string, cb: (payload: unknown) => void) => {
      listeners.get(event)?.delete(cb);
    }),
    emit(event: string, payload: unknown) {
      // Copy: handlers may unsubscribe themselves mid-dispatch.
      for (const cb of [...(listeners.get(event) ?? [])]) cb(payload);
    },
  };
}

interface RepoMockControls {
  net: ReturnType<typeof createEmittingNetworkSubsystem>;
  /** Constructor args captured from `new Repo(...)`. */
  repoCtorArgs: unknown[];
  findCalls: unknown[];
}

/**
 * Install a Repo mock wired to the emitting network subsystem.
 * `findImpl` lets individual tests control find() behavior per call.
 */
function installRepo<T>(
  handle: ReturnType<typeof createMockHandle<T>>['handle'],
  findImpl?: (callIndex: number) => Promise<unknown>,
): RepoMockControls {
  const net = createEmittingNetworkSubsystem();
  const repoCtorArgs: unknown[] = [];
  const findCalls: unknown[] = [];
  vi.mocked(Repo).mockImplementation(function (this: unknown, ...args: unknown[]) {
    repoCtorArgs.push(...args);
    Object.assign(this as Record<string, unknown>, {
      find: vi.fn((id: unknown) => {
        findCalls.push(id);
        return findImpl ? findImpl(findCalls.length - 1) : Promise.resolve(handle);
      }),
      import: vi.fn().mockReturnValue(handle),
      create: vi.fn().mockReturnValue(handle),
      networkSubsystem: net,
      // client.ts trackPeers() sweeps repo.handles on connect
      handles: {},
    });
    return this as Repo;
  } as unknown as typeof Repo);
  return { net, repoCtorArgs, findCalls };
}

/** Flush pending microtasks (works under fake timers too). */
async function flushMicrotasks(rounds = 10) {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

const INDEX_DOC: IndexDocument = { files: {}, version: 2, identities: {} };

describe('connect() options bag', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
    delete (globalThis as Record<string, unknown>)['indexedDB'];
  });

  // ── back-compat ──────────────────────────────────────────────────

  it('accepts the legacy numeric peerTimeoutMs as 6th param', async () => {
    const { handle } = createMockHandle(INDEX_DOC);
    installRepo(handle);
    const client = createSyncClient(noopCallbacks());
    //

    // 1ms-style probe: no peer ever arrives; offline fallback must
    // still work (this is hub-client's documented cold path).
    const files = await client.connect('ws://x', 'mock-doc-id', undefined, undefined, undefined, 1);
    expect(files).toEqual([]);
  });

  it('rejects when auth is passed both positionally and in options', async () => {
    const { handle } = createMockHandle(INDEX_DOC);
    installRepo(handle);
    const client = createSyncClient(noopCallbacks());
    const getBearer = () => Promise.resolve('tok');
    await expect(
      client.connect(
        'ws://x',
        'mock-doc-id',
        undefined,
        undefined,
        undefined,
        { auth: { getBearer } },
        { getBearer },
      ),
    ).rejects.toThrow(TypeError);
  });

  // ── peerTimeoutMs: Infinity ──────────────────────────────────────

  it('peerTimeoutMs: Infinity schedules no timeout and resolves on a late peer', async () => {
    vi.useFakeTimers();
    const { handle } = createMockHandle(INDEX_DOC);
    const { net } = installRepo(handle);
    const cbs = noopCallbacks();
    const client = createSyncClient(cbs);

    const connectPromise = client.connect(
      'ws://x',
      'mock-doc-id',
      undefined,
      undefined,
      undefined,
      { peerTimeoutMs: Infinity },
    );
    // Let connect() reach waitForPeer (async adapter construction).
    await flushMicrotasks();

    // The whole point: no deadline timer exists. (A finite budget
    // would have scheduled exactly one setTimeout by now.) The one
    // live timer is trackPeers' handle-watch interval, not a deadline.
    expect(vi.getTimerCount()).toBe(1);

    // Simulate the peer arriving far later than any old budget.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(vi.getTimerCount()).toBe(1); // still no deadline timer, still waiting
    net.emit('peer', { peerId: 'peer-1' });
    await flushMicrotasks();

    await expect(connectPromise).resolves.toEqual([]);
    // Online mode, never the offline fallback.
    expect(cbs.onConnectionChange).toHaveBeenCalledWith(true);
  });

  it('finite peerTimeoutMs still schedules a deadline timer (control)', async () => {
    vi.useFakeTimers();
    const { handle } = createMockHandle(INDEX_DOC);
    installRepo(handle);
    const client = createSyncClient(noopCallbacks());

    const connectPromise = client.connect(
      'ws://x',
      'mock-doc-id',
      undefined,
      undefined,
      undefined,
      { peerTimeoutMs: 5000 },
    );
    await flushMicrotasks();
    // The deadline timer plus trackPeers' handle-watch interval.
    expect(vi.getTimerCount()).toBe(2);

    await vi.advanceTimersByTimeAsync(5001);
    await expect(connectPromise).resolves.toEqual([]); // offline fallback
  });

  // ── storage selection ────────────────────────────────────────────

  it("storage: 'memory' uses MemoryStorageAdapter even when indexedDB exists", async () => {
    (globalThis as Record<string, unknown>)['indexedDB'] = {};
    const { handle } = createMockHandle(INDEX_DOC);
    const { repoCtorArgs } = installRepo(handle);
    const client = createSyncClient(noopCallbacks());

    await client.connect('ws://x', 'mock-doc-id', undefined, undefined, undefined, {
      storage: 'memory',
    });

    expect(IndexedDBStorageAdapter).not.toHaveBeenCalled();
    const repoOpts = repoCtorArgs[0] as { storage: unknown };
    expect(repoOpts.storage).toBeInstanceOf(MemoryStorageAdapter);
  });

  it('default storage remains IndexedDB when the global exists', async () => {
    (globalThis as Record<string, unknown>)['indexedDB'] = {};
    const { handle } = createMockHandle(INDEX_DOC);
    installRepo(handle);
    const client = createSyncClient(noopCallbacks());

    await client.connect('ws://x', 'mock-doc-id');

    expect(IndexedDBStorageAdapter).toHaveBeenCalledTimes(1);
  });

  // ── retryIntervalMs forwarding ───────────────────────────────────

  it('forwards retryIntervalMs to the browser adapter constructor', async () => {
    const { handle } = createMockHandle(INDEX_DOC);
    installRepo(handle);
    const client = createSyncClient(noopCallbacks());

    await client.connect('ws://x', 'mock-doc-id', undefined, undefined, undefined, {
      retryIntervalMs: 9000,
    });

    expect(BrowserWebSocketClientAdapter).toHaveBeenCalledWith('ws://x', 9000);
  });

  it('omits retryInterval (upstream default) when not configured', async () => {
    const { handle } = createMockHandle(INDEX_DOC);
    installRepo(handle);
    const client = createSyncClient(noopCallbacks());

    await client.connect('ws://x', 'mock-doc-id');

    expect(BrowserWebSocketClientAdapter).toHaveBeenCalledWith('ws://x');
  });

  // ── findDoc unavailable-recovery ─────────────────────────────────

  it('retries find() on "unavailable" when a peer is connected', async () => {
    const { handle } = createMockHandle(INDEX_DOC);
    const { net, findCalls } = installRepo(handle, (i) =>
      i === 0
        ? Promise.reject(new Error('Document mock-doc-id is unavailable'))
        : Promise.resolve(handle),
    );
    const cbs = noopCallbacks();
    const client = createSyncClient(cbs);

    const connectPromise = client.connect('ws://x', 'mock-doc-id', undefined, undefined, undefined, {
      peerTimeoutMs: 1000,
      findDocRetry: { attempts: 3, baseDelayMs: 1 },
    });
    // Deliver the peer so waitForPeer resolves and the retry gate sees
    // a connected peer.
    await flushMicrotasks();
    net.emit('peer', { peerId: 'peer-1' });

    await expect(connectPromise).resolves.toEqual([]);
    expect(findCalls.length).toBe(2);
    expect(cbs.onError).not.toHaveBeenCalled();
  });

  it('does not retry find() when no peer is connected (offline fast-fail)', async () => {
    const { handle } = createMockHandle(INDEX_DOC);
    const { findCalls } = installRepo(handle, () =>
      Promise.reject(new Error('Document mock-doc-id is unavailable')),
    );
    const client = createSyncClient(noopCallbacks());

    await expect(
      client.connect('ws://x', 'mock-doc-id', undefined, undefined, undefined, {
        peerTimeoutMs: 1,
        findDocRetry: { attempts: 3, baseDelayMs: 1 },
      }),
    ).rejects.toThrow(/unavailable/);
    expect(findCalls.length).toBe(1);
  });

  it('gives up after the configured retry attempts', async () => {
    const { handle } = createMockHandle(INDEX_DOC);
    const { net, findCalls } = installRepo(handle, () =>
      Promise.reject(new Error('Document mock-doc-id is unavailable')),
    );
    const client = createSyncClient(noopCallbacks());

    const connectPromise = client.connect('ws://x', 'mock-doc-id', undefined, undefined, undefined, {
      peerTimeoutMs: 1000,
      findDocRetry: { attempts: 2, baseDelayMs: 1 },
    });
    await flushMicrotasks();
    net.emit('peer', { peerId: 'peer-1' });

    await expect(connectPromise).rejects.toThrow(/unavailable/);
    expect(findCalls.length).toBe(3); // initial + 2 retries
  });

  it('rethrows non-"unavailable" find() errors without retrying', async () => {
    const { handle } = createMockHandle(INDEX_DOC);
    const { net, findCalls } = installRepo(handle, () =>
      Promise.reject(new Error('Invalid AutomergeUrl: nonsense')),
    );
    const client = createSyncClient(noopCallbacks());

    const connectPromise = client.connect('ws://x', 'mock-doc-id', undefined, undefined, undefined, {
      peerTimeoutMs: 1000,
      findDocRetry: { attempts: 3, baseDelayMs: 1 },
    });
    await flushMicrotasks();
    net.emit('peer', { peerId: 'peer-1' });

    await expect(connectPromise).rejects.toThrow(/Invalid AutomergeUrl/);
    expect(findCalls.length).toBe(1);
  });
});
