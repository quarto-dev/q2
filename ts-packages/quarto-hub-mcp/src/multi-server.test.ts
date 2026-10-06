/**
 * Multi-server projects (bd-qt7h8h5g, bd-zv8u2sxi).
 *
 * A share URL whose `server=` names a hub other than the configured one
 * used to be *rejected* — an agent handed such a link could not reach
 * the project at all. The connection manager is now keyed by
 * (server, indexDocId): a share URL's `server=` routes the call to
 * that hub. Because Bearer credentials are audience-bound, a foreign
 * hub is contacted **no-auth first and never with the configured hub's
 * token** — a foreign hub that demands auth yields a clear error, not
 * a credential leak.
 */

import { describe, it, expect, vi } from 'vitest';
import * as http from 'node:http';
import { once } from 'node:events';
import type { AddressInfo } from 'node:net';

import {
  ConnectionManager,
  type ConnectionManagerDeps,
} from './connection-manager.js';
import {
  CredentialStore,
  type CredentialBundle,
  type KeyringBackend,
} from './auth/credential-store.js';
import { RefreshManager } from './auth/refresh-manager.js';
import { createSyncClient, type SyncClient } from '@quarto/quarto-sync-client';
import { startTestHub, type TestHub } from './test-hub.js';
import {
  startInMemoryMcp,
  callTool,
} from './in-memory-fixture.js';

function shareUrl(indexDocId: string, server: string, file?: string): string {
  const q = new URLSearchParams({ server });
  if (file) q.set('file', file);
  return `https://quarto-hub.com/#/share/${indexDocId}?${q.toString()}`;
}

function textOf(result: { content: Array<{ type: string; text?: string }> }): string {
  const block = result.content[0];
  if (block?.type !== 'text' || typeof block.text !== 'string') {
    throw new Error(`expected a text content block, got ${JSON.stringify(block)}`);
  }
  return block.text;
}

// ---------------------------------------------------------------------------
// e2e: two in-process hubs, one MCP server
// ===========================================================================

describe('multi-server routing (bd-qt7h8h5g)', () => {
  it('a share URL naming a foreign hub connects to that hub (list/read/write)', async () => {
    const f = await startInMemoryMcp(); // configured hub A
    const hubB = await startTestHub();
    try {
      // A project that lives only on hub B (created through a B-bound manager).
      const managerB = new ConnectionManager({ serverUrl: hubB.url });
      const created = await managerB.createProject([
        { path: 'b-file.qmd', content: 'lives on hub B\n' },
      ]);
      await hubB.hubHasDoc(created.indexDocId, 8000);

      const share = shareUrl(created.indexDocId, hubB.url, 'b-file.qmd');

      const listed = await callTool(f, 'list_files', { project: share });
      expect(listed.isError).not.toBe(true);
      expect(textOf(listed)).toContain('b-file.qmd');

      // The share URL's file= supplies the default path across the hub boundary.
      const read = await callTool(f, 'read_file', { project: share });
      expect(read.isError).not.toBe(true);
      expect(textOf(read)).toContain('lives on hub B');

      // Writes route too (hub B is no-auth), with delivery confirmation.
      const written = await callTool(f, 'write_file', {
        project: share,
        content: 'written across the boundary\n',
      });
      expect(written.isError).not.toBe(true);
      const payload = JSON.parse(textOf(written)) as { synced?: boolean };
      expect(payload.synced).toBe(true);

      // Ground truth on hub B.
      const doc = await hubB.repo.find(created.files[0]!.docId as never);
      expect((doc.doc() as { text: string }).text).toBe('written across the boundary\n');

      await managerB.disconnectAll({ drainMs: 0 });
    } finally {
      await f.close();
      await hubB.stop();
    }
  }, 30000);

  it('connection state is keyed per (server, project): foreign connects cache separately', async () => {
    const hubA = await startTestHub();
    const hubB = await startTestHub();
    try {
      let factoryCalls = 0;
      const deps: ConnectionManagerDeps = {
        serverUrl: hubA.url,
        syncClientFactory: (cbs) => {
          factoryCalls += 1;
          return createSyncClient(cbs);
        },
      };
      const manager = new ConnectionManager(deps);

      const a = await manager.createProject([{ path: 'a.qmd', content: 'a\n' }]);
      const managerB = new ConnectionManager({ serverUrl: hubB.url });
      const b = await managerB.createProject([{ path: 'b.qmd', content: 'b\n' }]);
      expect(factoryCalls).toBe(1); // createProject on A

      const s1 = await manager.connect(a.indexDocId);
      const s2 = await manager.connect(b.indexDocId, { server: hubB.url });
      expect(s1).not.toBe(s2);
      expect(factoryCalls).toBe(2); // one client per (server, project)

      // Repeat connects hit the per-key cache — no third client.
      expect(await manager.connect(a.indexDocId)).toBe(s1);
      expect(await manager.connect(b.indexDocId, { server: hubB.url })).toBe(s2);
      expect(factoryCalls).toBe(2);

      // One manager tears down both servers' connections.
      await manager.disconnectAll({ drainMs: 0 });
      await managerB.disconnectAll({ drainMs: 0 });
    } finally {
      await hubA.stop();
      await hubB.stop();
    }
  }, 30000);

  it('a foreign hub that requires authentication errors clearly — and gets no Bearer', async () => {
    // A "hub" whose /health demands auth (the WS path is never reached).
    const seen: http.IncomingMessage[] = [];
    const server = http.createServer((req, res) => {
      seen.push(req);
      res.writeHead(401).end();
    });
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    const authHubUrl = `ws://127.0.0.1:${(server.address() as AddressInfo).port}/ws`;

    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'list_files', {
        project: shareUrl('some-doc-id', authHubUrl),
      });
      expect(result.isError).toBe(true);
      const text = textOf(result);
      expect(text).toContain('requires authentication');
      expect(text).not.toContain('Restart quarto-hub-mcp');
      // The probe that discovered the 401 carried no credentials.
      expect(seen.length).toBeGreaterThan(0);
      for (const req of seen) {
        expect(req.headers.authorization).toBeUndefined();
      }
    } finally {
      await f.close();
      server.close();
      await once(server, 'close');
    }
  }, 30000);

  it('never replays the configured hub’s Bearer to a foreign origin', async () => {
    const hubB = await startTestHub();
    try {
      // A manager holding (fake) credentials for the configured hub.
      const bundle: CredentialBundle = {
        idToken: 'id-token',
        refreshToken: 'refresh-token',
        idTokenExpiresAt: new Date(Date.now() + 3600_000),
        scopes: ['openid'],
      };
      const backend: KeyringBackend = {
        read: async () => JSON.stringify({ schema_version: 1, id_token: bundle.idToken, refresh_token: bundle.refreshToken, id_token_expires_at: bundle.idTokenExpiresAt.toISOString(), scopes: bundle.scopes }),
        write: async () => {},
        clear: async () => true,
      };
      const store = new CredentialStore(
        { issuer: 'https://accounts.google.com', clientId: 'test-client' },
        backend,
      );
      const refreshManager = Object.create(RefreshManager.prototype) as RefreshManager;
      Object.defineProperty(refreshManager, 'getValidIdToken', {
        value: vi.fn().mockResolvedValue('secret-token'),
      });

      const probes: string[] = [];
      const authOptionsSeen: unknown[] = [];
      const manager = new ConnectionManager({
        serverUrl: 'ws://127.0.0.1:1/ws', // configured hub — never contacted here
        credentialStore: store,
        refreshManager,
        fetch: ((input: unknown, init?: { headers?: Record<string, string> }) => {
          probes.push(`${input} auth=${init?.headers?.Authorization ?? 'none'}`);
          return Promise.resolve(new Response('{}', { status: 200 }));
        }) as unknown as typeof fetch,
        syncClientFactory: (cbs) => {
          const client = createSyncClient(cbs);
          const origConnect = client.connect.bind(client);
          client.connect = ((...args: unknown[]) => {
            authOptionsSeen.push((args[5] as { auth?: unknown })?.auth);
            return origConnect(...(args as Parameters<typeof origConnect>));
          }) as typeof client.connect;
          return client as SyncClient;
        },
      });

      const managerB = new ConnectionManager({ serverUrl: hubB.url });
      const b = await managerB.createProject([{ path: 'b.qmd', content: 'b\n' }]);
      await hubB.hubHasDoc(b.indexDocId, 8000);

      await manager.connect(b.indexDocId, { server: hubB.url });

      // The probe to hub B carried no Authorization header…
      expect(probes.length).toBeGreaterThan(0);
      for (const p of probes) expect(p).toContain('auth=none');
      // …and the sync client joined the foreign hub authorless.
      expect(authOptionsSeen).toEqual([undefined]);

      await manager.disconnectAll({ drainMs: 0 });
      await managerB.disconnectAll({ drainMs: 0 });
    } finally {
      await hubB.stop();
    }
  }, 30000);
});
