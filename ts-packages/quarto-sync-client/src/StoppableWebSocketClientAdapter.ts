/**
 * `BrowserWebSocketClientAdapter` whose `disconnect()` is terminal
 * (bd-jit6pdwq Phase 5), with browser triggers that force-reconnect a
 * silently-dead socket (bd-sob0j19j).
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
 *
 * Force-reconnect (bd-sob0j19j): a half-open TCP connection (laptop
 * sleep, network switch) never delivers a `close` event, so upstream's
 * reconnect-on-close never fires — the client stays "stale until
 * refresh" while the badge says Online. The sync protocol sends nothing
 * on an idle connection, so no traffic reveals the drop. Browser-
 * observable triggers close the suspect socket and run upstream
 * `onClose` directly (the same path a hub restart exercises):
 *
 *  - wake detection: a WAKE_CHECK_INTERVAL timer compares wall-clock
 *    `Date.now()` with the previous tick; a gap over WAKE_GAP_THRESHOLD
 *    means the process was suspended (sleep, frozen background tab);
 *  - the window `online` event;
 *  - `navigator.connection` `change` where it exists (Chromium only).
 *
 * Reconnecting to a server that's down (browser only): after an attempt
 * that never opened, the next one first asks the server over plain HTTP
 * (`fetch` of the same URL, `ws:` as `http:`), and opens a WebSocket
 * only if anything answers. In Firefox every WebSocket attempt to a dead
 * port feeds its reconnect delay for that address, and a socket waiting
 * out that delay holds the one CONNECTING slot for the whole IP address,
 * so a tab left open on a server that's gone held up every other tab's
 * connections to the same host for up to a minute at a time. HTTP
 * requests don't go through that queue. Likewise, a retry no longer
 * replaces a handshake that's still in progress: upstream's retry
 * interval did, without closing it, and the abandoned socket stayed in
 * the queue until the browser's open timeout. The first reconnect after
 * an open connection drops (a hub restart, a force-reconnect below)
 * still goes straight to a WebSocket. A page whose CSP allows the
 * `ws:`/`wss:` URL but not its `http:`/`https:` twin can't probe, and
 * reconnects only through those direct attempts.
 *
 * No `visibilitychange` trigger: a merely-hidden tab has a live socket,
 * and a frozen tab already shows up as a wake gap. Triggers attach once
 * on connect() and are torn down on disconnect(); in Node (no `window`)
 * they do not exist. Deliberately no heartbeat — silent drops on an
 * awake machine (NAT/VPN idle timeouts) remain uncovered; see
 * claude-notes/plans/2026-09-28-ws-wake-reconnect.md for the trade-offs.
 */

import { BrowserWebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket';
import type { PeerId, PeerMetadata } from '@automerge/automerge-repo/slim';

import { syncLog } from './log.js';
import { recordConnectionEvent } from './sync-activity.js';

/** Wake-detection poll cadence (ms). Exported for tests. */
export const WAKE_CHECK_INTERVAL = 10_000;

/**
 * Wall-clock gap between wake-check ticks (ms) treated as "the machine
 * slept". Must stay above background-tab timer throttling: Chrome's
 * intensive throttling runs chained timers in hidden tabs about once a
 * minute, so a 60 s gap is normal and must not trigger. The cost: a
 * sleep shorter than ~90 s is missed — rarely fatal to the connection,
 * and an `online` event or a later wake still catches it. Exported for
 * tests.
 */
export const WAKE_GAP_THRESHOLD = 90_000;

/** True in a browser, where reconnects probe the server over HTTP first. */
function canProbe(): boolean {
  return (
    (globalThis as { window?: unknown }).window !== undefined && typeof globalThis.fetch === 'function'
  );
}

/**
 * True if anything answers an HTTP request to the WebSocket URL's
 * server within `timeoutMs`. Any response counts, even an error status
 * or an opaque one: a server that answers is up.
 */
async function serverAnswers(wsUrl: string, timeoutMs: number): Promise<boolean> {
  try {
    const url = new URL(wsUrl);
    url.protocol = url.protocol === 'wss:' ? 'https:' : 'http:';
    await fetch(url, { mode: 'no-cors', cache: 'no-store', signal: AbortSignal.timeout(timeoutMs) });
    return true;
  } catch {
    return false;
  }
}

export class StoppableWebSocketClientAdapter extends BrowserWebSocketClientAdapter {
  #stopped = false;

  // Reconnect probing (browser only; see the module comment).
  /** Whether the current socket ever opened. */
  #opened = false;
  #probing = false;

  // Force-reconnect trigger state (browser only — see #attachTriggers).
  #triggersAttached = false;
  #wakeIntervalId: ReturnType<typeof setInterval> | null = null;
  #lastWakeTickAt = 0;
  #onlineTarget: EventTarget | null = null;
  #connectionTarget: EventTarget | null = null;

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
    // A retry after an attempt that never opened. (Upstream's retry
    // interval keeps calling connect() until a socket opens.)
    if (this.socket && !this.#opened && canProbe()) {
      // Let a handshake in progress finish rather than abandon it.
      if (this.socket.readyState === WebSocket.CONNECTING) return;
      if (this.#probing) return;
      this.#probing = true;
      void serverAnswers(this.url, this.retryInterval).then((answers) => {
        this.#probing = false;
        if (this.#stopped) return;
        if (answers) this.#open(peerId, peerMetadata);
        else recordConnectionEvent('ws-probe-failed', this.url);
      });
      return;
    }
    this.#open(peerId, peerMetadata);
  }

  #open(peerId: PeerId, peerMetadata?: PeerMetadata): void {
    super.connect(peerId, peerMetadata);
    this.#opened = false;
    const socket = this.socket;
    socket?.addEventListener('open', () => {
      if (this.socket === socket) this.#opened = true;
    });
    this.#attachTriggers();
  }

  override disconnect(): void {
    this.#stopped = true;
    this.#detachTriggers();
    super.disconnect();
  }

  /**
   * Attach the wake/network triggers once. connect() runs again on
   * every reconnect via the parent's onClose, so this must be
   * idempotent. Skipped entirely off-browser: this adapter also runs in
   * Node (vitest, MCP-side tooling), where `window` does not exist.
   * `window` / `navigator.connection` are read here, at attach time,
   * not at module load — and probed via `globalThis` rather than the
   * bare names, because Node-side consumers of this package
   * (quarto-hub-mcp) compile it without the DOM lib.
   */
  #attachTriggers(): void {
    if (this.#triggersAttached) return;
    const win = (globalThis as { window?: EventTarget }).window;
    if (!win) return;
    this.#triggersAttached = true;
    this.#lastWakeTickAt = Date.now();
    this.#wakeIntervalId = setInterval(this.#checkWakeGap, WAKE_CHECK_INTERVAL);
    win.addEventListener('online', this.#onOnline);
    this.#onlineTarget = win;
    // `navigator.connection` (NetworkInformation) is Chromium-only and
    // catches some network switches that never toggle navigator.onLine.
    // It also fires on signal-quality fluctuations — a spurious but
    // cheap reconnect, accepted: the API offers no reliable "same
    // network" signal to filter on.
    const connection = (globalThis as { navigator?: { connection?: EventTarget } }).navigator
      ?.connection;
    if (connection && typeof connection.addEventListener === 'function') {
      connection.addEventListener('change', this.#onConnectionChange);
      this.#connectionTarget = connection;
    }
  }

  #detachTriggers(): void {
    if (this.#wakeIntervalId !== null) {
      clearInterval(this.#wakeIntervalId);
      this.#wakeIntervalId = null;
    }
    this.#onlineTarget?.removeEventListener('online', this.#onOnline);
    this.#onlineTarget = null;
    this.#connectionTarget?.removeEventListener('change', this.#onConnectionChange);
    this.#connectionTarget = null;
    this.#triggersAttached = false;
  }

  // Instance arrow properties so detach removes the same references.
  #onOnline = (): void => this.#forceReconnect('online');
  #onConnectionChange = (): void => this.#forceReconnect('network-change');

  /**
   * Wall-clock gap since the previous tick. `Date.now()` deliberately,
   * not `performance.now()`: the latter pauses during sleep on some
   * platforms and would hide the gap.
   */
  #checkWakeGap = (): void => {
    const now = Date.now();
    const gap = now - this.#lastWakeTickAt;
    this.#lastWakeTickAt = now;
    if (gap > WAKE_GAP_THRESHOLD) {
      this.#forceReconnect(`wake gap ${Math.round(gap / 1000)}s`);
    }
  };

  /**
   * Close the suspect socket and run upstream `onClose` directly:
   * emit `peer-disconnected` (the badge flips Offline) and schedule the
   * ordinary reconnect after `retryInterval`. `socket.close()` alone
   * only starts the closing handshake; on a black-holed connection the
   * `close` event waits out the browser's closing-handshake timeout
   * (tens of seconds), so we stop listening for it and drive `onClose`
   * ourselves. A false positive costs one re-sync of already-in-sync
   * documents — cheaper than today's recovery (a page refresh).
   *
   * The readyState guard coalesces trigger bursts: after the first
   * force-reconnect the socket is CLOSING, so a simultaneous `online` +
   * wake gap produces a single reconnect. A CONNECTING or CLOSED socket
   * means the retry path is already running.
   */
  #forceReconnect(reason: string): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    syncLog(`WebSocket force-reconnect: ${reason}`);
    recordConnectionEvent('ws-force-reconnect', reason);
    socket.removeEventListener('close', this.onClose); // ignore its late close
    socket.close();
    this.onClose();
  }
}
