/**
 * Protocol hygiene conformance (Phase 1, bd-zv8u2sxi;
 * claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md).
 *
 * The wire-contract specs for the SDK v2 migration (BP-16) and its free
 * fallout: runtime input validation (BP-2, SEP-1303), unknown-tool
 * classification (BP-15), and tools/list cache hints + determinism
 * (BP-17, SEP-2549).
 *
 * The BP-2/BP-15 specs drive the shared in-memory fixture, so they run
 * against whatever SDK the fixture is built on. The dual-era and cache
 * specs construct their own v2 clients (a fixture client negotiates one
 * era for its whole lifetime), each against a fresh `createServer`
 * instance sharing the fixture's connection manager and test-hub.
 */

import { describe, it, expect } from 'vitest';
import { Client } from '@modelcontextprotocol/client';
import type { JSONRPCMessage, Transport } from '@modelcontextprotocol/client';
import { serveStdio, type StdioServerHandle } from '@modelcontextprotocol/server/stdio';

import { createServer } from './index.js';
import { ConnectionManager } from './connection-manager.js';
import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

/**
 * A message-level linked transport pair, stdio-shaped so the v2 entry
 * points treat it like a pipe. The server end goes to `serveStdio` —
 * the production stdio entry, which owns era classification, instance
 * pinning, and the modern-only handler install — so these tests
 * exercise the same wiring `main()` runs, not a test-only shortcut.
 * (The plain SDK `InMemoryTransport` carries no era classification, so
 * the 2026-07-28 era can never be negotiated over it.)
 */
function stdioShapedPair(): { serverWire: Transport; clientWire: Transport } {
  const make = (): Transport & { other?: Transport } => ({
    other: undefined,
    onmessage: undefined,
    onclose: undefined,
    onerror: undefined,
    async start() {},
    async send(message: JSONRPCMessage) {
      const other = this.other;
      queueMicrotask(() => {
        (other as { onmessage?: (m: JSONRPCMessage) => void })?.onmessage?.(message);
      });
    },
    async close() {
      const other = this.other;
      this.other = undefined;
      if (other) {
        (other as { other?: Transport }).other = undefined;
        (other as { onclose?: () => void }).onclose?.();
      }
      (this as { onclose?: () => void }).onclose?.();
    },
  });
  const serverWire = make();
  const clientWire = make();
  serverWire.other = clientWire;
  clientWire.other = serverWire;
  return { serverWire, clientWire };
}

/** A server served the production way plus its era-negotiated client. */
interface ServedClient {
  client: Client;
  handle: StdioServerHandle;
  close(): Promise<void>;
}

/**
 * Serve `createServer` through `serveStdio` (the same entry `main()`
 * uses) and connect a v2 client negotiating the requested era: default
 * (`legacy`) runs the 2025 `initialize` handshake; `modern` pins
 * 2026-07-28 via `server/discover`.
 */
async function serveClient(
  manager: ConnectionManager,
  era: 'legacy' | 'modern',
): Promise<ServedClient> {
  const { serverWire, clientWire } = stdioShapedPair();
  const handle = serveStdio(() => createServer({ manager, readOnly: false }), {
    transport: serverWire,
  });
  const client = new Client(
    { name: 'protocol-hygiene-client', version: '0.0.0' },
    {
      capabilities: {},
      ...(era === 'modern'
        ? {
            versionNegotiation: {
              mode: { pin: '2026-07-28' },
              // A server that never answers the probe must fail fast in
              // tests, not ride the 60 s default.
              probe: { timeoutMs: 5000 },
            },
          }
        : {}),
    },
  );
  await client.connect(clientWire);
  return {
    client,
    handle,
    async close() {
      await client.close().catch(() => {});
      await handle.close().catch(() => {});
    },
  };
}

/** First text block of a tool result, or a hard failure. */
function textOf(result: { content: Array<{ type: string; text?: string }> }): string {
  const block = result.content[0];
  if (block?.type !== 'text' || typeof block.text !== 'string') {
    throw new Error(`expected a text content block, got ${JSON.stringify(block)}`);
  }
  return block.text;
}

// ============================================================================
// BP-2 / SEP-1303: runtime input validation
// ============================================================================

describe('input validation (BP-2, SEP-1303)', () => {
  it('a wrong-typed argument is a tool error naming the parameter and expected type', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 42,
      });
      expect(result.isError).toBe(true);
      const text = textOf(result);
      expect(text).toMatch(/\bpath\b/);
      expect(text).toMatch(/string/i);
    } finally {
      await f.close();
    }
  });

  it('a missing required argument is a tool error naming the parameter', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        // path omitted
      });
      expect(result.isError).toBe(true);
      expect(textOf(result)).toMatch(/\bpath\b/);

      const noProject = await callTool(f, 'list_files', {});
      expect(noProject.isError).toBe(true);
      expect(textOf(noProject)).toMatch(/\bproject\b/);
    } finally {
      await f.close();
    }
  });
});

// ============================================================================
// BP-15: unknown tool names are protocol errors, not tool-execution errors
// ============================================================================

describe('unknown tool classification (BP-15)', () => {
  it('an unknown tool name rejects as JSON-RPC -32602, not an isError result', async () => {
    const f = await startInMemoryMcp();
    try {
      await expect(callTool(f, 'definitely_not_a_tool', {})).rejects.toMatchObject({
        code: -32602,
      });
    } finally {
      await f.close();
    }
  });
});

// ============================================================================
// BP-16: dual-era handshake — legacy initialize + modern server/discover
// ============================================================================

describe('dual-era handshake (BP-16)', () => {
  it(
    'serves a legacy 2025-11-25 client and a pinned 2026-07-28 client in one process',
    async () => {
      const f = await startInMemoryMcp();
      try {
        const seed = await seedProject(f, [{ path: 'era.qmd', content: 'v1\n' }]);

        // Legacy arm: default negotiation runs the 2025 initialize
        // handshake; instructions arrive on the initialize result.
        const legacy = await serveClient(f.manager, 'legacy');
        try {
          expect(legacy.client.getNegotiatedProtocolVersion()).toBe('2025-11-25');
          expect(legacy.client.getInstructions()).toBeTruthy();
          const tools = await legacy.client.listTools();
          expect(tools.tools.length).toBeGreaterThan(0);
          const read = await legacy.client.callTool({
            name: 'read_file',
            arguments: { project: seed.indexDocId, path: 'era.qmd' },
          });
          expect(read.isError).not.toBe(true);
        } finally {
          await legacy.close();
        }

        // Modern arm: pinned 2026-07-28 — connect probes server/discover,
        // instructions arrive on the discover result.
        const modern = await serveClient(f.manager, 'modern');
        try {
          expect(modern.client.getNegotiatedProtocolVersion()).toBe('2026-07-28');
          expect(modern.client.getDiscoverResult()?.instructions).toBeTruthy();
          const tools = await modern.client.listTools();
          expect(tools.tools.length).toBeGreaterThan(0);
          const read = await modern.client.callTool({
            name: 'read_file',
            arguments: { project: seed.indexDocId, path: 'era.qmd' },
          });
          expect(read.isError).not.toBe(true);
        } finally {
          await modern.close();
        }
      } finally {
        await f.close();
      }
    },
    30000,
  );
});

// ============================================================================
// BP-17 / SEP-2549: tools/list determinism and cache hints
// ============================================================================

describe('tools/list cache hints (BP-17, SEP-2549)', () => {
  it('tools/list order is identical across calls and carries ttlMs on the modern era', async () => {
    const f = await startInMemoryMcp();
    try {
      const served = await serveClient(f.manager, 'modern');
      try {
        const first = await served.client.listTools();
        const second = await served.client.listTools();
        expect(second.tools.map((t) => t.name)).toEqual(first.tools.map((t) => t.name));

        // The static tool list earns a long TTL (SEP-2549); the 2026-era
        // codec fills ttlMs/cacheScope from the server's configured hints.
        // Read structurally: the era-union result type doesn't surface the
        // cache fields statically.
        const hinted = first as unknown as { ttlMs?: unknown; cacheScope?: unknown };
        expect(typeof hinted.ttlMs).toBe('number');
        expect(hinted.ttlMs as number).toBeGreaterThan(0);
        expect(hinted.cacheScope).toBe('private');
      } finally {
        await served.close();
      }
    } finally {
      await f.close();
    }
  });
});
