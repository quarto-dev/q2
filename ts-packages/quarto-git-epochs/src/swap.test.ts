import { describe, expect, it } from 'vitest';
import { createSwapCoordinator } from './swap.js';
import type { ReplacedEvent, SwapHost } from './swap.js';

type H = { id: string };

interface Deferred {
  docId: string;
  signal: AbortSignal;
  resolve: (h: H) => void;
  reject: (e: Error) => void;
}

/** A fake clock: timers fire only when the test advances time. */
function makeClock() {
  let now = 0;
  let nextId = 1;
  const timers = new Map<number, { at: number; fn: () => void }>();
  const clock = {
    setTimeout: ((fn: () => void, ms: number) => {
      const id = nextId++;
      timers.set(id, { at: now + ms, fn });
      return id;
    }) as unknown as typeof setTimeout,
    clearTimeout: ((id: number) => {
      timers.delete(id);
    }) as unknown as typeof clearTimeout,
  };
  return {
    clock,
    pending: () => timers.size,
    advance(ms: number) {
      now += ms;
      for (const [id, t] of [...timers]) {
        if (t.at <= now) {
          timers.delete(id);
          t.fn();
        }
      }
    },
  };
}

function setup(opts?: { unconfirmed?: number; initial?: Record<string, string> }) {
  const current: Record<string, string> = { ...(opts?.initial ?? { 'a.qmd': 'A0', 'b.qmd': 'B0' }) };
  const loads: Deferred[] = [];
  const events: ReplacedEvent[] = [];
  const rebinds: { path: string; newDocId: string; newHandle: H; oldHandle?: H }[] = [];
  const fake = makeClock();
  const oldHandles: Record<string, H> = {};
  for (const [p, d] of Object.entries(current)) oldHandles[p] = { id: d };
  const host: SwapHost<H> = {
    currentDocId: (p) => current[p],
    currentHandle: (p) => (current[p] === undefined ? undefined : { id: current[p] }),
    loadDoc: (docId, signal) =>
      new Promise<H>((resolve, reject) => {
        loads.push({ docId, signal, resolve, reject });
      }),
    rebind: (path, newDocId, newHandle, oldHandle) => {
      rebinds.push({ path, newDocId, newHandle, oldHandle });
      current[path] = newDocId;
    },
    unconfirmedLocalEdits:
      opts?.unconfirmed === undefined ? undefined : () => opts.unconfirmed as number,
    onReplaced: (e) => events.push(e),
    clock: fake.clock,
  };
  const coord = createSwapCoordinator<H>(host, { timeoutMs: 5000, retiredCap: 8 });
  return { coord, loads, events, rebinds, current, fake, oldHandles };
}

const flush = async () => {
  for (let i = 0; i < 10; i++) await Promise.resolve();
};

describe('createSwapCoordinator', () => {
  it('1. fires pending then done and rebinds once with the new and old handles', async () => {
    const { coord, loads, events, rebinds } = setup();
    coord.indexChanged('a.qmd', 'A1');
    expect(events.map((e) => e.phase)).toEqual(['pending']);
    expect(events[0]).toMatchObject({ path: 'a.qmd', oldDocId: 'A0', newDocId: 'A1' });
    expect(loads).toHaveLength(1);
    loads[0].resolve({ id: 'A1' });
    await flush();
    expect(events.map((e) => e.phase)).toEqual(['pending', 'done']);
    expect(rebinds).toHaveLength(1);
    expect(rebinds[0].newHandle).toEqual({ id: 'A1' });
    expect(rebinds[0].newDocId).toBe('A1');
    expect(rebinds[0].oldHandle).toEqual({ id: 'A0' });
  });

  it('2. two quick flips swap once to the final docId and abort the first load', async () => {
    const { coord, loads, events, rebinds } = setup();
    coord.indexChanged('a.qmd', 'A1');
    coord.indexChanged('a.qmd', 'A2');
    await flush();
    expect(loads[0].signal.aborted).toBe(true);
    expect(events.filter((e) => e.phase === 'pending')).toHaveLength(1);
    // The aborted load resolving late must not bind the intermediate doc.
    loads[0].resolve({ id: 'A1' });
    expect(loads).toHaveLength(2);
    expect(loads[1].docId).toBe('A2');
    loads[1].resolve({ id: 'A2' });
    await flush();
    expect(rebinds).toHaveLength(1);
    expect(rebinds[0].newDocId).toBe('A2');
    const done = events.filter((e) => e.phase === 'done');
    expect(done).toHaveLength(1);
    expect(done[0]).toMatchObject({ oldDocId: 'A0', newDocId: 'A2' });
  });

  it('3. a stalled load for one path does not delay another path', async () => {
    const { coord, loads, rebinds } = setup();
    coord.indexChanged('a.qmd', 'A1');
    coord.indexChanged('b.qmd', 'B1');
    const b = loads.find((l) => l.docId === 'B1')!;
    b.resolve({ id: 'B1' });
    await flush();
    expect(rebinds.map((r) => r.path)).toEqual(['b.qmd']);
  });

  describe('4. resolveWriteTarget', () => {
    it('is live with no baseDocId', () => {
      const { coord } = setup();
      expect(coord.resolveWriteTarget('a.qmd')).toMatchObject({ kind: 'live' });
    });

    it('during pending, the old docId resolves to the old handle and counts as stranded', () => {
      const { coord } = setup();
      coord.indexChanged('a.qmd', 'A1');
      const t = coord.resolveWriteTarget('a.qmd', 'A0');
      expect(t.kind).not.toBe('drop');
      expect(coord.takeStranded('a.qmd')).toBe(1);
    });

    it('after done, the old docId resolves as retired and counts as stranded', async () => {
      const { coord, loads } = setup();
      coord.indexChanged('a.qmd', 'A1');
      loads[0].resolve({ id: 'A1' });
      await flush();
      const t = coord.resolveWriteTarget('a.qmd', 'A0');
      expect(t).toMatchObject({ kind: 'retired', docId: 'A0' });
      expect(coord.takeStranded('a.qmd')).toBe(1);
      expect(coord.resolveWriteTarget('a.qmd', 'A1')).toMatchObject({ kind: 'live', docId: 'A1' });
    });

    it('drops an unknown docId with a reason', () => {
      const { coord } = setup();
      const t = coord.resolveWriteTarget('a.qmd', 'NOPE');
      expect(t.kind).toBe('drop');
      if (t.kind === 'drop') expect(t.reason).toMatch(/NOPE/);
    });
  });

  describe('5. stranded counter', () => {
    it('takeStranded returns and resets; the done event carries the count at that moment', async () => {
      const { coord, loads, events } = setup();
      coord.indexChanged('a.qmd', 'A1');
      coord.resolveWriteTarget('a.qmd', 'A0');
      coord.resolveWriteTarget('a.qmd', 'A0');
      loads[0].resolve({ id: 'A1' });
      await flush();
      const done = events.find((e) => e.phase === 'done')!;
      expect(done.stranded).toBe(2);
      coord.resolveWriteTarget('a.qmd', 'A0');
      expect(coord.takeStranded('a.qmd')).toBe(3);
      expect(coord.takeStranded('a.qmd')).toBe(0);
    });

    it('adds the host unconfirmed local edits at swap start', async () => {
      const { coord, loads, events } = setup({ unconfirmed: 2 });
      coord.indexChanged('a.qmd', 'A1');
      loads[0].resolve({ id: 'A1' });
      await flush();
      const done = events.find((e) => e.phase === 'done')!;
      expect(done.stranded).toBeGreaterThanOrEqual(2);
    });
  });

  it('6. times out after 5 s with unavailable, keeps the old doc, and retries on a later change', async () => {
    const { coord, loads, events, rebinds, current, fake } = setup();
    coord.indexChanged('a.qmd', 'A1');
    fake.advance(4999);
    expect(events.map((e) => e.phase)).toEqual(['pending']);
    fake.advance(1);
    await flush();
    expect(events.map((e) => e.phase)).toEqual(['pending', 'unavailable']);
    expect(loads[0].signal.aborted).toBe(true);
    expect(rebinds).toHaveLength(0);
    expect(current['a.qmd']).toBe('A0');
    coord.indexChanged('a.qmd', 'A1');
    expect(loads).toHaveLength(2);
    loads[1].resolve({ id: 'A1' });
    await flush();
    expect(rebinds).toHaveLength(1);
  });

  it('6b. a load that rejects reports unavailable', async () => {
    const { coord, loads, events } = setup();
    coord.indexChanged('a.qmd', 'A1');
    loads[0].reject(new Error('gone'));
    await flush();
    expect(events.map((e) => e.phase)).toEqual(['pending', 'unavailable']);
  });

  it('7. keeps at most 8 retired handles per path and forgets the oldest', async () => {
    const { coord, loads } = setup();
    for (let i = 1; i <= 9; i++) {
      coord.indexChanged('a.qmd', `A${i}`);
      loads[loads.length - 1].resolve({ id: `A${i}` });
      await flush();
    }
    const retired = coord.retired('a.qmd');
    expect(retired).toHaveLength(8);
    // A0 was retired first and is forgotten; A1..A8 remain, A9 is live.
    expect(retired.map((r) => r.docId)).toEqual(['A1', 'A2', 'A3', 'A4', 'A5', 'A6', 'A7', 'A8']);
    expect(coord.resolveWriteTarget('a.qmd', 'A0').kind).toBe('drop');
  });

  it('8. flipping back reuses the retired handle; flipping to the current docId does nothing', async () => {
    const { coord, loads, rebinds, events } = setup();
    coord.indexChanged('a.qmd', 'A1');
    loads[0].resolve({ id: 'A1' });
    await flush();
    coord.indexChanged('a.qmd', 'A1'); // current: nothing
    expect(loads).toHaveLength(1);
    expect(events.filter((e) => e.phase === 'pending')).toHaveLength(1);
    coord.indexChanged('a.qmd', 'A0'); // flip back
    await flush();
    expect(loads).toHaveLength(1);
    expect(rebinds).toHaveLength(2);
    expect(rebinds[1].newDocId).toBe('A0');
    expect(events.at(-1)).toMatchObject({ phase: 'done', newDocId: 'A0' });
  });

  it('9. dispose aborts in-flight loads and clears timers', async () => {
    const { coord, loads, fake, events } = setup();
    coord.indexChanged('a.qmd', 'A1');
    coord.indexChanged('b.qmd', 'B1');
    expect(fake.pending()).toBe(2);
    coord.dispose();
    expect(loads.every((l) => l.signal.aborted)).toBe(true);
    expect(fake.pending()).toBe(0);
    loads[0].resolve({ id: 'A1' });
    await flush();
    expect(events.some((e) => e.phase === 'done')).toBe(false);
  });

  it('10. flipping back to the still-bound doc while pending abandons the swap and reports done', async () => {
    const { coord, loads, events, rebinds } = setup();
    coord.indexChanged('a.qmd', 'A1');
    coord.indexChanged('a.qmd', 'A0');
    expect(loads[0].signal.aborted).toBe(true);
    expect(events.map((e) => e.phase)).toEqual(['pending', 'done']);
    expect(events[1]).toMatchObject({ oldDocId: 'A0', newDocId: 'A0' });
    expect(rebinds).toHaveLength(0);
  });

  it('11. an index change during an async rebind is applied after it finishes', async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const fake = makeClock();
    const loads2: Deferred[] = [];
    const events2: ReplacedEvent[] = [];
    const cur: Record<string, string> = { 'a.qmd': 'A0' };
    const c2 = createSwapCoordinator<H>({
      currentDocId: (p) => cur[p],
      currentHandle: (p) => ({ id: cur[p] }),
      loadDoc: (docId, signal) =>
        new Promise<H>((resolve, reject) => loads2.push({ docId, signal, resolve, reject })),
      rebind: async (p, d) => {
        await gate;
        cur[p] = d;
      },
      onReplaced: (e) => events2.push(e),
      clock: fake.clock,
    });
    c2.indexChanged('a.qmd', 'A1');
    loads2[0].resolve({ id: 'A1' });
    await flush();
    c2.indexChanged('a.qmd', 'A2'); // arrives mid-rebind
    expect(loads2).toHaveLength(1);
    release();
    await flush();
    expect(loads2).toHaveLength(2);
    expect(loads2[1].docId).toBe('A2');
    expect(events2.map((e) => e.phase)).toEqual(['pending', 'done', 'pending']);
    expect(events2[2]).toMatchObject({ oldDocId: 'A1', newDocId: 'A2' });
  });
});
