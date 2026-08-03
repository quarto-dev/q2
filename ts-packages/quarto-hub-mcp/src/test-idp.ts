/**
 * Test helper: a minimal in-process OIDC identity provider.
 *
 * Serves the four endpoints the full auth stack needs — discovery,
 * JWKS, authorization (instant consent: 302 straight back to the
 * loopback redirect_uri), token (authorization-code + PKCE S256
 * verification, and refresh grants), and revocation — and mints REAL
 * RS256-signed ID tokens, because the hub validates signatures against
 * the JWKS while the TS client validates the claims.
 *
 * Counters expose what happened (code exchanges, refresh grants,
 * revoked tokens) so tests can assert on flow shape, not just
 * outcomes. `idTokenTtlSecs` below the refresh manager's 60s
 * early-refresh window forces the refresh grant deterministically.
 */

import * as crypto from 'node:crypto';
import * as http from 'node:http';
import { once } from 'node:events';

export interface TestIdpOptions {
  clientId: string;
  clientSecret: string;
  email: string;
  /** ID-token lifetime; < 60s forces proactive refresh on every use. */
  idTokenTtlSecs?: number;
}

/**
 * The single, never-rotated refresh token the IdP hands out
 * (Google-style). Exported so tests can simulate a mid-session grant
 * revocation by pushing it into `counters.revokedTokens` — every
 * subsequent refresh grant then answers `invalid_grant`.
 */
export const TEST_REFRESH_TOKEN = 'rt-test-refresh-token';

export interface TestIdp {
  issuer: string;
  counters: {
    codeExchanges: number;
    refreshGrants: number;
    revokedTokens: string[];
  };
  stop(): Promise<void>;
}

export async function startTestIdp(opts: TestIdpOptions): Promise<TestIdp> {
  const ttl = opts.idTokenTtlSecs ?? 3600;
  const { publicKey, privateKey } = crypto.generateKeyPairSync('rsa', {
    modulusLength: 2048,
  });
  const kid = 'test-key-1';
  const jwk = { ...publicKey.export({ format: 'jwk' }), kid, alg: 'RS256', use: 'sig' };

  const counters: TestIdp['counters'] = {
    codeExchanges: 0,
    refreshGrants: 0,
    revokedTokens: [],
  };
  // code -> the PKCE challenge it was issued against
  const pendingCodes = new Map<string, { challenge: string }>();
  const REFRESH_TOKEN = TEST_REFRESH_TOKEN;
  let issuer = ''; // assigned after listen()

  const b64url = (input: Buffer | string): string =>
    Buffer.from(input).toString('base64url');

  function mintIdToken(): string {
    const now = Math.floor(Date.now() / 1000);
    const header = b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid }));
    const payload = b64url(
      JSON.stringify({
        iss: issuer,
        aud: opts.clientId,
        azp: opts.clientId,
        sub: 'test-subject-1',
        email: opts.email,
        email_verified: true,
        iat: now,
        exp: now + ttl,
      }),
    );
    const signature = crypto
      .sign('sha256', Buffer.from(`${header}.${payload}`), privateKey)
      .toString('base64url');
    return `${header}.${payload}.${signature}`;
  }

  function tokenResponse(): string {
    return JSON.stringify({
      access_token: `at-${crypto.randomUUID()}`,
      token_type: 'bearer',
      expires_in: ttl,
      id_token: mintIdToken(),
      refresh_token: REFRESH_TOKEN, // Google-style: never rotated
    });
  }

  async function readBody(req: http.IncomingMessage): Promise<URLSearchParams> {
    const chunks: Buffer[] = [];
    for await (const chunk of req) chunks.push(chunk as Buffer);
    return new URLSearchParams(Buffer.concat(chunks).toString('utf8'));
  }

  const server = http.createServer((req, res) => {
    void handle(req, res).catch((err) => {
      res.writeHead(500, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'server_error', detail: String(err) }));
    });
  });

  async function handle(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    const url = new URL(req.url ?? '/', issuer);
    const json = (status: number, body: unknown): void => {
      res.writeHead(status, { 'content-type': 'application/json' });
      res.end(JSON.stringify(body));
    };

    if (url.pathname === '/.well-known/openid-configuration') {
      json(200, {
        issuer,
        authorization_endpoint: `${issuer}/authorize`,
        token_endpoint: `${issuer}/token`,
        jwks_uri: `${issuer}/jwks`,
        revocation_endpoint: `${issuer}/revoke`,
      });
      return;
    }

    if (url.pathname === '/jwks') {
      json(200, { keys: [jwk] });
      return;
    }

    if (url.pathname === '/authorize') {
      // Instant consent: validate the request shape, issue a code bound
      // to the PKCE challenge, bounce straight back to the loopback.
      const clientId = url.searchParams.get('client_id');
      const redirectUri = url.searchParams.get('redirect_uri');
      const state = url.searchParams.get('state');
      const challenge = url.searchParams.get('code_challenge');
      const method = url.searchParams.get('code_challenge_method');
      if (clientId !== opts.clientId || !redirectUri || !state || !challenge || method !== 'S256') {
        json(400, { error: 'invalid_request' });
        return;
      }
      const code = `code-${crypto.randomUUID()}`;
      pendingCodes.set(code, { challenge });
      const target = new URL(redirectUri);
      target.searchParams.set('code', code);
      target.searchParams.set('state', state);
      res.writeHead(302, { location: target.toString() });
      res.end();
      return;
    }

    if (url.pathname === '/token' && req.method === 'POST') {
      const body = await readBody(req);
      if (body.get('client_id') !== opts.clientId || body.get('client_secret') !== opts.clientSecret) {
        json(401, { error: 'invalid_client' });
        return;
      }
      const grant = body.get('grant_type');
      if (grant === 'authorization_code') {
        const code = body.get('code') ?? '';
        const verifier = body.get('code_verifier') ?? '';
        const pending = pendingCodes.get(code);
        if (!pending) {
          json(400, { error: 'invalid_grant' });
          return;
        }
        const expected = crypto.createHash('sha256').update(verifier).digest('base64url');
        if (expected !== pending.challenge) {
          json(400, { error: 'invalid_grant', error_description: 'PKCE verification failed' });
          return;
        }
        pendingCodes.delete(code);
        counters.codeExchanges += 1;
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(tokenResponse());
        return;
      }
      if (grant === 'refresh_token') {
        const rt = body.get('refresh_token');
        if (rt !== REFRESH_TOKEN || counters.revokedTokens.includes(REFRESH_TOKEN)) {
          json(400, { error: 'invalid_grant' });
          return;
        }
        counters.refreshGrants += 1;
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(tokenResponse());
        return;
      }
      json(400, { error: 'unsupported_grant_type' });
      return;
    }

    if (url.pathname === '/revoke' && req.method === 'POST') {
      const body = await readBody(req);
      const token = body.get('token');
      if (token) counters.revokedTokens.push(token);
      json(200, {});
      return;
    }

    json(404, { error: 'not_found' });
  }

  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (address === null || typeof address === 'string') {
    throw new Error('test IdP failed to bind a TCP port');
  }
  issuer = `http://127.0.0.1:${address.port}`;

  return {
    issuer,
    counters,
    async stop(): Promise<void> {
      server.close();
      await once(server, 'close');
    },
  };
}
