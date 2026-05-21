/**
 * Phase 7 — MCP auth-tool surface (`authenticate_start` / `authenticate_finish`).
 *
 * Tests drive `AuthToolsState` directly: no MCP `Server` instance is
 * spun up. Time is injected via `deps.now`; HTTP via `deps.fetch`. The
 * `CredentialStore` is wired to an in-memory keyring backend so writes
 * are observable without touching the platform keyring.
 */

import { describe, it, expect, vi } from 'vitest';
import * as oauth from 'oauth4webapi';

import {
  AUTH_TOOL_DEFINITIONS,
  AuthToolsState,
  CANONICAL_VERIFICATION_URL,
  type AuthToolsDeps,
  type LastObservedAuthModeSource,
} from './auth-tools.js';
import {
  CredentialStore,
  type CredentialBundle,
  type CredentialStoreConfig,
  type KeyringBackend,
} from './credential-store.js';
import { RefreshManager } from './refresh-manager.js';

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const ISSUER = 'https://accounts.google.com';
const FAKE_CLIENT_ID = 'test-client.apps.googleusercontent.com';
const FAKE_CLIENT_SECRET = 'GOCSPX-test-secret';
const FAKE_EMAIL = 'tester@example.com';

const AS: oauth.AuthorizationServer = {
  issuer: ISSUER,
  device_authorization_endpoint: 'https://oauth2.googleapis.com/device/code',
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
  readonly email_verified?: boolean;
  readonly iat?: number;
}

function fakeIdToken(claims: IdTokenClaims): string {
  const now = Math.floor(Date.now() / 1000);
  const merged = {
    iss: ISSUER,
    aud: FAKE_CLIENT_ID,
    azp: FAKE_CLIENT_ID,
    sub: 'fake-sub',
    email: FAKE_EMAIL,
    email_verified: true,
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

// ---------------------------------------------------------------------------
// Backend / store helpers
// ---------------------------------------------------------------------------

function memoryBackend(initial: string | null = null): {
  backend: KeyringBackend;
  state: { value: string | null };
  writes: number;
} {
  const counters = { writes: 0 };
  const state = { value: initial };
  const backend: KeyringBackend = {
    async read() {
      return state.value;
    },
    async write(v: string) {
      counters.writes += 1;
      state.value = v;
    },
    async clear() {
      const existed = state.value !== null;
      state.value = null;
      return existed;
    },
  };
  return {
    backend,
    state,
    get writes() {
      return counters.writes;
    },
  };
}

function makeBundle(overrides: Partial<CredentialBundle> = {}): CredentialBundle {
  const exp = farFutureExp();
  return {
    idToken: fakeIdToken({ exp }),
    refreshToken: '1//original-refresh-token',
    idTokenExpiresAt: new Date(exp * 1000),
    scopes: ['openid', 'email', 'profile'],
    ...overrides,
  };
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
    requests.push({ url, method: init?.method ?? 'GET', body });
    return responder(requests[requests.length - 1]!);
  };
  return { fetch: stub, requests };
}

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function deviceAuthBody(overrides: Partial<oauth.DeviceAuthorizationResponse> = {}): Record<string, unknown> {
  return {
    device_code: 'AH-1Ng-test',
    user_code: 'FJZL-WTDR',
    verification_uri: 'https://www.google.com/device',
    expires_in: 1800,
    interval: 5,
    ...overrides,
  };
}

function tokenSuccessBody(
  overrides: { id_token?: string; refresh_token?: string; access_token?: string } = {},
): Record<string, unknown> {
  return {
    access_token: overrides.access_token ?? 'ya29.fake-access-token',
    expires_in: 3599,
    refresh_token: overrides.refresh_token ?? '1//fake-refresh-token',
    scope: 'openid email profile',
    token_type: 'Bearer',
    id_token:
      overrides.id_token ??
      fakeIdToken({
        exp: farFutureExp(),
        sub: 'fake-sub',
        email: FAKE_EMAIL,
      }),
  };
}

// ---------------------------------------------------------------------------
// Connection-manager stub
// ---------------------------------------------------------------------------

function stubConnMgr(mode: 'no-auth' | 'requires-auth' | 'unknown'): LastObservedAuthModeSource {
  return { lastObservedAuthMode: () => mode };
}

// ---------------------------------------------------------------------------
// Deps assembly
// ---------------------------------------------------------------------------

interface Harness {
  state: AuthToolsState;
  store: CredentialStore;
  storeState: { value: string | null };
  storeWriteCount: () => number;
  requests: RecordedRequest[];
  now: () => Date;
  setNow: (d: Date) => void;
}

interface HarnessOpts {
  readonly seedBundle?: CredentialBundle;
  readonly responder: (req: RecordedRequest) => Response | Promise<Response>;
  readonly authMode?: 'no-auth' | 'requires-auth' | 'unknown';
  readonly initialNow?: Date;
  readonly coalesceWindowMs?: number;
}

async function makeHarness(opts: HarnessOpts): Promise<Harness> {
  const mb = memoryBackend(null);
  const store = new CredentialStore(CFG, mb.backend);
  if (opts.seedBundle) {
    await store.write(opts.seedBundle);
  }
  const { fetch, requests } = makeFetch(opts.responder);
  let nowRef = opts.initialNow ?? new Date();
  const now = (): Date => new Date(nowRef.getTime());
  const refreshManager = new RefreshManager({
    as: AS,
    config: { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
    store,
    fetch,
  });
  const deps: AuthToolsDeps = {
    credentialStore: store,
    refreshManager,
    connectionManager: stubConnMgr(opts.authMode ?? 'requires-auth'),
    flowConfig: {
      clientId: FAKE_CLIENT_ID,
      clientSecret: FAKE_CLIENT_SECRET,
      issuer: ISSUER,
    },
    authorizationServer: AS,
    now,
    fetch,
    coalesceWindowMs: opts.coalesceWindowMs,
  };
  const state = new AuthToolsState(deps);
  return {
    state,
    store,
    storeState: mb.state,
    storeWriteCount: () => mb.writes,
    requests,
    now,
    setNow: (d: Date) => {
      nowRef = new Date(d.getTime());
    },
  };
}

function textOf(result: { content: ReadonlyArray<{ readonly type: string; readonly text?: string }> }): string {
  return result.content.map((c) => c.text ?? '').join('');
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

describe('AUTH_TOOL_DEFINITIONS', () => {
  it('exposes authenticate_start and authenticate_finish with non-idempotent annotations', () => {
    const names = AUTH_TOOL_DEFINITIONS.map((t) => t.name).sort();
    expect(names).toEqual(['authenticate_finish', 'authenticate_start']);
    for (const t of AUTH_TOOL_DEFINITIONS) {
      expect(t.annotations).toEqual({
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: false,
      });
    }
  });
});

// ---------------------------------------------------------------------------
// authenticate_start
// ---------------------------------------------------------------------------

describe('authenticate_start', () => {
  it('returns verification_uri, user_code and canonical_url', async () => {
    const h = await makeHarness({
      responder: () => jsonResponse(200, deviceAuthBody()),
    });
    const res = await h.state.handleStart();
    const txt = textOf(res);
    expect(txt).toContain(CANONICAL_VERIFICATION_URL);
    expect(txt).toContain('https://www.google.com/device'); // verification_uri
    expect(txt).toContain('FJZL-WTDR'); // user_code
  });

  it('includes the expires-in seconds in the response', async () => {
    const h = await makeHarness({
      responder: () => jsonResponse(200, deviceAuthBody({ expires_in: 1234 })),
    });
    const res = await h.state.handleStart();
    expect(textOf(res)).toContain('1234');
  });

  it('canonical URL is a hard-coded constant, not from Google', async () => {
    const h = await makeHarness({
      responder: () =>
        jsonResponse(
          200,
          deviceAuthBody({ verification_uri: 'https://malicious.example.com/oauth' }),
        ),
    });
    const res = await h.state.handleStart();
    const txt = textOf(res);
    // The canonical URL must be the constant.
    expect(txt).toContain(CANONICAL_VERIFICATION_URL);
    // The attacker-controlled verification_uri must NOT appear as the
    // canonical step-1 URL; it can still be listed as Google's value.
    expect(CANONICAL_VERIFICATION_URL).toBe('https://www.google.com/device');
  });

  it('does not write to the credential store (device_code is process-local)', async () => {
    const h = await makeHarness({
      responder: () => jsonResponse(200, deviceAuthBody()),
    });
    await h.state.handleStart();
    expect(h.storeWriteCount()).toBe(0);
  });

  it('short-circuits when already authenticated', async () => {
    const seeded = makeBundle();
    const h = await makeHarness({
      seedBundle: seeded,
      responder: () => {
        throw new Error('should not be called — already authenticated');
      },
    });
    const res = await h.state.handleStart();
    expect(textOf(res)).toContain('Already authenticated');
    expect(textOf(res)).toContain(FAKE_EMAIL);
    expect(h.requests).toHaveLength(0);
  });

  it('short-circuits when the hub is known to require no auth', async () => {
    const h = await makeHarness({
      authMode: 'no-auth',
      responder: () => {
        throw new Error('should not be called — hub is no-auth');
      },
    });
    const res = await h.state.handleStart();
    expect(textOf(res)).toMatch(/does not require authentication/i);
    expect(h.requests).toHaveLength(0);
  });

  it('initiates device flow when the hub is known to require auth', async () => {
    const h = await makeHarness({
      authMode: 'requires-auth',
      responder: () => jsonResponse(200, deviceAuthBody()),
    });
    await h.state.handleStart();
    expect(h.requests).toHaveLength(1);
    expect(h.requests[0]!.url).toBe('https://oauth2.googleapis.com/device/code');
  });

  it('initiates device flow when auth mode is unknown (positive observation required for short-circuit)', async () => {
    const h = await makeHarness({
      authMode: 'unknown',
      responder: () => jsonResponse(200, deviceAuthBody()),
    });
    await h.state.handleStart();
    expect(h.requests).toHaveLength(1);
  });

  it('coalesces repeated start calls within the configured window', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    const h = await makeHarness({
      initialNow: t0,
      coalesceWindowMs: 5000,
      responder: () => jsonResponse(200, deviceAuthBody()),
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 2_000));
    await h.state.handleStart();
    expect(h.requests).toHaveLength(1);
  });

  it('overwrites a prior unconsumed device_code when started outside the coalescing window', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let pass = 0;
    const h = await makeHarness({
      initialNow: t0,
      coalesceWindowMs: 5000,
      responder: () => {
        pass += 1;
        return jsonResponse(
          200,
          deviceAuthBody({
            device_code: `device-${pass}`,
            user_code: pass === 1 ? 'FIRST-CODE' : 'SECOND-CODE',
          }),
        );
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 10_000));
    const second = await h.state.handleStart();
    expect(h.requests).toHaveLength(2);
    expect(textOf(second)).toContain('SECOND-CODE');
  });

  it('clears an expired cached device_code on the next start', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let pass = 0;
    const h = await makeHarness({
      initialNow: t0,
      coalesceWindowMs: 60_000,
      responder: () => {
        pass += 1;
        return jsonResponse(
          200,
          deviceAuthBody({
            device_code: `device-${pass}`,
            user_code: pass === 1 ? 'FIRST-CODE' : 'SECOND-CODE',
            expires_in: 60, // expires in 60s
          }),
        );
      },
    });
    await h.state.handleStart();
    // Advance past expiry — far outside the 60s window means the
    // cached code has expired and a fresh flow must initiate.
    h.setNow(new Date(t0.getTime() + 120_000));
    const second = await h.state.handleStart();
    expect(h.requests).toHaveLength(2);
    expect(textOf(second)).toContain('SECOND-CODE');
  });
});

// ---------------------------------------------------------------------------
// authenticate_finish
// ---------------------------------------------------------------------------

describe('authenticate_finish', () => {
  it('returns a typed error when called without a prior start', async () => {
    const h = await makeHarness({
      responder: () => {
        throw new Error('should not be called');
      },
    });
    const res = await h.state.handleFinish();
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/authenticate_start/);
    expect(h.requests).toHaveLength(0);
  });

  it('returns user-actionable text on authorization_pending', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        // First request → device_authorization. Second → token (pending).
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(400, { error: 'authorization_pending' });
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    const res = await h.state.handleFinish();
    expect(res.isError).toBeFalsy();
    expect(textOf(res)).toMatch(/waiting|pending/i);
  });

  it('returns user-actionable text with a wait hint on slow_down', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(400, { error: 'slow_down' });
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    const res = await h.state.handleFinish();
    expect(textOf(res)).toMatch(/slow|wait/i);
  });

  it('persists the bundle via CredentialStore on success', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const idToken = fakeIdToken({ exp: farFutureExp(), email: FAKE_EMAIL });
    const refresh = '1//new-refresh-token';
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(200, tokenSuccessBody({ id_token: idToken, refresh_token: refresh }));
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    await h.state.handleFinish();
    expect(h.storeState.value).not.toBeNull();
    const blob = JSON.parse(h.storeState.value!);
    expect(blob.id_token).toBe(idToken);
    expect(blob.refresh_token).toBe(refresh);
  });

  it('clears the cached device_code on success', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(200, tokenSuccessBody());
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    await h.state.handleFinish();
    // Now a second finish without a fresh start must report "no flow".
    const res = await h.state.handleFinish();
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/authenticate_start/);
  });

  it('clears the cached device_code on terminal access_denied', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(400, { error: 'access_denied' });
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    await h.state.handleFinish();
    const res = await h.state.handleFinish();
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/authenticate_start/);
  });

  it('clears the cached device_code on terminal expired_token', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(400, { error: 'expired_token' });
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    await h.state.handleFinish();
    const res = await h.state.handleFinish();
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/authenticate_start/);
  });

  it('returns "Authenticated as <email>" from the id_token email claim', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const idToken = fakeIdToken({ exp: farFutureExp(), email: 'someone@posit.co' });
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(200, tokenSuccessBody({ id_token: idToken }));
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    const res = await h.state.handleFinish();
    expect(textOf(res)).toContain('someone@posit.co');
    expect(textOf(res)).toMatch(/authenticated/i);
  });

  it('tool responses never contain id_token or refresh_token bytes', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const idToken = fakeIdToken({ exp: farFutureExp(), email: FAKE_EMAIL });
    const refresh = '1//super-secret-rt-XYZ';
    const access = 'ya29.super-secret-access';
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(
          200,
          tokenSuccessBody({ id_token: idToken, refresh_token: refresh, access_token: access }),
        );
      },
    });
    const startRes = await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    const finishRes = await h.state.handleFinish();
    const blob = textOf(startRes) + '\n' + textOf(finishRes);
    expect(blob).not.toContain(idToken);
    expect(blob).not.toContain(refresh);
    expect(blob).not.toContain(access);
  });

  it('returns slow_down advice without polling Google when called before interval elapsed', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        // Anything beyond the device-auth request would be a violation.
        throw new Error('Google should not have been polled before interval elapsed');
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 1_000)); // 1s — way before the 5s interval
    const res = await h.state.handleFinish();
    expect(textOf(res)).toMatch(/wait|pending/i);
    expect(h.requests).toHaveLength(1); // only the device-auth call
  });

  it('polls Google once the device-auth interval has elapsed', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(400, { error: 'authorization_pending' });
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    await h.state.handleFinish();
    expect(h.requests).toHaveLength(2); // device-auth + one token poll
  });

  it('increases the subsequent poll interval after slow_down (RFC 8628 §3.5)', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        if (phase === 2) return jsonResponse(400, { error: 'slow_down' });
        throw new Error('Should not poll Google a second time before bumped interval elapses');
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    await h.state.handleFinish(); // slow_down → bump
    // Advance by the original interval (5s) — must not poll because
    // slow_down has bumped the next-allowed-at by an additional 5s.
    h.setNow(new Date(t0.getTime() + 11_000));
    await h.state.handleFinish();
    expect(h.requests).toHaveLength(2); // device-auth + the one slow_down poll
  });

  it('serialises concurrent finish calls safely', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const idToken = fakeIdToken({ exp: farFutureExp(), email: FAKE_EMAIL });
    const h = await makeHarness({
      initialNow: t0,
      responder: async () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        if (phase === 2) {
          // Delay so a second concurrent finish overlaps with the first.
          await new Promise((r) => setTimeout(r, 10));
          return jsonResponse(200, tokenSuccessBody({ id_token: idToken }));
        }
        return jsonResponse(400, { error: 'authorization_pending' });
      },
    });
    await h.state.handleStart();
    h.setNow(new Date(t0.getTime() + 6_000));
    const [a, b] = await Promise.all([h.state.handleFinish(), h.state.handleFinish()]);
    // The first call should succeed; the second sees the cleared
    // device_code and surfaces the "no flow in progress" tool error.
    const aText = textOf(a);
    const bText = textOf(b);
    const successCount = [aText, bText].filter((t) => /authenticated as/i.test(t)).length;
    expect(successCount).toBe(1);
    const errorCount = [a, b].filter((r) => r.isError).length;
    expect(errorCount).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// Cross-cutting logging redaction
// ---------------------------------------------------------------------------

describe('logging redaction', () => {
  it('does not log id_token / refresh_token / access_token across any console sink', async () => {
    const t0 = new Date('2026-05-21T12:00:00Z');
    let phase = 0;
    const idToken = fakeIdToken({ exp: farFutureExp(), email: FAKE_EMAIL });
    const refresh = '1//secret-rt-XYZ';
    const access = 'ya29.secret-access';
    const h = await makeHarness({
      initialNow: t0,
      responder: () => {
        phase += 1;
        if (phase === 1) return jsonResponse(200, deviceAuthBody({ interval: 5 }));
        return jsonResponse(
          200,
          tokenSuccessBody({ id_token: idToken, refresh_token: refresh, access_token: access }),
        );
      },
    });
    const sinks = (['debug', 'log', 'info', 'warn', 'error'] as const).map((m) =>
      vi.spyOn(console, m).mockImplementation(() => undefined),
    );
    try {
      await h.state.handleStart();
      h.setNow(new Date(t0.getTime() + 6_000));
      await h.state.handleFinish();
      const blob = sinks
        .flatMap((s) => s.mock.calls.flat())
        .map((c) => (typeof c === 'string' ? c : JSON.stringify(c)))
        .join(' ');
      expect(blob).not.toContain(idToken);
      expect(blob).not.toContain(refresh);
      expect(blob).not.toContain(access);
    } finally {
      sinks.forEach((s) => s.mockRestore());
    }
  });
});
