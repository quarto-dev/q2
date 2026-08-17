import {
  NetworkAdapter,
  type PeerId,
  type PeerMetadata,
  type Message,
} from '@automerge/automerge-repo'
import type { MessageLogEntry } from '../types/messages'

type MessageCallback = (entry: MessageLogEntry) => void
type ConnectionCallback = (connected: boolean, remotePeerId?: PeerId) => void

let logEntryCounter = 0
function generateLogId(): string {
  return `log-${Date.now()}-${++logEntryCounter}`
}

/**
 * A NetworkAdapter that wraps another adapter and logs all messages.
 *
 * Intercepts both incoming and outgoing traffic and produces a MessageLogEntry
 * for each. Forwards all other events and method calls to the wrapped adapter.
 */
export class LoggingNetworkAdapter extends NetworkAdapter {
  #wrapped: NetworkAdapter
  #onMessage: MessageCallback
  #onConnection: ConnectionCallback
  #remotePeerId?: PeerId
  #includeData: boolean

  constructor(
    wrapped: NetworkAdapter,
    onMessage: MessageCallback,
    onConnection: ConnectionCallback,
    options?: { includeData?: boolean },
  ) {
    super()
    this.#wrapped = wrapped
    this.#onMessage = onMessage
    this.#onConnection = onConnection
    this.#includeData = options?.includeData ?? false
    this.#setupEventForwarding()
  }

  #setupEventForwarding() {
    this.#wrapped.on('message', (msg: Message) => {
      this.#logMessage(msg, 'incoming')
      this.emit('message', msg)
    })

    this.#wrapped.on('peer-candidate', (payload) => {
      this.#remotePeerId = payload.peerId
      this.#onConnection(true, payload.peerId)
      this.emit('peer-candidate', payload)
    })

    this.#wrapped.on('peer-disconnected', (payload) => {
      if (payload.peerId === this.#remotePeerId) {
        this.#remotePeerId = undefined
        this.#onConnection(false)
      }
      this.emit('peer-disconnected', payload)
    })

    this.#wrapped.on('close', () => {
      this.#remotePeerId = undefined
      this.#onConnection(false)
      this.emit('close')
    })
  }

  #logMessage(msg: Message, direction: 'incoming' | 'outgoing') {
    const entry: MessageLogEntry = {
      id: generateLogId(),
      timestamp: new Date(),
      direction,
      type: msg.type,
      senderId: msg.senderId,
      targetId: msg.targetId,
      documentId: msg.documentId,
      dataSize: msg.data?.byteLength,
    }
    if (this.#includeData && msg.data) {
      entry.data = msg.data
    }
    this.#onMessage(entry)
  }

  isReady(): boolean {
    return this.#wrapped.isReady()
  }

  whenReady(): Promise<void> {
    return this.#wrapped.whenReady()
  }

  connect(peerId: PeerId, peerMetadata?: PeerMetadata): void {
    this.peerId = peerId
    this.peerMetadata = peerMetadata
    this.#wrapped.connect(peerId, peerMetadata)
  }

  send(message: Message): void {
    this.#logMessage(message, 'outgoing')
    this.#wrapped.send(message)
  }

  disconnect(): void {
    // Guard against React StrictMode's double-mount cleanup: peerId is only
    // set after connect() has run.
    if (this.peerId) {
      this.#wrapped.disconnect()
    }
  }

  get remotePeerId(): PeerId | undefined {
    return this.#remotePeerId
  }
}
