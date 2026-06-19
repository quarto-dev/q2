/**
 * Handler-level tests for the `wait_for_change` tool (handleWaitForChange).
 *
 * These drive the REAL production handler through the REAL registration/dispatch
 * path: `registerTools` installs a CallTool handler on the server, and we invoke
 * the captured callback exactly as the MCP runtime would. The only thing faked
 * is `ConnectionManager.waitForChange` — a genuine network/sync collaborator —
 * which we use to (a) record the timeout it was handed and (b) return each of
 * the result shapes the handler must serialize.
 *
 * Coverage these pin (previously untested — fail-on-revert found the gap):
 *   - the 1..55s clamp on timeout_seconds (tools.ts: Math.max(1, Math.min(55,…)))
 *   - the default of 25s when timeout_seconds is absent
 *   - since_hash passthrough
 *   - the four result shapes: timeout / text / removed / binary
 */

import { describe, it, expect } from 'vitest';
import { CallToolRequestSchema } from '@modelcontextprotocol/sdk/types.js';
import type { Server } from '@modelcontextprotocol/sdk/server/index.js';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import { registerTools } from './tools.js';
import type { ConnectionManager } from './connection-manager.js';

type WaitResult = Awaited<ReturnType<ConnectionManager['waitForChange']>>;

interface WaitCall {
  project: string;
  path: string;
  timeoutMs: number;
  sinceHash: string | undefined;
}

/**
 * Build a fake ConnectionManager whose waitForChange records its arguments and
 * returns `result`, then register the real tool handlers against it. Returns the
 * captured CallTool callback plus the recorded calls.
 */
function harness(result: WaitResult): {
  call: (args: Record<string, unknown>) => Promise<CallToolResult>;
  calls: WaitCall[];
} {
  const calls: WaitCall[] = [];
  const manager = {
    async waitForChange(project: string, path: string, timeoutMs: number, sinceHash?: string) {
      calls.push({ project, path, timeoutMs, sinceHash });
      return result;
    },
  } as unknown as ConnectionManager;

  let callToolHandler:
    | ((req: { params: { name: string; arguments?: Record<string, unknown> } }, extra: unknown) => Promise<CallToolResult>)
    | undefined;
  const server = {
    setRequestHandler(schema: unknown, cb: unknown) {
      if (schema === CallToolRequestSchema) {
        callToolHandler = cb as typeof callToolHandler;
      }
    },
  } as unknown as Server;

  registerTools(server, manager, false);
  if (!callToolHandler) throw new Error('CallTool handler was not registered');

  const call = (args: Record<string, unknown>) =>
    callToolHandler!({ params: { name: 'wait_for_change', arguments: args } }, {});

  return { call, calls };
}

const TIMEOUT_RESULT: WaitResult = { changed: false, payload: null, hash: 'sha256:seed' };

function parse(result: CallToolResult): Record<string, unknown> {
  const block = result.content[0];
  if (block.type !== 'text') throw new Error('expected a text result block');
  return JSON.parse(block.text) as Record<string, unknown>;
}

describe('handleWaitForChange — timeout clamp + defaults', () => {
  it('defaults to 25s when timeout_seconds is absent', async () => {
    const h = harness(TIMEOUT_RESULT);
    await h.call({ project: 'idx', path: 'a.qmd' });
    expect(h.calls[0].timeoutMs).toBe(25_000);
  });

  it('clamps an over-range timeout_seconds down to 55s', async () => {
    const h = harness(TIMEOUT_RESULT);
    await h.call({ project: 'idx', path: 'a.qmd', timeout_seconds: 9999 });
    expect(h.calls[0].timeoutMs).toBe(55_000);
  });

  it('clamps an under-range timeout_seconds up to 1s', async () => {
    const h = harness(TIMEOUT_RESULT);
    await h.call({ project: 'idx', path: 'a.qmd', timeout_seconds: 0 });
    expect(h.calls[0].timeoutMs).toBe(1_000);
  });

  it('passes an in-range timeout_seconds through unchanged', async () => {
    const h = harness(TIMEOUT_RESULT);
    await h.call({ project: 'idx', path: 'a.qmd', timeout_seconds: 30 });
    expect(h.calls[0].timeoutMs).toBe(30_000);
  });

  it('forwards since_hash to the connection manager', async () => {
    const h = harness(TIMEOUT_RESULT);
    await h.call({ project: 'idx', path: 'a.qmd', since_hash: 'sha256:prev' });
    expect(h.calls[0].sinceHash).toBe('sha256:prev');
  });
});

describe('handleWaitForChange — result shapes', () => {
  it('serializes a timeout (changed:false) with a retry message naming the timeout', async () => {
    const h = harness({ changed: false, payload: null, hash: 'sha256:seed' });
    const out = parse(await h.call({ project: 'idx', path: 'a.qmd', timeout_seconds: 30 }));
    expect(out.changed).toBe(false);
    expect(out.hash).toBe('sha256:seed');
    expect(out.message).toContain('30s');
  });

  it('serializes a text change with its content', async () => {
    const h = harness({ changed: true, payload: { type: 'text', text: 'new body' }, hash: 'sha256:txt' });
    const out = parse(await h.call({ project: 'idx', path: 'a.qmd' }));
    expect(out.changed).toBe(true);
    expect(out.content).toBe('new body');
    expect(out.hash).toBe('sha256:txt');
  });

  it('serializes a removal as { changed:true, removed:true }', async () => {
    const h = harness({ changed: true, payload: null, hash: null });
    const out = parse(await h.call({ project: 'idx', path: 'a.qmd' }));
    expect(out.changed).toBe(true);
    expect(out.removed).toBe(true);
    expect(out.content).toBeUndefined();
  });

  it('serializes a binary change with its mimeType and no content body', async () => {
    const h = harness({
      changed: true,
      payload: { type: 'binary', data: new Uint8Array([1, 2, 3]), mimeType: 'image/png' },
      hash: 'sha256:img',
    });
    const out = parse(await h.call({ project: 'idx', path: 'a.png' }));
    expect(out.changed).toBe(true);
    expect(out.type).toBe('binary');
    expect(out.mimeType).toBe('image/png');
    expect(out.content).toBeUndefined();
  });
});
