/**
 * Test helper: a minimal in-process sync hub (JS automerge-repo).
 *
 * Sibling of ts-packages/quarto-hub-mcp/src/test-hub.ts (the MCP
 * package keeps its own copy for its e2e suites); this one adds the
 * `holdUpgrades` switch needed by the offline-fallback race tests
 * (bd-10bdjmjb): websocket upgrades queue until `releaseUpgrades()`,
 * deterministically reproducing "client started before it could
 * connect" — the window in which hub-client's 1 ms peer wait strands
 * every session.
 *
 * The hub-side repo is exposed so tests can assert what the server
 * actually received (`hubHasDoc`), which is the ground truth the
 * 2026-06-12 incident was about.
 */

import * as http from 'node:http';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { decodeChange, getAllChanges } from '@automerge/automerge';
import { decodeHeads, Repo, type DocumentId, type PeerId, type UrlHeads } from '@automerge/automerge-repo';
import { WebSocketServerAdapter } from '@automerge/automerge-repo-network-websocket';

import type { SyncClient } from './client.js';
import { MemoryStorageAdapter } from './storage-adapter.js';

export interface TestHub {
  /** ws:// URL of the sync endpoint. */
  url: string;
  /** The hub's own repo — server-side ground truth. */
  repo: Repo;
  /** Allow queued + future websocket upgrades to proceed. */
  releaseUpgrades(): void;
  /**
   * True iff the hub holds the document (bounded wait). Uses the
   * repo's find with an overall deadline; "unavailable" or timeout
   * map to false.
   *
   * Presence is not delivery: a client pushes its changes through
   * automerge-repo's 100 ms sync throttle, so the hub can hold a doc
   * that is still a change or two behind the client. A test that is
   * about to disconnect the client, or to read what it wrote from
   * another client, must wait with `hubHasHeads` / `hubHasHeadsOf`
   * instead (bd-c72wsugj: the doc-inventory test dropped its binary
   * file's index entry this way).
   */
  hubHasDoc(docId: string, timeoutMs?: number): Promise<boolean>;
  /**
   * True iff the hub holds the document with every change in `heads`
   * (bounded wait). `heads` are as `DocHandle.heads()` and
   * `SyncClient.getDocInventory()` report them (URL-encoded).
   */
  hubHasHeads(docId: string, heads: string[], timeoutMs?: number): Promise<boolean>;
  /**
   * True iff the hub holds every document `client` knows, at the heads
   * the client has right now (bounded wait). The discipline for a
   * creator about to disconnect: wait on the state the test asserts,
   * which is "the hub has what I wrote", not "the hub has heard of
   * my docs".
   */
  hubHasHeadsOf(client: SyncClient, timeoutMs?: number): Promise<boolean>;
  stop(): Promise<void>;
}

export interface TestHubOptions {
  /** Queue websocket upgrades until releaseUpgrades() is called. */
  holdUpgrades?: boolean;
}

export async function startTestHub(opts: TestHubOptions = {}): Promise<TestHub> {
  let holding = opts.holdUpgrades ?? false;
  const queued: Array<() => void> = [];

  const httpServer = http.createServer((req, res) => {
    if (req.url === '/health') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end('{"status":"ok"}');
    } else {
      res.writeHead(404);
      res.end();
    }
  });

  const wss = new WebSocketServer({ noServer: true });
  httpServer.on('upgrade', (req, socket, head) => {
    if (req.url !== '/ws') {
      socket.destroy();
      return;
    }
    const proceed = (): void => {
      // The socket may have died while queued.
      if (!socket.destroyed) {
        wss.handleUpgrade(req, socket, head, (ws) => wss.emit('connection', ws, req));
      }
    };
    if (holding) {
      queued.push(proceed);
    } else {
      proceed();
    }
  });

  const repo = new Repo({
    network: [new WebSocketServerAdapter(wss as never)],
    peerId: 'test-hub' as PeerId,
    sharePolicy: async () => true,
    // Storage gives the hub a storageId to announce in its handshake
    // metadata, like the real samod hub (which always announces one).
    // Clients key delivery confirmation off it (exit-drain,
    // bd-10deu8h4); a storage-less Repo announces none and would make
    // this hub unconfirmable in a way production never is.
    storage: new MemoryStorageAdapter(),
  });

  httpServer.listen(0, '127.0.0.1');
  await once(httpServer, 'listening');
  const address = httpServer.address();
  if (address === null || typeof address === 'string') {
    throw new Error('test hub failed to bind a TCP port');
  }

  /**
   * Poll `repo.find` until `ready(doc)` holds or the deadline passes.
   * "unavailable" (find rejects) keeps polling: the doc may still be
   * on its way from a peer.
   */
  async function pollDoc(
    docId: string,
    deadline: number,
    ready: (doc: unknown) => boolean,
  ): Promise<boolean> {
    while (Date.now() < deadline) {
      try {
        const handle = await repo.find(docId as DocumentId);
        const doc = handle.doc();
        if (doc !== undefined && ready(doc)) return true;
      } catch {
        // unavailable — keep polling until the deadline
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    return false;
  }

  const hubHasHeads = (docId: string, heads: string[], timeoutMs = 5000): Promise<boolean> => {
    // The handle API reports heads URL-encoded; change hashes are hex.
    const want = decodeHeads(heads as UrlHeads);
    return pollDoc(docId, Date.now() + timeoutMs, (doc) => {
      // Every change the hub holds, by hash; a head the hub has not
      // received is absent. (getHeads equality would also do for a hub
      // that only receives, but a hub-side edit must not mask delivery.)
      const have = new Set(
        getAllChanges(doc as Parameters<typeof getAllChanges>[0]).map((c) => decodeChange(c).hash),
      );
      return want.every((h) => have.has(h));
    });
  };

  return {
    url: `ws://127.0.0.1:${address.port}/ws`,
    repo,
    releaseUpgrades(): void {
      holding = false;
      for (const proceed of queued.splice(0)) proceed();
    },
    hubHasDoc(docId: string, timeoutMs = 5000): Promise<boolean> {
      return pollDoc(docId, Date.now() + timeoutMs, () => true);
    },
    hubHasHeads,
    async hubHasHeadsOf(client: SyncClient, timeoutMs = 8000): Promise<boolean> {
      const deadline = Date.now() + timeoutMs;
      // The client's heads as of now; a later local change is the
      // caller's business.
      for (const entry of client.getDocInventory()) {
        // A doc the client itself has not loaded cannot be delivered.
        if (entry.heads === null) return false;
        const ok = await hubHasHeads(entry.docId, entry.heads, Math.max(0, deadline - Date.now()));
        if (!ok) return false;
      }
      return true;
    },
    async stop(): Promise<void> {
      // shutdown() flushes storage, and flush() throws "DocHandle is
      // not ready" for docs that were announced but never delivered —
      // exactly the half-state the exit-drain tests (bd-10deu8h4)
      // leave behind. Teardown must not mask a test's own assertions.
      await repo.shutdown().catch(() => {});
      wss.close();
      httpServer.close();
      await once(httpServer, 'close');
    },
  };
}
