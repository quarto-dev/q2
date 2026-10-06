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

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  StoppableWebSocketClientAdapter,
  WAKE_CHECK_INTERVAL,
  WAKE_GAP_THRESHOLD,
} from './StoppableWebSocketClientAdapter.js';
import { getConnectionLog, resetSyncActivity } from './sync-activity.js';
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

// ---------------------------------------------------------------------------
// Force-reconnect on wake / network change (bd-sob0j19j).
//
// A half-open TCP connection (laptop sleep, network switch) delivers no
// close event, so upstream's reconnect-on-close never fires and the
// client stays "stale until refresh" while the badge says Online. The
// adapter's browser triggers — timer-drift wake detection, window
// `online`, `navigator.connection` `change` — close the suspect socket
// and run upstream `onClose` directly, reusing the reconnect path a hub
// restart already exercises.
// ---------------------------------------------------------------------------

/**
 * Longer than WAKE_CHECK_INTERVAL so a reconnect countdown and a wake
 * tick never collide inside one advanceTimersByTime window.
 */
const RETRY_MS = 20_000;

type SocketListener = (event?: unknown) => void;

/**
 * Controllable stand-in for the platform WebSocket. Never performs I/O:
 * state transitions and event delivery are test-driven, so a
 * black-holed socket (close() that never yields a close event) is
 * modelled directly. `instances` records construction order, which
 * makes reconnects countable.
 */
class FakeWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;

  static instances: FakeWebSocket[] = [];

  binaryType = 'arraybuffer';
  readyState: number = FakeWebSocket.CONNECTING;
  readonly sent: unknown[] = [];
  readonly close = vi.fn((): void => {
    // Black-hole by default: CLOSING, and no 'close' event ever follows.
    this.readyState = FakeWebSocket.CLOSING;
  });

  readonly #listeners = new Map<string, Set<SocketListener>>();

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this);
  }

  addEventListener(type: string, handler: SocketListener): void {
    let set = this.#listeners.get(type);
    if (!set) {
      set = new Set();
      this.#listeners.set(type, set);
    }
    set.add(handler);
  }

  removeEventListener(type: string, handler: SocketListener): void {
    this.#listeners.get(type)?.delete(handler);
  }

  send(data: unknown): void {
    this.sent.push(data);
  }

  // ── test controls ──────────────────────────────────────────────────

  /** Transition to OPEN and deliver the 'open' event. */
  open(): void {
    this.readyState = FakeWebSocket.OPEN;
    this.emit('open');
  }

  emit(type: string, event?: unknown): void {
    for (const handler of [...(this.#listeners.get(type) ?? [])]) handler(event);
  }

  listenerCount(type: string): number {
    return this.#listeners.get(type)?.size ?? 0;
  }
}

interface TriggerHarness {
  adapter: StoppableWebSocketClientAdapter;
  windowTarget: EventTarget;
  connectionTarget: EventTarget;
  socket: FakeWebSocket;
  peerDisconnected: ReturnType<typeof vi.fn>;
}

/**
 * Adapter mid-session: socket OPEN, handshake complete (remotePeerId
 * set, so onClose emits peer-disconnected), triggers attached to the
 * stubbed window / navigator.connection globals.
 */
function connectedAdapter(): TriggerHarness {
  const windowTarget = new EventTarget();
  const connectionTarget = new EventTarget();
  vi.stubGlobal('window', windowTarget);
  vi.stubGlobal('navigator', { connection: connectionTarget });
  vi.stubGlobal('WebSocket', FakeWebSocket);

  const adapter = new StoppableWebSocketClientAdapter(DEAD_URL, RETRY_MS);
  const peerDisconnected = vi.fn();
  adapter.on('peer-disconnected', peerDisconnected);
  adapter.connect('peer-test' as PeerId, {});
  const socket = FakeWebSocket.instances[0]!;
  socket.open(); // establish: clears upstream's retry interval, joins
  adapter.remotePeerId = 'hub-peer' as PeerId;
  return { adapter, windowTarget, connectionTarget, socket, peerDisconnected };
}

function forceReconnectEvents(): ReturnType<typeof getConnectionLog> {
  return getConnectionLog().filter((e) => e.kind === 'ws-force-reconnect');
}

describe('StoppableWebSocketClientAdapter wake/network force-reconnect', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetSyncActivity();
    FakeWebSocket.instances = [];
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('force-reconnects an OPEN socket when a wake gap exceeds the threshold', () => {
    const { socket, peerDisconnected } = connectedAdapter();

    // Simulate sleep: the wall clock jumps past the threshold while the
    // 10 s wake-check timer does not run.
    vi.setSystemTime(Date.now() + WAKE_GAP_THRESHOLD + 30_000);
    vi.advanceTimersByTime(WAKE_CHECK_INTERVAL);

    // The suspect socket was closed and upstream onClose ran directly:
    // peer drop is visible, reconnect scheduled after retryInterval.
    expect(socket.close).toHaveBeenCalledTimes(1);
    expect(socket.readyState).toBe(FakeWebSocket.CLOSING);
    expect(peerDisconnected).toHaveBeenCalledTimes(1);
    expect(peerDisconnected).toHaveBeenCalledWith({ peerId: 'hub-peer' });
    const events = forceReconnectEvents();
    expect(events).toHaveLength(1);
    expect(events[0]!.detail).toMatch(/^wake gap \d+s$/);

    expect(FakeWebSocket.instances).toHaveLength(1);
    vi.advanceTimersByTime(RETRY_MS - 1);
    expect(FakeWebSocket.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeWebSocket.instances).toHaveLength(2);
    expect(FakeWebSocket.instances[1]).not.toBe(socket);
  });

  it('ignores normal ticks and a ~60 s throttled-tab gap', () => {
    const { socket, peerDisconnected } = connectedAdapter();

    // On-time ticks: 10 s gaps must never trigger.
    vi.advanceTimersByTime(3 * WAKE_CHECK_INTERVAL);
    expect(socket.close).not.toHaveBeenCalled();

    // Chrome's intensive throttling runs hidden-tab timers about once a
    // minute — a 60 s gap is normal and must NOT trigger.
    vi.setSystemTime(Date.now() + 50_000);
    vi.advanceTimersByTime(WAKE_CHECK_INTERVAL); // 60 s since last tick

    expect(socket.close).not.toHaveBeenCalled();
    expect(peerDisconnected).not.toHaveBeenCalled();
    expect(forceReconnectEvents()).toHaveLength(0);
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it('force-reconnects on the window online event', () => {
    const { windowTarget, socket, peerDisconnected } = connectedAdapter();

    windowTarget.dispatchEvent(new Event('online'));

    expect(socket.close).toHaveBeenCalledTimes(1);
    expect(peerDisconnected).toHaveBeenCalledTimes(1);
    const events = forceReconnectEvents();
    expect(events).toHaveLength(1);
    expect(events[0]!.detail).toBe('online');

    vi.advanceTimersByTime(RETRY_MS);
    expect(FakeWebSocket.instances).toHaveLength(2);
  });

  it('force-reconnects on navigator.connection change (Chromium network switch)', () => {
    const { connectionTarget, socket } = connectedAdapter();

    connectionTarget.dispatchEvent(new Event('change'));

    expect(socket.close).toHaveBeenCalledTimes(1);
    const events = forceReconnectEvents();
    expect(events).toHaveLength(1);
    expect(events[0]!.detail).toBe('network-change');
  });

  const nonOpenStates: Array<[string, number]> = [
    ['CONNECTING', FakeWebSocket.CONNECTING],
    ['CLOSING', FakeWebSocket.CLOSING],
    ['CLOSED', FakeWebSocket.CLOSED],
  ];
  for (const [label, readyState] of nonOpenStates) {
    it(`is a no-op when the socket is ${label} at trigger time`, () => {
      const { windowTarget, socket, peerDisconnected } = connectedAdapter();
      socket.readyState = readyState;

      windowTarget.dispatchEvent(new Event('online'));

      // The retry path is already running (or the socket is gone):
      // triggers must not pile a second reconnect on top.
      expect(socket.close).not.toHaveBeenCalled();
      expect(peerDisconnected).not.toHaveBeenCalled();
      expect(forceReconnectEvents()).toHaveLength(0);
      vi.advanceTimersByTime(RETRY_MS);
      expect(FakeWebSocket.instances).toHaveLength(1);
    });
  }

  it('coalesces back-to-back triggers into exactly one reconnect', () => {
    const { windowTarget, socket, peerDisconnected } = connectedAdapter();

    // A resume often fires `online` and exposes the wake gap together.
    windowTarget.dispatchEvent(new Event('online'));
    vi.setSystemTime(Date.now() + WAKE_GAP_THRESHOLD + 30_000);
    vi.advanceTimersByTime(WAKE_CHECK_INTERVAL);

    // The first trigger moved the socket to CLOSING; the wake-gap
    // trigger saw a non-OPEN socket and was a no-op.
    expect(socket.close).toHaveBeenCalledTimes(1);
    expect(peerDisconnected).toHaveBeenCalledTimes(1);
    expect(forceReconnectEvents()).toHaveLength(1);

    vi.advanceTimersByTime(RETRY_MS);
    expect(FakeWebSocket.instances).toHaveLength(2);
  });

  it('reconnects without waiting for the old socket close event; a late close schedules no second reconnect', () => {
    const { windowTarget, socket, peerDisconnected } = connectedAdapter();

    windowTarget.dispatchEvent(new Event('online'));

    // The adapter stopped listening for the old socket's close: on a
    // black-holed connection that event may never arrive (the browser's
    // closing-handshake timeout is tens of seconds).
    expect(socket.listenerCount('close')).toBe(0);

    // The late close, when it eventually arrives, is ignored.
    socket.emit('close');

    // Exactly one reconnect, after retryInterval — a second scheduled
    // reconnect would construct a third socket at the same instant.
    expect(FakeWebSocket.instances).toHaveLength(1);
    vi.advanceTimersByTime(RETRY_MS - 1);
    expect(FakeWebSocket.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeWebSocket.instances).toHaveLength(2);
    expect(peerDisconnected).toHaveBeenCalledTimes(1);
  });

  it('disconnect() removes the triggers; later wake/online/change are no-ops', () => {
    const { adapter, windowTarget, connectionTarget, socket, peerDisconnected } =
      connectedAdapter();
    const windowRemove = vi.spyOn(windowTarget, 'removeEventListener');
    const connectionRemove = vi.spyOn(connectionTarget, 'removeEventListener');

    adapter.disconnect();
    socket.close.mockClear();
    peerDisconnected.mockClear(); // disconnect() itself emits one

    expect(windowRemove).toHaveBeenCalledWith('online', expect.any(Function));
    expect(connectionRemove).toHaveBeenCalledWith('change', expect.any(Function));
    // The wake-check interval is gone (upstream disconnect clears its
    // own timers; nothing may remain).
    expect(vi.getTimerCount()).toBe(0);

    vi.setSystemTime(Date.now() + WAKE_GAP_THRESHOLD + 30_000);
    vi.advanceTimersByTime(3 * WAKE_CHECK_INTERVAL);
    windowTarget.dispatchEvent(new Event('online'));
    connectionTarget.dispatchEvent(new Event('change'));

    expect(socket.close).not.toHaveBeenCalled();
    expect(peerDisconnected).not.toHaveBeenCalled();
    expect(forceReconnectEvents()).toHaveLength(0);
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it('attaches no triggers without a window global (Node)', () => {
    // window is NOT stubbed: the default vitest Node environment.
    vi.stubGlobal('WebSocket', FakeWebSocket);
    const adapter = new StoppableWebSocketClientAdapter(DEAD_URL, RETRY_MS);
    adapter.connect('peer-test' as PeerId, {});
    const socket = FakeWebSocket.instances[0]!;
    socket.open();
    adapter.remotePeerId = 'hub-peer' as PeerId;

    // Upstream's one-shot force-ready timeout is the only live timer;
    // no 10 s wake-check interval exists.
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(2000);
    expect(vi.getTimerCount()).toBe(0);

    // A wall-clock jump therefore changes nothing.
    vi.setSystemTime(Date.now() + WAKE_GAP_THRESHOLD + 30_000);
    vi.advanceTimersByTime(3 * WAKE_CHECK_INTERVAL);
    expect(socket.close).not.toHaveBeenCalled();
    expect(forceReconnectEvents()).toHaveLength(0);
    expect(FakeWebSocket.instances).toHaveLength(1);
    adapter.disconnect();
  });
});
