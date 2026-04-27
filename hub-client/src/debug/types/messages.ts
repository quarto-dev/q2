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
}

export type ConnectionState = 'disconnected' | 'connecting' | 'connected'

export interface ConnectionInfo {
  state: ConnectionState
  peerId?: PeerId
  remotePeerId?: PeerId
}
