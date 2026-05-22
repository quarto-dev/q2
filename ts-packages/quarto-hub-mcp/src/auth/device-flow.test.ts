/**
 * Phase 4 — device-flow primitives.
 *
 * Tests use a stub `customFetch` injected via `oauth4webapi`'s
 * symbol-keyed options; never call live Google.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import * as oauth from 'oauth4webapi';

import {
  DeviceFlowDeniedError,
  DeviceFlowError,
  DeviceFlowExpiredError,
  MissingCredentialsConfigError,
  buildAuthorizationServer,
  initiateDeviceFlow,
  loadDeviceFlowConfigFromEnv,
  pollDeviceFlowOnce,
  redactTokens,
} from './device-flow.js';

const ISSUER = 'https://accounts.google.com';
const FAKE_CLIENT_ID = 'test-client.apps.googleusercontent.com';
const FAKE_CLIENT_SECRET = 'GOCSPX-test-secret';

// Synthetic AuthorizationServer (skips discovery): the spec requires
// only that the endpoints we hit are present, but oauth4webapi also
// verifies `issuer` matches expectations.
const AS: oauth.AuthorizationServer = {
  issuer: ISSUER,
  device_authorization_endpoint: 'https://oauth2.googleapis.com/device/code',
  token_endpoint: 'https://oauth2.googleapis.com/token',
};

interface RecordedRequest {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: URLSearchParams;
}

function makeFetch(
  responder: (req: RecordedRequest) => Response | Promise<Response>
): {
  fetch: typeof fetch;
  requests: RecordedRequest[];
} {
  const requests: RecordedRequest[] = [];
  const stub: typeof fetch = async (input, init) => {
    const url = typeof input === 'string' ? input : (input as URL | Request).toString();
    const headersIn = new Headers(init?.headers);
    const headers: Record<string, string> = {};
    headersIn.forEach((v, k) => {
      headers[k] = v;
    });
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
      headers,
      body,
    };
    requests.push(req);
    return responder(req);
  };
  return { fetch: stub, requests };
}

function b64url(s: string): string {
  return Buffer.from(s, 'utf8').toString('base64url');
}

/**
 * Builds a structurally-valid (unsigned) JWT for fixture use.
 * oauth4webapi validates the header+payload are base64url-encoded JSON
 * and that the algorithm matches the expected `none` or signed form;
 * for token-endpoint responses the signature is not verified against
 * any key, only the shape.
 */
function fakeIdToken(payload: Record<string, unknown>): string {
  const header = b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: 'fake' }));
  const body = b64url(JSON.stringify(payload));
  // Any non-empty base64url string works as a placeholder signature.
  const sig = b64url('signature-bytes');
  return `${header}.${body}.${sig}`;
}

function jsonResponse(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function envOnly(values: Record<string, string | undefined>): NodeJS.ProcessEnv {
  const e: NodeJS.ProcessEnv = {};
  for (const [k, v] of Object.entries(values)) {
    if (v !== undefined) e[k] = v;
  }
  return e;
}

describe('loadDeviceFlowConfigFromEnv', () => {
  it('reads client_id and client_secret from process.env', () => {
    const env = envOnly({
      QUARTO_HUB_MCP_CLIENT_ID: FAKE_CLIENT_ID,
      QUARTO_HUB_MCP_CLIENT_SECRET: FAKE_CLIENT_SECRET,
    });
    const cfg = loadDeviceFlowConfigFromEnv(env);
    expect(cfg.clientId).toBe(FAKE_CLIENT_ID);
    expect(cfg.clientSecret).toBe(FAKE_CLIENT_SECRET);
  });

  it('throws MissingCredentialsConfigError naming both env vars when neither set', () => {
    const env = envOnly({});
    let err: unknown;
    try {
      loadDeviceFlowConfigFromEnv(env);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(MissingCredentialsConfigError);
    const msg = (err as Error).message;
    expect(msg).toContain('QUARTO_HUB_MCP_CLIENT_ID');
    expect(msg).toContain('QUARTO_HUB_MCP_CLIENT_SECRET');
  });

  it('throws MissingCredentialsConfigError when only client_id set', () => {
    const env = envOnly({ QUARTO_HUB_MCP_CLIENT_ID: FAKE_CLIENT_ID });
    let err: unknown;
    try {
      loadDeviceFlowConfigFromEnv(env);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(MissingCredentialsConfigError);
    expect((err as Error).message).toContain('QUARTO_HUB_MCP_CLIENT_SECRET');
  });

  it('throws MissingCredentialsConfigError when secret is empty string', () => {
    const env = envOnly({
      QUARTO_HUB_MCP_CLIENT_ID: FAKE_CLIENT_ID,
      QUARTO_HUB_MCP_CLIENT_SECRET: '   ',
    });
    expect(() => loadDeviceFlowConfigFromEnv(env)).toThrow(
      MissingCredentialsConfigError
    );
  });
});

describe('no_baked_default_client_id_or_secret', () => {
  // Sourcing rule lock-in: scan every non-test file under src/ for
  // hard-coded Google OAuth credentials. Test files are allowed to
  // carry fixture-shaped strings.
  it('contains no apps.googleusercontent.com literal in any source file', () => {
    const root = join(__dirname, '..');
    const violations: string[] = [];
    function walk(dir: string): void {
      for (const ent of readdirSync(dir)) {
        const p = join(dir, ent);
        const st = statSync(p);
        if (st.isDirectory()) {
          walk(p);
          continue;
        }
        if (!p.endsWith('.ts') || p.endsWith('.test.ts')) continue;
        const text = readFileSync(p, 'utf8');
        if (/[A-Za-z0-9_-]+\.apps\.googleusercontent\.com/.test(text)) {
          violations.push(`${p}: apps.googleusercontent.com literal`);
        }
        if (/GOCSPX-[A-Za-z0-9_-]+/.test(text)) {
          violations.push(`${p}: GOCSPX- literal`);
        }
      }
    }
    walk(root);
    expect(violations).toEqual([]);
  });
});

describe('initiateDeviceFlow', () => {
  let okResponse: oauth.DeviceAuthorizationResponse;

  beforeEach(() => {
    okResponse = {
      device_code: 'AH-1Ng-test',
      user_code: 'FJZL-WTDR',
      verification_uri: 'https://www.google.com/device',
      expires_in: 1800,
      interval: 5,
    };
  });

  it('posts to device_authorization_endpoint with client_id and scope', async () => {
    const { fetch, requests } = makeFetch(() => jsonResponse(200, okResponse));
    await initiateDeviceFlow(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET, scopes: ['openid', 'email', 'profile'] },
      { fetch }
    );
    expect(requests).toHaveLength(1);
    const req = requests[0]!;
    expect(req.url).toBe('https://oauth2.googleapis.com/device/code');
    expect(req.method).toBe('POST');
    expect(req.body.get('client_id')).toBe(FAKE_CLIENT_ID);
    expect(req.body.get('scope')).toBe('openid email profile');
  });

  it('does not send client_secret on the device-auth body', async () => {
    const { fetch, requests } = makeFetch(() => jsonResponse(200, okResponse));
    await initiateDeviceFlow(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET, scopes: ['openid', 'email', 'profile'] },
      { fetch }
    );
    expect(requests[0]!.body.has('client_secret')).toBe(false);
  });

  it('returns the full DeviceAuthorizationResponse', async () => {
    const { fetch } = makeFetch(() => jsonResponse(200, okResponse));
    const got = await initiateDeviceFlow(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET, scopes: ['openid', 'email', 'profile'] },
      { fetch }
    );
    expect(got.device_code).toBe('AH-1Ng-test');
    expect(got.user_code).toBe('FJZL-WTDR');
    expect(got.verification_uri).toBe('https://www.google.com/device');
    expect(got.expires_in).toBe(1800);
    expect(got.interval).toBe(5);
  });

  it('does not log the user_code or device_code', async () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    const log = vi.spyOn(console, 'log').mockImplementation(() => undefined);
    const info = vi.spyOn(console, 'info').mockImplementation(() => undefined);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const err = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    try {
      const { fetch } = makeFetch(() => jsonResponse(200, okResponse));
      await initiateDeviceFlow(
        AS,
        { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET, scopes: ['openid', 'email', 'profile'] },
        { fetch }
      );
      const calls = [debug, log, info, warn, err]
        .flatMap((s) => s.mock.calls.flat())
        .map((c) => (typeof c === 'string' ? c : JSON.stringify(c)));
      const blob = calls.join(' ');
      expect(blob).not.toContain(okResponse.user_code);
      expect(blob).not.toContain(okResponse.device_code);
    } finally {
      debug.mockRestore();
      log.mockRestore();
      info.mockRestore();
      warn.mockRestore();
      err.mockRestore();
    }
  });

  it('normalises Google\'s verification_url to RFC 8628 verification_uri', async () => {
    // Live Google /device/code returns `verification_url` (with `_url`,
    // not the RFC 8628 `verification_uri`). `oauth4webapi`'s
    // `processDeviceAuthorizationResponse` strictly asserts
    // `verification_uri` is a string and throws otherwise — so without
    // a normaliser, every live `authenticate_start` fails before the
    // device flow can begin. The Phase 0 verification log (2026-05-19)
    // captured Google's exact shape; this test pins it.
    const googleBody = {
      device_code: 'AH-1Ng-test',
      user_code: 'FJZL-WTDR',
      verification_url: 'https://www.google.com/device',
      expires_in: 1800,
      interval: 5,
    };
    const { fetch } = makeFetch(() => jsonResponse(200, googleBody));
    const got = await initiateDeviceFlow(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET, scopes: ['openid', 'email', 'profile'] },
      { fetch }
    );
    expect(got.verification_uri).toBe('https://www.google.com/device');
    expect(got.device_code).toBe('AH-1Ng-test');
    expect(got.user_code).toBe('FJZL-WTDR');
  });

  it('preserves verification_uri when both fields are present', async () => {
    // Defensive: if Google ever updates to send both spellings (or only
    // the RFC one), we must not clobber the canonical field.
    const both = {
      device_code: 'AH-1Ng-test',
      user_code: 'FJZL-WTDR',
      verification_uri: 'https://www.google.com/device',
      verification_url: 'https://attacker.example/device',
      expires_in: 1800,
      interval: 5,
    };
    const { fetch } = makeFetch(() => jsonResponse(200, both));
    const got = await initiateDeviceFlow(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET, scopes: ['openid', 'email', 'profile'] },
      { fetch }
    );
    expect(got.verification_uri).toBe('https://www.google.com/device');
  });

  it('honours abort signal', async () => {
    const ctl = new AbortController();
    const { fetch } = makeFetch(async () => {
      // never resolve unless aborted
      await new Promise<void>((_, reject) => {
        ctl.signal.addEventListener('abort', () => {
          reject(new DOMException('aborted', 'AbortError'));
        });
      });
      return jsonResponse(200, okResponse);
    });
    const p = initiateDeviceFlow(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET, scopes: ['openid', 'email', 'profile'] },
      { fetch, signal: ctl.signal }
    );
    setTimeout(() => ctl.abort(), 5);
    await expect(p).rejects.toThrow();
  });
});

describe('pollDeviceFlowOnce', () => {
  const ID_TOKEN = fakeIdToken({
    iss: ISSUER,
    aud: FAKE_CLIENT_ID,
    azp: FAKE_CLIENT_ID,
    sub: 'fake-sub-123',
    email: 'tester@example.com',
    email_verified: true,
    iat: 1_000_000_000,
    exp: 9_999_999_999,
  });
  const SUCCESS_BODY = {
    access_token: 'ya29.fake-access-token',
    expires_in: 3599,
    refresh_token: '1//fake-refresh-token',
    scope: 'openid email profile',
    token_type: 'Bearer',
    id_token: ID_TOKEN,
  };

  it('returns pending on authorization_pending', async () => {
    const { fetch } = makeFetch(() =>
      jsonResponse(400, { error: 'authorization_pending' })
    );
    const res = await pollDeviceFlowOnce(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
      'AH-1Ng-test',
      { fetch }
    );
    expect(res.kind).toBe('pending');
  });

  it('returns slow_down on slow_down', async () => {
    const { fetch } = makeFetch(() =>
      jsonResponse(400, { error: 'slow_down' })
    );
    const res = await pollDeviceFlowOnce(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
      'AH-1Ng-test',
      { fetch }
    );
    expect(res.kind).toBe('slow_down');
  });

  it('resolves with tokens on success', async () => {
    const { fetch, requests } = makeFetch(() => jsonResponse(200, SUCCESS_BODY));
    const res = await pollDeviceFlowOnce(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
      'AH-1Ng-test',
      { fetch }
    );
    expect(res.kind).toBe('tokens');
    if (res.kind !== 'tokens') throw new Error('unreachable');
    expect(res.bundle.id_token).toBe(ID_TOKEN);
    expect(res.bundle.refresh_token).toBe(SUCCESS_BODY.refresh_token);
    expect(res.bundle.access_token).toBe(SUCCESS_BODY.access_token);
    // The token endpoint must include client_secret per Google's contract.
    expect(requests[0]!.body.get('client_secret')).toBe(FAKE_CLIENT_SECRET);
    expect(requests[0]!.body.get('client_id')).toBe(FAKE_CLIENT_ID);
    expect(requests[0]!.body.get('device_code')).toBe('AH-1Ng-test');
    expect(requests[0]!.body.get('grant_type')).toBe(
      'urn:ietf:params:oauth:grant-type:device_code'
    );
  });

  it('throws DeviceFlowDeniedError on access_denied', async () => {
    const { fetch } = makeFetch(() =>
      jsonResponse(400, { error: 'access_denied' })
    );
    await expect(
      pollDeviceFlowOnce(
        AS,
        { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
        'AH-1Ng-test',
        { fetch }
      )
    ).rejects.toBeInstanceOf(DeviceFlowDeniedError);
  });

  it('throws DeviceFlowExpiredError on expired_token', async () => {
    const { fetch } = makeFetch(() =>
      jsonResponse(400, { error: 'expired_token' })
    );
    await expect(
      pollDeviceFlowOnce(
        AS,
        { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
        'AH-1Ng-test',
        { fetch }
      )
    ).rejects.toBeInstanceOf(DeviceFlowExpiredError);
  });

  it('throws generic DeviceFlowError on other oauth errors', async () => {
    const { fetch } = makeFetch(() =>
      jsonResponse(400, { error: 'invalid_grant' })
    );
    const p = pollDeviceFlowOnce(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
      'AH-1Ng-test',
      { fetch }
    );
    await expect(p).rejects.toBeInstanceOf(DeviceFlowError);
    await expect(p).rejects.not.toBeInstanceOf(DeviceFlowDeniedError);
    await expect(p).rejects.not.toBeInstanceOf(DeviceFlowExpiredError);
  });

  it('honours abort signal', async () => {
    const ctl = new AbortController();
    const { fetch } = makeFetch(async () => {
      await new Promise<void>((_, reject) => {
        ctl.signal.addEventListener('abort', () => {
          reject(new DOMException('aborted', 'AbortError'));
        });
      });
      return jsonResponse(200, SUCCESS_BODY);
    });
    const p = pollDeviceFlowOnce(
      AS,
      { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
      'AH-1Ng-test',
      { fetch, signal: ctl.signal }
    );
    setTimeout(() => ctl.abort(), 5);
    await expect(p).rejects.toThrow();
  });

  it('does not log id_token or refresh_token', async () => {
    const sinks = (['debug', 'log', 'info', 'warn', 'error'] as const).map((m) =>
      vi.spyOn(console, m).mockImplementation(() => undefined)
    );
    try {
      const { fetch } = makeFetch(() => jsonResponse(200, SUCCESS_BODY));
      await pollDeviceFlowOnce(
        AS,
        { clientId: FAKE_CLIENT_ID, clientSecret: FAKE_CLIENT_SECRET },
        'AH-1Ng-test',
        { fetch }
      );
      const blob = sinks
        .flatMap((s) => s.mock.calls.flat())
        .map((c) => (typeof c === 'string' ? c : JSON.stringify(c)))
        .join(' ');
      expect(blob).not.toContain(ID_TOKEN);
      expect(blob).not.toContain(SUCCESS_BODY.refresh_token);
      expect(blob).not.toContain(SUCCESS_BODY.access_token);
    } finally {
      sinks.forEach((s) => s.mockRestore());
    }
  });
});

describe('redactTokens', () => {
  it('redacts Google access tokens (ya29.*)', () => {
    const s = 'token=ya29.aBcDeF_-1234567890XYZ end';
    expect(redactTokens(s)).not.toContain('ya29.aBcDeF');
    expect(redactTokens(s)).toContain('[redacted-token]');
  });

  it('redacts Google refresh tokens (1//*)', () => {
    const s = 'rt=1//0abcDEF-1234_xyz end';
    expect(redactTokens(s)).not.toContain('1//0abcDEF');
    expect(redactTokens(s)).toContain('[redacted-token]');
  });

  it('redacts JWT-shaped substrings (xxx.yyy.zzz)', () => {
    const jwt =
      'eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ4In0.signature_bytes_here_AB12';
    expect(redactTokens(`auth: ${jwt} end`)).not.toContain(jwt);
    expect(redactTokens(`auth: ${jwt} end`)).toContain('[redacted-token]');
  });

  it('passes through strings with no token shapes', () => {
    expect(redactTokens('hello world')).toBe('hello world');
  });
});

describe('buildAuthorizationServer', () => {
  // Sanity: the helper that synthesises (or caches) an AuthorizationServer
  // returns a usable object for the local-fetch path. Discovery against
  // live Google is not exercised here.
  it('returns an AuthorizationServer with the requested issuer', () => {
    const as = buildAuthorizationServer({
      issuer: ISSUER,
      device_authorization_endpoint: AS.device_authorization_endpoint!,
      token_endpoint: AS.token_endpoint!,
    });
    expect(as.issuer).toBe(ISSUER);
    expect(as.device_authorization_endpoint).toBe(
      'https://oauth2.googleapis.com/device/code'
    );
    expect(as.token_endpoint).toBe('https://oauth2.googleapis.com/token');
  });
});
