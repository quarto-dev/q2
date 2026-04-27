/**
 * Unit tests for LoggingNetworkAdapter.
 *
 * Verifies that the wrapper observes outgoing sends, forwards incoming
 * messages, tracks the remote peer across connect/disconnect, and respects
 * the StrictMode double-mount guard on disconnect.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { NetworkAdapter, type Message, type PeerId } from '@automerge/automerge-repo'
import { LoggingNetworkAdapter } from './LoggingNetworkAdapter'
import type { MessageLogEntry } from '../types/messages'

class StubAdapter extends NetworkAdapter {
  sent: Message[] = []
  disconnectCalls = 0
  connectCalls: Array<{ peerId: PeerId; metadata?: unknown }> = []

  isReady() {
    return true
  }
  whenReady() {
    return Promise.resolve()
  }
  connect(peerId: PeerId, peerMetadata?: unknown) {
    this.connectCalls.push({ peerId, metadata: peerMetadata })
  }
  send(message: Message) {
    this.sent.push(message)
  }
  disconnect() {
    this.disconnectCalls++
  }
}

function buildMessage(overrides: Partial<Message> = {}): Message {
  return {
    type: 'sync',
    senderId: 'peer-a' as PeerId,
    targetId: 'peer-b' as PeerId,
    documentId: 'doc-123' as Message['documentId'],
    data: new Uint8Array([1, 2, 3, 4]),
    ...overrides,
  } as Message
}

describe('LoggingNetworkAdapter', () => {
  let stub: StubAdapter
  let messages: MessageLogEntry[]
  let connections: Array<{ connected: boolean; remote?: PeerId }>
  let adapter: LoggingNetworkAdapter

  beforeEach(() => {
    stub = new StubAdapter()
    messages = []
    connections = []
    adapter = new LoggingNetworkAdapter(
      stub,
      (entry) => messages.push(entry),
      (connected, remote) => connections.push({ connected, remote }),
    )
  })

  it('forwards outgoing sends and logs them as outgoing', () => {
    const msg = buildMessage()
    adapter.send(msg)

    expect(stub.sent).toEqual([msg])
    expect(messages).toHaveLength(1)
    expect(messages[0]).toMatchObject({
      direction: 'outgoing',
      type: 'sync',
      senderId: 'peer-a',
      targetId: 'peer-b',
      documentId: 'doc-123',
      dataSize: 4,
    })
  })

  it('intercepts incoming messages emitted by the wrapped adapter', () => {
    const seen = vi.fn()
    adapter.on('message', seen)

    const msg = buildMessage({ type: 'request' })
    stub.emit('message', msg)

    expect(messages).toHaveLength(1)
    expect(messages[0]).toMatchObject({ direction: 'incoming', type: 'request' })
    expect(seen).toHaveBeenCalledWith(msg)
  })

  it('tracks the remote peer on peer-candidate and clears it on disconnect', () => {
    const peerId = 'server-xyz' as PeerId
    stub.emit('peer-candidate', { peerId, peerMetadata: {} })

    expect(adapter.remotePeerId).toBe(peerId)
    expect(connections).toEqual([{ connected: true, remote: peerId }])

    stub.emit('peer-disconnected', { peerId })
    expect(adapter.remotePeerId).toBeUndefined()
    expect(connections).toEqual([
      { connected: true, remote: peerId },
      { connected: false, remote: undefined },
    ])
  })

  it('ignores peer-disconnected for a peer that is not the tracked remote', () => {
    stub.emit('peer-candidate', { peerId: 'server-1' as PeerId, peerMetadata: {} })
    connections.length = 0

    stub.emit('peer-disconnected', { peerId: 'server-2' as PeerId })
    expect(connections).toHaveLength(0)
    expect(adapter.remotePeerId).toBe('server-1')
  })

  it('does not call wrapped.disconnect when connect() was never invoked (StrictMode guard)', () => {
    adapter.disconnect()
    expect(stub.disconnectCalls).toBe(0)
  })

  it('forwards disconnect after connect() has been called', () => {
    adapter.connect('local-peer' as PeerId)
    expect(stub.connectCalls).toHaveLength(1)

    adapter.disconnect()
    expect(stub.disconnectCalls).toBe(1)
  })
})
