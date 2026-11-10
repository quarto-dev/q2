import { next as A } from '@automerge/automerge';
import type { DocHandle } from '@automerge/automerge-repo';
import { vi } from 'vitest';

import { MAX_BATCH_SIZE, SLICE_BUDGET_MS } from '../services/attribution-runs';

interface TDoc {
  text: string;
}

export const COLD_BUILD_ACTOR = 'f'.repeat(32);
// One more same-second change than fits in a batch, so the build has a batch
// boundary at which it can yield.
export const COLD_BUILD_TEXT_LENGTH = MAX_BATCH_SIZE + 1;

export interface ColdBuildAttributionFixture {
  handle: DocHandle<unknown>;
  readonly text: string;
  append(value: string, time: number): void;
}

/** A live handle with enough history to pause a cold build after its first batch. */
export function createColdBuildAttributionFixture(): ColdBuildAttributionFixture {
  let doc = A.from<TDoc>({ text: '' }, { actor: COLD_BUILD_ACTOR });
  for (let i = 0; i < COLD_BUILD_TEXT_LENGTH; i++) {
    doc = A.change(doc, { time: 1 }, d => A.splice(d, ['text'], i, 0, 'a'));
  }

  const view = {
    doc: () => doc,
  };

  return {
    handle: view as unknown as DocHandle<unknown>,
    get text() {
      return doc.text;
    },
    append(value: string, time: number) {
      doc = A.change(doc, { time }, d => A.splice(d, ['text'], doc.text.length, 0, value));
    },
  };
}

/**
 * Replace the clock the producer reads for its slice budget. Everything else
 * on `performance` keeps working. Undone by `vi.unstubAllGlobals()`.
 */
export function stubAttributionClock(now: () => number): void {
  const real = performance;
  vi.stubGlobal('performance', new Proxy(real, {
    get(target, prop) {
      if (prop === 'now') return now;
      const value = Reflect.get(target, prop, target);
      return typeof value === 'function' ? value.bind(target) : value;
    },
  }));
}

/**
 * A manually released requestIdleCallback queue for the real producer. The
 * clock overruns the slice budget until the first release, then stops: each
 * build pauses at its first batch boundary, and a released build runs to
 * completion.
 */
export function installColdBuildIdleGate() {
  const callbacks: IdleRequestCallback[] = [];
  let released = false;
  let clock = 0;
  stubAttributionClock(() => (released ? clock : (clock += SLICE_BUDGET_MS)));
  vi.stubGlobal('requestIdleCallback', (callback: IdleRequestCallback) => {
    callbacks.push(callback);
    return callbacks.length;
  });

  return {
    get pending() {
      return callbacks.length;
    },
    runNext() {
      const callback = callbacks.shift();
      if (!callback) throw new Error('No cold-build idle callback is pending');
      released = true;
      callback({ didTimeout: false, timeRemaining: () => 50 });
    },
  };
}
