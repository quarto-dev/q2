import { next as A } from '@automerge/automerge';
import { encodeHeads } from '@automerge/automerge-repo';
import type { DocHandle } from '@automerge/automerge-repo';
import { vi } from 'vitest';

import { CHUNK_SIZE } from '../services/attribution-runs';
import type { ViewableHandle } from '../services/attribution-runs';

interface TDoc {
  text: string;
}

export const COLD_BUILD_ACTOR = 'f'.repeat(32);
export const COLD_BUILD_TEXT_LENGTH = CHUNK_SIZE + 1;

export interface ColdBuildAttributionFixture {
  handle: DocHandle<unknown>;
  readonly text: string;
  append(value: string, time: number): void;
}

/** A live handle with enough history to pause a cold build after its first chunk. */
export function createColdBuildAttributionFixture(): ColdBuildAttributionFixture {
  let doc = A.from<TDoc>({ text: '' }, { actor: COLD_BUILD_ACTOR });
  for (let i = 0; i < COLD_BUILD_TEXT_LENGTH; i++) {
    doc = A.change(doc, { time: 1 }, d => A.splice(d, ['text'], i, 0, 'a'));
  }

  const view: ViewableHandle = {
    history: () => A.topoHistoryTraversal(doc).map(h => encodeHeads([h])),
    metadata: () => undefined,
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

/** A manually released requestIdleCallback queue for the real chunked producer. */
export function installColdBuildIdleGate() {
  const callbacks: IdleRequestCallback[] = [];
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
      callback({ didTimeout: false, timeRemaining: () => 50 });
    },
  };
}
