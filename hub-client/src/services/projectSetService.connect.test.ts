/**
 * Tests for connectCollection failure classification (bd-tux4m6od).
 *
 * The join/connect path used to surface automerge-repo's raw
 * `Document <id> is unavailable` for every failure shape. These tests
 * drive connectCollection against a real in-process automerge "server"
 * Repo behind a controllable fake websocket adapter, and pin:
 *
 *  - the 1 s forceReady race fix: a peer that connects only after
 *    `repo.find()` already failed still produces a successful join;
 *  - `not-found` when a live sync peer genuinely lacks the document;
 *  - `auth-expired` / `offline` / `sync-unreachable` classification via
 *    the /auth/me probe when no sync peer can be established;
 *  - the local-cache fast path keeps working with no network at all.
 *
 * The fake adapter mimics the real BrowserWebSocketClientAdapter's
 * contract, including its "force ready after a timeout even with no
 * connection" behavior (WebSocketClientAdapter.js marks itself ready
 * after 1 s so requests don't block forever) — that behavior is exactly
 * what turned a slow websocket into "Document … is unavailable".
 */

import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { Repo, NetworkAdapter } from '@automerge/automerge-repo';
import type { Message, PeerId, PeerMetadata } from '@automerge/automerge-repo';
import { CURRENT_PROJECT_SET_SCHEMA_VERSION } from '@quarto/quarto-automerge-schema';
import { CollectionConnectError } from './collectionConnectError';

// ============================================================================
// Fake BrowserWebSocketClientAdapter (client side)
// ============================================================================

interface FakeAdapterHooks {
  /** Delay before the adapter force-marks itself ready (real: 1000 ms). */
  forceReadyMs: number;
  /** ms after connect() at which the "socket opens" (peer handshake with
   * the harness server); null = never connects. */
  openAfterMs: number | null;
  /** The server harness to open against (when openAfterMs !== null). */
  server: ServerHarness | null;
}

const control = vi.hoisted(() => ({
  hooks: {
    forceReadyMs: 10,
    openAfterMs: null as number | null,
    server: null as unknown,
  },
}));

vi.mock('@automerge/automerge-repo-network-websocket', async () => {
  const { NetworkAdapter: Base } = await import('@automerge/automerge-repo');

  class FakeBrowserWebSocketClientAdapter extends Base {
    url: string;
    remote: { deliver(message: unknown): void; drop(): void } | null = null;
    #ready = false;
    #readyResolvers: Array<() => void> = [];

    constructor(url: string) {
      super();
      this.url = url;
    }

    isReady() {
      return this.#ready;
    }

    whenReady(): Promise<void> {
      if (this.#ready) return Promise.resolve();
      return new Promise((resolve) => this.#readyResolvers.push(resolve));
    }

    #forceReady() {
      if (this.#ready) return;
      this.#ready = true;
      this.#readyResolvers.forEach((resolve) => resolve());
      this.#readyResolvers = [];
    }

    connect(peerId: PeerId, peerMetadata?: PeerMetadata) {
      this.peerId = peerId;
      this.peerMetadata = peerMetadata;
      // Mimic the real adapter: mark ready after a timeout whether or not
      // any connection was established.
      setTimeout(() => this.#forceReady(), control.hooks.forceReadyMs);
      const { openAfterMs, server } = control.hooks;
      if (openAfterMs !== null && server) {
        setTimeout(() => {
          (server as ServerHarness).open(this);
          this.#forceReady();
        }, openAfterMs);
      }
    }

    send(message: Message) {
      this.remote?.deliver(message);
    }

    disconnect() {
      this.remote?.drop();
      this.remote = null;
    }
  }

  return { BrowserWebSocketClientAdapter: FakeBrowserWebSocketClientAdapter };
});

// Instances of the mocked adapter class (typed loosely; the class is
// created inside the mock factory).
type FakeAdapter = InstanceType<typeof NetworkAdapter> & {
  remote: { deliver(message: unknown): void; drop(): void } | null;
  emit(event: string, payload: unknown): void;
  peerId?: PeerId;
};

// ============================================================================
// /auth/me probe mock
// ============================================================================

const auth = vi.hoisted(() => ({
  fetchAuthMe: vi.fn<() => Promise<{ email: string } | null>>(),
}));

vi.mock('./authService', () => ({
  fetchAuthMe: auth.fetchAuthMe,
}));

// ============================================================================
// In-process sync server harness
// ============================================================================

/** Server end of the loopback connection: a NetworkAdapter feeding a real
 * Repo, delivering messages to/from the fake client adapter. */
class ServerSideAdapter extends NetworkAdapter {
  client: FakeAdapter | null = null;

  isReady() {
    return true;
  }
  whenReady(): Promise<void> {
    return Promise.resolve();
  }
  connect(peerId: PeerId, peerMetadata?: PeerMetadata) {
    this.peerId = peerId;
    this.peerMetadata = peerMetadata;
  }
  send(message: Message) {
    const client = this.client;
    if (!client) return;
    setTimeout(() => client.emit('message', message), 0);
  }
  disconnect() {
    this.client = null;
  }
}

class ServerHarness {
  repo: Repo;
  adapter: ServerSideAdapter;

  constructor() {
    this.adapter = new ServerSideAdapter();
    // Ephemeral in-memory server repo; generous share policy like a hub.
    this.repo = new Repo({
      network: [this.adapter],
      sharePolicy: async () => true,
    });
  }

  /** Create a collection document on the server; returns its doc id. */
  createCollectionDoc(name: string): string {
    const handle = this.repo.create({
      projects: {},
      version: CURRENT_PROJECT_SET_SCHEMA_VERSION,
      name,
    });
    return handle.documentId;
  }

  /** Complete the "socket open" handshake with a client adapter. */
  open(clientAdapter: FakeAdapter) {
    this.adapter.client = clientAdapter;
    clientAdapter.remote = {
      deliver: (message: unknown) =>
        setTimeout(() => this.adapter.emit('message', message as Message), 0),
      drop: () => {
        this.adapter.client = null;
      },
    };
    // Announce each side to the other (what the real ws handshake does).
    clientAdapter.emit('peer-candidate', {
      peerId: this.repo.peerId,
      peerMetadata: {},
    });
    this.adapter.emit('peer-candidate', {
      peerId: clientAdapter.peerId!,
      peerMetadata: {},
    });
  }
}

// ============================================================================
// Test setup
// ============================================================================

// The service under test — static import AFTER the mocks above.
import * as projectSetService from './projectSetService';

/** Short tuning so failure paths resolve quickly in tests. */
const FAST = {
  attemptTimeoutMs: 500,
  connectWaitMs: 300,
  docWaitMs: 500,
};

/** Generous tuning for success paths that must survive a slow connect. */
const PATIENT = {
  attemptTimeoutMs: 2000,
  connectWaitMs: 2000,
  docWaitMs: 2000,
};

let urlCounter = 0;
/** Unique sync-server URL per test so each test gets a fresh Repo. */
function freshUrl(): string {
  urlCounter += 1;
  return `wss://test-${urlCounter}.example.com/ws`;
}

async function expectConnectError(
  promise: Promise<unknown>,
): Promise<CollectionConnectError> {
  try {
    await promise;
  } catch (err) {
    expect(err).toBeInstanceOf(CollectionConnectError);
    return err as CollectionConnectError;
  }
  throw new Error('expected connectCollection to reject');
}

beforeEach(() => {
  Object.defineProperty(globalThis, 'indexedDB', {
    value: new IDBFactory(),
    writable: true,
    configurable: true,
  });
  control.hooks.forceReadyMs = 10;
  control.hooks.openAfterMs = null;
  control.hooks.server = null;
  auth.fetchAuthMe.mockReset();
});

afterEach(async () => {
  vi.unstubAllEnvs();
  await projectSetService.disconnect();
});

// ============================================================================
// Tests
// ============================================================================

describe('connectCollection failure classification', () => {
  it('race fix: a peer connecting after the ready-timeout still joins successfully', async () => {
    // The adapter force-readies at 10 ms (so repo.find() fails its first
    // attempt with zero peers) but the "socket" only opens at 150 ms.
    // Old behavior: raw "Document <id> is unavailable". New behavior:
    // wait for the peer, re-request, succeed.
    const server = new ServerHarness();
    const docId = server.createCollectionDoc('Team');
    control.hooks.server = server;
    control.hooks.openAfterMs = 150;

    const snapshot = await projectSetService.connectCollection(
      { projectSetDocId: docId, syncServer: freshUrl() },
      PATIENT,
    );

    expect(snapshot.docId).toBe(docId);
    expect(snapshot.name).toBe('Team');
    expect(snapshot.entries).toEqual([]);
  });

  it('not-found: a live sync peer that lacks the document', async () => {
    const server = new ServerHarness();
    control.hooks.server = server;
    control.hooks.openAfterMs = 0;

    // A valid doc id the server has never seen (created on a detached repo).
    const detached = new Repo({});
    const missingDocId = detached.create({ v: 1 }).documentId;

    const err = await expectConnectError(
      projectSetService.connectCollection(
        { projectSetDocId: missingDocId, syncServer: freshUrl() },
        FAST,
      ),
    );
    expect(err.kind).toBe('not-found');
    expect(err.docId).toBe(missingDocId);
    expect(err.message).toContain(missingDocId);
    expect(err.message).toContain('sync server');
  });

  it('auth-expired: no sync peer and /auth/me rejects the session (auth enabled)', async () => {
    vi.stubEnv('VITE_GOOGLE_CLIENT_ID', 'test-client-id');
    auth.fetchAuthMe.mockResolvedValue(null); // 401/403 shape
    const server = new ServerHarness();
    const docId = server.createCollectionDoc('Team');
    // Server exists but the "socket" never opens (as with a 401 upgrade).
    control.hooks.server = null;
    control.hooks.openAfterMs = null;

    const err = await expectConnectError(
      projectSetService.connectCollection(
        { projectSetDocId: docId, syncServer: freshUrl() },
        FAST,
      ),
    );
    expect(err.kind).toBe('auth-expired');
    expect(err.message).toContain('session has expired');
  });

  it('auth-disabled builds never report auth-expired: a 401 probe maps to sync-unreachable', async () => {
    // Without VITE_GOOGLE_CLIENT_ID (local-prod / --allow-insecure-auth),
    // /auth/me always 401s; that must not read as session expiry.
    auth.fetchAuthMe.mockResolvedValue(null);
    const server = new ServerHarness();
    const docId = server.createCollectionDoc('Team');

    const err = await expectConnectError(
      projectSetService.connectCollection(
        { projectSetDocId: docId, syncServer: freshUrl() },
        FAST,
      ),
    );
    expect(err.kind).toBe('sync-unreachable');
  });

  it('offline: no sync peer and /auth/me is unreachable', async () => {
    auth.fetchAuthMe.mockRejectedValue(new TypeError('fetch failed'));
    const server = new ServerHarness();
    const docId = server.createCollectionDoc('Team');

    const err = await expectConnectError(
      projectSetService.connectCollection(
        { projectSetDocId: docId, syncServer: freshUrl() },
        FAST,
      ),
    );
    expect(err.kind).toBe('offline');
    expect(err.message).toContain('offline');
  });

  it('sync-unreachable: no sync peer but /auth/me says the session is fine', async () => {
    auth.fetchAuthMe.mockResolvedValue({ email: 'user@example.com' });
    const server = new ServerHarness();
    const docId = server.createCollectionDoc('Team');

    const err = await expectConnectError(
      projectSetService.connectCollection(
        { projectSetDocId: docId, syncServer: freshUrl() },
        FAST,
      ),
    );
    expect(err.kind).toBe('sync-unreachable');
    expect(err.message).toContain('sync connection');
  });

  it('local-cache fast path: a previously-synced collection loads with no network', async () => {
    // Phase 1: connect online so the doc lands in (fake) IndexedDB.
    const server = new ServerHarness();
    const docId = server.createCollectionDoc('Cached');
    control.hooks.server = server;
    control.hooks.openAfterMs = 0;
    const url = freshUrl();

    const first = await projectSetService.connectCollection(
      { projectSetDocId: docId, syncServer: url },
      PATIENT,
    );
    expect(first.name).toBe('Cached');

    // Let the client repo's debounced storage write flush (default 100 ms).
    await new Promise((resolve) => setTimeout(resolve, 400));
    await projectSetService.disconnect();

    // Phase 2: same storage, no network at all — must still resolve.
    control.hooks.server = null;
    control.hooks.openAfterMs = null;
    auth.fetchAuthMe.mockRejectedValue(new TypeError('fetch failed'));

    const second = await projectSetService.connectCollection(
      { projectSetDocId: docId, syncServer: url },
      FAST,
    );
    expect(second.docId).toBe(docId);
    expect(second.name).toBe('Cached');
  });
});
