/**
 * `BrowserWebSocketClientAdapter` whose `disconnect()` is terminal
 * (bd-jit6pdwq Phase 5).
 *
 * Upstream bug (automerge-repo 2.5.6,
 * `packages/automerge-repo-network-websocket/src/WebSocketClientAdapter.ts`):
 * `disconnect()` clears the retry `setInterval`, but the `onClose`
 * handler schedules reconnects with a one-shot `setTimeout` that
 * `disconnect()` never cancels. An adapter discarded after its socket
 * closed (server died, port now dead) resurrects itself when that
 * timer fires and retries the dead port every `retryInterval`,
 * forever. In Firefox those zombie attempts occupy the browser-wide
 * per-IP WebSocket handshake queue — the mechanism behind
 * bd-jit6pdwq — so a "torn down" stale tab kept breaking other tabs.
 *
 * The subclass gates `connect()` behind a stopped flag set by
 * `disconnect()`. Reconnect-after-disconnect is never desired here:
 * every connection in this codebase builds a fresh adapter.
 *
 * Upstreaming the `disconnect()` fix to automerge/automerge-repo is
 * tracked as follow-up work on the strand; if upstream lands it, this
 * subclass (and its control test, which detects exactly that) can be
 * retired.
 */

import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import type { PeerId, PeerMetadata } from '@automerge/automerge-repo/slim';

import { syncLog } from './log.js';

export class StoppableWebSocketClientAdapter extends BrowserWebSocketClientAdapter {
  #stopped = false;

  /**
   * Upstream's `onError` rethrows any node error that isn't
   * ECONNREFUSED — but a throw inside an event callback can't reach a
   * caller; it becomes an uncaughtException and kills the host
   * process (bd-xzspx4r9: the hub MCP server died on a mid-handshake
   * 'socket hang up'). Recovery is the close/retry machinery's job;
   * here we only record the diagnostic.
   */
  override onError = (event: unknown): void => {
    const err = (event as { error?: { code?: string; message?: string } }).error;
    syncLog(
      `WebSocket error (will retry): ${err?.code ?? 'unknown'} ${err?.message ?? ''}`.trim(),
    );
  };

  override connect(peerId: PeerId, peerMetadata?: PeerMetadata): void {
    if (this.#stopped) return;
    super.connect(peerId, peerMetadata);
  }

  override disconnect(): void {
    this.#stopped = true;
    super.disconnect();
  }
}
