/**
 * Full-stack auth e2e (bd-81cfshmw Phase 3): real hub binary, real
 * loopback listener, real OS keyring, real `q2 mcp` launcher — only
 * the IdP is a mock (test-idp.ts), and even it mints real RS256 JWTs
 * that the hub verifies against its JWKS.
 *
 * Flow under test:
 *   1. unauthenticated tool call against an auth-enabled hub fails
 *      with a sign-in hint;
 *   2. `authenticate` runs the loopback+PKCE flow — the test scrapes
 *      the authorization URL from server stderr and plays the role of
 *      the browser (a fake `open` on PATH keeps the real one closed);
 *   3. authenticated `create_project` / `read_file` work over Bearer
 *      websocket;
 *   4. a SECOND process — the real `q2 mcp` launcher when its embed is
 *      fresh, else the dist build — reuses the keyring credential
 *      without re-authenticating (the npx/q2 channels share
 *      credentials by design);
 *   5. short-TTL ID tokens (45s < the 60s early-refresh window) force
 *      refresh grants, observed at the IdP;
 *   6. `authenticate_clear` revokes at the IdP and empties the keyring.
 *
 * Gating (skips loudly, never fails, when):
 *   - target/debug/hub is missing (cargo build -p quarto-hub);
 *   - the OS keyring is unusable (headless CI without a keychain);
 *   - for the q2-launcher channel only: target/debug/q2 is missing or
 *     its embedded bundle is a placeholder / from a different commit
 *     (cargo xtask build-hub-mcp-bundle && cargo build --bin q2).
 *
 * macOS-first (Carlos, 2026-06-11); the fake-`open` shim is unix-only.
 * Keyring hygiene: entries live under service 'dev.quarto.hub-mcp'
 * with account '<issuer>:<clientId>'; the issuer embeds an ephemeral
 * port, so afterAll deletes the entry — a crashed run can leak one
 * keychain entry for a 127.0.0.1 issuer (harmless, garbage account).
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { execFileSync, spawn, spawnSync, type ChildProcess } from 'node:child_process';
import { once } from 'node:events';
import * as fs from 'node:fs';
import * as net from 'node:net';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { AsyncEntry } from '@napi-rs/keyring';

import { McpTestClient } from './mcp-test-client.js';
import { startTestIdp, TEST_REFRESH_TOKEN, type TestIdp } from './test-idp.js';

const pkgRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = path.resolve(pkgRoot, '..', '..');
const hubBin = path.join(repoRoot, 'target', 'debug', 'hub');
const q2Bin = path.join(repoRoot, 'target', 'debug', 'q2');

const CLIENT_ID = 'q2-e2e-test-client.local';
const CLIENT_SECRET = 'q2-e2e-test-secret';
const EMAIL = 'tester@example.com';
const KEYRING_SERVICE = 'dev.quarto.hub-mcp';

// ---------------------------------------------------------------------------
// Gates (module-level await: vitest test files are ESM)
// ---------------------------------------------------------------------------

const hubAvailable = fs.existsSync(hubBin) && process.platform !== 'win32';

async function keyringUsable(): Promise<boolean> {
  try {
    const probe = new AsyncEntry(KEYRING_SERVICE, `e2e-probe-${process.pid}`);
    await probe.setPassword('probe');
    await probe.deletePassword();
    return true;
  } catch {
    return false;
  }
}
const keyringOk = hubAvailable ? await keyringUsable() : false;

const runSuite = hubAvailable && keyringOk;
if (!runSuite) {
  // eslint-disable-next-line no-console
  console.error(
    `[e2e-auth] SKIPPING: ${!hubAvailable ? `hub binary missing at ${hubBin} (cargo build -p quarto-hub) or non-unix` : 'OS keyring unusable in this environment'}`,
  );
}

/** q2-channel gate: embed must be real and from the current commit. */
function q2EmbedFresh(): { ok: boolean; reason: string } {
  if (!fs.existsSync(q2Bin)) return { ok: false, reason: `q2 binary missing at ${q2Bin}` };
  const info = spawnSync(q2Bin, ['mcp', '--launcher-info'], { encoding: 'utf8' });
  if (info.status !== 0) return { ok: false, reason: `launcher-info failed: ${info.stderr}` };
  if (info.stdout.includes('PLACEHOLDER')) {
    return {
      ok: false,
      reason: 'q2 embeds the placeholder bundle (cargo xtask build-hub-mcp-bundle && cargo build --bin q2)',
    };
  }
  const m = info.stdout.match(/"gitCommit":\s*"([0-9a-f]+)"/);
  const head = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim();
  if (!m || m[1] !== head) {
    return {
      ok: false,
      reason: `q2 embed is from commit ${m?.[1] ?? '?'} but HEAD is ${head} — rebuild (cargo xtask build-hub-mcp-bundle && cargo build --bin q2)`,
    };
  }
  return { ok: true, reason: '' };
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async function freePort(): Promise<number> {
  const srv = net.createServer();
  srv.listen(0, '127.0.0.1');
  await once(srv, 'listening');
  const port = (srv.address() as net.AddressInfo).port;
  srv.close();
  await once(srv, 'close');
  return port;
}

async function waitForHealth(url: string, timeoutMs = 20000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastErr: unknown;
  while (Date.now() < deadline) {
    try {
      // Any HTTP answer means the hub is up: an auth-enabled hub
      // serves 401 on /health to unauthenticated probes (that 401 is
      // exactly how the connection manager detects auth mode).
      const resp = await fetch(url);
      if (resp.status < 500) return;
      lastErr = `HTTP ${resp.status}`;
    } catch (err) {
      lastErr = err;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`hub did not answer at ${url}: ${lastErr}`);
}

function makeFakeOpenShim(dir: string): void {
  const shim = path.join(dir, 'open');
  fs.writeFileSync(shim, '#!/bin/sh\nexit 0\n');
  fs.chmodSync(shim, 0o755);
}

// ---------------------------------------------------------------------------
// Suite
// ---------------------------------------------------------------------------

describe.runIf(runSuite)('auth e2e (real hub + keyring + loopback)', () => {
  let idp: TestIdp;
  let hub: ChildProcess;
  let hubUrl: string;
  let hubPort: number;
  let tmpDir: string;
  let serverEnv: NodeJS.ProcessEnv;
  let keyringAccount: string;
  let projectId: string;

  function spawnHub(): ChildProcess {
    return spawn(
      hubBin,
      [
        '--data-dir', path.join(tmpDir, 'hub-data'),
        '-P', String(hubPort),
        '-H', '127.0.0.1',
        '--oidc-client-id', CLIENT_ID,
        '--oidc-issuer', idp.issuer,
        '--allowed-emails', EMAIL,
        '--allow-insecure-auth',
        // Hub-side auth audit trail when debugging (DEBUG_MCP=1).
        ...(process.env['DEBUG_MCP'] ? ['-vv'] : []),
      ],
      { stdio: ['ignore', 'ignore', process.env['DEBUG_MCP'] ? 'inherit' : 'ignore'] },
    );
  }

  beforeAll(async () => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'q2-e2e-auth-'));
    const shimDir = path.join(tmpDir, 'bin');
    fs.mkdirSync(shimDir);
    makeFakeOpenShim(shimDir);

    // 45s TTL: inside the refresh manager's 60s early-refresh window,
    // so every authenticated use forces a refresh grant.
    idp = await startTestIdp({
      clientId: CLIENT_ID,
      clientSecret: CLIENT_SECRET,
      email: EMAIL,
      idTokenTtlSecs: 45,
    });
    keyringAccount = `${idp.issuer}:${CLIENT_ID}`;
    // Defensive: a crashed previous run can't have this exact account
    // (ephemeral port), but clear anyway in case of port reuse.
    await new AsyncEntry(KEYRING_SERVICE, keyringAccount).deletePassword().catch(() => {});

    hubPort = await freePort();
    hubUrl = `ws://127.0.0.1:${hubPort}/ws`;
    hub = spawnHub();
    await waitForHealth(`http://127.0.0.1:${hubPort}/health`);

    serverEnv = {
      ...process.env,
      PATH: `${shimDir}:${process.env['PATH'] ?? ''}`,
      QUARTO_HUB_MCP_CLIENT_ID: CLIENT_ID,
      QUARTO_HUB_MCP_CLIENT_SECRET: CLIENT_SECRET,
      QUARTO_HUB_MCP_ISSUER: idp.issuer,
      QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH: '1',
    };
  }, 60000);

  afterAll(async () => {
    hub?.kill();
    await idp?.stop();
    await new AsyncEntry(KEYRING_SERVICE, keyringAccount).deletePassword().catch(() => {});
    if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it('authenticates via loopback, works over Bearer, shares the keyring across channels, clears', async () => {
    // ── channel A: the dist server ────────────────────────────────
    const a = new McpTestClient();
    try {
      await a.start(['--server', hubUrl], { env: serverEnv });

      // 1. Before auth: tool calls fail with a sign-in hint.
      const before = await a.callTool('create_project', { files: [] });
      expect(before.content[0]!.text).toMatch(/authenticate|sign.?in|auth/i);

      // 2. authenticate — we are the browser.
      const authPromise = a.callTool('authenticate', {});
      const urlLine = await a.waitForStderr(/open this URL to sign in: /, 15000);
      const authUrl = urlLine.slice(urlLine.indexOf('http'));
      const browserResp = await fetch(authUrl); // follows the 302 to the loopback
      expect(browserResp.ok).toBe(true);
      const authResult = await authPromise;
      expect(authResult.content[0]!.text).toContain(EMAIL);
      expect(idp.counters.codeExchanges).toBe(1);

      // 3. Authenticated writes + reads over Bearer ws.
      const created = await a.callTool('create_project', {
        files: [{ path: 'auth-e2e.qmd', content: '---\ntitle: Auth e2e\n---\n\nBearer ws ✓\n' }],
      });
      const createdText = created.content[0]!.text;
      expect(createdText, createdText).toContain('indexDocId');
      projectId = (JSON.parse(createdText) as { indexDocId: string }).indexDocId;

      const read = await a.callTool('read_file', { project: projectId, path: 'auth-e2e.qmd' });
      expect(read.content[0]!.text).toContain('Bearer ws ✓');

      // 4. The 45s TTL forces refresh grants on use.
      expect(idp.counters.refreshGrants).toBeGreaterThanOrEqual(1);

      // Keyring now holds the credential.
      const stored = await new AsyncEntry(KEYRING_SERVICE, keyringAccount).getPassword();
      expect(stored).toBeTruthy();
      expect(a.stdoutPollution).toEqual([]);
    } finally {
      await a.stop();
    }

    // ── channel B: a separate process reuses the credential ───────
    // Prefer the real `q2 mcp` launcher; fall back to the dist build
    // (still a fresh process == still proves keyring reuse) when the
    // embed is stale, so the credential-sharing assertion always runs.
    const fresh = q2EmbedFresh();
    if (!fresh.ok) {
      // eslint-disable-next-line no-console
      console.error(`[e2e-auth] q2-launcher channel unavailable (${fresh.reason}); using dist build for channel B`);
    }
    const exchangesBeforeB = idp.counters.codeExchanges;
    const b = new McpTestClient();
    try {
      await b.start(['--server', hubUrl], {
        env: serverEnv,
        ...(fresh.ok ? { command: { program: q2Bin, args: ['mcp'] } } : {}),
      });

      // No authenticate call: the keyring credential must carry it.
      const connected = await b.callTool('connect_project', { project: projectId });
      expect(connected.content[0]!.text).toContain('auth-e2e.qmd');
      const read = await b.callTool('read_file', { project: projectId, path: 'auth-e2e.qmd' });
      expect(read.content[0]!.text).toContain('Bearer ws ✓');
      expect(idp.counters.codeExchanges).toBe(exchangesBeforeB); // no new sign-in

      // 6. Clear: revokes at the IdP, empties the keyring.
      const cleared = await b.callTool('authenticate_clear', {});
      expect(cleared.content[0]!.text).toMatch(/clear|revok/i);
      expect(idp.counters.revokedTokens.length).toBeGreaterThanOrEqual(1);
      const after = await new AsyncEntry(KEYRING_SERVICE, keyringAccount).getPassword();
      expect(after).toBeNull();
    } finally {
      await b.stop();
    }
  }, 90000);

  /** Play the browser's role in the loopback+PKCE flow for `client`. */
  async function authenticateViaLoopback(client: McpTestClient): Promise<void> {
    const authPromise = client.callTool('authenticate', {});
    const urlLine = await client.waitForStderr(/open this URL to sign in: /, 15000);
    const authUrl = urlLine.slice(urlLine.indexOf('http'));
    const browserResp = await fetch(authUrl);
    expect(browserResp.ok).toBe(true);
    const result = await authPromise;
    expect(result.content[0]!.text).toContain(EMAIL);
  }

  // ── mid-session auth rejection (bd-l3b1brn8) ─────────────────────

  it('reports ReauthRequired promptly when the grant is revoked mid-session', async () => {
    // The main test's authenticate_clear left the (never-rotated)
    // refresh token in the IdP's revoked list — reset so this test's
    // own session works before its revocation moment.
    idp.counters.revokedTokens.length = 0;

    const c = new McpTestClient();
    try {
      await c.start(['--server', hubUrl], { env: serverEnv });
      await authenticateViaLoopback(c);

      // The session works.
      const connected = await c.callTool('connect_project', { project: projectId });
      expect(connected.content[0]!.text).toContain('auth-e2e.qmd');

      // The user revokes the grant at the IdP (the real-world analog is
      // myaccount.google.com → Remove Access): every further refresh
      // grant answers invalid_grant. The 45s TTL sits inside the 60s
      // early-refresh window, so the next auth-touching tool call MUST
      // refresh — and must fail terminally, not spin.
      idp.counters.revokedTokens.push(TEST_REFRESH_TOKEN);

      const started = Date.now();
      const result = await c.callTool('create_project', {
        files: [{ path: 'after-revocation.qmd', content: 'x\n' }],
      });
      const elapsed = Date.now() - started;
      const resultText = result.content[0]!.text;
      expect(resultText).toMatch(/expired or were revoked/i);
      expect(resultText).toMatch(/authenticate/i);
      // Fail-fast is the point: nowhere near the 15 s peer timeout.
      expect(elapsed).toBeLessThan(10_000);

      // The dead grant was wiped so a fresh `authenticate` starts clean.
      expect(
        await new AsyncEntry(KEYRING_SERVICE, keyringAccount).getPassword(),
      ).toBeNull();
    } finally {
      idp.counters.revokedTokens.length = 0;
      await c.stop();
    }
  }, 90000);

  it('surfaces the terminal denial message when the sub is banned mid-session (F1 bd-jkih1ql7 + F2)', async () => {
    idp.counters.revokedTokens.length = 0;

    const c = new McpTestClient();
    try {
      await c.start(['--server', hubUrl], { env: serverEnv });
      await authenticateViaLoopback(c);
      const connected = await c.callTool('connect_project', { project: projectId });
      expect(connected.content[0]!.text).toContain('auth-e2e.qmd');

      // Operator bans the sub — the documented stopped-hub procedure.
      const exited = new Promise((r) => hub.once('exit', r));
      hub.kill();
      await exited;
      fs.writeFileSync(
        path.join(tmpDir, 'hub-data', 'revocations.json'),
        JSON.stringify({ version: 1, not_before: {}, banned: ['test-subject-1'] }),
      );
      hub = spawnHub();
      await waitForHealth(`http://127.0.0.1:${hubPort}/health`);

      // The dropped socket reconnects (≤5 s retry); the upgrade gets the
      // hub's 403 (F1's Bearer-path ledger enforcement); the adapter
      // reports it as evidence; the manager enters the terminal denial
      // state — announced on stderr.
      await c.waitForStderr(/not allowed on this Quarto Hub/i, 30000);

      // The next tool call fails fast with the same clear message…
      const started = Date.now();
      const result = await c.callTool('read_file', {
        project: projectId,
        path: 'auth-e2e.qmd',
      });
      expect(result.content[0]!.text).toMatch(/banned|allowlist/i);
      expect(Date.now() - started).toBeLessThan(10_000);

      // …and the keyring is kept: identity denial, not credential
      // failure — re-auth with the same account cannot help.
      expect(
        await new AsyncEntry(KEYRING_SERVICE, keyringAccount).getPassword(),
      ).toBeTruthy();

      await c.callTool('authenticate_clear', {}).catch(() => {});
    } finally {
      idp.counters.revokedTokens.length = 0;
      await c.stop();
    }
  }, 120000);
});
