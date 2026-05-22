/**
 * Phase 8 — connection-manager tests.
 *
 * Exercises the try-then-fallback auth policy, the
 * `lastObservedAuthMode` state machine, the insecure-transport gate,
 * and the redact-everywhere invariant.
 *
 * Tests inject:
 *   - a fake `fetch` so `/health` probe outcomes are scripted;
 *   - a fake `syncClientFactory` so we don't open a real WS;
 *   - a `RefreshManager` whose `getValidIdToken` / `forceRefresh` are
 *     vi-mocked so we can drive the 401-then-refresh-then-retry path.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { SyncClient, SyncClientCallbacks } from '@quarto/quarto-sync-client';

import {
  AuthRequiredError,
  ConnectionManager,
  InsecureTransportError,
  isLoopbackHost,
} from './connection-manager.js';
import {
  CredentialStore,
  type CredentialBundle,
  type KeyringBackend,
} from './auth/credential-store.js';
import { ReauthRequired, RefreshManager } from './auth/refresh-manager.js';

// ---------------------------------------------------------------------------
// Fake CredentialStore / RefreshManager helpers
// ---------------------------------------------------------------------------

function inMemoryBackend(initial?: string | null): KeyringBackend {
  let v: string | null = initial ?? null;
  return {
    async read() {
      return v;
    },
    async write(value: string) {
      v = value;
    },
    async clear() {
      const had = v !== null;
      v = null;
      return had;
    },
  };
}

function bundleSerialized(bundle: CredentialBundle, issuer: string, clientId: string): string {
  return JSON.stringify({
    schema_version: 1,
    issuer,
    client_id: clientId,
    id_token: bundle.idToken,
    refresh_token: bundle.refreshToken,
    id_token_expires_at: bundle.idTokenExpiresAt.toISOString(),
    scopes: bundle.scopes,
  });
}

interface SeededAuth {
  store: CredentialStore;
  refresh: RefreshManager;
  getValid: ReturnType<typeof vi.fn>;
  forceRefresh: ReturnType<typeof vi.fn>;
}

function seededAuth(opts?: { initialToken?: string }): SeededAuth {
  const issuer = 'https://accounts.google.com';
  const clientId = 'test-client.apps.googleusercontent.com';

  const initial: CredentialBundle = {
    idToken: opts?.initialToken ?? 'initial-id-token',
    refreshToken: 'refresh-token-value',
    idTokenExpiresAt: new Date(Date.now() + 60 * 60 * 1000),
    scopes: ['openid', 'email', 'profile'],
  };
  const backend = inMemoryBackend(bundleSerialized(initial, issuer, clientId));
  const store = new CredentialStore({ issuer, clientId }, backend);

  // We bypass RefreshManager's actual /token round-trip by replacing
  // the two public methods. The connection-manager only calls these
  // two; this keeps the test focused on the auth dispatch logic.
  const refresh = Object.create(RefreshManager.prototype) as RefreshManager;
  const getValid = vi.fn().mockResolvedValue(initial.idToken);
  const forceRefresh = vi.fn().mockResolvedValue(initial.idToken);
  Object.defineProperty(refresh, 'getValidIdToken', { value: getValid });
  Object.defineProperty(refresh, 'forceRefresh', { value: forceRefresh });

  return { store, refresh, getValid, forceRefresh };
}

// ---------------------------------------------------------------------------
// Sync-client factory fake
// ---------------------------------------------------------------------------

interface SyncClientCallSpy {
  factory: (cbs: SyncClientCallbacks) => SyncClient;
  connectCalls: Array<{ url: string; indexDocId: string; auth: unknown }>;
  createCalls: Array<{ url: string; auth: unknown }>;
}

function spySyncClientFactory(): SyncClientCallSpy {
  const connectCalls: SyncClientCallSpy['connectCalls'] = [];
  const createCalls: SyncClientCallSpy['createCalls'] = [];
  const factory = (cbs: SyncClientCallbacks): SyncClient => {
    const stub: Partial<SyncClient> = {
      connect: vi.fn(
        async (
          syncServerUrl: string,
          indexDocId: string,
          _actorId?: string,
          _screenName?: string,
          _color?: string,
          _peerTimeoutMs?: number,
          auth?: unknown,
        ) => {
          connectCalls.push({ url: syncServerUrl, indexDocId, auth });
          // Pretend a file came in so callbacks don't choke.
          cbs.onFileAdded?.('test.qmd', { type: 'text', text: '' });
          return [];
        },
      ) as unknown as SyncClient['connect'],
      createNewProject: vi.fn(
        async (
          options: { syncServer: string; auth?: unknown },
        ) => {
          createCalls.push({ url: options.syncServer, auth: options.auth });
          return { indexDocId: 'idx-new', files: [] };
        },
      ) as unknown as SyncClient['createNewProject'],
      disconnect: vi.fn().mockResolvedValue(undefined) as unknown as SyncClient['disconnect'],
    };
    return stub as SyncClient;
  };
  return { factory, connectCalls, createCalls };
}

// ---------------------------------------------------------------------------
// Fetch mock helpers
// ---------------------------------------------------------------------------

interface FetchSpy {
  fetch: typeof fetch;
  calls: Array<{ url: string; headers: Record<string, string> }>;
}

function scriptedFetch(responses: Array<number | Error>): FetchSpy {
  const calls: FetchSpy['calls'] = [];
  let i = 0;
  const fetchImpl: typeof fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : (input as URL).toString();
    const rawHeaders = (init?.headers as Record<string, string>) ?? {};
    calls.push({ url, headers: rawHeaders });
    const next = responses[i++];
    if (next === undefined) {
      throw new Error(`scriptedFetch ran out of responses (call #${i})`);
    }
    if (next instanceof Error) throw next;
    return new Response('', { status: next });
  };
  return { fetch: fetchImpl, calls };
}

// ---------------------------------------------------------------------------
// Spy console capture
// ---------------------------------------------------------------------------

let consoleSpies: ReturnType<typeof vi.spyOn>[];

beforeEach(() => {
  consoleSpies = [
    vi.spyOn(console, 'log').mockImplementation(() => undefined),
    vi.spyOn(console, 'warn').mockImplementation(() => undefined),
    vi.spyOn(console, 'error').mockImplementation(() => undefined),
    vi.spyOn(console, 'debug').mockImplementation(() => undefined),
  ];
});

afterEach(() => {
  for (const s of consoleSpies) s.mockRestore();
  vi.restoreAllMocks();
});

// ---------------------------------------------------------------------------
// Loopback host helper
// ---------------------------------------------------------------------------

describe('isLoopbackHost', () => {
  it.each([
    ['localhost', true],
    ['127.0.0.1', true],
    ['::1', true],
    ['[::1]', true],
    ['hub.localhost', true],
    ['hub.example.com', false],
    ['localhost.attacker.com', false],
  ])('classifies %s as loopback=%s', (host, expected) => {
    expect(isLoopbackHost(host)).toBe(expected);
  });
});

// ---------------------------------------------------------------------------
// No-creds → no-auth hub
// ---------------------------------------------------------------------------

describe('ConnectionManager without creds', () => {
  it('succeeds against a no-auth hub without sending Authorization', async () => {
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();

    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    expect(fetchSpy.calls).toHaveLength(1);
    expect(fetchSpy.calls[0]!.headers.Authorization).toBeUndefined();
    expect(sync.connectCalls).toHaveLength(1);
    expect(sync.connectCalls[0]!.auth).toBeUndefined();
    expect(mgr.lastObservedAuthMode()).toBe('no-auth');
  });

  it('surfaces AuthRequired when hub demands auth and no creds attached', async () => {
    const fetchSpy = scriptedFetch([401]);
    const sync = spySyncClientFactory();

    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(AuthRequiredError);
    expect(sync.connectCalls).toHaveLength(0);
    expect(mgr.lastObservedAuthMode()).toBe('requires-auth');
  });
});

// ---------------------------------------------------------------------------
// With creds — happy + refresh-on-401 + reauth
// ---------------------------------------------------------------------------

describe('ConnectionManager with creds', () => {
  it('threads Bearer through fetch and into the sync-client auth getter', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();

    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    expect(fetchSpy.calls[0]!.headers.Authorization).toBe('Bearer initial-id-token');
    expect(sync.connectCalls).toHaveLength(1);
    const passed = sync.connectCalls[0]!.auth as
      | { getBearer: () => Promise<string> }
      | undefined;
    expect(passed).toBeDefined();
    // getBearer should pull a fresh token through the refresh manager.
    await expect(passed!.getBearer()).resolves.toBe('initial-id-token');
    expect(auth.getValid).toHaveBeenCalled();
    expect(mgr.lastObservedAuthMode()).toBe('requires-auth');
  });

  it('handles 401-then-refresh-then-200 with one extra probe', async () => {
    const auth = seededAuth();
    auth.getValid.mockResolvedValueOnce('stale-token');
    auth.forceRefresh.mockResolvedValueOnce('fresh-token');
    const fetchSpy = scriptedFetch([401, 200]);
    const sync = spySyncClientFactory();

    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    expect(fetchSpy.calls).toHaveLength(2);
    expect(fetchSpy.calls[0]!.headers.Authorization).toBe('Bearer stale-token');
    expect(fetchSpy.calls[1]!.headers.Authorization).toBe('Bearer fresh-token');
    expect(auth.forceRefresh).toHaveBeenCalledOnce();
    expect(sync.connectCalls).toHaveLength(1);
    expect(mgr.lastObservedAuthMode()).toBe('requires-auth');
  });

  it('surfaces ReauthRequired after a second consecutive 401', async () => {
    const auth = seededAuth();
    auth.forceRefresh.mockResolvedValueOnce('still-bad-token');
    const fetchSpy = scriptedFetch([401, 401]);
    const sync = spySyncClientFactory();

    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(ReauthRequired);

    expect(fetchSpy.calls).toHaveLength(2);
    expect(sync.connectCalls).toHaveLength(0);
    expect(mgr.lastObservedAuthMode()).toBe('requires-auth');
  });

  it('clears the credential store on persistent 401 so authenticate_start starts a fresh flow', async () => {
    // Regression: before this, ConnectionManager threw ReauthRequired
    // but left a freshly-refreshed id_token in the store. The next
    // `authenticate_start` would then short-circuit on
    // `RefreshManager.getValidIdToken()` and reply "Already
    // authenticated as …", trapping the agent in a state mismatch
    // between local view (token works) and hub view (token rejected).
    // After this fix, the store is cleared on persistent 401 so the
    // next `getValidIdToken` raises ReauthRequired and the device
    // flow runs.
    const auth = seededAuth();
    auth.forceRefresh.mockResolvedValueOnce('still-bad-token');
    const fetchSpy = scriptedFetch([401, 401]);
    const sync = spySyncClientFactory();

    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });

    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(ReauthRequired);
    expect(await auth.store.read()).toBeNull();
  });

  it('succeeds against a no-auth hub even with stale creds in the store', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();

    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    expect(sync.connectCalls).toHaveLength(1);
    // 200 + creds attached → 'requires-auth' (conservative).
    expect(mgr.lastObservedAuthMode()).toBe('requires-auth');
  });
});

// ---------------------------------------------------------------------------
// Insecure-transport gate
// ---------------------------------------------------------------------------

describe('Insecure-transport gate', () => {
  it('allows Bearer over loopback ws:// without env flag', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'ws://localhost:3000/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
      env: {},
    });
    await mgr.connect('idx-1');
    expect(sync.connectCalls).toHaveLength(1);
  });

  it('refuses Bearer over ws:// to non-loopback without env flag', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([]); // no HTTP issued
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'ws://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
      env: {},
    });
    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(InsecureTransportError);
    expect(fetchSpy.calls).toHaveLength(0);
    expect(sync.connectCalls).toHaveLength(0);
  });

  it('permits Bearer over ws:// to non-loopback with env flag, with warning', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'ws://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
      env: { QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH: '1' },
    });
    await mgr.connect('idx-1');
    expect(sync.connectCalls).toHaveLength(1);
    const warnCalls = (console.warn as unknown as { mock: { calls: unknown[][] } })
      .mock.calls;
    expect(warnCalls.length).toBeGreaterThanOrEqual(1);
    const text = warnCalls.flat().map(String).join(' ');
    expect(text).toContain('QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH');
  });

  it('skips the gate entirely when no Bearer is being attached', async () => {
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'ws://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
      env: {},
    });
    await mgr.connect('idx-1');
    expect(sync.connectCalls).toHaveLength(1);
  });

  it('permits Bearer over wss:// to non-loopback without env flag', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
      env: {},
    });
    await mgr.connect('idx-1');
    expect(sync.connectCalls).toHaveLength(1);
  });
});

// ---------------------------------------------------------------------------
// lastObservedAuthMode state machine
// ---------------------------------------------------------------------------

describe('lastObservedAuthMode state machine', () => {
  it('starts as unknown', () => {
    const mgr = new ConnectionManager({ serverUrl: 'wss://hub.example.com/ws' });
    expect(mgr.lastObservedAuthMode()).toBe('unknown');
  });

  it('flips to no-auth on 200 without creds attached', async () => {
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');
    expect(mgr.lastObservedAuthMode()).toBe('no-auth');
  });

  it('flips to requires-auth on 200 with creds attached', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');
    expect(mgr.lastObservedAuthMode()).toBe('requires-auth');
  });

  it('flips to requires-auth on 401 (with or without creds)', async () => {
    const fetchSpy = scriptedFetch([401]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(AuthRequiredError);
    expect(mgr.lastObservedAuthMode()).toBe('requires-auth');
  });

  it('stays unchanged on a network error', async () => {
    const fetchSpy = scriptedFetch([new Error('connect ECONNREFUSED')]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await expect(mgr.connect('idx-1')).rejects.toThrowError(/Failed to reach Quarto Hub/);
    expect(mgr.lastObservedAuthMode()).toBe('unknown');
  });
});

// ---------------------------------------------------------------------------
// Redaction invariants
// ---------------------------------------------------------------------------

describe('redact-everywhere', () => {
  it('does not log the Bearer token in any console sink', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    const all = consoleSpies.flatMap((spy) =>
      (spy.mock.calls as unknown[][]).flat().map(String),
    );
    for (const line of all) {
      expect(line).not.toContain('initial-id-token');
      expect(line).not.toContain('Bearer ');
    }
  });
});
