// End-to-end verification for hub server-minted sliding sessions (C7,
// bd-6s83nc38). Drives the REAL `hub` binary over HTTP with a
// standalone mock OIDC IdP (this script). Node built-ins only.
//
// Steps:
//  1. login via POST /auth/refresh with a SHORT-LIVED Google-style
//     token (exp = now + 45 s) -> hub-minted session cookie (compact).
//  2. /auth/me -> 200, sliding exp ~ now + 7 d.
//  3. hard break: the Google token in the cookie -> 401.
//  4. cross-path: session token as Bearer -> 401; Google Bearer -> 200.
//  5. WS upgrade with the session cookie -> 101.
//  6. wait 120 s (Google token now dead even with 60 s leeway).
//  7. /auth/me with the session cookie -> STILL 200: the session
//     outlived the Google credential with zero IdP interaction.
//  8. second login (device B); logout-everywhere from A; both A and B
//     -> 401; fresh re-login -> 200.
import { generateKeyPairSync, sign, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CLIENT_ID = 'spa.e2e.test';
const HUB_PORT = 3997;
const HUB = `http://127.0.0.1:${HUB_PORT}`;

const b64u = (buf) => Buffer.from(buf).toString('base64url');
const now = () => Math.floor(Date.now() / 1000);

// ── mock IdP ────────────────────────────────────────────────────────
const { publicKey, privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const jwk = { ...publicKey.export({ format: 'jwk' }), alg: 'RS256', use: 'sig', kid: 'e2e-kid-1' };

function googleToken({ sub, email, expIn }) {
  const header = b64u(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: 'e2e-kid-1' }));
  const payload = b64u(JSON.stringify({
    iss: issuer, sub, aud: CLIENT_ID, email, email_verified: true,
    name: 'E2E User', iat: now() - 5, exp: now() + expIn,
  }));
  const sig = sign('sha256', Buffer.from(`${header}.${payload}`), privateKey);
  return `${header}.${payload}.${b64u(sig)}`;
}

const idp = createServer((req, res) => {
  res.setHeader('content-type', 'application/json');
  if (req.url === '/.well-known/openid-configuration') {
    res.end(JSON.stringify({ issuer, jwks_uri: `${issuer}/jwks.json` }));
  } else if (req.url === '/jwks.json') {
    res.end(JSON.stringify({ keys: [jwk] }));
  } else {
    res.statusCode = 404; res.end('{}');
  }
});
await new Promise((r) => idp.listen(0, '127.0.0.1', r));
const issuer = `http://127.0.0.1:${idp.address().port}`;
console.log(`[idp] serving discovery+jwks at ${issuer}`);

// ── the real hub binary ─────────────────────────────────────────────
const dataDir = mkdtempSync(join(tmpdir(), 'hub-e2e-'));
const hubArgs = [
  '--data-dir', dataDir, '--port', String(HUB_PORT),
  '--oidc-client-id', CLIENT_ID, '--oidc-issuer', issuer,
  '--allow-insecure-auth',
];
console.log(`[hub] target/debug/hub ${hubArgs.join(' ')}`);
const hub = spawn('target/debug/hub', hubArgs, { stdio: ['ignore', 'pipe', 'pipe'] });
hub.stderr.on('data', () => {});
hub.stdout.on('data', () => {});
process.on('exit', () => hub.kill());

for (let i = 0; ; i++) {
  try { await fetch(`${HUB}/auth/me`); break; }
  catch { if (i > 50) throw new Error('hub did not start'); await new Promise((r) => setTimeout(r, 200)); }
}
console.log('[hub] up');

// ── helpers ─────────────────────────────────────────────────────────
let failures = 0;
const check = (label, cond, detail) => {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${label}${detail ? ` — ${detail}` : ''}`);
  if (!cond) failures++;
};
const cookieOf = (res) => {
  const sc = res.headers.getSetCookie().find((c) => c.startsWith('quarto_hub_token='));
  return sc ? sc.split(';')[0].slice('quarto_hub_token='.length) : null;
};
const login = async (sub, expIn = 45) => {
  const g = googleToken({ sub, email: 'e2e@posit.co', expIn });
  const res = await fetch(`${HUB}/auth/refresh`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-requested-with': 'XMLHttpRequest' },
    body: JSON.stringify({ credential: g }),
  });
  return { res, google: g, session: cookieOf(res) };
};
const me = (cookie) => fetch(`${HUB}/auth/me`, { headers: { cookie: `quarto_hub_token=${cookie}` } });

// ── 1. login mints a compact session cookie ─────────────────────────
const a = await login(`sub-${randomUUID()}`);
check('login (POST /auth/refresh) returns 200', a.res.status === 200, `status=${a.res.status}`);
check('session cookie minted (not the Google JWT)', !!a.session && a.session !== a.google);
check('session cookie is compact', a.session.length < 1024,
  `${a.session.length} bytes (Google token was ${a.google.length} bytes)`);

// ── 2. sliding exp ~ 7 days ─────────────────────────────────────────
const meRes = await me(a.session);
const meBody = await meRes.json();
const expDelta = meBody.exp - now();
check('/auth/me 200 with session cookie', meRes.status === 200);
check('exp is sliding (~7 d out, not Google\'s 45 s)',
  expDelta > 6.9 * 86400 && expDelta <= 7 * 86400 + 60, `exp - now = ${expDelta}s`);

// ── 3./4. hard break + cross-path ───────────────────────────────────
const legacy = await me(a.google);
check('hard break: Google JWT in cookie -> 401', legacy.status === 401, `status=${legacy.status}`);
const crossBearer = await fetch(`${HUB}/health`, { headers: { authorization: `Bearer ${a.session}` } });
check('cross-path: session token as Bearer -> 401', crossBearer.status === 401, `status=${crossBearer.status}`);
const mcpBearer = await fetch(`${HUB}/health`, { headers: { authorization: `Bearer ${a.google}` } });
check('MCP path: Google token as Bearer -> 200 (unaffected)', mcpBearer.status === 200, `status=${mcpBearer.status}`);

// ── 5. WS upgrade with session cookie ───────────────────────────────
const wsStatus = await new Promise((resolve) => {
  const req = http.request(`${HUB}/ws`, {
    headers: {
      connection: 'Upgrade', upgrade: 'websocket', 'sec-websocket-version': '13',
      'sec-websocket-key': 'dGVzdHNvY2tleS0xMjM0NTY3OA==',
      origin: HUB, host: `127.0.0.1:${HUB_PORT}`,
      cookie: `quarto_hub_token=${a.session}`,
    },
  });
  req.on('upgrade', (res) => { resolve(res.statusCode); req.destroy(); });
  req.on('response', (res) => resolve(res.statusCode));
  req.on('error', () => resolve(-1));
  req.end();
});
check('WS upgrade with session cookie -> 101', wsStatus === 101, `status=${wsStatus}`);

// ── 6./7. session outlives the Google credential ────────────────────
console.log('[wait] 120 s so the 45 s Google token is dead beyond the 60 s leeway…');
await new Promise((r) => setTimeout(r, 120_000));
const googleDead = await fetch(`${HUB}/health`, { headers: { authorization: `Bearer ${a.google}` } });
check('the Google token is now definitively expired (Bearer -> 401)', googleDead.status === 401,
  `status=${googleDead.status}`);
const stillAlive = await me(a.session);
check('session cookie STILL authenticates — outlived the Google token, zero IdP interaction',
  stillAlive.status === 200, `status=${stillAlive.status}`);

// ── 8. logout-everywhere across two sessions ────────────────────────
const subShared = `sub-${randomUUID()}`;
const devA = await login(subShared, 600);
const devB = await login(subShared, 600);
check('two devices logged in (A, B)', devA.res.status === 200 && devB.res.status === 200);
const revoke = await fetch(`${HUB}/auth/logout-everywhere`, {
  method: 'POST',
  headers: { cookie: `quarto_hub_token=${devA.session}`, 'x-requested-with': 'XMLHttpRequest' },
});
check('POST /auth/logout-everywhere from device A -> 200', revoke.status === 200, `status=${revoke.status}`);
const bAfter = await me(devB.session);
const aAfter = await me(devA.session);
check("device B's next request -> 401 (logged-out flow)", bAfter.status === 401, `status=${bAfter.status}`);
check("device A's token also dead", aAfter.status === 401, `status=${aAfter.status}`);
const relogin = await login(subShared, 600);
const reloginMe = await me(relogin.session);
check('immediate re-login works after revocation', reloginMe.status === 200, `status=${reloginMe.status}`);

console.log(failures === 0 ? '\nALL CHECKS PASSED' : `\n${failures} CHECK(S) FAILED`);
hub.kill();
idp.close();
process.exit(failures === 0 ? 0 : 1);
