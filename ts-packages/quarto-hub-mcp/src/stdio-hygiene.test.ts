/**
 * Stdio-protocol hygiene for the MCP server (bd-sl4o01y0, bd-9jq2a060).
 *
 * A stdio MCP server has two hard contracts with its host:
 *
 *  1. stdout carries exclusively JSON-RPC frames — any stray
 *     `console.log` (ours or a dependency's) corrupts the protocol.
 *  2. The server exits when the host closes its stdin — that is how
 *     MCP hosts (Claude Desktop, Claude Code, Cursor) terminate
 *     servers; a process that lingers leaks once per session.
 *
 * Both tests drive the real server binary against an in-process sync
 * peer (see test-hub.ts): `create_project` only reaches the sync
 * machinery (peer-wait logging, live websockets, reconnect timers)
 * when a peer actually answers — an unreachable URL fails fast at the
 * health probe and exercises neither bug.
 */

import { describe, it, expect, beforeAll, afterAll, afterEach } from 'vitest';
import { McpTestClient } from './mcp-test-client.js';
import { startTestHub, type TestHub } from './test-hub.js';

describe('stdio hygiene', () => {
  let hub: TestHub;
  let client: McpTestClient;

  beforeAll(async () => {
    hub = await startTestHub();
  });

  afterAll(async () => {
    await hub.stop();
  });

  afterEach(async () => {
    await client.stop();
  });

  it('emits nothing but JSON-RPC on stdout while sync-client is active', async () => {
    client = new McpTestClient();
    await client.start(['--server', hub.url]);

    // Triggers sync-client's peer-wait / project-creation code paths,
    // which historically wrote progress lines to stdout.
    const result = await client.callTool('create_project', {
      files: [{ path: 'hygiene.qmd', content: 'stdout purity probe\n' }],
    });
    expect(result.content[0]!.text).toBeTruthy();

    expect(client.stdoutPollution).toEqual([]);
  }, 30000);

  it('exits on stdin EOF while sync connections are live', async () => {
    client = new McpTestClient();
    await client.start(['--server', hub.url]);

    // Ensure the event loop has more than the stdio transport keeping
    // it alive: live websocket reconnect timers from the sync client.
    await client.callTool('create_project', {
      files: [{ path: 'linger.qmd', content: 'stdin EOF probe\n' }],
    });

    expect(await client.endStdinAndWaitForExit(5000)).toBe(true);
  }, 30000);
});
