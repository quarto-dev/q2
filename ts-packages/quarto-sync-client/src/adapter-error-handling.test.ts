/**
 * bd-xzspx4r9: a websocket 'error' event must never crash the process.
 *
 * Upstream automerge-repo's WebSocketClientAdapter.onError rethrows
 * any node error whose code isn't ECONNREFUSED. A throw inside an
 * event callback cannot reach any caller — it becomes an
 * uncaughtException and kills the host (the hub MCP server exits
 * mid-session on a mid-handshake 'socket hang up'). Recovery belongs
 * to the close/retry machinery; the error handler's only job is to
 * record the diagnostic.
 *
 * These tests invoke the handlers directly with the event shapes `ws`
 * produces on node.
 */

import { describe, it, expect } from 'vitest';

import { StoppableWebSocketClientAdapter } from './StoppableWebSocketClientAdapter.js';
import { NodeWebSocketClientAdapter } from './NodeWebSocketClientAdapter.js';

const RESET = { error: { code: 'ECONNRESET', message: 'socket hang up' } };
const REFUSED = { error: { code: 'ECONNREFUSED', message: 'connect ECONNREFUSED' } };
const DNS = { error: { code: 'ENOTFOUND', message: 'getaddrinfo ENOTFOUND nope.invalid' } };

describe('StoppableWebSocketClientAdapter.onError', () => {
  it('never throws, whatever the error code', () => {
    const adapter = new StoppableWebSocketClientAdapter('ws://127.0.0.1:9/ws');
    const onError = (adapter as unknown as { onError: (e: unknown) => void }).onError;
    expect(() => onError(RESET)).not.toThrow();
    expect(() => onError(REFUSED)).not.toThrow();
    expect(() => onError(DNS)).not.toThrow();
  });
});

describe('NodeWebSocketClientAdapter.onError', () => {
  it('never throws, whatever the error code', () => {
    const adapter = new NodeWebSocketClientAdapter('ws://127.0.0.1:9/ws', {
      getBearer: async () => 'token',
    });
    const onError = (adapter as unknown as { onError: (e: unknown) => void }).onError;
    expect(() => onError(RESET)).not.toThrow();
    expect(() => onError(REFUSED)).not.toThrow();
    expect(() => onError(DNS)).not.toThrow();
  });
});
