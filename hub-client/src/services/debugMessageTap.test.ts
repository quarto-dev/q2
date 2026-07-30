/**
 * @vitest-environment jsdom
 *
 * Tests for the in-context sync-message tap (bd-6ogrov5r; plan:
 * claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md).
 *
 * The tap registers a network-adapter wrapper with quarto-sync-client
 * (mocked here — its application at Repo construction is tested in
 * that package's network-wrapper.test.ts) and keeps a ring buffer of
 * message summaries for `quartoDebug.am.messages()`.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import {
  NetworkAdapter,
  type Message,
  type PeerId,
} from '@automerge/automerge-repo';

const syncClientMocks = vi.hoisted(() => ({
  setNetworkAdapterWrapper: vi.fn<(fn: unknown) => void>(),
}));

vi.mock('@quarto/quarto-sync-client', () => syncClientMocks);

import {
  installMessageTap,
  uninstallMessageTap,
  getTapMessages,
  getTapStatus,
} from './debugMessageTap';

class StubAdapter extends NetworkAdapter {
  sent: Message[] = [];
  isReady() {
    return true;
  }
  whenReady() {
    return Promise.resolve();
  }
  connect() {}
  send(message: Message) {
    this.sent.push(message);
  }
  disconnect() {}
}

function buildMessage(overrides: Partial<Message> = {}): Message {
  return {
    type: 'sync',
    senderId: 'peer-a' as PeerId,
    targetId: 'peer-b' as PeerId,
    documentId: 'doc-123' as Message['documentId'],
    data: new Uint8Array([104, 105]), // "hi"
    ...overrides,
  } as Message;
}

/** Install the tap and run its wrapper over a stub adapter. */
function installAndWrap(opts?: Parameters<typeof installMessageTap>[0]): {
  stub: StubAdapter;
  wrapped: NetworkAdapter;
} {
  installMessageTap(opts);
  const wrapper = syncClientMocks.setNetworkAdapterWrapper.mock.lastCall![0] as (
    a: NetworkAdapter,
  ) => NetworkAdapter;
  const stub = new StubAdapter();
  return { stub, wrapped: wrapper(stub) };
}

beforeEach(() => {
  vi.clearAllMocks();
  uninstallMessageTap();
});

describe('installMessageTap', () => {
  it('registers a wrapper and reports installed status', () => {
    installMessageTap();
    expect(syncClientMocks.setNetworkAdapterWrapper).toHaveBeenCalledWith(
      expect.any(Function),
    );
    expect(getTapStatus()).toEqual({
      installed: true,
      capture: 'summary',
      limit: 500,
      recorded: 0,
      dropped: 0,
      attached: false,
    });
  });

  it('records summaries of traffic through the wrapped adapter', () => {
    const { stub, wrapped } = installAndWrap();
    expect(getTapStatus().attached).toBe(true);

    wrapped.send(buildMessage());
    stub.emit('message', buildMessage({ type: 'ephemeral' }));

    const messages = getTapMessages();
    expect(messages).toHaveLength(2);
    // Newest first.
    expect(messages[0].direction).toBe('incoming');
    expect(messages[0].type).toBe('ephemeral');
    expect(messages[1]).toEqual({
      at: expect.any(Number),
      direction: 'outgoing',
      type: 'sync',
      senderId: 'peer-a',
      targetId: 'peer-b',
      documentId: 'doc-123',
      byteLength: 2,
    });
    // Summary mode: no payloads anywhere.
    expect(messages.every((m) => m.data === undefined)).toBe(true);
    // Traffic still reached the wrapped adapter.
    expect(stub.sent).toHaveLength(1);
  });

  it('captures base64 payloads in full mode', () => {
    const { wrapped } = installAndWrap({ capture: 'full' });
    wrapped.send(buildMessage());
    expect(getTapMessages()[0].data).toBe('aGk='); // base64("hi")
    expect(getTapStatus().capture).toBe('full');
  });

  it('evicts oldest entries beyond the ring limit and counts drops', () => {
    const { wrapped } = installAndWrap({ limit: 3 });
    for (let i = 0; i < 5; i++) {
      wrapped.send(buildMessage({ type: `t${i}` } as Partial<Message>));
    }
    const messages = getTapMessages();
    expect(messages.map((m) => m.type)).toEqual(['t4', 't3', 't2']);
    expect(getTapStatus()).toMatchObject({
      recorded: 5,
      dropped: 2,
      limit: 3,
    });
  });

  it('filters by type and caps via limit', () => {
    const { stub, wrapped } = installAndWrap();
    wrapped.send(buildMessage());
    stub.emit('message', buildMessage({ type: 'ephemeral' }));
    wrapped.send(buildMessage());

    expect(getTapMessages({ type: 'sync' })).toHaveLength(2);
    expect(getTapMessages({ limit: 1 })).toHaveLength(1);
    expect(getTapMessages({ limit: 1 })[0].type).toBe('sync');
  });

  it('re-install resets the ring', () => {
    const { wrapped } = installAndWrap();
    wrapped.send(buildMessage());
    expect(getTapMessages()).toHaveLength(1);

    installMessageTap();
    expect(getTapMessages()).toEqual([]);
    expect(getTapStatus().recorded).toBe(0);
  });

  it('uninstall clears the wrapper but keeps messages for post-mortem', () => {
    const { wrapped } = installAndWrap();
    wrapped.send(buildMessage());

    uninstallMessageTap();
    expect(syncClientMocks.setNetworkAdapterWrapper).toHaveBeenLastCalledWith(null);
    expect(getTapStatus().installed).toBe(false);
    expect(getTapMessages()).toHaveLength(1);
  });
});
