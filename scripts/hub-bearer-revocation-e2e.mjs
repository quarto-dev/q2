// End-to-end verification for revocation-ledger enforcement on the
// Bearer path (F1, bd-jkih1ql7). Drives the REAL `hub` binary over
// HTTP with a standalone mock OIDC IdP (this script). Node built-ins
// only. Plan:
// claude-notes/plans/2026-08-03-bearer-revocation-and-mcp-auth-followups.md
//
// Steps:
//  1. baseline: Google Bearer for subs A/B/C -> /health 200, /ws 101
//     (A also shows the WS upgrade shape an MCP client uses).
//  2. stop the hub; apply the documented stopped-hub operator
//     procedure to revocations.json: ban A, write a not_before floor
//     for B; restart.
//  3. banned A -> 403 on /health AND on the WS upgrade, with any
//     token (even a fresh one — bans are iat-independent).
//  4. B's pre-floor token (iat < not_before) -> 401; B's post-floor
//     token (fresh iat) -> 200 (the MCP self-heal-on-refresh path).
//  5. untouched C still works -> 200 / 101.
//
// Prereq: cargo build --bin hub
import { generateKeyPairSync, sign, randomUUID } from 'node:crypto';
import { createServer } from 'node:http';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CLIENT_ID = 'mcp.e2e.test';
const HUB_PORT = 3996;
const HUB = `http://127.0.0.1:${HUB_PORT}`;

const b64u = (buf) => Buffer.from(buf).toString('base64url');
const now = () => Math.floor(Date.now() / 1000);

// ── mock IdP ────────────────────────────────────────────────────────
const { publicKey, privateKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
const jwk = { ...publicKey.export({ format: 'jwk' }), alg: 'RS256', use: 'sig', kid: 'e2e-kid-1' };

function googleToken({ sub, iat = now() - 5, expIn = 600 }) {
  const header = b64u(JSON.stringify({ alg: 'RS256', typ: 'JWT', kid: 'e2e-kid-1' }));
  const payload = b64u(JSON.stringify({
    iss: issuer, sub, aud: CLIENT_ID, email: 'e2e@posit.co', email_verified: true,
    name: 'E2E User', iat, exp: now() + expIn,
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
const dataDir = mkdtempSync(join(tmpdir(), 'hub-bearer-revocation-e2e-'));
const hubArgs = [
  '--data-dir', dataDir, '--port', String(HUB_PORT),
  '--oidc-client-id', CLIENT_ID, '--oidc-issuer', issuer,
  '--allow-insecure-auth',
];

let hub;
async function startHub() {
  console.log(`[hub] target/debug/hub ${hubArgs.join(' ')}`);
  hub = spawn('target/debug/hub', hubArgs, { stdio: ['ignore', 'pipe', 'pipe'] });
  hub.stderr.on('data', () => {});
  hub.stdout.on('data', () => {});
  for (let i = 0; ; i++) {
    try { await fetch(`${HUB}/auth/me`); break; }
    catch { if (i > 50) throw new Error('hub did not start'); await new Promise((r) => setTimeout(r, 200)); }
  }
  console.log('[hub] up');
}
async function stopHub() {
  const exited = new Promise((r) => hub.once('exit', r));
  hub.kill();
  await exited;
  console.log('[hub] stopped');
}
process.on('exit', () => hub?.kill());

// ── helpers ─────────────────────────────────────────────────────────
let failures = 0;
const check = (label, cond, detail) => {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${label}${detail ? ` — ${detail}` : ''}`);
  if (!cond) failures++;
};
const health = (token) =>
  fetch(`${HUB}/health`, { headers: { authorization: `Bearer ${token}` } });
const wsUpgrade = (token) => new Promise((resolve) => {
  const req = http.request(`${HUB}/ws`, {
    headers: {
      connection: 'Upgrade', upgrade: 'websocket', 'sec-websocket-version': '13',
      'sec-websocket-key': 'dGVzdHNvY2tleS0xMjM0NTY3OA==',
      host: `127.0.0.1:${HUB_PORT}`,
      authorization: `Bearer ${token}`,
    },
  });
  req.on('upgrade', (res) => { resolve(res.statusCode); req.destroy(); });
  req.on('response', (res) => resolve(res.statusCode));
  req.on('error', () => resolve(-1));
  req.end();
});

const T = now();
const subA = `banned-${randomUUID()}`;
const subB = `revoked-${randomUUID()}`;
const subC = `untouched-${randomUUID()}`;
// Explicit iats so the checks are deterministic no matter how long the
// restart takes: B's old token predates the floor, its fresh one follows it.
const tokenA = googleToken({ sub: subA, iat: T - 5 });
const tokenBOld = googleToken({ sub: subB, iat: T - 100 });
const tokenC = googleToken({ sub: subC, iat: T - 5 });
const NOT_BEFORE_B = T - 50;

// ── 1. baseline: everyone authenticates ─────────────────────────────
await startHub();
check('baseline: A /health -> 200', (await health(tokenA)).status === 200);
check('baseline: B /health -> 200', (await health(tokenBOld)).status === 200);
check('baseline: C /health -> 200', (await health(tokenC)).status === 200);
check('baseline: A /ws -> 101', (await wsUpgrade(tokenA)) === 101);

// ── 2. stopped-hub operator procedure ───────────────────────────────
await stopHub();
const revPath = join(dataDir, 'revocations.json');
writeFileSync(revPath, JSON.stringify({
  version: 1,
  not_before: { [subB]: NOT_BEFORE_B },
  banned: [subA],
}));
console.log(`[operator] wrote ${revPath}: ${readFileSync(revPath, 'utf8')}`);
await startHub();

// ── 3. banned sub: 403 on probe and WS, iat-independent ────────────
check('banned A /health -> 403', (await health(tokenA)).status === 403);
check('banned A /ws -> 403', (await wsUpgrade(tokenA)) === 403);
const tokenAFresh = googleToken({ sub: subA });
check('banned A with a FRESH token -> still 403 (bans are iat-independent)',
  (await health(tokenAFresh)).status === 403);

// ── 4. not_before floor: old iat dies, fresh iat self-heals ────────
check('B pre-floor token (iat < not_before) /health -> 401',
  (await health(tokenBOld)).status === 401);
check('B pre-floor token /ws -> 401', (await wsUpgrade(tokenBOld)) === 401);
const tokenBFresh = googleToken({ sub: subB, iat: now() - 5 });
check('B post-floor token (fresh iat) -> 200 (self-heal on refresh)',
  (await health(tokenBFresh)).status === 200);

// ── 5. untouched identity unaffected ────────────────────────────────
check('untouched C /health -> 200', (await health(tokenC)).status === 200);
check('untouched C /ws -> 101', (await wsUpgrade(tokenC)) === 101);

await stopHub();
idp.close();
console.log(failures === 0 ? '\nALL CHECKS PASSED' : `\n${failures} CHECK(S) FAILED`);
process.exit(failures === 0 ? 0 : 1);
