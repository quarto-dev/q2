/**
 * Handler-level tests for the project-wide `wait_for_change` arm
 * (CAP-18) and progress emission (BP-4) — Phase 3 (bd-3qe7unp7).
 *
 * Same style as wait-for-change-handler.test.ts: the REAL registration
 * path with only the ConnectionManager faked. The captured registerTool
 * callback is invoked with a fabricated request context carrying a
 * progressToken + notify capture, which is how BP-4's emission is
 * driven deterministically (fake timers).
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import type { CallToolResult, McpServer } from '@modelcontextprotocol/server';
import { registerTools } from './tools.js';
import type { ConnectionManager } from './connection-manager.js';
import type { ProgressNotification } from './auth/auth-tools.js';

type AnyWaitResult = Awaited<ReturnType<ConnectionManager['waitForAnyChange']>>;

interface AnyWaitCall {
  project: string;
  timeoutMs: number;
  options: { sinceHash?: string; signal?: AbortSignal; server?: string } | undefined;
}

function harness(result: AnyWaitResult | (() => Promise<AnyWaitResult>)): {
  call: (args: Record<string, unknown>, ctx?: unknown) => Promise<CallToolResult>;
  calls: AnyWaitCall[];
} {
  const calls: AnyWaitCall[] = [];
  const manager = {
    configuredServerUrl: 'ws://test/',
    async waitForAnyChange(
      project: string,
      timeoutMs: number,
      options?: AnyWaitCall['options'],
    ) {
      calls.push({ project, timeoutMs, options });
      return typeof result === 'function' ? result() : result;
    },
  } as unknown as ConnectionManager;

  let toolCallback:
    | ((args: Record<string, unknown>, ctx?: unknown) => Promise<CallToolResult>)
    | undefined;
  const server = {
    registerTool(name: string, _config: unknown, cb: unknown) {
      if (name === 'wait_for_change') {
        toolCallback = cb as typeof toolCallback;
      }
    },
  } as unknown as McpServer;

  registerTools(server, manager, false);
  if (!toolCallback) throw new Error('wait_for_change callback was not registered');
  return { call: (args, ctx) => toolCallback!(args, ctx), calls };
}

function parse(result: CallToolResult): Record<string, unknown> {
  const block = result.content[0];
  if (block.type !== 'text') throw new Error('expected a text result block');
  return JSON.parse(block.text) as Record<string, unknown>;
}

/** A fabricated SDK request context with a progressToken and notify capture. */
function progressCtx(notifications: ProgressNotification[]): unknown {
  return {
    mcpReq: {
      signal: new AbortController().signal,
      _meta: { progressToken: 'tok-1' },
      notify: async (n: unknown) => {
        notifications.push(n as ProgressNotification);
      },
    },
  };
}

describe('handleWaitForChange — project-wide dispatch (CAP-18)', () => {
  it('omitting path dispatches to waitForAnyChange with the timeout and since_hash', async () => {
    const h = harness({ changed: false, changes: [] });
    await h.call({ project: 'idx', timeout_seconds: 7, since_hash: 'sha256:x' });
    expect(h.calls).toHaveLength(1);
    expect(h.calls[0].project).toBe('idx');
    expect(h.calls[0].timeoutMs).toBe(7000);
    expect(h.calls[0].options?.sinceHash).toBe('sha256:x');
  });

  it('serializes a changed project-wide result', async () => {
    const h = harness({
      changed: true,
      changes: [
        { path: 'a.qmd', hash: 'sha256:a', kind: 'edited' },
        { path: 'b.qmd', hash: null, kind: 'removed' },
      ],
    });
    const out = parse(await h.call({ project: 'idx' }));
    expect(out.changed).toBe(true);
    expect(out.changes).toEqual([
      { path: 'a.qmd', hash: 'sha256:a', kind: 'edited' },
      { path: 'b.qmd', hash: null, kind: 'removed' },
    ]);
  });

  it('serializes a project-wide timeout with an empty changes array and retry message', async () => {
    const h = harness({ changed: false, changes: [] });
    const out = parse(await h.call({ project: 'idx', timeout_seconds: 4 }));
    expect(out.changed).toBe(false);
    expect(out.changes).toEqual([]);
    expect(out.message).toContain('4s');
  });
});

describe('handleWaitForChange — progress notifications (BP-4)', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('emits progress 0 immediately, then periodically until the wait settles', async () => {
    vi.useFakeTimers();
    let settle: ((r: AnyWaitResult) => void) | undefined;
    const h = harness(
      () =>
        new Promise<AnyWaitResult>((resolve) => {
          settle = resolve;
        }),
    );
    const notifications: ProgressNotification[] = [];
    const call = h.call({ project: 'idx', timeout_seconds: 30 }, progressCtx(notifications));

    // Immediate emission at 0 out of the 30s budget.
    await vi.advanceTimersByTimeAsync(0);
    expect(notifications).toHaveLength(1);
    expect(notifications[0].method).toBe('notifications/progress');
    expect(notifications[0].params.progressToken).toBe('tok-1');
    expect(notifications[0].params.progress).toBe(0);
    expect(notifications[0].params.total).toBe(30_000);

    await vi.advanceTimersByTimeAsync(5_000);
    expect(notifications).toHaveLength(2);
    expect(notifications[1].params.progress).toBe(5_000);

    await vi.advanceTimersByTimeAsync(10_000);
    expect(notifications).toHaveLength(4);
    expect(notifications[3].params.progress).toBe(15_000);

    // Settling the wait stops the ticker — no notifications after.
    settle!({ changed: false, changes: [] });
    await call;
    const settledCount = notifications.length;
    await vi.advanceTimersByTimeAsync(20_000);
    expect(notifications).toHaveLength(settledCount);
  });

  it('emits nothing when the caller supplied no progressToken', async () => {
    vi.useFakeTimers();
    const h = harness({ changed: false, changes: [] });
    const notifications: ProgressNotification[] = [];
    // A ctx with notify but NO progressToken: emission is token-gated.
    const ctx = {
      mcpReq: {
        signal: new AbortController().signal,
        notify: async (n: unknown) => {
          notifications.push(n as ProgressNotification);
        },
      },
    };
    await h.call({ project: 'idx', timeout_seconds: 1 }, ctx);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(notifications).toHaveLength(0);
  });
});
