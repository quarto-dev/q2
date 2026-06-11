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
import { Repo, type PeerId } from '@automerge/automerge-repo';
import { WebSocketServerAdapter } from '@automerge/automerge-repo-network-websocket';

export interface TestHub {
  /** ws:// URL of the sync endpoint, e.g. `ws://127.0.0.1:NNNNN/ws`. */
  url: string;
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
  });

  httpServer.listen(0, '127.0.0.1');
  await once(httpServer, 'listening');
  const address = httpServer.address();
  if (address === null || typeof address === 'string') {
    throw new Error('test hub failed to bind a TCP port');
  }

  return {
    url: `ws://127.0.0.1:${address.port}/ws`,
    async stop(): Promise<void> {
      await repo.shutdown();
      wss.close();
      httpServer.close();
      await once(httpServer, 'close');
    },
  };
}
