/**
 * GE-7 test specification 6: the "paused for git" indicator, against a stubbed `/api/preview/git-status`.
 *
 * The hub route lands in GE-4. Until then these tests drive a stub of the response shape in GE-1's contract.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';

import { createGitStatusPoller } from './gitStatus';

function fakeClock() {
  let now = 0;
  let nextId = 1;
  const timers = new Map<number, { at: number; fn: () => void }>();
  return {
    clock: {
      setTimeout: ((fn: () => void, ms: number) => {
        const id = nextId++;
        timers.set(id, { at: now + ms, fn });
        return id;
      }) as unknown as typeof setTimeout,
      clearTimeout: ((id: number) => {
        timers.delete(id);
      }) as unknown as typeof clearTimeout,
    },
    async advance(ms: number) {
      now += ms;
      for (const [id, t] of [...timers]) {
        if (t.at <= now) {
          timers.delete(id);
          t.fn();
        }
      }
      // Let the poller's awaited fetch settle.
      await new Promise((r) => queueMicrotask(() => r(undefined)));
      await new Promise((r) => queueMicrotask(() => r(undefined)));
    },
    pending: () => timers.size,
  };
}

function stub(responses: Array<{ status: number; body?: unknown } | Error>) {
  const calls: string[] = [];
  let i = 0;
  const fetchFn = (async (url: string) => {
    calls.push(String(url));
    const r = responses[Math.min(i++, responses.length - 1)];
    if (r instanceof Error) throw r;
    return new Response(r.body === undefined ? null : JSON.stringify(r.body), { status: r.status });
  }) as unknown as typeof fetch;
  return { fetchFn, calls };
}

const body = (over: Record<string, unknown> = {}) => ({
  tracking: true,
  paused: false,
  pending: false,
  markers: [],
  decisions: [],
  ...over,
});

async function settle(): Promise<void> {
  for (let n = 0; n < 5; n++) await new Promise((r) => queueMicrotask(() => r(undefined)));
}

describe('git status poller', () => {
  it('shows paused while the response says paused, and hides when it is not', async () => {
    const f = fakeClock();
    const { fetchFn } = stub([
      { status: 200, body: body({ paused: true }) },
      { status: 200, body: body({ paused: false }) },
    ]);
    const poller = createGitStatusPoller({ url: '/api/preview/git-status', fetch: fetchFn, clock: f.clock });
    poller.start();
    await settle();
    expect(poller.getSnapshot().paused).toBe(true);

    await f.advance(2000);
    expect(poller.getSnapshot().paused).toBe(false);
    poller.stop();
  });

  it('keeps polling every 2 s while tracking is true', async () => {
    const f = fakeClock();
    const { fetchFn, calls } = stub([{ status: 200, body: body() }]);
    const poller = createGitStatusPoller({ url: '/g', fetch: fetchFn, clock: f.clock });
    poller.start();
    await settle();
    await f.advance(2000);
    await f.advance(2000);
    expect(calls).toHaveLength(3);
    poller.stop();
  });

  it('hides and stops polling when the route answers 404', async () => {
    const f = fakeClock();
    const { fetchFn, calls } = stub([{ status: 404 }]);
    const poller = createGitStatusPoller({ url: '/g', fetch: fetchFn, clock: f.clock });
    poller.start();
    await settle();
    expect(poller.getSnapshot()).toEqual({ tracking: false, paused: false });
    expect(f.pending()).toBe(0);
    await f.advance(10000);
    expect(calls).toHaveLength(1);
  });

  it('treats a 200 that is not JSON (a server that answers every path with its page) as no route, and stops', async () => {
    const f = fakeClock();
    const calls: string[] = [];
    const fetchFn = (async (url: string) => {
      calls.push(String(url));
      return new Response('<!doctype html><html></html>', { status: 200, headers: { 'content-type': 'text/html' } });
    }) as unknown as typeof fetch;
    const poller = createGitStatusPoller({ url: '/g', fetch: fetchFn, clock: f.clock });
    poller.start();
    await settle();
    expect(poller.getSnapshot()).toEqual({ tracking: false, paused: false });
    expect(f.pending()).toBe(0);
    await f.advance(10000);
    expect(calls).toHaveLength(1);
  });

  it('hides and stops polling when tracking is false', async () => {
    const f = fakeClock();
    const { fetchFn, calls } = stub([{ status: 200, body: body({ tracking: false, paused: true }) }]);
    const poller = createGitStatusPoller({ url: '/g', fetch: fetchFn, clock: f.clock });
    poller.start();
    await settle();
    expect(poller.getSnapshot()).toEqual({ tracking: false, paused: false });
    await f.advance(10000);
    expect(calls).toHaveLength(1);
  });

  it('keeps trying after a network error and does not show paused', async () => {
    const f = fakeClock();
    const { fetchFn, calls } = stub([new Error('offline'), { status: 200, body: body({ paused: true }) }]);
    const poller = createGitStatusPoller({ url: '/g', fetch: fetchFn, clock: f.clock });
    poller.start();
    await settle();
    expect(poller.getSnapshot().paused).toBe(false);
    await f.advance(2000);
    expect(calls).toHaveLength(2);
    expect(poller.getSnapshot().paused).toBe(true);
    poller.stop();
  });

  it('stop cancels the next poll, and notifies subscribers only on change', async () => {
    const f = fakeClock();
    const { fetchFn } = stub([{ status: 200, body: body() }]);
    const poller = createGitStatusPoller({ url: '/g', fetch: fetchFn, clock: f.clock });
    let notified = 0;
    poller.subscribe(() => notified++);
    poller.start();
    await settle();
    const afterFirst = notified;
    await f.advance(2000);
    expect(notified).toBe(afterFirst); // identical status: no new snapshot
    poller.stop();
    expect(f.pending()).toBe(0);
  });
});

describe('default timers in a browser', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('does not call the global timers as methods of another object (browser "Illegal invocation")', async () => {
    const realSet = globalThis.setTimeout;
    vi.stubGlobal('setTimeout', function (this: unknown, ...args: Parameters<typeof setTimeout>) {
      if (this !== undefined && this !== globalThis) throw new TypeError('Illegal invocation');
      return realSet(...args);
    });
    const { fetchFn, calls } = stub([{ status: 200, body: body({ paused: true }) }]);
    const poller = createGitStatusPoller({ url: '/g', fetch: fetchFn, intervalMs: 5 });
    poller.start();
    await new Promise((r) => realSet(r, 60));
    poller.stop();
    expect(calls.length).toBeGreaterThanOrEqual(2); // it scheduled the next poll without throwing
    expect(poller.getSnapshot().paused).toBe(true);
  });
});
