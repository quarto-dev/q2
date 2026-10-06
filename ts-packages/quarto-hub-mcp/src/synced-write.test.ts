/**
 * Write delivery confirmation (ERG-2, bd-zv8u2sxi;
 * claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md).
 *
 * Writes used to be fire-and-forget: the tool returned the moment the
 * local automerge change was made, so an agent could say "done" while
 * the hub — and every collaborator's web client — still lacked the
 * bytes. Write tools now wait (bounded, ≈2 s) for the hub to
 * acknowledge the change and report `synced: true|false`;
 * `wait_for_sync: false` opts out. The write is never rolled back:
 * `synced: false` means "not yet confirmed", not "lost".
 */

import { describe, it, expect, vi } from 'vitest';
import * as net from 'node:net';
import { once } from 'node:events';

import { ConnectionManager } from './connection-manager.js';
import { createServer } from './index.js';
import { Client, InMemoryTransport } from '@modelcontextprotocol/client';
import { startTestHub, type TestHub } from './test-hub.js';
import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

/** Parse the JSON text payload of a tool result. */
function parseJson(result: { content: Array<{ type: string; text?: string }> }): Record<string, unknown> {
  const block = result.content[0];
  if (block?.type !== 'text' || typeof block.text !== 'string') {
    throw new Error(`expected a text content block, got ${JSON.stringify(block)}`);
  }
  return JSON.parse(block.text) as Record<string, unknown>;
}

// ============================================================================
// A TCP proxy that can pause/resume forwarding — a network partition
// between the MCP server and an otherwise-healthy hub.
// ============================================================================

interface PausingProxy {
  /** ws:// URL the MCP server should connect to. */
  readonly url: string;
  pause(): void;
  resume(): void;
  close(): Promise<void>;
}

async function startPausingProxy(targetWsUrl: string): Promise<PausingProxy> {
  const targetPort = Number(new URL(targetWsUrl).port);
  let paused = false;
  const pending: Array<() => void> = [];
  const sockets = new Set<net.Socket>();

  const server = net.createServer((clientSock) => {
    sockets.add(clientSock);
    const upstream = net.createConnection(targetPort, '127.0.0.1');
    sockets.add(upstream);
    const drop = (s: net.Socket) => sockets.delete(s);
    const pump = (from: net.Socket, to: net.Socket) => {
      from.on('data', (chunk) => {
        if (!paused) {
          to.write(chunk);
          return;
        }
        pending.push(() => to.write(chunk));
      });
      from.on('error', () => to.destroy());
      from.on('close', () => {
        drop(from);
        to.destroy();
      });
    };
    pump(clientSock, upstream);
    pump(upstream, clientSock);
    upstream.on('error', () => clientSock.destroy());
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (address === null || typeof address === 'string') {
    throw new Error('pausing proxy failed to bind a TCP port');
  }

  return {
    url: `ws://127.0.0.1:${address.port}/ws`,
    pause() {
      paused = true;
    },
    resume() {
      paused = false;
      for (const flush of pending.splice(0)) flush();
    },
    async close() {
      for (const s of sockets) s.destroy();
      server.close();
      await once(server, 'close');
    },
  };
}

// ============================================================================
// Happy path — the in-process hub acknowledges writes promptly
// ============================================================================

describe('write delivery confirmation (ERG-2)', () => {
  it('write_file carries synced: true once the hub has acknowledged the change', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const result = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2\n',
      });
      expect(result.isError).not.toBe(true);
      const payload = parseJson(result);
      expect(payload.hash).toMatch(/^sha256:/);
      expect(payload.synced).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('patch_file and delete_file carry synced: true as well (file doc and index-only writes)', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const patched = parseJson(
        await callTool(f, 'patch_file', {
          project: seed.indexDocId,
          path: 'a.qmd',
          old_string: 'v1',
          new_string: 'v2',
        }),
      );
      expect(patched.synced).toBe(true);

      const deleted = parseJson(
        await callTool(f, 'delete_file', { project: seed.indexDocId, path: 'a.qmd' }),
      );
      expect(deleted.synced).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('wait_for_sync: false opts out of the delivery wait', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'v1\n' }]);
      const result = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'a.qmd',
        content: 'v2\n',
        wait_for_sync: false,
      });
      expect(result.isError).not.toBe(true);
      const payload = parseJson(result);
      expect(payload.hash).toMatch(/^sha256:/);
      expect(payload.synced).toBeUndefined();
    } finally {
      await f.close();
    }
  });

  it(
    'with the hub stalled, write_file reports synced: false within the bound — and the write lands when the hub returns',
    async () => {
      // Own fixture path: a pausing TCP proxy partitions the server from
      // the hub without dropping the websocket.
      const hub = await startTestHub();
      const proxy = await startPausingProxy(hub.url);
      const manager = new ConnectionManager({ serverUrl: proxy.url });
      const server = createServer({ manager, readOnly: false });
      const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
      const client = new Client({ name: 'sync-test', version: '0.0.0' }, { capabilities: {} });
      await server.connect(serverTransport);
      await client.connect(clientTransport);
      const fixture: InMemoryMcpFixture = {
        hub,
        manager,
        server,
        client,
        async close() {
          await client.close();
          await server.close();
          await manager.disconnectAll({ drainMs: 0 });
          await proxy.close();
          await hub.stop();
        },
      };

      try {
        const seed = await seedProject(fixture, [{ path: 'a.qmd', content: 'v1\n' }]);
        const docId = seed.files[0]!.docId;

        proxy.pause();
        const stalled = parseJson(
          await callTool(fixture, 'write_file', {
            project: seed.indexDocId,
            path: 'a.qmd',
            content: 'v2 through the partition\n',
          }),
        );
        expect(stalled.synced).toBe(false);
        // The hub cannot have the new text yet — nothing flowed.
        const stalledDoc = await hub.repo.find(docId as never);
        expect((stalledDoc.doc() as { text: string }).text).toBe('v1\n');

        proxy.resume();
        // The write was never rolled back; delivery completes once the
        // partition heals. Ground truth is the hub's own repo.
        await vi.waitFor(
          async () => {
            const handle = await hub.repo.find(docId as never);
            expect((handle.doc() as { text: string }).text).toBe('v2 through the partition\n');
          },
          { timeout: 10000, interval: 100 },
        );
      } finally {
        await fixture.close();
      }
    },
    60000,
  );
});
