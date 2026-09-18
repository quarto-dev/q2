/**
 * Smoke test for the self-contained bundle (`npm run bundle`).
 *
 * The bundle is what `q2 mcp` embeds and what the npx channel ships
 * (bd-81cfshmw / bd-3tak0lyy), so it must work with NOTHING from this
 * repo on disk: the test builds it, copies it to a temp dir outside
 * the repo tree (no node_modules anywhere up the path), and drives it
 * with plain `node` — keyring addon resolution via the bundle's mini
 * node_modules, base64-inlined automerge wasm, full MCP round-trip
 * against an in-process sync peer, stdout purity, and stdin-EOF exit.
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { execFileSync, spawnSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { McpTestClient } from './mcp-test-client.js';
import { startTestHub, type TestHub } from './test-hub.js';

const pkgRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

let tmpDir: string;
let bundleEntry: string;
let hub: TestHub;

beforeAll(async () => {
  execFileSync('node', [path.join(pkgRoot, 'scripts/bundle.mjs')], {
    stdio: 'pipe',
  });
  // os.tmpdir() is outside the repo: node resolution from the copied
  // bundle cannot accidentally reach the workspace node_modules.
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'hub-mcp-bundle-test-'));
  fs.cpSync(path.join(pkgRoot, 'dist-bundle'), path.join(tmpDir, 'bundle'), {
    recursive: true,
  });
  bundleEntry = path.join(tmpDir, 'bundle', 'index.mjs');
  hub = await startTestHub();
}, 60000);

afterAll(async () => {
  await hub?.stop();
  if (tmpDir) fs.rmSync(tmpDir, { recursive: true, force: true });
});

describe('bundle smoke', () => {
  it('ships the expected artifacts', () => {
    const bundleDir = path.join(tmpDir, 'bundle');
    expect(fs.existsSync(path.join(bundleDir, 'index.mjs'))).toBe(true);
    // get_errors local validation: the WASM host + binary must ride in
    // the bundle (index.mjs dynamic-imports ./wasm-host.mjs at first use).
    expect(fs.existsSync(path.join(bundleDir, 'wasm-host.mjs'))).toBe(true);
    const wasm = path.join(bundleDir, 'wasm_quarto_hub_client_bg.wasm');
    expect(fs.existsSync(wasm)).toBe(true);
    expect(fs.statSync(wasm).size).toBeGreaterThan(10_000_000);
    const info = JSON.parse(
      fs.readFileSync(path.join(bundleDir, 'build-info.json'), 'utf8'),
    ) as { gitCommit: string; nodeTarget: string; keyringPackages: string[] };
    expect(info.nodeTarget).toBe('node24');
    expect(info.keyringPackages).toContain('keyring');
    expect(info.keyringPackages.length).toBeGreaterThanOrEqual(2);
    // the keyring loader package plus at least one platform addon
    const napiDir = path.join(bundleDir, 'node_modules', '@napi-rs');
    expect(fs.readdirSync(napiDir)).toContain('keyring');
  });

  it('prints --help (stderr, stdout stays pure) and exits 0 from outside the repo', () => {
    const res = spawnSync('node', [bundleEntry, '--help'], {
      cwd: tmpDir,
      encoding: 'utf8',
    });
    expect(res.status).toBe(0);
    // package convention: even pre-protocol output avoids stdout
    expect(res.stdout).toBe('');
    expect(res.stderr).toContain('--server');
  });

  it('full MCP round-trip from outside the repo: create, patch, read', async () => {
    const client = new McpTestClient();
    try {
      await client.start(['--server', hub.url], { entry: bundleEntry });

      const tools = (await client.listTools()).map((t) => t.name);
      expect(tools).toContain('connect_project');
      expect(tools).toContain('patch_file');

      const created = await client.callTool('create_project', {
        files: [{ path: 'smoke.qmd', content: 'bundle smoke ÄöÜ → 🧪\n' }],
      });
      const { indexDocId } = JSON.parse(created.content[0]!.text) as {
        indexDocId: string;
      };
      expect(indexDocId).toBeTruthy();

      await client.callTool('connect_project', { project: indexDocId });
      await client.callTool('patch_file', {
        project: indexDocId,
        path: 'smoke.qmd',
        old_string: 'bundle smoke',
        new_string: 'bundle smoke (patched)',
      });
      const readBack = await client.callTool('read_file', {
        project: indexDocId,
        path: 'smoke.qmd',
      });
      expect(readBack.content[0]!.text).toContain('bundle smoke (patched) ÄöÜ → 🧪');

      // stdout purity holds for the bundled artifact too
      expect(client.stdoutPollution).toEqual([]);
      // ...and so does stdin-EOF shutdown
      expect(await client.endStdinAndWaitForExit(5000)).toBe(true);
    } finally {
      await client.stop();
    }
  }, 30000);
});
