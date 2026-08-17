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
  HubAccessDeniedError,
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
  invalidate: ReturnType<typeof vi.fn>;
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
  // The real RefreshManager.invalidate clears its own store; the mock
  // routes to the same real store so persistent-401 clear assertions hold.
  const invalidate = vi.fn(async () => {
    await store.clear();
  });
  Object.defineProperty(refresh, 'getValidIdToken', { value: getValid });
  Object.defineProperty(refresh, 'forceRefresh', { value: forceRefresh });
  Object.defineProperty(refresh, 'invalidate', { value: invalidate });

  return { store, refresh, getValid, forceRefresh, invalidate };
}

// ---------------------------------------------------------------------------
// Sync-client factory fake
// ---------------------------------------------------------------------------

interface SyncClientCallSpy {
  factory: (cbs: SyncClientCallbacks) => SyncClient;
  connectCalls: Array<{
    url: string;
    indexDocId: string;
    auth: unknown;
    options: Record<string, unknown>;
  }>;
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
          peerTimeoutMsOrOptions?: number | Record<string, unknown>,
          positionalAuth?: unknown,
        ) => {
          // The manager passes the ConnectOptions bag (auth +
          // requireOnline + peerTimeoutMs, bd-xnmd5ni1); the legacy
          // positional auth is still captured for contract coverage.
          const options =
            typeof peerTimeoutMsOrOptions === 'object' && peerTimeoutMsOrOptions !== null
              ? peerTimeoutMsOrOptions
              : {};
          connectCalls.push({
            url: syncServerUrl,
            indexDocId,
            auth: (options as { auth?: unknown }).auth ?? positionalAuth,
            options,
          });
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
    // Server-backed clients demand a live peer (bd-xnmd5ni1).
    expect(sync.connectCalls[0]!.options['requireOnline']).toBe(true);
    expect(sync.connectCalls[0]!.options['peerTimeoutMs']).toBeGreaterThan(1000);
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

  it('clears the credential store on persistent 401 so authenticate starts a fresh flow', async () => {
    // Regression: before this, ConnectionManager threw ReauthRequired
    // but left a freshly-refreshed id_token in the store. The next
    // `authenticate` would then short-circuit on
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
// Probe-skip optimization (don't re-probe once auth-mode is established)
// ---------------------------------------------------------------------------

describe('ConnectionManager probe-skip', () => {
  it('skips the /health probe on a later connect once creds are confirmed', async () => {
    const auth = seededAuth();
    // Only one scripted response: a second probe would throw "ran out of
    // responses", so this asserts the probe is not re-issued.
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
    await mgr.connect('idx-2');

    expect(fetchSpy.calls).toHaveLength(1);
    expect(sync.connectCalls).toHaveLength(2);
    // Still resolves a token for the skipped connect (proactive refresh /
    // early error surfacing).
    expect(auth.getValid).toHaveBeenCalled();
    expect(mgr.lastObservedAuthMode()).toBe('requires-auth');
  });

  it('skips the probe on a later connect to a known no-auth hub (no creds)', async () => {
    const fetchSpy = scriptedFetch([200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });

    await mgr.connect('idx-1');
    await mgr.connect('idx-2');

    expect(fetchSpy.calls).toHaveLength(1);
    expect(sync.connectCalls).toHaveLength(2);
    expect(sync.connectCalls.every((c) => c.auth === undefined)).toBe(true);
  });

  it('throws AuthRequired without re-probing once the hub is known to require auth (no creds)', async () => {
    const fetchSpy = scriptedFetch([401]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });

    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(AuthRequiredError);
    await expect(mgr.connect('idx-2')).rejects.toBeInstanceOf(AuthRequiredError);

    expect(fetchSpy.calls).toHaveLength(1);
    expect(sync.connectCalls).toHaveLength(0);
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
// waitForChange (long-poll)
// ---------------------------------------------------------------------------

/**
 * Sync-client factory that captures the callbacks object so a test can
 * drive `onFileChanged` / `onFileRemoved` to simulate remote edits. The
 * `connect` stub seeds one text file ('test.qmd' = 'hello') via
 * `onFileAdded`, mirroring a real initial sync.
 */
function capturingSyncClientFactory() {
  let captured: SyncClientCallbacks | undefined;
  const factory = (cbs: SyncClientCallbacks): SyncClient => {
    captured = cbs;
    const stub: Partial<SyncClient> = {
      connect: vi.fn(async () => {
        cbs.onFileAdded?.('test.qmd', { type: 'text', text: 'hello' });
        return [];
      }) as unknown as SyncClient['connect'],
      disconnect: vi.fn().mockResolvedValue(undefined) as unknown as SyncClient['disconnect'],
    };
    return stub as SyncClient;
  };
  return { factory, cbs: () => captured! };
}

describe('waitForChange (long-poll)', () => {
  it('resolves when the watched file changes', async () => {
    const fetchSpy = scriptedFetch([200]);
    const cap = capturingSyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: cap.factory,
    });
    await mgr.connect('idx-1');

    const pending = mgr.waitForChange('idx-1', 'test.qmd', 5000);
    // Simulate a remote edit arriving on the watched path.
    cap.cbs().onFileChanged('test.qmd', 'hello world', []);

    const res = await pending;
    expect(res.changed).toBe(true);
    expect(res.payload?.type).toBe('text');
    expect((res.payload as { type: 'text'; text: string }).text).toBe('hello world');
    expect(res.hash).toMatch(/^sha256:/);
  });

  it('times out with changed=false when nothing changes', async () => {
    const fetchSpy = scriptedFetch([200]);
    const cap = capturingSyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: cap.factory,
    });
    await mgr.connect('idx-1');

    const res = await mgr.waitForChange('idx-1', 'test.qmd', 20);
    expect(res.changed).toBe(false);
  });

  it('returns immediately when since_hash differs from current (gap-close)', async () => {
    const fetchSpy = scriptedFetch([200]);
    const cap = capturingSyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: cap.factory,
    });
    await mgr.connect('idx-1');

    // No onFileChanged fired; current is the seeded 'hello'. A stale
    // baseline hash means a change already happened in the gap → resolve now.
    const res = await mgr.waitForChange('idx-1', 'test.qmd', 5000, 'sha256:stale');
    expect(res.changed).toBe(true);
    expect((res.payload as { type: 'text'; text: string }).text).toBe('hello');
  });

  it('ignores changes to other paths', async () => {
    const fetchSpy = scriptedFetch([200]);
    const cap = capturingSyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      fetch: fetchSpy.fetch,
      syncClientFactory: cap.factory,
    });
    await mgr.connect('idx-1');

    const pending = mgr.waitForChange('idx-1', 'test.qmd', 40);
    // A change to a different file must not satisfy the waiter.
    cap.cbs().onFileChanged('other.qmd', 'nope', []);
    const res = await pending;
    expect(res.changed).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Mid-session auth rejection (bd-l3b1brn8)
// ---------------------------------------------------------------------------
//
// The sync adapter reports definitive evidence (upgrade 401/403,
// terminal refresh failure) through `onAuthRejected`; the manager owns
// policy: one forceRefresh+reprobe cycle on 401 (coalesced across
// projects), invalidate + reauth-required on persistent 401, a
// keyring-preserving terminal "not allowed" state on 403. Network
// errors never reach this path (the adapter never reports them).

/** The upgrade-401 evidence shape the adapter reports. */
const evidence401 = { kind: 'upgrade-status', status: 401 } as const;
const evidence403 = { kind: 'upgrade-status', status: 403 } as const;

describe('mid-session auth rejection', () => {
  it('wires onAuthRejected into the sync-client auth options', async () => {
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

    const passed = sync.connectCalls[0]!.auth as {
      onAuthRejected?: (evidence: unknown) => void;
    };
    expect(typeof passed.onAuthRejected).toBe('function');
  });

  it('recovers silently when one forceRefresh+reprobe fixes a 401', async () => {
    const auth = seededAuth();
    auth.forceRefresh.mockResolvedValue('fresh-token');
    // initial probe 200, recheck probe 200.
    const fetchSpy = scriptedFetch([200, 200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    await mgr.handleAuthRejected(evidence401);

    expect(auth.forceRefresh).toHaveBeenCalledOnce();
    expect(fetchSpy.calls).toHaveLength(2);
    expect(fetchSpy.calls[1]!.headers.Authorization).toBe('Bearer fresh-token');
    // Recovery is invisible: no invalidate, project handle intact — the
    // adapter's own retry picks the fresh token up via getBearer.
    expect(auth.invalidate).not.toHaveBeenCalled();
    expect(mgr.get('idx-1')).toBeDefined();
  });

  it('invalidates and enters reauth-required on persistent 401; the next call fails fast with ReauthRequired', async () => {
    const auth = seededAuth();
    auth.forceRefresh.mockResolvedValue('still-bad-token');
    // initial probe 200, recheck probe 401 — and NOTHING more scripted:
    // the post-state connect must fail without touching the network.
    const fetchSpy = scriptedFetch([200, 401]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    await mgr.handleAuthRejected(evidence401);

    expect(auth.invalidate).toHaveBeenCalledOnce();
    expect(await auth.store.read()).toBeNull();
    // The dead project handle was disconnected and dropped…
    expect(mgr.get('idx-1')).toBeUndefined();
    // …and the next tool call fails fast with the reauth message instead
    // of hanging into the peer timeout (no probe: fetch is exhausted).
    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(ReauthRequired);
  });

  it('self-heals from reauth-required after the user re-authenticates', async () => {
    const auth = seededAuth();
    auth.forceRefresh.mockResolvedValue('still-bad-token');
    const fetchSpy = scriptedFetch([200, 401, 200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');
    await mgr.handleAuthRejected(evidence401);
    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(ReauthRequired);

    // The user runs `authenticate`: fresh credentials land in the store.
    await auth.store.write({
      idToken: 'post-reauth-token',
      refreshToken: 'post-reauth-refresh',
      idTokenExpiresAt: new Date(Date.now() + 3600_000),
      scopes: ['openid', 'email', 'profile'],
    });
    auth.getValid.mockResolvedValue('post-reauth-token');

    // The gate clears and a normal probed connect runs (the third 200).
    await mgr.connect('idx-1');
    expect(mgr.get('idx-1')).toBeDefined();
  });

  it('treats 403 evidence as terminal denial: keyring kept, clear message on the next call', async () => {
    const auth = seededAuth();
    // initial probe 200; the next connect's re-probe answers 403.
    const fetchSpy = scriptedFetch([200, 403]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    await mgr.handleAuthRejected(evidence403);

    // Identity denial, not credential failure: no refresh, no wipe.
    expect(auth.forceRefresh).not.toHaveBeenCalled();
    expect(auth.invalidate).not.toHaveBeenCalled();
    expect(await auth.store.read()).not.toBeNull();
    expect(mgr.get('idx-1')).toBeUndefined();

    // Next call re-probes (fast HTTP, no peer timeout) and surfaces the
    // clear denial message; a later operator un-ban would self-heal here.
    const err = await mgr.connect('idx-1').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(HubAccessDeniedError);
    expect(String((err as Error).message)).toMatch(/banned|allowlist/i);
  });

  it('maps a 403 on the recheck probe after 401 evidence to the same denial state', async () => {
    const auth = seededAuth();
    auth.forceRefresh.mockResolvedValue('fresh-token');
    // initial 200; recheck probe 403 (banned mid-session while a refresh
    // was in flight); next connect re-probes 403.
    const fetchSpy = scriptedFetch([200, 403, 403]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    await mgr.handleAuthRejected(evidence401);

    expect(auth.invalidate).not.toHaveBeenCalled();
    expect(await auth.store.read()).not.toBeNull();
    expect(mgr.get('idx-1')).toBeUndefined();
    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(HubAccessDeniedError);
  });

  it('enters reauth-required directly on token-refresh-terminal evidence', async () => {
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

    // The refresh manager already invalidated the grant when it threw
    // ReauthRequired from getBearer; simulate that store state.
    await auth.store.clear();
    await mgr.handleAuthRejected({ kind: 'token-refresh-terminal' });

    expect(auth.forceRefresh).not.toHaveBeenCalled(); // pointless, skip it
    expect(mgr.get('idx-1')).toBeUndefined();
    await expect(mgr.connect('idx-1')).rejects.toBeInstanceOf(ReauthRequired);
  });

  it('coalesces concurrent reports into one forceRefresh+reprobe cycle', async () => {
    const auth = seededAuth();
    auth.forceRefresh.mockResolvedValue('fresh-token');
    // Exactly one recheck response scripted: a second cycle would throw
    // "ran out of responses".
    const fetchSpy = scriptedFetch([200, 200]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    // Two project adapters both fire after a hub-wide event.
    await Promise.all([
      mgr.handleAuthRejected(evidence401),
      mgr.handleAuthRejected(evidence401),
    ]);

    expect(auth.forceRefresh).toHaveBeenCalledOnce();
    expect(fetchSpy.calls).toHaveLength(2);
  });

  it('leaves state untouched when the recheck refresh fails transiently (TokenRefreshError)', async () => {
    const auth = seededAuth();
    const transient = new Error('IdP hiccup');
    transient.name = 'TokenRefreshError';
    auth.forceRefresh.mockRejectedValue(transient);
    const fetchSpy = scriptedFetch([200]); // no recheck probe happens
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });
    await mgr.connect('idx-1');

    await mgr.handleAuthRejected(evidence401);

    expect(auth.invalidate).not.toHaveBeenCalled();
    expect(mgr.get('idx-1')).toBeDefined(); // nothing torn down
    expect(await auth.store.read()).not.toBeNull();
  });

  it('surfaces the clear denial message on an initial-probe 403 (not "Unexpected status")', async () => {
    const auth = seededAuth();
    const fetchSpy = scriptedFetch([403]);
    const sync = spySyncClientFactory();
    const mgr = new ConnectionManager({
      serverUrl: 'wss://hub.example.com/ws',
      credentialStore: auth.store,
      refreshManager: auth.refresh,
      fetch: fetchSpy.fetch,
      syncClientFactory: sync.factory,
    });

    const err = await mgr.connect('idx-1').catch((e: unknown) => e);
    expect(err).toBeInstanceOf(HubAccessDeniedError);
    expect(String((err as Error).message)).not.toMatch(/unexpected status/i);
    expect(String((err as Error).message)).toMatch(/banned|allowlist/i);
    expect(sync.connectCalls).toHaveLength(0);
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
