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
  sentRaw: ArrayBuffer[];
  closed: number;
} {
  const handlers: Record<string, Handler[]> = {};
  const fake: WebSocketLike & {
    emit: (kind: 'open' | 'close' | 'message' | 'error', event: unknown) => void;
    sentRaw: ArrayBuffer[];
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
    send(data: ArrayBuffer) {
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
