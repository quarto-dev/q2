// End-to-end verification of the `/?auth_error=<reason>` redirect codes
// (E1/E2, bd-htis60s7 + bd-sxnfoefn). Drives the REAL `hub` binary over
// HTTP. Node built-ins only.
//
//   cargo build --bin hub
//   node scripts/hub-auth-error-reasons-e2e.mjs
//
// WHY THIS SCRIPT DOES NOT MOCK THE IdP
// ------------------------------------
// `POST /auth/callback` is registered only when the provider is Google,
// and the provider is derived from the issuer being exactly
// `https://accounts.google.com` (`AuthConfig::new`). Point the hub at a
// mock IdP on localhost and the route 404s, so the sibling script
// `hub-sliding-sessions-e2e.mjs` — which mocks the IdP — structurally
// cannot reach any of these paths. This script therefore uses Google's
// real (public) discovery + JWKS endpoints, and needs outbound network.
//
// WHAT THAT BUYS, AND WHAT IT COSTS
// ---------------------------------
// Reachable: every rejection that happens *before* the credential is
// validated — the CSRF pair, and an undecodable credential. Both map to
// `restart`.
//
// Not reachable: `stale_client`, `denied` and `server` all require first
// *passing* `authenticate_claims`, i.e. a credential signed by Google for
// our audience. Forging one is the thing the hub exists to prevent. Those
// three are covered by the integration tests in
// `crates/quarto-hub/tests/integration/login_nonce.rs`, which drive the
// real router over real HTTP against a mock provider whose JWKS the hub
// actually fetches — bypassing only CLI parsing and OIDC discovery, both
// of which *this* script covers.
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const CLIENT_ID = 'e2e-reasons.apps.googleusercontent.com';
const HUB_PORT = 3992;
const HUB = `http://127.0.0.1:${HUB_PORT}`;

const dataDir = mkdtempSync(join(tmpdir(), 'hub-reasons-e2e-'));
const hubArgs = [
  '--data-dir', dataDir, '--port', String(HUB_PORT),
  '--oidc-client-id', CLIENT_ID,
  '--oidc-issuer', 'https://accounts.google.com',
  '--allow-insecure-auth',
];
console.log(`[hub] target/debug/hub ${hubArgs.join(' ')}`);
// RUST_LOG=info because the credential-path audit details (`jwt_decode:*`,
// `user_not_allowlisted`) are emitted at INFO, while the login-state and
// CSRF ones are WARN. At the hub's default verbosity only the WARNs show,
// so an operator chasing a credential failure needs `-v` / RUST_LOG.
const hub = spawn('target/debug/hub', hubArgs, {
  stdio: ['ignore', 'pipe', 'pipe'],
  env: { ...process.env, RUST_LOG: 'info' },
});
// The fmt layer writes ANSI escapes when it thinks a terminal is present,
// which lands *between* a field name and its `=`. Strip them before matching.
const ANSI = /\[[0-9;]*m/g;
let hubLog = '';
const collect = (d) => { hubLog += d.toString().replace(ANSI, ''); };
hub.stderr.on('data', collect);
hub.stdout.on('data', collect);
process.on('exit', () => hub.kill());

for (let i = 0; ; i++) {
  try { await fetch(`${HUB}/auth/me`); break; }
  catch {
    if (i > 75) throw new Error('hub did not start (OIDC discovery needs network)');
    await new Promise((r) => setTimeout(r, 200));
  }
}
console.log('[hub] up (Google discovery + JWKS fetched)');

let failures = 0;
const check = (label, cond, detail) => {
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${label}${detail ? ` — ${detail}` : ''}`);
  if (!cond) failures++;
};

const postCallback = (body, cookie) =>
  fetch(`${HUB}/auth/callback`, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded', ...(cookie ? { cookie } : {}) },
    body,
    redirect: 'manual',
  });

// ── the route exists at all (what a mock issuer cannot give us) ──────
const routed = await postCallback('credential=x&g_csrf_token=x', 'g_csrf_token=x');
check('POST /auth/callback is routed for a Google issuer', routed.status !== 404,
  `status=${routed.status}`);

// ── CSRF mismatch -> restart (E2's newly-audited path) ──────────────
const mismatch = await postCallback(
  'credential=irrelevant&g_csrf_token=from-the-form', 'g_csrf_token=from-the-cookie');
check('CSRF pair mismatch -> /?auth_error=restart',
  mismatch.headers.get('location') === '/?auth_error=restart',
  `location=${mismatch.headers.get('location')}`);

const missingCsrf = await postCallback('credential=irrelevant', undefined);
check('CSRF field absent entirely -> /?auth_error=restart',
  missingCsrf.headers.get('location') === '/?auth_error=restart',
  `location=${missingCsrf.headers.get('location')}`);

// ── undecodable credential -> restart (the 401 family) ──────────────
const badJwt = await postCallback('credential=not-a-jwt&g_csrf_token=m', 'g_csrf_token=m');
check('undecodable credential -> /?auth_error=restart',
  badJwt.headers.get('location') === '/?auth_error=restart',
  `location=${badJwt.headers.get('location')}`);

// No path may mint a session on the way out, and every one clears the
// sealed login-state blob.
for (const [label, res] of [['csrf', mismatch], ['jwt', badJwt]]) {
  const cookies = res.headers.getSetCookie();
  check(`${label}: no session cookie minted`,
    !cookies.some((c) => c.startsWith('quarto_hub_token=') && !c.includes('Max-Age=0')),
    cookies.join(' | ') || '(none)');
  check(`${label}: login-state cookie cleared`,
    cookies.some((c) => c.startsWith('quarto_hub_login=') && c.includes('Max-Age=0')),
    cookies.join(' | ') || '(none)');
}

// ── the audit trail an operator would grep (E2) ──────────────────────
await new Promise((r) => setTimeout(r, 300));
check('callback_csrf is in the audit log (was silent before E2)',
  /detail="?callback_csrf/.test(hubLog),
  hubLog.split('\n').find((l) => l.includes('callback_csrf')) ?? '(absent)');
check('the jwt_decode detail survives, unburied by a blanket emit',
  /detail="?jwt_decode:/.test(hubLog),
  hubLog.split('\n').find((l) => l.includes('jwt_decode:')) ?? '(absent)');

// ── E3: auto-generated secrets are loud, exactly once ────────────────
check('startup warns that it generated a session secret',
  /generated a new session secret/.test(hubLog) && /QUARTO_HUB_SESSION_SECRET/.test(hubLog));
check('startup warns that it generated a server secret',
  /generated a new server secret/.test(hubLog) && /QUARTO_HUB_SERVER_SECRET/.test(hubLog));

console.log(failures === 0 ? '\nALL CHECKS PASSED' : `\n${failures} CHECK(S) FAILED`);
hub.kill();
process.exit(failures === 0 ? 0 : 1);
