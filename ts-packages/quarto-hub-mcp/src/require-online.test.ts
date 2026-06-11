/**
 * bd-xnmd5ni1: the MCP server must never fabricate success in offline
 * mode. Its sync clients use in-memory storage, so an "offline"
 * project lives only in process memory and dies with the session —
 * `create_project` used to return an indexDocId for a project the hub
 * never received whenever the websocket lost the (1 ms!) peer-wait
 * race, which auth latency made deterministic.
 *
 * The hub here answers /health but destroys websocket upgrades:
 * exactly the reachable-HTTP / unreachable-sync split that used to
 * produce the silent offline fallback.
 */

import { describe, it, expect, beforeAll, afterAll, afterEach } from 'vitest';
import { McpTestClient } from './mcp-test-client.js';
import { startTestHub, type TestHub } from './test-hub.js';

describe('requireOnline through the MCP server', () => {
  let hub: TestHub;
  let client: McpTestClient;

  beforeAll(async () => {
    hub = await startTestHub({ acceptWs: false });
  });

  afterAll(async () => {
    await hub.stop();
  });

  afterEach(async () => {
    await client.stop();
  });

  it('create_project fails loudly when the sync endpoint is unreachable', async () => {
    client = new McpTestClient();
    await client.start(['--server', hub.url]);

    const result = await client.callTool('create_project', {
      files: [{ path: 'doomed.qmd', content: 'never syncs\n' }],
    });
    const text = result.content[0]!.text;
    expect(text).toMatch(/^Error/);
    expect(text).toMatch(/no peer connection|refusing to continue offline/i);
    // The old behavior fabricated an indexDocId; that must be gone.
    expect(text).not.toContain('indexDocId');
  }, 30000);

  it('connect_project fails loudly when the sync endpoint is unreachable', async () => {
    client = new McpTestClient();
    await client.start(['--server', hub.url]);

    const result = await client.callTool('connect_project', {
      project: 'badc0ffee0ddf00d',
    });
    const text = result.content[0]!.text;
    expect(text).toMatch(/^Error/);
    expect(text).toMatch(/no peer connection|refusing to continue offline/i);
  }, 30000);
});
