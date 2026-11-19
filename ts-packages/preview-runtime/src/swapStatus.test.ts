/**
 * GE-7 test specification 4 (the runtime part): the swap status store turns the sync client's replaced events into
 * the two things a UI needs: which paths are frozen (pending), and one aggregate notice per batch.
 */
import { describe, it, expect, vi } from 'vitest';
import type { ReplacedEvent } from '@quarto/quarto-git-epochs';

import { createSwapStatusStore } from './swapStatus';

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

const ev = (
  path: string,
  phase: ReplacedEvent['phase'],
  extra: Partial<ReplacedEvent> = {},
): ReplacedEvent => ({ path, phase, oldDocId: 'a', newDocId: 'b', ...extra });

describe('swap status store', () => {
  it('tracks pending paths and clears them on done and on unavailable', () => {
    const { clock } = fakeClock();
    const store = createSwapStatusStore({ clock });
    store.handle(ev('index.qmd', 'pending'));
    store.handle(ev('other.qmd', 'pending'));
    expect([...store.getSnapshot().pending].sort()).toEqual(['index.qmd', 'other.qmd']);
    store.handle(ev('index.qmd', 'done'));
    store.handle(ev('other.qmd', 'unavailable'));
    expect(store.getSnapshot().pending).toEqual([]);
  });

  it('publishes one aggregate notice per batch after a quiet window', () => {
    const f = fakeClock();
    const store = createSwapStatusStore({ clock: f.clock, quietMs: 100 });
    store.handle(ev('a.qmd', 'pending'));
    store.handle(ev('b.qmd', 'pending'));
    store.handle(ev('a.qmd', 'done'));
    expect(store.getSnapshot().notice).toBeNull();
    f.advance(50);
    store.handle(ev('b.qmd', 'done'));
    f.advance(99);
    expect(store.getSnapshot().notice).toBeNull();
    f.advance(1);
    expect(store.getSnapshot().notice).toBe(
      "2 files were replaced because the project owner's checked-out version changed.",
    );
  });

  it('says "this file" for one file, appends stranded edits, and never shows a notice for unavailable', () => {
    const f = fakeClock();
    const store = createSwapStatusStore({ clock: f.clock, quietMs: 10 });
    store.handle(ev('index.qmd', 'unavailable'));
    f.advance(10);
    expect(store.getSnapshot().notice).toBeNull();

    store.handle(ev('index.qmd', 'done', { stranded: 3 }));
    f.advance(10);
    expect(store.getSnapshot().notice).toBe(
      "This file was replaced because the project owner's checked-out version changed. 3 of your recent edits stayed with the previous version.",
    );
  });

  it('names the file when it is not the open one', () => {
    const f = fakeClock();
    const store = createSwapStatusStore({ clock: f.clock, quietMs: 10 });
    store.setOpenFile('index.qmd');
    store.handle(ev('other.qmd', 'done'));
    f.advance(10);
    expect(store.getSnapshot().notice).toBe(
      "The file other.qmd was replaced because the project owner's checked-out version changed.",
    );
  });

  it('appends extra lines (for example the branches that stay) to the batch notice', () => {
    const f = fakeClock();
    const store = createSwapStatusStore({ clock: f.clock, quietMs: 10 });
    store.handle(ev('index.qmd', 'done'));
    store.appendToNextNotice('2 branches of this file stay with the previous version');
    f.advance(10);
    expect(store.getSnapshot().notice).toMatch(/ 2 branches of this file stay with the previous version$/);
  });

  it('two batches produce two notices, and dismissing clears the current one', () => {
    const f = fakeClock();
    const store = createSwapStatusStore({ clock: f.clock, quietMs: 10 });
    store.handle(ev('a.qmd', 'done'));
    f.advance(10);
    expect(store.getSnapshot().noticeSeq).toBe(1);
    store.handle(ev('b.qmd', 'done'));
    f.advance(10);
    expect(store.getSnapshot().noticeSeq).toBe(2);
    expect(store.getSnapshot().notice).toMatch(/^The file b\.qmd|^This file/);
    store.dismissNotice();
    expect(store.getSnapshot().notice).toBeNull();
  });

  it('returns a stable snapshot until something changes, and notifies subscribers', () => {
    const { clock } = fakeClock();
    const store = createSwapStatusStore({ clock });
    const first = store.getSnapshot();
    expect(store.getSnapshot()).toBe(first);
    const listener = vi.fn();
    const off = store.subscribe(listener);
    store.handle(ev('a.qmd', 'pending'));
    expect(listener).toHaveBeenCalledTimes(1);
    expect(store.getSnapshot()).not.toBe(first);
    off();
    store.handle(ev('a.qmd', 'done'));
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('reset clears pending paths, the notice, and the pending batch', () => {
    const f = fakeClock();
    const store = createSwapStatusStore({ clock: f.clock, quietMs: 10 });
    store.handle(ev('a.qmd', 'pending'));
    store.handle(ev('b.qmd', 'done'));
    store.reset();
    f.advance(10);
    expect(store.getSnapshot()).toMatchObject({ pending: [], notice: null });
  });
});
