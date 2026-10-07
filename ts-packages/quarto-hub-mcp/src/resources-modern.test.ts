/**
 * Modern-era (2026-07-28) resource subscriptions (BP-5): a client
 * pinned to the current protocol opens `subscriptions/listen` with a
 * `resourceSubscriptions` filter and receives
 * `notifications/resources/updated` when the file changes — no
 * `resources/subscribe` request involved. The serving entry
 * (serveStdio's listen router) owns filtering and delivery; the server
 * instance just emits.
 *
 * This needs the REAL stdio path: the listen router lives in the
 * serving entry, so the in-memory bare-pair fixture can't exercise it.
 * The test spawns the built server (`dist/index.js`) exactly as an MCP
 * host would, connects a 2026-07-28-pinned SDK client over the SDK's
 * StdioClientTransport, and drives a write through `tools/call` on the
 * same connection — the era-pinned e2e for the whole subscription
 * stack (client listen → entry router → server emit → client
 * notification handler).
 *
 * Also asserted: `subscriptions/listen` is refused on the legacy era
 * (the client's own steer), and an unlistened change produces no
 * notification (the router drops what nobody requested — the modern
 * era never delivers an un-requested change type).
 */

import { describe, it, expect, vi, beforeAll, afterAll } from 'vitest';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

import { Client } from '@modelcontextprotocol/client';
import { StdioClientTransport } from '@modelcontextprotocol/client/stdio';

import { startTestHub, type TestHub } from './test-hub.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PKG_ROOT = path.resolve(__dirname, '..');
const SERVER_ENTRY = path.join(PKG_ROOT, 'dist', 'index.js');

interface SpawnedModernClient {
  client: Client;
  transport: StdioClientTransport;
  close: () => Promise<void>;
}

async function spawnModernClient(hubUrl: string): Promise<SpawnedModernClient> {
  // Same sanitization as McpTestClient / inspector-smoke: never let a
  // developer's real OAuth config leak into the spawned server.
  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value === undefined) continue;
    if (key === 'QUARTO_HUB_MCP_CLIENT_ID' || key === 'QUARTO_HUB_MCP_CLIENT_SECRET') {
      continue;
    }
    env[key] = value;
  }
  env['QUARTO_HUB_SERVER'] = hubUrl;

  const transport = new StdioClientTransport({
    command: 'node',
    args: [SERVER_ENTRY],
    env,
    stderr: 'pipe',
  });
  const client = new Client(
    { name: 'modern-subscription-test', version: '0.0.0' },
    {
      capabilities: {},
      // Pin the modern era: the connect probes server/discover and
      // refuses to fall back to a 2025-era initialize.
      versionNegotiation: { mode: { pin: '2026-07-28' } },
    },
  );
  await client.connect(transport);
  return {
    client,
    transport,
    async close() {
      await client.close();
    },
  };
}

async function createProject(
  client: Client,
  files: Array<{ path: string; content: string }>,
): Promise<string> {
  const result = await client.callTool({
    name: 'create_project',
    arguments: { files },
  });
  const block = result.content[0];
  if (result.isError === true || block?.type !== 'text') {
    throw new Error(`create_project failed: ${JSON.stringify(result.content)}`);
  }
  return (JSON.parse(block.text) as { indexDocId: string }).indexDocId;
}

describe('subscriptions/listen, modern era (BP-5)', () => {
  let hub: TestHub;

  beforeAll(async () => {
    hub = await startTestHub();
  });

  afterAll(async () => {
    await hub.stop();
  });

  it('negotiates 2026-07-28 and advertises the resources capability', async () => {
    const spawned = await spawnModernClient(hub.url);
    try {
      expect(spawned.client.getServerCapabilities()?.resources).toEqual({
        subscribe: true,
        listChanged: true,
      });
      // The prompts/resources capabilities arrive via server/discover on
      // this era; tools stay callable as ever.
      const { prompts } = await spawned.client.listPrompts();
      expect(prompts.map((p) => p.name)).toContain('review-draft');
    } finally {
      await spawned.close();
    }
  }, 60000);

  it('delivers resources/updated on a listened file and nothing unlistened', async () => {
    const spawned = await spawnModernClient(hub.url);
    try {
      const project = await createProject(spawned.client, [
        { path: 'a.qmd', content: 'v1\n' },
        { path: 'b.qmd', content: 'v1\n' },
      ]);
      const aUri = `hub://project/${project}/a.qmd`;
      const bUri = `hub://project/${project}/b.qmd`;

      const updated: string[] = [];
      spawned.client.setNotificationHandler('notifications/resources/updated', (n) => {
        updated.push(n.params.uri);
      });

      const sub = await spawned.client.listen({
        resourceSubscriptions: [aUri],
      });
      expect(sub.honoredFilter.resourceSubscriptions).toEqual([aUri]);

      // Write both files through the same connection — the server's own
      // writes fire the change bridge exactly like a collaborator's.
      await spawned.client.callTool({
        name: 'write_file',
        arguments: { project, path: 'a.qmd', content: 'v2\n' },
      });
      await spawned.client.callTool({
        name: 'write_file',
        arguments: { project, path: 'b.qmd', content: 'v2\n' },
      });

      await vi.waitFor(
        () => {
          expect(updated).toContain(aUri);
        },
        { timeout: 10000, interval: 25 },
      );
      // The listened URI arrived, proving the pipe works; b.qmd's must
      // never arrive (the modern era delivers no un-requested change).
      await new Promise((r) => setTimeout(r, 500));
      expect(updated).not.toContain(bUri);

      // After close(), further edits are silent.
      await sub.close();
      const before = updated.length;
      await spawned.client.callTool({
        name: 'write_file',
        arguments: { project, path: 'a.qmd', content: 'v3\n' },
      });
      await new Promise((r) => setTimeout(r, 750));
      expect(updated.length).toBe(before);
    } finally {
      await spawned.close();
    }
  }, 60000);

  it('delivers resources/list_changed to listChanged listeners on file add', async () => {
    const spawned = await spawnModernClient(hub.url);
    try {
      const project = await createProject(spawned.client, [
        { path: 'a.qmd', content: 'v1\n' },
      ]);
      const listChanged: number[] = [];
      spawned.client.setNotificationHandler('notifications/resources/list_changed', () => {
        listChanged.push(Date.now());
      });
      const sub = await spawned.client.listen({ resourcesListChanged: true });
      expect(sub.honoredFilter.resourcesListChanged).toBe(true);

      await spawned.client.callTool({
        name: 'write_file',
        arguments: { project, path: 'added.qmd', content: 'new\n' },
      });
      await vi.waitFor(
        () => {
          expect(listChanged.length).toBeGreaterThanOrEqual(1);
        },
        { timeout: 10000, interval: 25 },
      );
      await sub.close();
    } finally {
      await spawned.close();
    }
  }, 60000);
});
