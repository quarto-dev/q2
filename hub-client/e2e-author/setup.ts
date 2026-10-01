/**
 * Playwright global setup/teardown for the author-attribution E2E
 * (playwright.author-e2e.config.ts).
 *
 * Stands up the full auth-ON stack the way scripts/hub-sliding-sessions-e2e.mjs
 * does — a standalone mock OIDC IdP (Node built-ins only) in front of the
 * real `hub` binary — plus the auth-DISABLED stack for the parity scenario,
 * and static+proxy servers (scripts/local-prod-server.mjs) so the built
 * client reaches each hub through the same origin its session cookie belongs
 * to. This is deliberately separate from e2e/helpers/globalSetup.ts: that
 * one boots the auth-disabled hub the whole default suite shares.
 *
 * Runs once before all tests in this config; globalTeardown kills everything.
 * Test workers read the written info file (ports, per-user credentials).
 */

import { generateKeyPairSync, sign, type KeyObject } from 'node:crypto';
import { createServer, type Server } from 'node:http';
import { spawn, type ChildProcess } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { startHubServer, type HubServerHandle } from '../e2e/helpers/syncServer';

/** Well-known path where runtime info is written for test workers. */
export const AUTHOR_E2E_INFO_PATH = '/tmp/hub-author-e2e.json';

export interface AuthorE2EUser {
  /** Google-style ID token (works as a Bearer on the hub's HTTP/WS APIs). */
  google: string;
  /** Hub-minted session cookie value (quarto_hub_token). */
  session: string;
  /** The OIDC `sub` claim. */
  sub: string;
}

export interface AuthorE2EInfo {
  /** Authenticated stack: static proxy origin the browser uses. */
  proxyAuthUrl: string;
  /** Authenticated hub's direct HTTP base (Node-side clients). */
  hubAuthHttpUrl: string;
  /** Authenticated hub's direct WS URL (Node-side sync clients). */
  hubAuthWsUrl: string;
  /** Auth-disabled stack: static proxy origin the browser uses. */
  proxyOpenUrl: string;
  /** Auth-disabled hub's direct HTTP base. */
  hubOpenHttpUrl: string;
  /** Auth-disabled hub's direct WS URL. */
  hubOpenWsUrl: string;
  alice: AuthorE2EUser;
  bob: AuthorE2EUser;
}

// Ports distinct from the default suite (3031 hub / 5174 preview) and from
// the other scripts/hub-*-e2e.mjs helpers (3997).
const AUTH_HUB_PORT = 3998;
const AUTH_PROXY_PORT = 3999;
const OPEN_HUB_PORT = 3995;
const OPEN_PROXY_PORT = 3996;
const CLIENT_ID = 'author-e2e.test';

const b64u = (buf: Buffer | string) => Buffer.from(buf).toString('base64url');
const now = () => Math.floor(Date.now() / 1000);

// ---------------------------------------------------------------------------
// Mock OIDC IdP (discovery + JWKS only — the hub's /auth/refresh flow
// verifies the Google-style token signature against these keys)
// ---------------------------------------------------------------------------

interface IdpHandle {
  issuer: string;
  close(): Promise<void>;
}

async function startMockIdp(): Promise<{
  handle: IdpHandle;
  mintToken: (opts: { sub: string; name: string }) => string;
}> {
  const { publicKey, privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
  const jwk = {
    ...(publicKey as KeyObject).export({ format: 'jwk' }),
    alg: 'RS256',
    use: 'sig',
    kid: 'author-e2e-kid-1',
  };

  let issuer = '';
  const idp: Server = createServer((req, res) => {
    res.setHeader('content-type', 'application/json');
    if (req.url === '/.well-known/openid-configuration') {
      res.end(JSON.stringify({ issuer, jwks_uri: `${issuer}/jwks.json` }));
    } else if (req.url === '/jwks.json') {
      res.end(JSON.stringify({ keys: [jwk] }));
    } else {
      res.statusCode = 404;
      res.end('{}');
    }
  });
  await new Promise<void>((r) => idp.listen(0, '127.0.0.1', r));
  const port = (idp.address() as { port: number }).port;
  issuer = `http://127.0.0.1:${port}`;

  const mintToken = ({ sub, name }: { sub: string; name: string }): string => {
    const header = b64u(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: 'author-e2e-kid-1' }));
    const payload = b64u(
      JSON.stringify({
        iss: issuer,
        sub,
        aud: CLIENT_ID,
        email: `${sub}@e2e.posit.co`,
        email_verified: true,
        name,
        iat: now() - 5,
        // Long-lived: the tokens double as Node-side Bearer credentials
        // for the whole run.
        exp: now() + 3600,
      }),
    );
    const signature = sign('sha256', Buffer.from(`${header}.${payload}`), privateKey);
    return `${header}.${payload}.${b64u(signature)}`;
  };

  return {
    handle: {
      issuer,
      async close() {
        await new Promise<void>((r) => idp.close(() => r()));
      },
    },
    mintToken,
  };
}

// ---------------------------------------------------------------------------
// Static + proxy server (scripts/local-prod-server.mjs as a child process)
// ---------------------------------------------------------------------------

interface ProxyHandle {
  url: string;
  stop(): Promise<void>;
}

async function startStaticProxy(staticPort: number, hubPort: number): Promise<ProxyHandle> {
  const serverScript = resolve(import.meta.dirname, '../../scripts/local-prod-server.mjs');
  const proc: ChildProcess = spawn(process.execPath, [serverScript], {
    env: { ...process.env, STATIC_PORT: String(staticPort), HUB_PORT: String(hubPort) },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  proc.stdout?.on('data', () => {});
  proc.stderr?.on('data', (d: Buffer) => console.error(`  [proxy:${staticPort}] ${d}`));

  const url = `http://127.0.0.1:${staticPort}`;
  const deadline = Date.now() + 30_000;
  for (;;) {
    try {
      const res = await fetch(url);
      if (res.ok) break;
    } catch {
      // not up yet
    }
    if (Date.now() > deadline) throw new Error(`static proxy :${staticPort} did not start`);
    await new Promise((r) => setTimeout(r, 100));
  }
  return {
    url,
    async stop() {
      if (!proc.killed) proc.kill('SIGTERM');
    },
  };
}

// ---------------------------------------------------------------------------
// Login: POST a Google-style token to /auth/session, harvest the session cookie
// ---------------------------------------------------------------------------
// `/auth/session` is the direct-JSON mint endpoint for Generic (non-form-post)
// OIDC providers — what our mock IdP looks like to the hub. (The older
// scripts/hub-sliding-sessions-e2e.mjs posts to /auth/refresh, the
// pre-rename path.)

async function login(proxyUrl: string, google: string): Promise<string> {
  const res = await fetch(`${proxyUrl}/auth/session`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-requested-with': 'XMLHttpRequest' },
    body: JSON.stringify({ credential: google }),
  });
  if (res.status !== 200) {
    throw new Error(`login failed: /auth/session -> ${res.status}`);
  }
  const cookie = res.headers
    .getSetCookie()
    .find((c) => c.startsWith('quarto_hub_token='));
  if (!cookie) throw new Error('login did not set quarto_hub_token');
  return cookie.split(';')[0]!.slice('quarto_hub_token='.length);
}

// ---------------------------------------------------------------------------
// Playwright lifecycle
// ---------------------------------------------------------------------------

export default async function globalSetup() {
  console.log('\n--- Author-attribution E2E Global Setup ---');

  // Track everything started so a mid-setup failure doesn't orphan
  // processes holding the fixed ports (the next run would bind-fail).
  const started: Array<{ stop(): Promise<void> }> = [];
  try {
    const { handle: idp, mintToken } = await startMockIdp();
    started.push({ stop: () => idp.close() });
    console.log(`[idp] discovery+jwks at ${idp.issuer}`);

    const hubAuth = await startHubServer(AUTH_HUB_PORT, {
      oidc: { clientId: CLIENT_ID, issuer: idp.issuer },
    });
    started.push(hubAuth);
    console.log(`[hub:auth] ${hubAuth.url} (OIDC on)`);

    const hubOpen = await startHubServer(OPEN_HUB_PORT);
    started.push(hubOpen);
    console.log(`[hub:open] ${hubOpen.url} (auth disabled)`);

    const proxyAuth = await startStaticProxy(AUTH_PROXY_PORT, AUTH_HUB_PORT);
    started.push(proxyAuth);
    const proxyOpen = await startStaticProxy(OPEN_PROXY_PORT, OPEN_HUB_PORT);
    started.push(proxyOpen);
    console.log(`[proxy] auth=${proxyAuth.url} open=${proxyOpen.url}`);

    const aliceGoogle = mintToken({ sub: 'author-e2e-alice', name: 'Alice E2E' });
    const bobGoogle = mintToken({ sub: 'author-e2e-bob', name: 'Bob E2E' });
    const alice: AuthorE2EUser = {
      google: aliceGoogle,
      session: await login(proxyAuth.url, aliceGoogle),
      sub: 'author-e2e-alice',
    };
    const bob: AuthorE2EUser = {
      google: bobGoogle,
      session: await login(proxyAuth.url, bobGoogle),
      sub: 'author-e2e-bob',
    };
    console.log('[auth] alice + bob logged in (session cookies minted)');

    const info: AuthorE2EInfo = {
      proxyAuthUrl: proxyAuth.url,
      hubAuthHttpUrl: `http://127.0.0.1:${AUTH_HUB_PORT}`,
      hubAuthWsUrl: `ws://127.0.0.1:${AUTH_HUB_PORT}/ws`,
      proxyOpenUrl: proxyOpen.url,
      hubOpenHttpUrl: `http://127.0.0.1:${OPEN_HUB_PORT}`,
      hubOpenWsUrl: `ws://127.0.0.1:${OPEN_HUB_PORT}/ws`,
      alice,
      bob,
    };
    writeFileSync(AUTHOR_E2E_INFO_PATH, JSON.stringify(info));

    (globalThis as Record<string, unknown>).__AUTHOR_E2E__ = {
      idp,
      hubAuth,
      hubOpen,
      proxyAuth,
      proxyOpen,
    };
    console.log('--- Author-attribution E2E Global Setup Complete ---\n');
  } catch (err) {
    // Roll back in reverse start order; never leave a process behind.
    for (const handle of started.reverse()) {
      await handle.stop().catch(() => {});
    }
    throw err;
  }
}

export async function globalTeardown() {
  const handles = (globalThis as Record<string, unknown>).__AUTHOR_E2E__ as
    | {
        idp: IdpHandle;
        hubAuth: HubServerHandle;
        hubOpen: HubServerHandle;
        proxyAuth: ProxyHandle;
        proxyOpen: ProxyHandle;
      }
    | undefined;
  if (!handles) return;
  await handles.proxyAuth.stop();
  await handles.proxyOpen.stop();
  await handles.hubAuth.stop();
  await handles.hubOpen.stop();
  await handles.idp.close();
}
