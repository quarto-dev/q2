/**
 * Unit tests for the execution-channel wire format + pure helpers
 * (bd-sfet3264, Phase 2B).
 *
 * These cover the cross-language wire contract (the Rust executor mirrors it
 * in Phase 4) and the pure live-executor bookkeeping, with no timers or
 * DocHandles involved.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { DocHandleEphemeralMessagePayload } from '@automerge/automerge-repo';
import {
  BEACON_INTERVAL_MS,
  BEACON_TIMEOUT_MS,
  makeBeacon,
  makeExecuteRequest,
  parseExecMessage,
  applyBeacon,
  pruneExecutors,
  createExecutionChannel,
  type ExecMessage,
  type ExecRequestMessage,
  type LiveExecutor,
} from './executionChannel';

describe('execution-channel timing constants', () => {
  it('beacon timeout is 1.5x the interval (D2 liveness contract)', () => {
    expect(BEACON_TIMEOUT_MS).toBe(BEACON_INTERVAL_MS * 1.5);
  });
});

describe('message builders', () => {
  it('makeBeacon builds a well-formed beacon', () => {
    expect(makeBeacon('actor-1', ['knitr', 'jupyter'], 2)).toEqual({
      kind: 'exec/beacon',
      actorId: 'actor-1',
      engines: ['knitr', 'jupyter'],
      generation: 2,
    });
  });

  it('makeExecuteRequest builds a well-formed request', () => {
    expect(makeExecuteRequest('index.qmd', 'req-7', 'actor-9')).toEqual({
      kind: 'exec/request',
      path: 'index.qmd',
      requestId: 'req-7',
      requesterActorId: 'actor-9',
    });
  });
});

describe('parseExecMessage', () => {
  it('accepts a valid beacon and returns it typed', () => {
    const raw = { kind: 'exec/beacon', actorId: 'a', engines: ['knitr'], generation: 0 };
    expect(parseExecMessage(raw)).toEqual(raw);
  });

  it('accepts a valid request and returns it typed', () => {
    const raw = { kind: 'exec/request', path: 'p.qmd', requestId: 'r', requesterActorId: 'a' };
    expect(parseExecMessage(raw)).toEqual(raw);
  });

  it('rejects unknown / missing kind', () => {
    expect(parseExecMessage({ kind: 'presence', x: 1 })).toBeNull();
    expect(parseExecMessage({ actorId: 'a' })).toBeNull();
  });

  it('rejects a beacon with wrong field types', () => {
    expect(parseExecMessage({ kind: 'exec/beacon', actorId: 'a', engines: 'knitr', generation: 0 })).toBeNull();
    expect(parseExecMessage({ kind: 'exec/beacon', actorId: 1, engines: [], generation: 0 })).toBeNull();
    expect(parseExecMessage({ kind: 'exec/beacon', actorId: 'a', engines: [1], generation: 0 })).toBeNull();
  });

  it('rejects a request missing required fields', () => {
    expect(parseExecMessage({ kind: 'exec/request', path: 'p.qmd' })).toBeNull();
  });

  it('rejects non-objects', () => {
    expect(parseExecMessage(null)).toBeNull();
    expect(parseExecMessage('exec/beacon')).toBeNull();
    expect(parseExecMessage(42)).toBeNull();
  });
});

describe('applyBeacon', () => {
  it('inserts a new executor with lastSeen set to now', () => {
    const out = applyBeacon(new Map(), makeBeacon('a', ['knitr'], 0), 1000);
    expect(out.get('a')).toEqual({ actorId: 'a', engines: ['knitr'], generation: 0, lastSeen: 1000 });
  });

  it('refreshes lastSeen (and engines/generation) for an existing executor', () => {
    const prev = new Map<string, LiveExecutor>([
      ['a', { actorId: 'a', engines: ['knitr'], generation: 0, lastSeen: 1000 }],
    ]);
    const out = applyBeacon(prev, makeBeacon('a', ['knitr', 'jupyter'], 1), 5000);
    expect(out.get('a')).toEqual({ actorId: 'a', engines: ['knitr', 'jupyter'], generation: 1, lastSeen: 5000 });
    // Does not mutate the input map.
    expect(prev.get('a')!.lastSeen).toBe(1000);
  });
});

describe('pruneExecutors', () => {
  const base = (): Map<string, LiveExecutor> =>
    new Map([
      ['fresh', { actorId: 'fresh', engines: [], generation: 0, lastSeen: 10_000 }],
      ['stale', { actorId: 'stale', engines: [], generation: 0, lastSeen: 1_000 }],
    ]);

  it('drops executors older than the timeout and keeps fresh ones', () => {
    // now = 12_000: fresh is 2_000ms old (<= 4_500, kept); stale is 11_000ms
    // old (> 4_500, dropped).
    const out = pruneExecutors(base(), 12_000);
    expect(out.has('fresh')).toBe(true);
    expect(out.has('stale')).toBe(false);
  });

  it('keeps an executor exactly at the timeout boundary (<= timeout)', () => {
    const out = pruneExecutors(base(), 10_000 + BEACON_TIMEOUT_MS);
    expect(out.has('fresh')).toBe(true);
  });
});

// ── Stub-responder service tests (Phase 2C) ─────────────────────────────

/** A fake index DocHandle that records broadcasts and lets a test inject
 *  inbound ephemeral messages (simulating a remote executor). */
function fakeHandle() {
  const handlers = new Set<(p: DocHandleEphemeralMessagePayload<unknown>) => void>();
  const broadcasts: unknown[] = [];
  return {
    handle: {
      broadcast: (m: unknown) => broadcasts.push(m),
      on: (_e: 'ephemeral-message', h: (p: DocHandleEphemeralMessagePayload<unknown>) => void) =>
        handlers.add(h),
      off: (_e: 'ephemeral-message', h: (p: DocHandleEphemeralMessagePayload<unknown>) => void) =>
        handlers.delete(h),
    },
    broadcasts,
    /** Deliver a message as if a remote peer broadcast it. */
    inject: (message: unknown) =>
      handlers.forEach((h) => h({ message } as DocHandleEphemeralMessagePayload<unknown>)),
    handlerCount: () => handlers.size,
  };
}

describe('createExecutionChannel (Phase 2C)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('a remote executor beacon makes it appear, and stops listening on stop()', () => {
    const fake = fakeHandle();
    let clock = 1000;
    const onExecutorsChange = vi.fn();
    const ch = createExecutionChannel({
      getIndexHandle: () => fake.handle,
      onExecutorsChange,
      now: () => clock,
      pruneIntervalMs: 1000,
    });

    ch.start();
    expect(fake.handlerCount()).toBe(1);

    fake.inject(makeBeacon('exec-1', ['knitr'], 0));
    expect(onExecutorsChange).toHaveBeenLastCalledWith([
      { actorId: 'exec-1', engines: ['knitr'], generation: 0, lastSeen: 1000 },
    ]);
    expect(ch.getExecutors()).toHaveLength(1);

    ch.stop();
    expect(fake.handlerCount()).toBe(0);
    expect(ch.getExecutors()).toHaveLength(0);
  });

  it('prunes an executor once its beacon goes stale (1.5x interval)', () => {
    const fake = fakeHandle();
    let clock = 1000;
    const onExecutorsChange = vi.fn();
    const ch = createExecutionChannel({
      getIndexHandle: () => fake.handle,
      onExecutorsChange,
      now: () => clock,
      pruneIntervalMs: 1000,
    });
    ch.start();
    fake.inject(makeBeacon('exec-1', ['knitr'], 0));
    expect(ch.getExecutors()).toHaveLength(1);

    // Advance the clock well past the beacon timeout, then let the prune
    // timer fire. The executor should be dropped and a change emitted.
    clock += BEACON_TIMEOUT_MS + 1;
    onExecutorsChange.mockClear();
    vi.advanceTimersByTime(1000);

    expect(ch.getExecutors()).toHaveLength(0);
    expect(onExecutorsChange).toHaveBeenLastCalledWith([]);
    ch.stop();
  });

  it('requestExecution broadcasts a well-formed exec/request and returns its id', () => {
    const fake = fakeHandle();
    const ch = createExecutionChannel({
      getIndexHandle: () => fake.handle,
      onExecutorsChange: vi.fn(),
      selfActorId: 'me',
      now: () => 5000,
      generateRequestId: () => 'req-fixed',
    });
    ch.start();

    const id = ch.requestExecution('docs/index.qmd');
    expect(id).toBe('req-fixed');

    const sent = fake.broadcasts.at(-1) as ExecRequestMessage;
    expect(sent).toEqual({
      kind: 'exec/request',
      path: 'docs/index.qmd',
      requestId: 'req-fixed',
      requesterActorId: 'me',
    });
    // It must be a valid message by the shared parser (round-trip contract).
    expect(parseExecMessage(sent)).toEqual<ExecMessage>(sent);
    ch.stop();
  });

  it("ignores the editor's own beacon (self actor)", () => {
    const fake = fakeHandle();
    const onExecutorsChange = vi.fn();
    const ch = createExecutionChannel({
      getIndexHandle: () => fake.handle,
      onExecutorsChange,
      selfActorId: 'me',
      now: () => 1000,
    });
    ch.start();
    fake.inject(makeBeacon('me', ['knitr'], 0));
    expect(ch.getExecutors()).toHaveLength(0);
    expect(onExecutorsChange).not.toHaveBeenCalled();
    ch.stop();
  });

  it('requestExecution returns null when not connected (no index handle)', () => {
    const ch = createExecutionChannel({
      getIndexHandle: () => null,
      onExecutorsChange: vi.fn(),
    });
    ch.start();
    expect(ch.requestExecution('doc.qmd')).toBeNull();
  });
});
