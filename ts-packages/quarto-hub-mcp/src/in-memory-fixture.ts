/**
 * Test fixture: an in-process MCP client ↔ server pair linked over the
 * SDK's `InMemoryTransport`, booted against the in-process `test-hub`.
 *
 * This is the Phase 0 conformance harness's foundation
 * (claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md, bd-f1dr7gs1).
 * It is deliberately a *new* fixture rather than an extension of
 * `mcp-test-client.ts`: that client is raw JSON-RPC over a spawned stdio
 * process with no schema validation — right for the stdio-hygiene tests,
 * wrong for wire-contract assertions. This fixture drives the real SDK
 * `Client`, so results pass through the SDK's own validation and the
 * harness can use `ToolSchema` / JSON Schema 2020-12 directly. Writing
 * against the SDK `Client` API (not the stdio process) is also what lets
 * the fixture survive the SDK v2 migration (BP-16): only the import path
 * changes, not the call pattern.
 *
 * The server side is built by the same {@link createServer} factory the
 * stdio entrypoint uses, so identity, instructions, and the registered
 * tool surface are exactly what a real client sees.
 */

import { Client, InMemoryTransport } from '@modelcontextprotocol/client';
import type { CallToolResult } from '@modelcontextprotocol/client';
import type { McpServer } from '@modelcontextprotocol/server';

import { ConnectionManager } from './connection-manager.js';
import { createServer } from './index.js';
import { startTestHub, type TestHub } from './test-hub.js';

export interface InMemoryMcpFixture {
  /** The in-process hub stand-in (server-side ground truth). */
  readonly hub: TestHub;
  /** The connection manager under test — same instance the server uses. */
  readonly manager: ConnectionManager;
  /** The server under test. */
  readonly server: McpServer;
  /** A connected SDK client linked to the server. */
  readonly client: Client;
  /** Tear down client, server, manager connections, and hub. */
  close(): Promise<void>;
}

export interface InMemoryMcpOptions {
  /** Serve only the read tools (mirrors `--read-only`). */
  readOnly?: boolean;
}

export async function startInMemoryMcp(
  opts: InMemoryMcpOptions = {},
): Promise<InMemoryMcpFixture> {
  const hub = await startTestHub();
  const manager = new ConnectionManager({ serverUrl: hub.url });
  const server = createServer({ manager, readOnly: opts.readOnly ?? false });

  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  const client = new Client(
    { name: 'in-memory-conformance-client', version: '0.0.0' },
    { capabilities: {} },
  );

  await server.connect(serverTransport);
  await client.connect(clientTransport);

  return {
    hub,
    manager,
    server,
    client,
    async close(): Promise<void> {
      await client.close();
      await server.close();
      // Seeding waits for real delivery before returning, so there is
      // nothing left to drain at teardown.
      await manager.disconnectAll({ drainMs: 0 });
      await hub.stop();
    },
  };
}

export interface SeededProject {
  readonly indexDocId: string;
  readonly files: ReadonlyArray<{ path: string; docId: string }>;
}

/**
 * `client.callTool` with the result typed as `CallToolResult`.
 * Cancellation tests that need `RequestOptions` (signal) call
 * `client.callTool` directly.
 */
export async function callTool(
  fixture: InMemoryMcpFixture,
  name: string,
  args: Record<string, unknown>,
): Promise<CallToolResult> {
  return fixture.client.callTool({ name, arguments: args });
}

/**
 * Seed a project *through the server under test* (`create_project` is a
 * tool call like any other) and wait until the hub actually holds every
 * document — creation syncs in the background, so the tool result alone
 * is not proof the hub can serve the project (same wait pattern as
 * dangling-entries.test.ts).
 */
export async function seedProject(
  fixture: InMemoryMcpFixture,
  files: Array<{ path: string; content: string }>,
): Promise<SeededProject> {
  const result = await callTool(fixture, 'create_project', { files });
  const block = result.content[0];
  if (result.isError === true || block?.type !== 'text') {
    throw new Error(`seedProject: create_project failed: ${JSON.stringify(result.content)}`);
  }
  const parsed = JSON.parse(block.text) as {
    indexDocId: string;
    files: Array<{ path: string; docId: string }>;
  };
  if (!(await fixture.hub.hubHasDoc(parsed.indexDocId, 8000))) {
    throw new Error('seedProject: hub never received the index document');
  }
  for (const f of parsed.files) {
    if (!(await fixture.hub.hubHasDoc(f.docId, 8000))) {
      throw new Error(`seedProject: hub never received ${f.path}`);
    }
  }
  return parsed;
}
