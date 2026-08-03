/**
 * Tests for {@link NodeWebSocketClientAdapter} — the Node-only sync
 * WebSocket adapter that threads `Authorization: Bearer <token>` into
 * the upgrade request.
 *
 * Tokens are injected via an async `getBearer` getter (so the retry
 * loop sees a freshly-refreshed token) and the WebSocket factory is
 * a test seam, so these specs never open a real socket.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import * as net from 'node:net';
import type { AddressInfo } from 'node:net';
import { cbor } from '@automerge/automerge-repo/slim';
import type { PeerId } from '@automerge/automerge-repo/slim';

import {
  NodeWebSocketClientAdapter,
  redactAuthorization,
  type WebSocketFactory,
  type WebSocketLike,
} from './NodeWebSocketClientAdapter.js';

type Handler = (event: unknown) => void;

/** Minimal in-memory WebSocket fake the adapter can drive. */
function makeFakeSocket(): WebSocketLike & {
  emit: (kind: 'open' | 'close' | 'message' | 'error', event: unknown) => void;
  sentRaw: Uint8Array[];
  closed: number;
} {
  const handlers: Record<string, Handler[]> = {};
  const fake: WebSocketLike & {
    emit: (kind: 'open' | 'close' | 'message' | 'error', event: unknown) => void;
    sentRaw: Uint8Array[];
    closed: number;
  } = {
    readyState: 1, // OPEN
    binaryType: '',
    addEventListener(type, handler) {
      (handlers[type] ??= []).push(handler);
    },
    removeEventListener(type, handler) {
      const list = handlers[type];
      if (!list) return;
      const i = list.indexOf(handler);
      if (i >= 0) list.splice(i, 1);
    },
    close() {
      this.closed += 1;
    },
    send(data: Uint8Array) {
      this.sentRaw.push(data);
    },
    emit(kind, event) {
      for (const h of handlers[kind] ?? []) h(event);
    },
    sentRaw: [],
    closed: 0,
  };
  return fake;
}

interface FactoryCall {
  url: string;
  protocols: readonly string[];
  options: { headers: Record<string, string> };
}

function makeFactory(socket: WebSocketLike): {
  factory: WebSocketFactory;
  calls: FactoryCall[];
} {
  const calls: FactoryCall[] = [];
  const factory: WebSocketFactory = (url, protocols, options) => {
    calls.push({ url, protocols, options });
    return socket;
  };
  return { factory, calls };
}

const peerId = 'test-peer' as PeerId;

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('NodeWebSocketClientAdapter', () => {
  it('passes the Authorization header to the WebSocket factory on connect', async () => {
    const socket = makeFakeSocket();
    const { factory, calls } = makeFactory(socket);
    const getBearer = vi.fn().mockResolvedValue('test-token-abc');

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer,
      webSocketFactory: factory,
      retryInterval: 0,
    });

    adapter.connect(peerId);
    // Drain the microtask queue for openSocket()'s await.
    await vi.runOnlyPendingTimersAsync();

    expect(getBearer).toHaveBeenCalledOnce();
    expect(calls).toHaveLength(1);
    expect(calls[0]!.url).toBe('wss://hub.example.com/ws');
    expect(calls[0]!.options.headers).toEqual({
      Authorization: 'Bearer test-token-abc',
    });
    // Protocols list is empty (parity with upstream's `new WebSocket(url)`).
    expect(calls[0]!.protocols).toEqual([]);
  });

  it('does not write the token to any console sink', async () => {
    const logSpy = vi.spyOn(console, 'log').mockImplementation(() => undefined);
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const debugSpy = vi.spyOn(console, 'debug').mockImplementation(() => undefined);

    const socket = makeFakeSocket();
    const { factory } = makeFactory(socket);
    const token = 'ya29.SECRET-TOKEN-VALUE';
    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer: async () => token,
      webSocketFactory: factory,
      retryInterval: 0,
    });

    adapter.connect(peerId);
    await vi.runOnlyPendingTimersAsync();
    // Drive an open + a fake error to exercise the error path.
    socket.emit('open', {});

    const all = [logSpy, warnSpy, errorSpy, debugSpy].flatMap((spy) =>
      spy.mock.calls.flat().map(String),
    );
    for (const line of all) {
      expect(line).not.toContain(token);
      expect(line).not.toContain('Bearer ');
    }
  });

  it('redacts Authorization values in arbitrary strings', () => {
    const raw = 'request failed with headers: Authorization: Bearer ya29.SOMETOKEN.AAAA';
    const redacted = redactAuthorization(raw);
    expect(redacted).toBe('request failed with headers: Authorization: [redacted]');
    expect(redacted).not.toContain('ya29');
  });

  it('disconnect is terminal: the onClose-scheduled reconnect cannot resurrect the adapter (bd-jit6pdwq)', async () => {
    // The onClose handler schedules `setTimeout(connect, retryInterval)`
    // which disconnect() cannot cancel. Without the stopped-flag gate,
    // a discarded adapter reconnects to a dead endpoint forever.
    const socket1 = makeFakeSocket();
    const socket2 = makeFakeSocket();
    const sockets = [socket1, socket2];
    const calls: FactoryCall[] = [];
    const factory: WebSocketFactory = (url, protocols, options) => {
      calls.push({ url, protocols, options });
      return sockets.shift()!;
    };

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer: async () => 'tok',
      webSocketFactory: factory,
      retryInterval: 1000,
    });

    adapter.connect(peerId);
    // 0 ms advance flushes the async socket creation WITHOUT firing
    // the 1000 ms retry interval (runOnlyPendingTimersAsync would).
    await vi.advanceTimersByTimeAsync(0);
    expect(calls.length).toBe(1);

    // Server dies: the socket closes, scheduling the zombie timer…
    socket1.emit('close', {});
    // …and the owner discards the adapter.
    adapter.disconnect();

    // The zombie timer (and the retry interval) fire — repeatedly.
    await vi.advanceTimersByTimeAsync(5000);

    expect(calls.length).toBe(1); // no resurrection
  });

  it('returns the same fresh token on each reconnect (getBearer called each time)', async () => {
    const socket1 = makeFakeSocket();
    const socket2 = makeFakeSocket();
    const sockets = [socket1, socket2];
    const calls: FactoryCall[] = [];
    const factory: WebSocketFactory = (url, protocols, options) => {
      calls.push({ url, protocols, options });
      return sockets.shift()!;
    };

    const tokens = ['token-1', 'token-2'];
    const getBearer = vi.fn().mockImplementation(async () => tokens.shift()!);

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer,
      webSocketFactory: factory,
      retryInterval: 0,
    });

    adapter.connect(peerId);
    await vi.runOnlyPendingTimersAsync();

    // Trigger a second connect (simulating upstream's reconnect path).
    adapter.connect(peerId);
    await vi.runOnlyPendingTimersAsync();

    expect(getBearer).toHaveBeenCalledTimes(2);
    expect(calls[0]!.options.headers).toEqual({ Authorization: 'Bearer token-1' });
    expect(calls[1]!.options.headers).toEqual({ Authorization: 'Bearer token-2' });
  });

  it('skips socket creation when getBearer rejects (retry will try again)', async () => {
    const socket = makeFakeSocket();
    const { factory, calls } = makeFactory(socket);
    const getBearer = vi.fn().mockRejectedValueOnce(new Error('refresh failed'));

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer,
      webSocketFactory: factory,
      retryInterval: 0,
    });

    adapter.connect(peerId);
    await vi.runOnlyPendingTimersAsync();

    expect(getBearer).toHaveBeenCalledOnce();
    expect(calls).toHaveLength(0); // no socket was constructed
  });
});

// ---------------------------------------------------------------------------
// Auth-rejection evidence (bd-l3b1brn8)
// ---------------------------------------------------------------------------
//
// The adapter reports *definitive* auth evidence — a 401/403 upgrade
// status surfaced by the factory's optional capability, or a terminal
// refresh failure (`ReauthRequired`-named error from getBearer) — via
// `onAuthRejected`, debounced to one report per failure episode (an
// episode ends at the next successful peer handshake). Network errors
// never fire it. Policy (refresh, invalidate, user messaging) lives in
// hub-mcp's connection manager, not here.

/** Fake-socket factory whose calls expose the upgrade-status capability. */
interface StatusFactoryCall extends FactoryCall {
  options: {
    headers: Record<string, string>;
    onUpgradeStatus?: (status: number) => void;
  };
}

function makeStatusFactory(sockets: WebSocketLike[]): {
  factory: WebSocketFactory;
  calls: StatusFactoryCall[];
} {
  const queue = [...sockets];
  const calls: StatusFactoryCall[] = [];
  const factory: WebSocketFactory = (url, protocols, options) => {
    calls.push({ url, protocols, options } as StatusFactoryCall);
    return queue.shift() ?? makeFakeSocket();
  };
  return { factory, calls };
}

/** Encoded server `peer` message — completes the sync handshake. */
function peerMessageEvent(): { data: Uint8Array } {
  const cborApi = cbor as { encode(value: unknown): Uint8Array };
  return {
    data: cborApi.encode({
      type: 'peer',
      senderId: 'server-peer',
      peerMetadata: {},
    }),
  };
}

describe('NodeWebSocketClientAdapter auth-rejection evidence', () => {
  it('reports an upgrade 401 once per failure episode, resetting on a successful handshake', async () => {
    const s1 = makeFakeSocket();
    const s2 = makeFakeSocket();
    const s3 = makeFakeSocket();
    const s4 = makeFakeSocket();
    const { factory, calls } = makeStatusFactory([s1, s2, s3, s4]);
    const onAuthRejected = vi.fn();

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer: async () => 'tok',
      webSocketFactory: factory,
      retryInterval: 1000,
      onAuthRejected,
    });

    adapter.connect(peerId);
    await vi.advanceTimersByTimeAsync(0);
    expect(calls).toHaveLength(1);
    expect(calls[0]!.options.onUpgradeStatus).toBeDefined();

    // Attempt 1 fails the upgrade with 401 (ws fires no close/error when
    // the unexpected-response capability is consumed).
    calls[0]!.options.onUpgradeStatus!(401);
    expect(onAuthRejected).toHaveBeenCalledTimes(1);
    expect(onAuthRejected).toHaveBeenCalledWith({
      kind: 'upgrade-status',
      status: 401,
    });

    // Retry interval fires attempt 2 — also 401. Same episode: no new report.
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls).toHaveLength(2);
    calls[1]!.options.onUpgradeStatus!(401);
    expect(onAuthRejected).toHaveBeenCalledTimes(1);

    // Attempt 3 succeeds: open + server `peer` message ends the episode.
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls).toHaveLength(3);
    s3.emit('open', {});
    s3.emit('message', peerMessageEvent());

    // Hub closes the live socket; the one-shot reconnect gets 401 again —
    // a NEW episode, so a second report fires.
    s3.emit('close', {});
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls).toHaveLength(4);
    calls[3]!.options.onUpgradeStatus!(401);
    expect(onAuthRejected).toHaveBeenCalledTimes(2);
  });

  it('keeps retrying after a mid-session 403 that fires no close event (ws unexpected-response shape)', async () => {
    const s1 = makeFakeSocket();
    const s2 = makeFakeSocket();
    const s3 = makeFakeSocket();
    const { factory, calls } = makeStatusFactory([s1, s2, s3]);
    const onAuthRejected = vi.fn();

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer: async () => 'tok',
      webSocketFactory: factory,
      retryInterval: 1000,
      onAuthRejected,
    });

    // Live session: open + peer.
    adapter.connect(peerId);
    await vi.advanceTimersByTimeAsync(0);
    s1.emit('open', {}); // clears the retry interval
    s1.emit('message', peerMessageEvent());

    // Hub closes the socket (e.g. restart after a ban). The one-shot
    // reconnect's upgrade is refused 403 — and per ws semantics with an
    // unexpected-response listener, NO close/error fires on s2.
    s1.emit('close', {});
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls).toHaveLength(2);
    calls[1]!.options.onUpgradeStatus!(403);
    expect(onAuthRejected).toHaveBeenCalledTimes(1);
    expect(onAuthRejected).toHaveBeenCalledWith({
      kind: 'upgrade-status',
      status: 403,
    });

    // Retry continuity rests on the interval connect() re-created for the
    // reconnect — a further attempt must still happen with a fresh token.
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls.length).toBeGreaterThanOrEqual(3);
  });

  it('never reports on plain network close/error and keeps retrying', async () => {
    const s1 = makeFakeSocket();
    const s2 = makeFakeSocket();
    const { factory, calls } = makeStatusFactory([s1, s2]);
    const onAuthRejected = vi.fn();

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer: async () => 'tok',
      webSocketFactory: factory,
      retryInterval: 1000,
      onAuthRejected,
    });

    adapter.connect(peerId);
    await vi.advanceTimersByTimeAsync(0);
    s1.emit('error', { error: { code: 'ECONNREFUSED', message: 'refused' } });
    s1.emit('close', {});

    await vi.advanceTimersByTimeAsync(1000);
    expect(calls.length).toBeGreaterThanOrEqual(2); // still retrying
    expect(onAuthRejected).not.toHaveBeenCalled();
  });

  it('reports token-refresh-terminal and stops the retry loop when getBearer throws a ReauthRequired-named error', async () => {
    const { factory, calls } = makeStatusFactory([]);
    const reauth = new Error('credentials revoked');
    reauth.name = 'ReauthRequired';
    const getBearer = vi.fn().mockRejectedValue(reauth);
    const onAuthRejected = vi.fn();

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer,
      webSocketFactory: factory,
      retryInterval: 1000,
      onAuthRejected,
    });

    adapter.connect(peerId);
    await vi.advanceTimersByTimeAsync(0);
    expect(onAuthRejected).toHaveBeenCalledTimes(1);
    expect(onAuthRejected).toHaveBeenCalledWith({ kind: 'token-refresh-terminal' });
    expect(calls).toHaveLength(0); // no socket constructed

    // The retry loop is stopped: no further getBearer calls, ever.
    const before = getBearer.mock.calls.length;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(getBearer.mock.calls.length).toBe(before);
  });

  it('treats TokenRefreshError-named failures as transient: no report, retry continues', async () => {
    const { factory } = makeStatusFactory([]);
    const transient = new Error('IdP hiccup');
    transient.name = 'TokenRefreshError';
    const getBearer = vi.fn().mockRejectedValue(transient);
    const onAuthRejected = vi.fn();

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer,
      webSocketFactory: factory,
      retryInterval: 1000,
      onAuthRejected,
    });

    adapter.connect(peerId);
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(2000);

    expect(getBearer.mock.calls.length).toBeGreaterThanOrEqual(3);
    expect(onAuthRejected).not.toHaveBeenCalled();
  });

  it('degrades to today’s behavior with a factory that lacks the status capability', async () => {
    const socket = makeFakeSocket();
    const { factory, calls } = makeFactory(socket); // ignores onUpgradeStatus
    const onAuthRejected = vi.fn();

    const adapter = new NodeWebSocketClientAdapter('wss://hub.example.com/ws', {
      getBearer: async () => 'tok',
      webSocketFactory: factory,
      retryInterval: 1000,
      onAuthRejected,
    });

    adapter.connect(peerId);
    await vi.advanceTimersByTimeAsync(0);
    expect(calls).toHaveLength(1);
    // A failed upgrade folds into a generic error/close, exactly as today.
    socket.emit('error', { error: { code: 'ECONNRESET', message: 'reset' } });
    socket.emit('close', {});
    await vi.advanceTimersByTimeAsync(1000);

    expect(onAuthRejected).not.toHaveBeenCalled(); // no evidence, no report
  });
});

// ---------------------------------------------------------------------------
// Default `ws` factory: unexpected-response must not leak connections
// ---------------------------------------------------------------------------
//
// With an 'unexpected-response' listener attached, ws@8 skips its own
// abortHandshake — no error/close fires and the HTTP request stays open
// unless the listener aborts it. These specs run the REAL `ws` package
// against a local HTTP server that answers upgrades with 403 and keeps
// the TCP connection open (keep-alive), so a leak is observable as a
// lingering server-side socket.

describe('default ws factory unexpected-response handling (real sockets)', () => {
  beforeEach(() => {
    vi.useRealTimers();
  });

  it('surfaces the 403, aborts each failed handshake, and leaks no connections', async () => {
    // Raw net server (not http.Server): after an 'upgrade' handoff the
    // http server never reads the socket again, so a client FIN would
    // sit unobserved and 'close' would not fire — a net server reads
    // the bytes itself and sees every close. It answers any request
    // with 403 and does NOT close: a keep-alive server leaves closing
    // to the client — exactly where the leak would appear.
    const live = new Set<import('node:net').Socket>();
    const server = net.createServer((socket) => {
      live.add(socket);
      socket.on('close', () => live.delete(socket));
      socket.on('error', () => undefined);
      socket.on('data', () => {
        socket.write('HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n');
      });
    });
    await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
    const port = (server.address() as AddressInfo).port;

    const getBearer = vi.fn().mockResolvedValue('tok');
    const onAuthRejected = vi.fn();
    const adapter = new NodeWebSocketClientAdapter(`ws://127.0.0.1:${port}/ws`, {
      getBearer,
      retryInterval: 50, // real default factory: webSocketFactory omitted
      onAuthRejected,
    });

    try {
      adapter.connect(peerId);
      // Let several retry attempts run.
      await vi.waitFor(
        () => {
          expect(getBearer.mock.calls.length).toBeGreaterThanOrEqual(3);
        },
        { timeout: 5000 },
      );

      expect(onAuthRejected).toHaveBeenCalledTimes(1);
      expect(onAuthRejected).toHaveBeenCalledWith({
        kind: 'upgrade-status',
        status: 403,
      });

      adapter.disconnect();
      // Every failed attempt must have aborted its handshake: the server
      // sees each connection close.
      await vi.waitFor(
        () => {
          expect(live.size).toBe(0);
        },
        { timeout: 5000 },
      );
    } finally {
      adapter.disconnect();
      await new Promise<void>((r) => server.close(() => r()));
    }
  });
});
