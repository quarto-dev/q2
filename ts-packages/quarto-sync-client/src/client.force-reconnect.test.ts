/**
 * The wake/network force-reconnect seen from the sync client
 * (bd-sob0j19j): an adapter trigger drops the peer and the online
 * indicator chain runs Offline → Online across the reconnect —
 * `onConnectionChange(false)` then `(true)` — with no page refresh.
 *
 * Real in-process hub, real sockets, real timers; only `window` is
 * stubbed (an EventTarget) so the adapter attaches its browser
 * triggers in Node. client.ts's own optimistic `online` listener keys
 * off `globalThis.addEventListener`, which Node lacks — the stubbed
 * `window` reaches only the adapter, so both observed transitions come
 * from the peer drop/rejoin, not the optimistic path.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

import { createSyncClient, type SyncClient } from './client.js';
import { startTestHub, type TestHub } from './test-hub.js';
import type { IndexDocument } from '@quarto/quarto-automerge-schema';

let hub: TestHub;
let client: SyncClient | null = null;

beforeEach(async () => {
  hub = await startTestHub();
});

afterEach(async () => {
  vi.unstubAllGlobals();
  await client?.disconnect().catch(() => {});
  client = null;
  await hub.stop();
});

/** Poll until `cond` holds; bounded so a regression fails fast. */
async function waitFor(cond: () => boolean, what: string, timeoutMs = 8000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (cond()) return;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`timed out waiting for ${what}`);
}

describe('force-reconnect online-indicator chain', () => {
  it('online trigger ⇒ onConnectionChange(false) then (true) across the reconnect', async () => {
    const windowTarget = new EventTarget();
    vi.stubGlobal('window', windowTarget);

    const index = hub.repo.create<IndexDocument>();
    index.change((doc) => {
      doc.version = 2;
      doc.files = {};
      doc.identities = {};
    });

    const onConnectionChange = vi.fn();
    client = createSyncClient({ onConnectionChange });
    await client.connect(hub.url, index.documentId, undefined, undefined, undefined, {
      storage: 'memory',
      peerTimeoutMs: 5000,
      retryIntervalMs: 250,
    });

    const saw = (value: boolean) => onConnectionChange.mock.calls.some(([v]) => v === value);

    await waitFor(() => saw(true), 'initial Online');
    onConnectionChange.mockClear();

    // The wake/network trigger: the adapter closes its OPEN socket and
    // runs upstream onClose directly — the peer drops now and rejoins
    // after retryIntervalMs.
    windowTarget.dispatchEvent(new Event('online'));

    await waitFor(() => saw(false), 'Offline flip after the trigger');
    await waitFor(() => saw(true), 'Online again after the reconnect');
  }, 20000);
});
