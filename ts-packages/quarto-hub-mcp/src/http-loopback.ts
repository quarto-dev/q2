/**
 * Test-only loopback HTTP listener for the official conformance suite
 * (Phase 6, bd-8iv9jty5). The suite (`@modelcontextprotocol/conformance`)
 * only speaks HTTP; our server is stdio-only until CAP-16 lands. This
 * listener bridges the two: a 127.0.0.1 `node:http` server that runs
 * every request through the SDK's `createMcpHandler` (per-request
 * serving, the 2026-07-28 stateless model, with the stateless legacy
 * fallback for 2025-era probes) into the same {@link createServer}
 * construction the stdio entrypoint uses — so the suite exercises our
 * real registration, validation, and serialization layers, never a
 * fixture-only side build.
 *
 * It is also the rehearsal for CAP-16: the same handler shape the hub
 * will eventually mount, minus auth (the suite's required server
 * scenarios need none) and with `conformanceFixtures` enabled — the
 * `test_*` surface the official scenarios are written against, which
 * production listings never expose.
 *
 * Security posture (this is what the `dns-rebinding-protection`
 * scenario asserts): Host and Origin headers are validated before any
 * request reaches the handler — loopback hostnames only, and Origin
 * (when present, i.e. browser-like clients) restricted to loopback
 * origins. Requests outside `/mcp` get a 404.
 */

import { createServer as createHttpServer, type Server as HttpServer } from 'node:http';

import {
  createMcpHandler,
  hostHeaderValidationResponse,
  localhostAllowedHostnames,
  localhostAllowedOrigins,
  originValidationResponse,
  type McpHttpHandler,
} from '@modelcontextprotocol/server';
import { toNodeHandler } from '@modelcontextprotocol/node';

import { ConnectionManager } from './connection-manager.js';
import { createServer } from './index.js';

export interface HttpLoopback {
  /** The MCP endpoint URL, e.g. `http://127.0.0.1:51234/mcp`. */
  readonly url: string;
  /** The connection manager shared by every per-request server instance. */
  readonly manager: ConnectionManager;
  /** Tear down the HTTP server, the handler, and the manager. */
  close(): Promise<void>;
}

export interface HttpLoopbackOptions {
  /** The sync server URL the manager connects through (the test-hub). */
  readonly serverUrl: string;
  readonly readOnly?: boolean;
  readonly allowRender?: boolean;
  /** Register the `test_*` conformance fixture surface (the whole point
   * of this listener); defaults to true. Exposed only to make the
   * default explicit at call sites. */
  readonly conformanceFixtures?: boolean;
}

export async function startHttpLoopback(opts: HttpLoopbackOptions): Promise<HttpLoopback> {
  const manager = new ConnectionManager({ serverUrl: opts.serverUrl });
  const handler: McpHttpHandler = createMcpHandler(
    (ctx) =>
      createServer({
        manager,
        readOnly: opts.readOnly ?? false,
        allowRender: opts.allowRender ?? false,
        era: ctx.era,
        conformanceFixtures: opts.conformanceFixtures ?? true,
      }),
    {
      // Silent on purpose: the suite deliberately sends malformed
      // requests (header-validation scenarios), and the SDK reports
      // every rejection through this hook — logging here would fill the
      // test transcript with expected probe traffic. Genuine handler
      // failures surface in the suite's own failure output.
      // 'auto' (the default responseMode) is load-bearing, not cosmetic:
      // the per-request transport answers a terminal error as a
      // status-mapped JSON response (MissingRequiredClientCapability →
      // HTTP 400, which the server-stateless scenario asserts) — an SSE
      // upgrade commits HTTP 200 before the error exists and the mapping
      // is lost. Only exchanges whose handler emits related messages
      // stream.
    },
  );

  const nodeHandler = toNodeHandler({
    fetch: async (request: Request): Promise<Response> => {
      const rejected =
        hostHeaderValidationResponse(request, localhostAllowedHostnames()) ??
        originValidationResponse(request, localhostAllowedOrigins());
      return rejected ?? handler.fetch(request);
    },
  });

  const http: HttpServer = createHttpServer((req, res) => {
    const pathname = new URL(req.url ?? '/', 'http://127.0.0.1').pathname;
    if (pathname !== '/mcp') {
      res.writeHead(404, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: 'not found — the MCP endpoint is /mcp' }));
      return;
    }
    void nodeHandler(req, res);
  });

  await new Promise<void>((resolve, reject) => {
    http.once('error', reject);
    http.listen(0, '127.0.0.1', () => resolve());
  });
  const address = http.address();
  if (address === null || typeof address === 'string') {
    throw new Error('loopback listener: no address after listen()');
  }

  return {
    url: `http://127.0.0.1:${address.port}/mcp`,
    manager,
    async close(): Promise<void> {
      await new Promise<void>((resolve) => http.close(() => resolve()));
      await handler.close();
      await manager.disconnectAll({ drainMs: 0 });
    },
  };
}
