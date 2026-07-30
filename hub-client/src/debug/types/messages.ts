import type { PeerId, DocumentId } from '@automerge/automerge-repo'

/**
 * A log entry for a message sent to or received from the network.
 */
export interface MessageLogEntry {
  id: string
  timestamp: Date
  direction: 'incoming' | 'outgoing'
  type: string
  senderId: PeerId
  targetId: PeerId
  documentId?: DocumentId
  dataSize?: number
  /**
   * Raw payload bytes; present only when the producing adapter was
   * constructed with `includeData` (the in-context tap's 'full' mode,
   * bd-6ogrov5r). The debug.html message log never sets it.
   */
  data?: Uint8Array
}

export type ConnectionState = 'disconnected' | 'connecting' | 'connected'

export interface ConnectionInfo {
  state: ConnectionState
  peerId?: PeerId
  remotePeerId?: PeerId
}
