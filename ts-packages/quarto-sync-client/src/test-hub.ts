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
// cbor-x is automerge-repo's own wire codec (a direct dependency of
// @automerge/automerge-repo); used here to decode frames for the
// dropMessagesFor() lost-request switch.
import { decode as cborDecode } from 'cbor-x';
import { Repo, type DocumentId, type PeerId } from '@automerge/automerge-repo';
import { WebSocketServerAdapter } from '@automerge/automerge-repo-network-websocket';

import { MemoryStorageAdapter } from './storage-adapter.js';

export interface TestHub {
  /** ws:// URL of the sync endpoint. */
  url: string;
  /** The hub's own repo — server-side ground truth. */
  repo: Repo;
  /** Allow queued + future websocket upgrades to proceed. */
  releaseUpgrades(): void;
  /**
   * Silently drop incoming client messages that concern `docId`
   * (request/sync), before the hub's repo ever sees them. Models the
   * production "lost request" failure: the doc exists on the hub, but
   * the client's request never arrives, so nothing is ever delivered
   * unless the client RE-requests. Call {@link stopDroppingMessages}
   * to restore normal delivery.
   */
  dropMessagesFor(docId: string): void;
  /** Stop dropping; subsequent client messages flow normally. */
  stopDroppingMessages(): void;
  /**
   * True iff the hub holds the document (bounded wait). Uses the
   * repo's find with an overall deadline; "unavailable" or timeout
   * map to false.
   */
  hubHasDoc(docId: string, timeoutMs?: number): Promise<boolean>;
  stop(): Promise<void>;
}

export interface TestHubOptions {
  /** Queue websocket upgrades until releaseUpgrades() is called. */
  holdUpgrades?: boolean;
  /**
   * Whether the hub proactively announces its documents to connected
   * peers (automerge-repo generous share). Default true, matching the
   * JS reference hub. Set false to model the production samod hub's
   * request-response shape: a document is only delivered when a client
   * explicitly requests it — required by the stranded-doc recovery
   * tests, where a generous announce would mask a client that never
   * re-requests.
   */
  announce?: boolean;
}

export async function startTestHub(opts: TestHubOptions = {}): Promise<TestHub> {
  let holding = opts.holdUpgrades ?? false;
  const queued: Array<() => void> = [];
  let droppingDocId: string | null = null;

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
        wss.handleUpgrade(req, socket, head, (ws) => {
          // Intercept incoming frames BEFORE the WebSocketServerAdapter's
          // 'message' listener sees them, so dropMessagesFor() can model a
          // lost request (see the TestHub interface doc). The adapter
          // subscribes via ws.on('message', ...), which is driven by
          // ws.emit — wrapping emit filters every listener uniformly.
          const origEmit = ws.emit.bind(ws);
          (ws as { emit: typeof ws.emit }).emit = ((event: string | symbol, ...args: unknown[]) => {
            if (event === 'message' && droppingDocId !== null) {
              try {
                const raw = args[0] as Buffer;
                const msg = cborDecode(new Uint8Array(raw)) as {
                  type?: string;
                  documentId?: string;
                };
                if (msg?.documentId === droppingDocId) {
                  return true; // swallowed — the hub never learns of it
                }
              } catch {
                // Not a decodable repo message — let it through.
              }
            }
            return origEmit(event, ...(args as [unknown, unknown]));
          }) as typeof ws.emit;
          wss.emit('connection', ws, req);
        });
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
    sharePolicy: async () => opts.announce ?? true,
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
    releaseUpgrades(): void {
      holding = false;
      for (const proceed of queued.splice(0)) proceed();
    },
    dropMessagesFor(docId: string): void {
      droppingDocId = docId;
    },
    stopDroppingMessages(): void {
      droppingDocId = null;
    },
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
