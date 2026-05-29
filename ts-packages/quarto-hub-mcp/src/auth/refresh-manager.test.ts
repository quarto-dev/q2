/**
 * Phase 6 — refresh-on-401 / proactive-refresh manager.
 *
 * Tests stub Google's `/token` endpoint via `oauth4webapi`'s
 * symbol-keyed `customFetch` option — never call live Google. The
 * `CredentialStore` is wired to an in-memory backend so writes are
 * observable without touching the platform keyring.
 */

import { describe, it, expect, vi } from 'vitest';
import * as oauth from 'oauth4webapi';

import {
  CredentialStore,
  type CredentialBundle,
  type CredentialStoreConfig,
  type KeyringBackend,
} from './credential-store.js';
import {
  ReauthRequired,
  RefreshManager,
  TokenRefreshError,
  type RefreshManagerDeps,
} from './refresh-manager.js';

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const ISSUER = 'https://accounts.google.com';
const FAKE_CLIENT_ID = 'test-client.apps.googleusercontent.com';
const FAKE_CLIENT_SECRET = 'GOCSPX-test-secret';

const AS: oauth.AuthorizationServer = {
  issuer: ISSUER,
  token_endpoint: 'https://oauth2.googleapis.com/token',
};

const CFG: CredentialStoreConfig = { issuer: ISSUER, clientId: FAKE_CLIENT_ID };

function b64url(s: string): string {
  return Buffer.from(s, 'utf8').toString('base64url');
}

interface IdTokenClaims {
  readonly exp: number;
  readonly iss?: string;
  readonly aud?: string;
  readonly azp?: string;
  readonly sub?: string;
  readonly email?: string;
  readonly iat?: number;
}

// Defaults fill in the claims that `oauth4webapi.processRefreshTokenResponse`
// requires (iss, aud, iat) so callers can write `fakeIdToken({ exp: ... })`
// without rebuilding the full claim set every time.
function fakeIdToken(claims: IdTokenClaims): string {
  const now = Math.floor(Date.now() / 1000);
  const merged = {
    iss: ISSUER,
    aud: FAKE_CLIENT_ID,
    azp: FAKE_CLIENT_ID,
    sub: 'fake-sub',
    email: 'tester@example.com',
    iat: now,
    ...claims,
  };
  const header = b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: 'fake' }));
  const body = b64url(JSON.stringify(merged));
  const sig = b64url('signature-bytes');
  return `${header}.${body}.${sig}`;
}

function farFutureExp(): number {
  return Math.floor(Date.now() / 1000) + 3600;
}

function makeBundle(overrides: Partial<CredentialBundle> = {}): CredentialBundle {
  const exp = farFutureExp();
  return {
    idToken: fakeIdToken({
      iss: ISSUER,
      aud: FAKE_CLIENT_ID,
      azp: FAKE_CLIENT_ID,
      sub: 'fake-sub',
      email: 'tester@example.com',
      iat: Math.floor(Date.now() / 1000),
      exp,
    }),
    refreshToken: '1//original-refresh-token',
    idTokenExpiresAt: new Date(exp * 1000),
    scopes: ['openid', 'email', 'profile'],
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// In-memory keyring backend (mirrors credential-store.test.ts)
// ---------------------------------------------------------------------------

function memoryBackend(initial: string | null = null): {
  backend: KeyringBackend;
  state: { value: string | null };
} {
  const state = { value: initial };
  const backend: KeyringBackend = {
    async read() {
      return state.value;
    },
    async write(v: string) {
      state.value = v;
    },
    async clear() {
      const existed = state.value !== null;
      state.value = null;
      return existed;
    },
  };
  return { backend, state };
}

// ---------------------------------------------------------------------------
// Fetch stub
// ---------------------------------------------------------------------------

interface RecordedRequest {
  url: string;
  method: string;
  body: URLSearchParams;
}

function makeFetch(
  responder: (req: RecordedRequest) => Response | Promise<Response>,
): { fetch: typeof fetch; requests: RecordedRequest[] } {
  const requests: RecordedRequest[] = [];
  const stub: typeof fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : (input as URL | Request).toString();
    let body: URLSearchParams;
    if (init?.body instanceof URLSearchParams) {
      body = new URLSearchParams(init.body.toString());
    } else if (typeof init?.body === 'string') {
      body = new URLSearchParams(init.body);
    } else {
      body = new URLSearchParams();
    }
    const req: RecordedRequest = {
      url,
      method: init?.method ?? 'GET',
      body,
    };
    requests.push(req);
    return responder(req);
  };
  return { fetch: stub, requests };
}

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

interface TokenResponseOverrides {
  readonly id_token?: string;
  readonly refresh_token?: string | undefined;
  readonly access_token?: string;
  readonly expires_in?: number;
  readonly token_type?: string;
  readonly scope?: string;
}

function tokenResponseBody(overrides: TokenResponseOverrides = {}): Record<string, unknown> {
  const body: Record<string, unknown> = {
    access_token: 'ya29.refreshed-access',
    expires_in: 3599,
    token_type: 'Bearer',
    scope: 'openid email profile',
    id_token:
      overrides.id_token ??
      fakeIdToken({
        iss: ISSUER,
        aud: FAKE_CLIENT_ID,
        azp: FAKE_CLIENT_ID,
        sub: 'fake-sub',
        email: 'tester@example.com',
        iat: Math.floor(Date.now() / 1000),
        exp: farFutureExp(),
      }),
  };
  if (overrides.access_token !== undefined) body.access_token = overrides.access_token;
  if (overrides.expires_in !== undefined) body.expires_in = overrides.expires_in;
  if (overrides.token_type !== undefined) body.token_type = overrides.token_type;
  if (overrides.scope !== undefined) body.scope = overrides.scope;
  if (overrides.refresh_token !== undefined) body.refresh_token = overrides.refresh_token;
  return body;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function seedStore(
  bundle: CredentialBundle,
  initialBlob?: string,
): Promise<{ store: CredentialStore; state: { value: string | null } }> {
  const { backend, state } = memoryBackend(initialBlob ?? null);
  const store = new CredentialStore(CFG, backend);
  if (initialBlob === undefined) {
    await store.write(bundle);
  }
  return { store, state };
}

function makeRm(
  store: CredentialStore,
  fetchImpl: typeof fetch,
  extra: Partial<Omit<RefreshManagerDeps, 'as' | 'config' | 'store' | 'fetch'>> = {},
): RefreshManager {
  return new RefreshManager({
    as: AS,
    config: { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
    store,
    fetch: fetchImpl,
    ...extra,
  });
}

// ---------------------------------------------------------------------------
// forceRefresh — token-endpoint contract
// ---------------------------------------------------------------------------

describe('RefreshManager.forceRefresh — token-endpoint contract', () => {
  it('posts to token_endpoint with grant_type=refresh_token, client credentials, and refresh_token', async () => {
    const { store } = await seedStore(makeBundle());
    const { fetch, requests } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const rm = makeRm(store, fetch);
    await rm.forceRefresh();
    expect(requests).toHaveLength(1);
    const req = requests[0]!;
    expect(req.url).toBe('https://oauth2.googleapis.com/token');
    expect(req.method).toBe('POST');
    expect(req.body.get('grant_type')).toBe('refresh_token');
    expect(req.body.get('client_id')).toBe(FAKE_CLIENT_ID);
    expect(req.body.get('client_secret')).toBe(FAKE_CLIENT_SECRET);
    expect(req.body.get('refresh_token')).toBe('1//original-refresh-token');
  });

  it('returns the new id_token to the caller (refresh_called_on_401 primitive)', async () => {
    const { store } = await seedStore(makeBundle());
    const newIdToken = fakeIdToken({ exp: farFutureExp(), sub: 'fake-sub' });
    const { fetch } = makeFetch(() =>
      jsonResponse(200, tokenResponseBody({ id_token: newIdToken })),
    );
    const rm = makeRm(store, fetch);
    const got = await rm.forceRefresh();
    expect(got).toBe(newIdToken);
  });

  it('persists the new id_token and computed expiry to the store', async () => {
    const original = makeBundle();
    const { store, state } = await seedStore(original);
    const newExp = Math.floor(Date.now() / 1000) + 3600;
    const newIdToken = fakeIdToken({ exp: newExp, sub: 'fake-sub' });
    const { fetch } = makeFetch(() =>
      jsonResponse(200, tokenResponseBody({ id_token: newIdToken })),
    );
    const rm = makeRm(store, fetch);
    await rm.forceRefresh();
    expect(state.value).not.toBeNull();
    const parsed = JSON.parse(state.value!);
    expect(parsed.id_token).toBe(newIdToken);
    expect(parsed.id_token_expires_at).toBe(new Date(newExp * 1000).toISOString());
    expect(parsed.scopes).toEqual([...original.scopes]);
  });
});

// ---------------------------------------------------------------------------
// Refresh-token persistence rule (empirical Google behaviour)
// ---------------------------------------------------------------------------

describe('RefreshManager refresh-token persistence rule', () => {
  it('keeps original refresh_token when Google omits the field', async () => {
    const original = makeBundle({ refreshToken: '1//keep-me-alive' });
    const { store, state } = await seedStore(original);
    const { fetch } = makeFetch(() =>
      // No refresh_token in response — the live Google case.
      jsonResponse(200, tokenResponseBody({ refresh_token: undefined })),
    );
    const rm = makeRm(store, fetch);
    await rm.forceRefresh();
    const parsed = JSON.parse(state.value!);
    expect(parsed.refresh_token).toBe('1//keep-me-alive');
  });

  it('keeps original refresh_token when Google returns the same value', async () => {
    const original = makeBundle({ refreshToken: '1//keep-me-alive' });
    const { store, state } = await seedStore(original);
    const { fetch } = makeFetch(() =>
      jsonResponse(200, tokenResponseBody({ refresh_token: '1//keep-me-alive' })),
    );
    const rm = makeRm(store, fetch);
    await rm.forceRefresh();
    const parsed = JSON.parse(state.value!);
    expect(parsed.refresh_token).toBe('1//keep-me-alive');
  });

  it('persists rotated refresh_token when Google returns a new value', async () => {
    const original = makeBundle({ refreshToken: '1//old-rt' });
    const { store, state } = await seedStore(original);
    const { fetch } = makeFetch(() =>
      jsonResponse(200, tokenResponseBody({ refresh_token: '1//rotated-rt' })),
    );
    const rm = makeRm(store, fetch);
    await rm.forceRefresh();
    const parsed = JSON.parse(state.value!);
    expect(parsed.refresh_token).toBe('1//rotated-rt');
  });
});

// ---------------------------------------------------------------------------
// invalid_grant → ReauthRequired, clears store
// ---------------------------------------------------------------------------

describe('RefreshManager.forceRefresh failure handling', () => {
  it('clears store and throws ReauthRequired on invalid_grant', async () => {
    const { store, state } = await seedStore(makeBundle());
    const { fetch } = makeFetch(() => jsonResponse(400, { error: 'invalid_grant' }));
    const rm = makeRm(store, fetch);
    let err: unknown;
    try {
      await rm.forceRefresh();
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(ReauthRequired);
    expect((err as Error).message).toMatch(/expired|revoked|authenticate again/i);
    expect(state.value).toBeNull();
  });

  it('does not persist partial state on network failure', async () => {
    const original = makeBundle();
    const { store, state } = await seedStore(original);
    const beforeBlob = state.value;
    const networkErr = new Error('ECONNRESET');
    const { fetch } = makeFetch(() => {
      throw networkErr;
    });
    const rm = makeRm(store, fetch);
    await expect(rm.forceRefresh()).rejects.toThrow();
    // The store must be byte-identical to the pre-call blob — no
    // partial write, no clear.
    expect(state.value).toBe(beforeBlob);
  });

  it('propagates non-invalid_grant oauth errors without clearing the store', async () => {
    const { store, state } = await seedStore(makeBundle());
    const beforeBlob = state.value;
    const { fetch } = makeFetch(() => jsonResponse(500, { error: 'server_error' }));
    const rm = makeRm(store, fetch);
    await expect(rm.forceRefresh()).rejects.toThrow();
    expect(state.value).toBe(beforeBlob);
  });

  it('throws an actionable TokenRefreshError on invalid_client (wrong client secret), store intact', async () => {
    const { store, state } = await seedStore(makeBundle());
    const beforeBlob = state.value;
    const { fetch } = makeFetch(() =>
      jsonResponse(401, {
        error: 'invalid_client',
        error_description: 'The provided client secret is invalid.',
      }),
    );
    const rm = makeRm(store, fetch);
    let err: unknown;
    try {
      await rm.forceRefresh();
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(TokenRefreshError);
    const tre = err as TokenRefreshError;
    expect(tre.oauthError).toBe('invalid_client');
    expect(tre.oauthErrorDescription).toBe('The provided client secret is invalid.');
    expect(tre.isConfigError).toBe(true);
    // Message surfaces the code, the description, and the config remediation.
    expect(tre.message).toContain('invalid_client');
    expect(tre.message).toContain('The provided client secret is invalid.');
    expect(tre.message).toMatch(/QUARTO_HUB_MCP_CLIENT_(ID|SECRET)/);
    // Not the opaque oauth4webapi default.
    expect(tre.message).not.toBe('server responded with an error in the response body');
    // The stored credential is the user's grant, not the problem — keep it.
    expect(state.value).toBe(beforeBlob);
  });

  it('wraps a 4xx non-config oauth error in a TokenRefreshError flagged non-config', async () => {
    const { store } = await seedStore(makeBundle());
    // oauth4webapi only parses an error *body* for 4xx; a non-config code
    // like invalid_request becomes a ResponseBodyError we can wrap.
    const { fetch } = makeFetch(() => jsonResponse(400, { error: 'invalid_request' }));
    const rm = makeRm(store, fetch);
    let err: unknown;
    try {
      await rm.forceRefresh();
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(TokenRefreshError);
    const tre = err as TokenRefreshError;
    expect(tre.oauthError).toBe('invalid_request');
    expect(tre.isConfigError).toBe(false);
    expect(tre.message).toMatch(/transient/i);
  });
});

// ---------------------------------------------------------------------------
// getValidIdToken — proactive refresh + caching
// ---------------------------------------------------------------------------

describe('RefreshManager.getValidIdToken', () => {
  it('returns the cached id_token when expiry is well in the future', async () => {
    const original = makeBundle();
    const { store } = await seedStore(original);
    const { fetch, requests } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const rm = makeRm(store, fetch);
    const got = await rm.getValidIdToken();
    expect(got).toBe(original.idToken);
    expect(requests).toHaveLength(0);
  });

  it('proactively refreshes when id_token is within 60s skew of expiry', async () => {
    const nearExp = Math.floor(Date.now() / 1000) + 30; // 30s — inside default 60s skew
    const original = makeBundle({
      idToken: fakeIdToken({ exp: nearExp, sub: 'fake-sub' }),
      idTokenExpiresAt: new Date(nearExp * 1000),
    });
    const { store } = await seedStore(original);
    const newIdToken = fakeIdToken({ exp: farFutureExp(), sub: 'fake-sub' });
    const { fetch, requests } = makeFetch(() =>
      jsonResponse(200, tokenResponseBody({ id_token: newIdToken })),
    );
    const rm = makeRm(store, fetch);
    const got = await rm.getValidIdToken();
    expect(got).toBe(newIdToken);
    expect(requests).toHaveLength(1);
  });

  it('refreshes when the cached id_token is already expired', async () => {
    const pastExp = Math.floor(Date.now() / 1000) - 60;
    const original = makeBundle({
      idToken: fakeIdToken({ exp: pastExp, sub: 'fake-sub' }),
      idTokenExpiresAt: new Date(pastExp * 1000),
    });
    const { store } = await seedStore(original);
    const newIdToken = fakeIdToken({ exp: farFutureExp(), sub: 'fake-sub' });
    const { fetch, requests } = makeFetch(() =>
      jsonResponse(200, tokenResponseBody({ id_token: newIdToken })),
    );
    const rm = makeRm(store, fetch);
    const got = await rm.getValidIdToken();
    expect(got).toBe(newIdToken);
    expect(requests).toHaveLength(1);
  });

  it('throws ReauthRequired when the credential store is empty', async () => {
    const { backend } = memoryBackend(null);
    const store = new CredentialStore(CFG, backend);
    const { fetch, requests } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const rm = makeRm(store, fetch);
    await expect(rm.getValidIdToken()).rejects.toBeInstanceOf(ReauthRequired);
    expect(requests).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// Concurrency
// ---------------------------------------------------------------------------

describe('RefreshManager concurrency', () => {
  it('coalesces concurrent forceRefresh calls to a single /token POST', async () => {
    const { store } = await seedStore(makeBundle());
    const newIdToken = fakeIdToken({ exp: farFutureExp(), sub: 'fake-sub' });
    const { fetch, requests } = makeFetch(async () => {
      // Delay so the three concurrent callers all see an in-flight
      // refresh and coalesce onto it.
      await new Promise((r) => setTimeout(r, 5));
      return jsonResponse(200, tokenResponseBody({ id_token: newIdToken }));
    });
    const rm = makeRm(store, fetch);
    const results = await Promise.all([
      rm.forceRefresh(),
      rm.forceRefresh(),
      rm.forceRefresh(),
    ]);
    expect(requests).toHaveLength(1);
    expect(results).toEqual([newIdToken, newIdToken, newIdToken]);
  });

  it('coalesces concurrent proactive getValidIdToken refreshes onto a single /token POST', async () => {
    const nearExp = Math.floor(Date.now() / 1000) + 30;
    const original = makeBundle({
      idToken: fakeIdToken({ exp: nearExp, sub: 'fake-sub' }),
      idTokenExpiresAt: new Date(nearExp * 1000),
    });
    const { store } = await seedStore(original);
    const newIdToken = fakeIdToken({ exp: farFutureExp(), sub: 'fake-sub' });
    const { fetch, requests } = makeFetch(async () => {
      await new Promise((r) => setTimeout(r, 5));
      return jsonResponse(200, tokenResponseBody({ id_token: newIdToken }));
    });
    const rm = makeRm(store, fetch);
    const results = await Promise.all([
      rm.getValidIdToken(),
      rm.getValidIdToken(),
      rm.getValidIdToken(),
    ]);
    expect(requests).toHaveLength(1);
    expect(results).toEqual([newIdToken, newIdToken, newIdToken]);
  });

  it('allows a fresh refresh after a previous one rejected', async () => {
    const { store } = await seedStore(makeBundle());
    let calls = 0;
    const newIdToken = fakeIdToken({ exp: farFutureExp(), sub: 'fake-sub' });
    const { fetch, requests } = makeFetch(() => {
      calls += 1;
      if (calls === 1) {
        return jsonResponse(500, { error: 'server_error' });
      }
      return jsonResponse(200, tokenResponseBody({ id_token: newIdToken }));
    });
    const rm = makeRm(store, fetch);
    await expect(rm.forceRefresh()).rejects.toThrow();
    const got = await rm.forceRefresh();
    expect(got).toBe(newIdToken);
    expect(requests).toHaveLength(2);
  });
});

// ---------------------------------------------------------------------------
// Logging redaction
// ---------------------------------------------------------------------------

describe('RefreshManager logging redaction', () => {
  it('does not log id_token or refresh_token across any console sink', async () => {
    const original = makeBundle({ refreshToken: '1//secret-rt-XYZ' });
    const { store } = await seedStore(original);
    const newIdToken = fakeIdToken({ exp: farFutureExp(), sub: 'fake-sub' });
    const newRefresh = '1//rotated-secret-RT';
    const { fetch } = makeFetch(() =>
      jsonResponse(
        200,
        tokenResponseBody({ id_token: newIdToken, refresh_token: newRefresh }),
      ),
    );
    const sinks = (['debug', 'log', 'info', 'warn', 'error'] as const).map((m) =>
      vi.spyOn(console, m).mockImplementation(() => undefined),
    );
    try {
      const rm = makeRm(store, fetch);
      await rm.forceRefresh();
      const blob = sinks
        .flatMap((s) => s.mock.calls.flat())
        .map((c) => (typeof c === 'string' ? c : JSON.stringify(c)))
        .join(' ');
      expect(blob).not.toContain(original.idToken);
      expect(blob).not.toContain(original.refreshToken);
      expect(blob).not.toContain(newIdToken);
      expect(blob).not.toContain(newRefresh);
    } finally {
      sinks.forEach((s) => s.mockRestore());
    }
  });
});
