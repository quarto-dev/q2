/**
 * Test helper: a minimal in-process Quarto Hub stand-in.
 *
 * Provides exactly the two surfaces the MCP server's connection
 * manager needs from an unauthenticated hub:
 *
 *   - `GET /health` → 200 (the auth-mode probe), and
 *   - a real automerge-repo sync peer on the `/ws` websocket path.
 *
 * Real network sockets on 127.0.0.1, in-memory document storage. This
 * is what lets stdio-hygiene tests exercise the *live-sync* code paths
 * (peer-wait logging, reconnect timers) deterministically — an
 * unreachable URL fails fast at the probe and never reaches them.
 */

import * as http from 'node:http';
import { once } from 'node:events';
import { WebSocketServer } from 'ws';
import { Repo, type DocumentId, type PeerId } from '@automerge/automerge-repo';
import { WebSocketServerAdapter } from '@automerge/automerge-repo-network-websocket';
import { MemoryStorageAdapter } from '@quarto/quarto-sync-client';

export interface TestHub {
  /** ws:// URL of the sync endpoint, e.g. `ws://127.0.0.1:NNNNN/ws`. */
  url: string;
  /**
   * The hub's own repo — server-side ground truth. Tests use it to
   * assert what the server actually received, and to mint dangling
   * index entries by mutating the index document directly
   * (bd-vm5e5u10; mirrors quarto-sync-client/src/test-hub.ts).
   */
  repo: Repo;
  /**
   * True iff the hub holds the document (bounded wait; "unavailable"
   * or timeout map to false). Server-side ground truth for exit-drain
   * tests (bd-10deu8h4): the 2026-06-12 incident was exactly "the
   * client believed, the hub never had it".
   */
  hubHasDoc(docId: string, timeoutMs?: number): Promise<boolean>;
  stop(): Promise<void>;
}

export interface TestHubOptions {
  /**
   * When false, `/health` still answers but every websocket upgrade is
   * destroyed — models a hub whose sync endpoint is unreachable, for
   * requireOnline tests (bd-xnmd5ni1).
   */
  acceptWs?: boolean;
}

export async function startTestHub(opts: TestHubOptions = {}): Promise<TestHub> {
  const acceptWs = opts.acceptWs ?? true;
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
    if (acceptWs && req.url === '/ws') {
      wss.handleUpgrade(req, socket, head, (ws) => wss.emit('connection', ws, req));
    } else {
      socket.destroy();
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

  return {
    url: `ws://127.0.0.1:${address.port}/ws`,
    repo,
    async hubHasDoc(docId: string, timeoutMs = 5000): Promise<boolean> {
      const deadline = Date.now() + timeoutMs;
      while (Date.now() < deadline) {
        try {
          const handle = await repo.find(docId as DocumentId);
          if (handle.doc() !== undefined) return true;
        } catch {
          // unavailable — keep polling until the deadline
        }
        await new Promise((r) => setTimeout(r, 100));
      }
      return false;
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
