/**
 * Zombie-adapter regression (bd-jit6pdwq Phase 5).
 *
 * Upstream `BrowserWebSocketClientAdapter.disconnect()` (≤ 2.5.6)
 * cleared the retry *interval* but not the one-shot reconnect
 * `setTimeout` that `onClose` schedules after a failed/closed socket.
 * A disconnected adapter whose socket had already closed therefore
 * RESURRECTED itself when that timer fired — reconnecting to a dead
 * port every `retryInterval` forever. Found by the end-to-end WS-churn
 * check: the preview SPA's teardown-on-server-gone was calling
 * disconnect() and the churn continued anyway. automerge-repo
 * 2.6.0-alpha.3 fixed the timer path upstream; a direct `connect()`
 * after `disconnect()` still recreates the socket on the base class
 * (the control test below), which is what the subclass keeps closed.
 *
 * `StoppableWebSocketClientAdapter` makes disconnect() terminal:
 * any later connect() (the zombie timer's, or anyone else's) is a
 * no-op. Discarded adapters must stay dead — reconnect-after-
 * disconnect is never desired in this codebase; both hub-client and
 * the preview SPA build a fresh adapter per connection.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { StoppableWebSocketClientAdapter } from './StoppableWebSocketClientAdapter.js';
import type { PeerId } from '@automerge/automerge-repo/slim';

// Nothing listens on this port (the "dead server"). The adapter
// constructs its socket synchronously; connection failure arrives
// asynchronously and triggers the onClose reconnect path.
const DEAD_URL = 'ws://127.0.0.1:1/ws';

function getSocket(adapter: unknown): unknown {
  return (adapter as { socket?: unknown }).socket;
}

/**
 * Node-only test plumbing: under isomorphic-ws/`ws`, `close()` on a
 * still-CONNECTING socket emits an 'error' event ("WebSocket was
 * closed before the connection was established"), and the adapter's
 * own error listener is removed by `disconnect()` before `close()`.
 * Browsers don't surface this; in Node an unlistened 'error' becomes
 * an unhandled exception. Attach a swallow-listener that survives
 * the adapter's listener removal.
 */
function silenceSocket(adapter: unknown): void {
  const socket = getSocket(adapter) as
    | { addEventListener: (t: string, h: () => void) => void; on?: (t: string, h: () => void) => void }
    | undefined;
  socket?.addEventListener('error', () => {});
  socket?.on?.('error', () => {});
}

describe('StoppableWebSocketClientAdapter', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('creates a socket on connect like the upstream adapter', () => {
    const adapter = new StoppableWebSocketClientAdapter(DEAD_URL, 50);
    adapter.connect('peer-test' as PeerId, {});
    expect(getSocket(adapter)).toBeDefined();
    silenceSocket(adapter);
    adapter.disconnect();
  });

  it('stays dead after disconnect: a later connect() is a no-op', () => {
    const adapter = new StoppableWebSocketClientAdapter(DEAD_URL, 50);
    adapter.connect('peer-test' as PeerId, {});
    silenceSocket(adapter);
    adapter.disconnect();
    expect(getSocket(adapter)).toBeUndefined();

    // The zombie path: upstream's onClose schedules
    // `setTimeout(() => this.connect(...), retryInterval)`, which
    // disconnect() never cancels. Simulate that timer firing.
    adapter.connect('peer-test' as PeerId, {});

    expect(getSocket(adapter)).toBeUndefined();
  });

  it('control: the resurrection path exists upstream (connect after disconnect recreates a socket on the base class)', async () => {
    // Documents WHY the subclass exists. If this control ever fails,
    // upstream made disconnect() terminal and the subclass can be
    // retired.
    const { BrowserWebSocketClientAdapter } = await import(
      '@automerge/automerge-repo-network-websocket'
    );
    const adapter = new BrowserWebSocketClientAdapter(DEAD_URL, 50);
    adapter.connect('peer-test' as PeerId, {});
    silenceSocket(adapter);
    adapter.disconnect();
    expect(getSocket(adapter)).toBeUndefined();

    adapter.connect('peer-test' as PeerId, {});
    expect(getSocket(adapter)).toBeDefined(); // resurrected
    silenceSocket(adapter);
    adapter.disconnect();
  });
});
