/**
 * Official-client smoke test (Phase 0, bd-f1dr7gs1): drive the real
 * server binary with the official `@modelcontextprotocol/inspector` CLI
 * over stdio — the same client a human debugging this server would
 * reach for — and check the `tools/list` it gets back.
 *
 * Complements the in-memory conformance harness (conformance.test.ts):
 * that one asserts the wire contract in-process, this one proves the
 * spawned-stdio path (what MCP hosts actually run) works end to end
 * against an official client. The heavier official conformance suite
 * (`@modelcontextprotocol/conformance`, needs an HTTP transport) is
 * deferred to Phase 6 by the plan.
 *
 * Two inspector-CLI specifics worth knowing:
 *   - The server URL is passed via `-e QUARTO_HUB_SERVER=…` (inspector
 *     env forwarding) rather than the server's own `--server` flag,
 *     because the CLI parses `--server` as *its* server-catalog option
 *     instead of forwarding it to the spawned command.
 *   - `mcp-inspector` is spawned through `node` against the .bin shim so
 *     the test doesn't depend on the package's internal layout.
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { spawn } from 'node:child_process';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ToolSchema } from '@modelcontextprotocol/core';

import { startTestHub, type TestHub } from './test-hub.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = path.resolve(__dirname, '..');
const REPO_ROOT = path.resolve(PKG_ROOT, '../..');
const INSPECTOR_BIN = path.join(REPO_ROOT, 'node_modules', '.bin', 'mcp-inspector');
const SERVER_ENTRY = path.join(PKG_ROOT, 'dist', 'index.js');

const EXPECTED_RW_TOOLS = [
  'connect_project',
  'create_file',
  'create_project',
  'delete_file',
  'list_files',
  'patch_file',
  'read_file',
  'rename_file',
  'wait_for_change',
  'write_file',
];

describe('official inspector CLI smoke (stdio)', () => {
  let hub: TestHub;

  beforeAll(async () => {
    hub = await startTestHub();
  });

  afterAll(async () => {
    await hub.stop();
  });

  it('tools/list over stdio returns our surface, ToolSchema-valid', async () => {
    // Same sanitization as McpTestClient: never let a developer's real
    // OAuth config leak into the spawned server.
    const env = { ...process.env };
    delete env['QUARTO_HUB_MCP_CLIENT_ID'];
    delete env['QUARTO_HUB_MCP_CLIENT_SECRET'];

    const result = await new Promise<{ code: number | null; stdout: string; stderr: string }>(
      (resolve, reject) => {
        const child = spawn(
          'node',
          [
            INSPECTOR_BIN,
            '--cli',
            'node',
            SERVER_ENTRY,
            '-e',
            `QUARTO_HUB_SERVER=${hub.url}`,
            '--method',
            'tools/list',
          ],
          { env, stdio: ['ignore', 'pipe', 'pipe'] },
        );
        let stdout = '';
        let stderr = '';
        child.stdout.setEncoding('utf-8').on('data', (d: string) => (stdout += d));
        child.stderr.setEncoding('utf-8').on('data', (d: string) => (stderr += d));
        child.on('error', reject);
        child.on('exit', (code) => resolve({ code, stdout, stderr }));
        setTimeout(() => {
          child.kill('SIGKILL');
          reject(new Error(`inspector CLI timed out; stderr so far:\n${stderr}`));
        }, 55000);
      },
    );

    expect(result.code, `inspector stderr:\n${result.stderr}`).toBe(0);
    const parsed = JSON.parse(result.stdout) as { tools: Array<Record<string, unknown>> };
    expect(parsed.tools.map((t) => t.name).sort()).toEqual(EXPECTED_RW_TOOLS);
    for (const tool of parsed.tools) {
      const check = ToolSchema.safeParse(tool);
      expect(
        check.success,
        `tool ${String(tool.name)} fails ToolSchema: ${check.success ? '' : JSON.stringify(check.error.issues)}`,
      ).toBe(true);
    }
  }, 60000);
});
