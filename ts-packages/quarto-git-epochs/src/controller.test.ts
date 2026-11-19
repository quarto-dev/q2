import { describe, expect, it } from 'vitest';
import { createController, type Clock } from './controller.js';
import {
  type CheapState,
  type ControllerProbe,
  type MarkerState,
  type ReflogSlice,
  GitUnavailableError,
} from './probe-types.js';

class FakeClock implements Clock {
  t = 1_000_000;
  now() {
    return this.t;
  }
  advance(ms: number) {
    this.t += ms;
  }
}

class FakeProbe implements ControllerProbe {
  head: string | null | Error = 'h1';
  markers: MarkerState[] = [];
  reflog: { size: number; mtimeMs: number } | null = { size: 100, mtimeMs: 1 };
  subjects: string[] = [];
  headCalls = 0;
  async headSha() {
    this.headCalls++;
    if (this.head instanceof Error) throw this.head;
    return this.head;
  }
  cheapState(): CheapState {
    return { markers: [...this.markers], reflog: this.reflog };
  }
  async reflogSince(): Promise<ReflogSlice | null> {
    if (!this.reflog) return null;
    return { subjects: [...this.subjects], tail: { sha: String(this.head), lines: 1 }, markFound: true };
  }
  /** Simulates git appending a reflog entry. */
  appendReflog(subject: string) {
    this.reflog = { size: (this.reflog?.size ?? 0) + 80, mtimeMs: (this.reflog?.mtimeMs ?? 0) + 1 };
    this.subjects.push(subject);
  }
}

const lock = (clock: Clock, ageMs = 0): MarkerState => ({ name: 'index.lock', kind: 'lock', mtimeMs: clock.now() - ageMs });
const stateMarker = (clock: Clock, name = 'MERGE_HEAD'): MarkerState => ({ name, kind: 'state', mtimeMs: clock.now() });

async function setup(opts: Partial<Parameters<typeof createController>[1]> = {}) {
  const clock = new FakeClock();
  const probe = new FakeProbe();
  const logs: string[] = [];
  const c = createController(probe, { log: (m) => logs.push(m), ...opts }, clock);
  await c.init();
  return { clock, probe, logs, c };
}

describe('controller gate', () => {
  it('is open and idle with no markers and no change', async () => {
    const { c } = await setup();
    expect(c.check()).toEqual({ mode: 'open', pending: false, markers: [], held: [] });
  });

  for (const [name, kind] of [['index.lock', 'lock'], ['MERGE_HEAD', 'state'], ['rebase-merge', 'state'], ['.jj/working_copy/working_copy.lock', 'lock']] as const) {
    it(`closes on the ${name} marker`, async () => {
      const { c, probe, clock } = await setup();
      probe.markers = [{ name, kind, mtimeMs: clock.now() }];
      const g = c.check();
      expect(g.mode).toBe('closed');
      expect(g.pending).toBe(true);
      expect(g.markers).toEqual([name]);
    });
  }

  it('check() sees a changed reflog tail before any tick', async () => {
    const { c, probe } = await setup();
    probe.appendReflog('checkout: moving from a to b');
    expect(probe.headCalls).toBe(1); // only init
    expect(c.check().mode).toBe('closed');
    expect(probe.headCalls).toBe(1); // check() ran no subprocess
  });

  it('a missing reflog is "no reflog", not an error', async () => {
    const { c, probe } = await setup();
    probe.reflog = null;
    expect(c.check().mode).toBe('closed'); // went from a reflog to none: that is a change
    const fresh = await setup();
    fresh.probe.reflog = null;
    const c2 = createController(fresh.probe, {}, fresh.clock);
    await c2.init();
    expect(c2.check()).toEqual({ mode: 'open', pending: false, markers: [], held: [] });
  });

  it('a HEAD change seen by a tick makes the gate pending', async () => {
    const { c, probe } = await setup();
    probe.head = 'h2';
    await c.tick();
    expect(c.check().pending).toBe(true);
  });

  it('holds every file whose disk differs from its last synced content, frozen at pending entry', async () => {
    let differing = ['a.qmd', 'b.qmd'];
    const { c, probe, clock } = await setup({ diskDiffersFromSynced: () => differing });
    probe.markers = [lock(clock)];
    expect(c.check().held).toEqual(['a.qmd', 'b.qmd']);
    differing = ['a.qmd', 'b.qmd', 'c.qmd']; // git rewrote another file meanwhile
    expect(c.check().held).toEqual(['a.qmd', 'b.qmd']);
  });
});

describe('controller decisions', () => {
  it('decides only after 1 s of quiet with stable state', async () => {
    const { c, probe, clock } = await setup();
    probe.appendReflog('checkout: moving from a to b');
    probe.head = 'h2';
    c.check();
    await c.tick(); // observes the new HEAD at the start of the window
    clock.advance(999);
    expect((await c.tick()).actions).toEqual([]);
    clock.advance(1);
    const r = await c.tick();
    expect(r.actions).toHaveLength(1);
    expect(r.actions[0]).toMatchObject({ type: 'decision', head: 'h2', reflog: ['checkout: moving from a to b'], held: [] });
    expect(c.check().pending).toBe(false);
  });

  it('a further reflog change restarts the quiet window', async () => {
    const { c, probe, clock } = await setup();
    probe.appendReflog('checkout: moving from a to b');
    c.check();
    clock.advance(800);
    probe.appendReflog('commit: x');
    c.check();
    clock.advance(800);
    expect((await c.tick()).actions).toEqual([]);
    clock.advance(200);
    expect((await c.tick()).actions).toHaveLength(1);
  });

  it("the peer's own writes do not restart the quiet window, a foreign write does", async () => {
    const { c, probe, clock } = await setup({ isOwnWrite: (p) => p === 'mine.qmd' });
    probe.appendReflog('checkout: moving from a to b');
    c.check();
    clock.advance(900);
    c.noteActivity('mine.qmd');
    clock.advance(100);
    expect((await c.tick()).actions).toHaveLength(1);

    probe.appendReflog('checkout: moving from b to a');
    c.check();
    clock.advance(900);
    c.noteActivity('theirs.qmd');
    clock.advance(100);
    expect((await c.tick()).actions).toEqual([]);
    clock.advance(900);
    expect((await c.tick()).actions).toHaveLength(1);
  });

  it('decides at the 30 s cap even when state keeps changing', async () => {
    const { c, probe, clock } = await setup();
    probe.appendReflog('checkout: moving from a to b');
    c.check();
    let decided = false;
    for (let i = 0; i < 80 && !decided; i++) {
      clock.advance(400);
      probe.appendReflog('rebase (pick): x');
      c.check();
      decided = (await c.tick()).actions.length > 0;
      if (!decided) expect(clock.now() - 1_000_000).toBeLessThan(30_400);
    }
    expect(decided).toBe(true);
    expect(clock.now() - 1_000_000).toBeGreaterThanOrEqual(30_000);
  });

  it('treats a lock older than 60 s as stale, resumes with one warning', async () => {
    const { c, probe, clock, logs } = await setup();
    probe.markers = [lock(clock)];
    c.check();
    clock.advance(61_000);
    probe.markers = [lock(clock, 61_000)];
    const r1 = await c.tick();
    expect(r1.actions).toEqual([]); // the quiet window starts when the lock is judged stale
    expect(logs.filter((l) => /stale/i.test(l))).toHaveLength(1);
    clock.advance(1000);
    expect((await c.tick()).actions).toHaveLength(1);
    expect(logs.filter((l) => /stale/i.test(l))).toHaveLength(1);
  });

  it('state markers pause indefinitely, and resume after one quiet window once gone', async () => {
    const { c, probe, clock } = await setup();
    probe.markers = [stateMarker(clock)];
    c.check();
    clock.advance(300_000);
    expect((await c.tick()).actions).toEqual([]);
    expect(c.check().mode).toBe('closed');
    probe.markers = [];
    c.check();
    clock.advance(500);
    expect((await c.tick()).actions).toEqual([]);
    clock.advance(500);
    expect((await c.tick()).actions).toHaveLength(1);
  });

  it('asks for a 250 ms tick while pending', async () => {
    const { c, probe, clock } = await setup();
    probe.markers = [lock(clock)];
    c.check();
    expect((await c.tick()).nextTickMs).toBe(250);
  });

  it('an unresolvable HEAD means no change, and the controller keeps re-checking', async () => {
    const { c, probe, clock } = await setup();
    probe.appendReflog('checkout: moving from a to b');
    c.check();
    probe.head = new Error('fatal: bad object HEAD');
    clock.advance(5000);
    const r = await c.tick();
    expect(r.actions).toEqual([]);
    expect(r.nextTickMs).toBe(250);
    probe.head = 'h2';
    expect((await c.tick()).actions).toEqual([]); // the new HEAD is activity: one quiet window first
    clock.advance(1000);
    expect((await c.tick()).actions).toHaveLength(1);
  });
});

describe('controller off and unborn', () => {
  it('goes off with one message when git is missing or refused', async () => {
    const clock = new FakeClock();
    const probe = new FakeProbe();
    probe.head = new GitUnavailableError('git: command not found');
    const logs: string[] = [];
    const c = createController(probe, { log: (m) => logs.push(m) }, clock);
    await c.init();
    expect(c.check()).toMatchObject({ mode: 'off', reason: 'git-missing' });
    await c.tick();
    await c.tick();
    expect(logs).toHaveLength(1);
  });

  it('refuses an unborn repository and recovers without a decision after the first commit', async () => {
    const clock = new FakeClock();
    const probe = new FakeProbe();
    probe.head = null;
    probe.reflog = null;
    const c = createController(probe, {}, clock);
    await c.init();
    expect(c.check()).toMatchObject({ mode: 'off', reason: 'unborn' });
    probe.head = 'h1';
    probe.reflog = { size: 80, mtimeMs: 5 };
    const r = await c.tick();
    expect(r.actions).toEqual([]);
    expect(c.check()).toMatchObject({ mode: 'open', pending: false });
  });
});
