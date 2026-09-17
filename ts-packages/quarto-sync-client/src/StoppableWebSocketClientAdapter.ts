/**
 * `BrowserWebSocketClientAdapter` whose `disconnect()` is terminal
 * (bd-jit6pdwq Phase 5).
 *
 * Upstream bug (automerge-repo 2.5.6,
 * `packages/automerge-repo-network-websocket/src/WebSocketClientAdapter.ts`):
 * `disconnect()` cleared the retry `setInterval`, but the `onClose`
 * handler scheduled reconnects with a one-shot `setTimeout` that
 * `disconnect()` never cancelled. An adapter discarded after its socket
 * closed (server died, port now dead) resurrected itself when that
 * timer fired and retried the dead port every `retryInterval`,
 * forever. In Firefox those zombie attempts occupy the browser-wide
 * per-IP WebSocket handshake queue — the mechanism behind
 * bd-jit6pdwq — so a "torn down" stale tab kept breaking other tabs.
 *
 * Upstream fixed the timer path in 2.6.0-alpha.3 (automerge-repo PR
 * 690: `disconnect()` now clears the pending reconnect and a
 * `#disconnected` flag skips one already queued). What upstream still
 * allows is a *direct* `connect()` after `disconnect()`, which resets
 * that flag and recreates the socket — so this subclass keeps
 * `disconnect()` terminal by gating `connect()` behind its own stopped
 * flag. Reconnect-after-disconnect is never desired here: every
 * connection in this codebase builds a fresh adapter.
 */

import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import type { PeerId, PeerMetadata } from '@automerge/automerge-repo/slim';

import { syncLog } from './log.js';
import { recordConnectionEvent } from './sync-activity.js';

export class StoppableWebSocketClientAdapter extends BrowserWebSocketClientAdapter {
  #stopped = false;

  /**
   * Record the socket error as a diagnostic. Upstream ≤ 2.5.6 rethrew
   * any node error that wasn't ECONNREFUSED — a throw inside an event
   * callback can't reach a caller; it became an uncaughtException and
   * killed the host process (bd-xzspx4r9: the hub MCP server died on a
   * mid-handshake 'socket hang up'). Since 2.6.0-alpha.3 upstream only
   * logs, and types the handler as `() => void`; the browser passes an
   * opaque Event, Node's `ws` an ErrorEvent with `.error`, so the
   * parameter stays optional and we keep recording what is there.
   * Recovery is the close/retry machinery's job.
   */
  override onError = (event?: unknown): void => {
    const err = (event as { error?: { code?: string; message?: string } }).error;
    syncLog(
      `WebSocket error (will retry): ${err?.code ?? 'unknown'} ${err?.message ?? ''}`.trim(),
    );
    recordConnectionEvent('ws-error', `${err?.code ?? 'unknown'} ${err?.message ?? ''}`.trim());
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
