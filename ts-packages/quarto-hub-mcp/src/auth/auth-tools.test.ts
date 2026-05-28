/**
 * MCP auth-tool surface — Authorization Code + PKCE + loopback.
 *
 * Tests drive `AuthToolsState` directly: no MCP `Server` is spun up. The
 * loopback listener and the browser opener are injected seams; HTTP is
 * injected via `deps.fetch`; the `CredentialStore` is wired to an
 * in-memory keyring backend so writes are observable.
 */

import { EventEmitter } from 'node:events';
import { describe, it, expect, vi } from 'vitest';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import type * as oauth from 'oauth4webapi';

import {
  AUTH_TOOL_DEFINITIONS,
  AuthToolsState,
  type AuthToolContext,
  type LastObservedAuthModeSource,
  type ProgressNotification,
} from './auth-tools.js';
import {
  CredentialStore,
  type CredentialBundle,
  type CredentialStoreConfig,
  type KeyringBackend,
} from './credential-store.js';
import {
  LoopbackAbortedError,
  LoopbackTimeoutError,
  type LoopbackListener,
  type StartLoopbackOptions,
  startLoopbackListener,
} from './loopback.js';
import { openBrowser } from './browser.js';
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
  authorization_endpoint: 'https://accounts.google.com/o/oauth2/v2/auth',
  token_endpoint: 'https://oauth2.googleapis.com/token',
  revocation_endpoint: 'https://oauth2.googleapis.com/revoke',
};

const CFG: CredentialStoreConfig = { issuer: ISSUER, clientId: FAKE_CLIENT_ID };

function b64url(s: string): string {
  return Buffer.from(s, 'utf8').toString('base64url');
}

interface IdTokenClaims {
  readonly exp: number;
  readonly email?: string;
  readonly [k: string]: unknown;
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
// In-memory keyring backend
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

function makeBundle(overrides: Partial<CredentialBundle> = {}): CredentialBundle {
  const exp = farFutureExp();
  return {
    idToken: fakeIdToken({ exp, email: FAKE_EMAIL }),
    refreshToken: '1//original-refresh-token',
    idTokenExpiresAt: new Date(exp * 1000),
    scopes: ['openid', 'email', 'profile'],
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// fetch recorder
// ---------------------------------------------------------------------------

interface RecordedRequest {
  url: string;
  method: string;
  body: URLSearchParams;
  headers: Record<string, string>;
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
    const headers: Record<string, string> = {};
    if (init?.headers) {
      for (const [k, v] of Object.entries(init.headers as Record<string, string>)) {
        headers[k.toLowerCase()] = v;
      }
    }
    requests.push({ url, method: init?.method ?? 'GET', body, headers });
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

function tokenResponseBody(
  overrides: { id_token?: string; refresh_token?: string | undefined } = {},
): Record<string, unknown> {
  const body: Record<string, unknown> = {
    access_token: 'ya29.fresh-access',
    expires_in: 3599,
    token_type: 'Bearer',
    scope: 'openid email profile',
    id_token: overrides.id_token ?? fakeIdToken({ exp: farFutureExp(), email: FAKE_EMAIL }),
    refresh_token: '1//new-refresh-token',
  };
  if ('refresh_token' in overrides) {
    if (overrides.refresh_token === undefined) delete body.refresh_token;
    else body.refresh_token = overrides.refresh_token;
  }
  return body;
}

// ---------------------------------------------------------------------------
// Injected seams: loopback listener + browser opener
// ---------------------------------------------------------------------------

interface FakeListenerControl {
  start: typeof startLoopbackListener;
  recorded: { expectedState?: string; redirectUri?: string; calls: number; signal?: AbortSignal };
}

function fakeListener(opts: {
  code?: string;
  rejectWith?: Error;
  port?: number;
}): FakeListenerControl {
  const recorded: FakeListenerControl['recorded'] = { calls: 0 };
  const start: typeof startLoopbackListener = async (o: StartLoopbackOptions) => {
    recorded.calls += 1;
    recorded.expectedState = o.expectedState;
    recorded.signal = o.signal;
    const port = opts.port ?? 51234;
    const redirectUri = `http://127.0.0.1:${port}/callback`;
    recorded.redirectUri = redirectUri;
    const code = opts.code ?? 'auth-code-xyz';
    const result: Promise<{ code: string; state: string; params: URLSearchParams }> =
      opts.rejectWith
        ? Promise.reject(opts.rejectWith)
        : Promise.resolve({
            code,
            state: o.expectedState,
            params: new URLSearchParams({ code, state: o.expectedState }),
          });
    // Keep an unobserved rejection from surfacing before the handler awaits.
    result.catch(() => undefined);
    const listener: LoopbackListener = { port, redirectUri, result, close: () => undefined };
    return listener;
  };
  return { start, recorded };
}

interface FakeBrowserControl {
  open: typeof openBrowser;
  calls: string[];
}

function fakeBrowser(opts: { fail?: boolean } = {}): FakeBrowserControl {
  const calls: string[] = [];
  const open: typeof openBrowser = (url: string) => {
    calls.push(url);
    if (opts.fail) return undefined;
    return new EventEmitter() as unknown as ReturnType<typeof openBrowser>;
  };
  return { open, calls };
}

// ---------------------------------------------------------------------------
// State construction
// ---------------------------------------------------------------------------

function authMode(mode: 'no-auth' | 'requires-auth' | 'unknown'): LastObservedAuthModeSource {
  return { lastObservedAuthMode: () => mode };
}

interface MakeStateArgs {
  store: CredentialStore;
  mode?: 'no-auth' | 'requires-auth' | 'unknown';
  fetch?: typeof fetch;
  startListener?: typeof startLoopbackListener;
  openBrowser?: typeof openBrowser;
  promptConsent?: boolean;
  logger?: (m: string) => void;
}

function makeState(args: MakeStateArgs): AuthToolsState {
  const refreshManager = new RefreshManager({
    as: AS,
    config: { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
    store: args.store,
    // Never used in the loopback path (store.read() short-circuits first).
    fetch: (async () => {
      throw new Error('RefreshManager fetch should not be called in these tests');
    }) as unknown as typeof fetch,
  });
  return new AuthToolsState({
    credentialStore: args.store,
    refreshManager,
    connectionManager: authMode(args.mode ?? 'requires-auth'),
    flowConfig: { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET, issuer: ISSUER },
    authorizationServer: AS,
    fetch: args.fetch,
    startListener: args.startListener,
    openBrowser: args.openBrowser,
    promptConsent: args.promptConsent,
    logger: args.logger ?? (() => undefined),
  });
}

function emptyStore(): { store: CredentialStore; state: { value: string | null } } {
  const { backend, state } = memoryBackend(null);
  return { store: new CredentialStore(CFG, backend), state };
}

async function seededStore(
  bundle: CredentialBundle = makeBundle(),
): Promise<{ store: CredentialStore; state: { value: string | null } }> {
  const { backend, state } = memoryBackend(null);
  const store = new CredentialStore(CFG, backend);
  await store.write(bundle);
  return { store, state };
}

function textOf(res: CallToolResult): string {
  return res.content
    .filter((c): c is { type: 'text'; text: string } => c.type === 'text')
    .map((c) => c.text)
    .join('\n');
}

// ===========================================================================
// Tool definitions
// ===========================================================================

describe('AUTH_TOOL_DEFINITIONS', () => {
  it('exposes exactly authenticate and authenticate_clear', () => {
    const names = AUTH_TOOL_DEFINITIONS.map((t) => t.name).sort();
    expect(names).toEqual(['authenticate', 'authenticate_clear']);
  });

  it('marks authenticate non-idempotent / non-destructive and clear destructive / idempotent', () => {
    const auth = AUTH_TOOL_DEFINITIONS.find((t) => t.name === 'authenticate')!;
    const clear = AUTH_TOOL_DEFINITIONS.find((t) => t.name === 'authenticate_clear')!;
    expect(auth.annotations?.idempotentHint).toBe(false);
    expect(auth.annotations?.destructiveHint).toBe(false);
    expect(clear.annotations?.destructiveHint).toBe(true);
    expect(clear.annotations?.idempotentHint).toBe(true);
  });

  it('clear description names the best-effort revoke and drops the old disclaimer', () => {
    const clear = AUTH_TOOL_DEFINITIONS.find((t) => t.name === 'authenticate_clear')!;
    expect(clear.description).toMatch(/revoke/i);
    expect(clear.description).toContain('myaccount.google.com');
    expect(clear.description).not.toMatch(/Does not touch Google-side grants/i);
  });
});

// ===========================================================================
// authenticate — happy path + token-exchange contract
// ===========================================================================

describe('authenticate', () => {
  it('runs the loopback flow, exchanges the code, and stores the credentials', async () => {
    const { store, state } = emptyStore();
    const listener = fakeListener({ code: 'the-code' });
    const browser = fakeBrowser();
    const { fetch } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const auth = makeState({
      store,
      fetch,
      startListener: listener.start,
      openBrowser: browser.open,
    });

    const res = await auth.handleAuthenticate({});
    expect(textOf(res)).toBe(`Authenticated as ${FAKE_EMAIL}.`);
    expect(res.isError).toBeUndefined();

    const stored = await store.read();
    expect(stored?.refreshToken).toBe('1//new-refresh-token');
    expect(state.value).not.toBeNull();
  });

  it('sends both code_verifier and client_secret on the token exchange', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({ code: 'the-code' });
    const browser = fakeBrowser();
    const { fetch, requests } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const auth = makeState({ store, fetch, startListener: listener.start, openBrowser: browser.open });

    await auth.handleAuthenticate({});

    const tokenReq = requests.find((r) => r.url === AS.token_endpoint)!;
    expect(tokenReq.body.get('grant_type')).toBe('authorization_code');
    expect(tokenReq.body.get('code')).toBe('the-code');
    expect(tokenReq.body.get('code_verifier')).toBeTruthy();
    expect(tokenReq.body.get('client_secret')).toBe(FAKE_CLIENT_SECRET);
    expect(tokenReq.body.get('redirect_uri')).toBe(listener.recorded.redirectUri);
  });

  it('builds an authorization URL with PKCE, offline access, and prompt=consent', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({});
    const browser = fakeBrowser();
    const { fetch } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const auth = makeState({ store, fetch, startListener: listener.start, openBrowser: browser.open });

    await auth.handleAuthenticate({});

    expect(browser.calls).toHaveLength(1);
    const url = new URL(browser.calls[0]!);
    expect(url.origin + url.pathname).toBe('https://accounts.google.com/o/oauth2/v2/auth');
    expect(url.searchParams.get('response_type')).toBe('code');
    expect(url.searchParams.get('client_id')).toBe(FAKE_CLIENT_ID);
    expect(url.searchParams.get('redirect_uri')).toBe(listener.recorded.redirectUri);
    expect(url.searchParams.get('scope')).toBe('openid email profile');
    expect(url.searchParams.get('code_challenge_method')).toBe('S256');
    expect(url.searchParams.get('code_challenge')).toBeTruthy();
    expect(url.searchParams.get('state')).toBe(listener.recorded.expectedState);
    expect(url.searchParams.get('access_type')).toBe('offline');
    expect(url.searchParams.get('include_granted_scopes')).toBe('true');
    expect(url.searchParams.get('prompt')).toBe('consent');
  });

  it('omits prompt=consent when promptConsent is false', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({});
    const browser = fakeBrowser();
    const { fetch } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const auth = makeState({
      store,
      fetch,
      startListener: listener.start,
      openBrowser: browser.open,
      promptConsent: false,
    });
    await auth.handleAuthenticate({});
    const url = new URL(browser.calls[0]!);
    expect(url.searchParams.get('prompt')).toBeNull();
  });

  it('appends a manual-sign-in note when the browser launch fails', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({});
    const browser = fakeBrowser({ fail: true });
    const { fetch } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const auth = makeState({ store, fetch, startListener: listener.start, openBrowser: browser.open });

    const res = await auth.handleAuthenticate({});
    expect(textOf(res)).toContain('Browser launch failed; you signed in manually.');
  });

  // -------------------------------------------------------------------------
  // Pre-flight short-circuits
  // -------------------------------------------------------------------------

  it('short-circuits when already authenticated, without binding a listener', async () => {
    const { store } = await seededStore();
    const listener = fakeListener({});
    const browser = fakeBrowser();
    const auth = makeState({ store, startListener: listener.start, openBrowser: browser.open });

    const res = await auth.handleAuthenticate({});
    expect(textOf(res)).toBe(`Already authenticated as ${FAKE_EMAIL}. No action needed.`);
    expect(listener.recorded.calls).toBe(0);
    expect(browser.calls).toHaveLength(0);
  });

  it('short-circuits when the hub does not require auth', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({});
    const browser = fakeBrowser();
    const auth = makeState({
      store,
      mode: 'no-auth',
      startListener: listener.start,
      openBrowser: browser.open,
    });

    const res = await auth.handleAuthenticate({});
    expect(textOf(res)).toMatch(/does not require authentication/);
    expect(listener.recorded.calls).toBe(0);
  });

  // -------------------------------------------------------------------------
  // Progress notifications
  // -------------------------------------------------------------------------

  it('sends exactly one bind-time progress notification when a token is supplied', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({});
    const browser = fakeBrowser();
    const { fetch } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const sendNotification = vi.fn((_n: ProgressNotification) => Promise.resolve());
    const ctx: AuthToolContext = { progressToken: 'p1', sendNotification };
    const auth = makeState({ store, fetch, startListener: listener.start, openBrowser: browser.open });

    await auth.handleAuthenticate(ctx);

    expect(sendNotification).toHaveBeenCalledTimes(1);
    const note = sendNotification.mock.calls[0]![0];
    expect(note.method).toBe('notifications/progress');
    expect(note.params.progressToken).toBe('p1');
    expect(note.params.progress).toBe(0);
    expect(note.params.total).toBe(1);
    expect(note.params.message).toContain('https://accounts.google.com/o/oauth2/v2/auth');
  });

  it('sends no progress notification when no token is supplied', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({});
    const browser = fakeBrowser();
    const { fetch } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const sendNotification = vi.fn((_n: ProgressNotification) => Promise.resolve());
    const ctx: AuthToolContext = { sendNotification };
    const auth = makeState({ store, fetch, startListener: listener.start, openBrowser: browser.open });

    const res = await auth.handleAuthenticate(ctx);
    expect(textOf(res)).toBe(`Authenticated as ${FAKE_EMAIL}.`);
    expect(sendNotification).not.toHaveBeenCalled();
  });

  // -------------------------------------------------------------------------
  // Cancellation / errors / sequential reuse
  // -------------------------------------------------------------------------

  it('returns cancelled without binding a listener if the signal is already aborted', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({});
    const auth = makeState({ store, startListener: listener.start, openBrowser: fakeBrowser().open });
    const ac = new AbortController();
    ac.abort();

    const res = await auth.handleAuthenticate({ signal: ac.signal });
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/cancelled/i);
    expect(listener.recorded.calls).toBe(0);
  });

  it('maps a mid-flight abort (listener rejection) to a typed cancellation result', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({ rejectWith: new LoopbackAbortedError() });
    const auth = makeState({ store, startListener: listener.start, openBrowser: fakeBrowser().open });

    const res = await auth.handleAuthenticate({});
    expect(res.isError).toBe(true);
    expect(textOf(res)).toBe('Sign-in was cancelled.');
  });

  it('reports a timeout with the manual-paste URL', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({ rejectWith: new LoopbackTimeoutError() });
    const browser = fakeBrowser();
    const auth = makeState({ store, startListener: listener.start, openBrowser: browser.open });

    const res = await auth.handleAuthenticate({});
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/Timed out/);
    expect(textOf(res)).toContain('https://accounts.google.com/o/oauth2/v2/auth');
  });

  it('reports a listener bind failure', async () => {
    const { store } = emptyStore();
    const failingStart: typeof startLoopbackListener = async () => {
      throw new Error('EADDRINUSE');
    };
    const auth = makeState({ store, startListener: failingStart, openBrowser: fakeBrowser().open });
    const res = await auth.handleAuthenticate({});
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/Failed to start the local sign-in listener/);
  });

  it('reports a token-exchange failure', async () => {
    const { store } = emptyStore();
    const listener = fakeListener({});
    const browser = fakeBrowser();
    const { fetch } = makeFetch(() =>
      jsonResponse(400, { error: 'invalid_grant', error_description: 'bad code' }),
    );
    const auth = makeState({ store, fetch, startListener: listener.start, openBrowser: browser.open });

    const res = await auth.handleAuthenticate({});
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/Token exchange failed/);
  });

  it('accepts a follow-up call after a failed one (no stale state)', async () => {
    const { store } = emptyStore();
    const browser = fakeBrowser();
    const timeoutListener = fakeListener({ rejectWith: new LoopbackTimeoutError() });
    const first = makeState({
      store,
      startListener: timeoutListener.start,
      openBrowser: browser.open,
    });
    const r1 = await first.handleAuthenticate({});
    expect(r1.isError).toBe(true);

    const okListener = fakeListener({});
    const { fetch } = makeFetch(() => jsonResponse(200, tokenResponseBody()));
    const second = makeState({
      store,
      fetch,
      startListener: okListener.start,
      openBrowser: browser.open,
    });
    const r2 = await second.handleAuthenticate({});
    expect(r2.isError).toBeUndefined();
    expect(textOf(r2)).toBe(`Authenticated as ${FAKE_EMAIL}.`);
  });
});

// ===========================================================================
// authenticate_clear — revocation contract
// ===========================================================================

describe('authenticate_clear', () => {
  it('revokes the refresh token at Google then clears the keyring (clean case)', async () => {
    const { store, state } = await seededStore(makeBundle({ refreshToken: '1//to-revoke' }));
    const { fetch, requests } = makeFetch(() => new Response('', { status: 200 }));
    const auth = makeState({ store, fetch });

    const res = await auth.handleClear();

    const revokeReq = requests.find((r) => r.url === AS.revocation_endpoint)!;
    expect(revokeReq.method).toBe('POST');
    expect(revokeReq.body.get('token')).toBe('1//to-revoke');
    expect(revokeReq.body.get('token_type_hint')).toBe('refresh_token');
    expect(revokeReq.body.get('client_id')).toBeNull();
    expect(revokeReq.body.get('client_secret')).toBeNull();
    expect(revokeReq.headers['content-type']).toBe('application/x-www-form-urlencoded');

    expect(state.value).toBeNull();
    expect(textOf(res)).toMatch(/cleared and revoked at Google/);
    expect(res.isError).toBeUndefined();
  });

  it('clears locally even when the revoke fails, without leaking the token', async () => {
    const { store, state } = await seededStore(makeBundle({ refreshToken: '1//secret-rt' }));
    const { fetch } = makeFetch(() => new Response('boom', { status: 500 }));
    const auth = makeState({ store, fetch });

    const res = await auth.handleClear();

    expect(state.value).toBeNull();
    expect(textOf(res)).toMatch(/cleared locally/);
    expect(textOf(res)).toMatch(/revocation failed/);
    expect(textOf(res)).toContain('myaccount.google.com');
    expect(textOf(res)).not.toContain('1//secret-rt');
    expect(res.isError).toBeUndefined();
  });

  it('does not hit the revoke endpoint when there is nothing to clear', async () => {
    const { store } = emptyStore();
    const { fetch, requests } = makeFetch(() => new Response('', { status: 200 }));
    const auth = makeState({ store, fetch });

    const res = await auth.handleClear();
    expect(requests).toHaveLength(0);
    expect(textOf(res)).toMatch(/cleared\. Call authenticate to sign in again/);
    expect(res.isError).toBeUndefined();
  });

  it('uses the failure path and notes the revoke ran when the local delete fails', async () => {
    // Backend that yields a bundle on read but throws on clear.
    const seed = memoryBackend(null);
    const store = new CredentialStore(CFG, {
      read: seed.backend.read,
      write: seed.backend.write,
      async clear() {
        throw new Error('keyring locked');
      },
    });
    await store.write(makeBundle({ refreshToken: '1//rt' }));

    const { fetch } = makeFetch(() => new Response('', { status: 200 }));
    const auth = makeState({ store, fetch });

    const res = await auth.handleClear();
    expect(res.isError).toBe(true);
    expect(textOf(res)).toMatch(/Failed to clear the OS keyring entry/);
    expect(textOf(res)).toMatch(/revoked at Google/);
  });
});
