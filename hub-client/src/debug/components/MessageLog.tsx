import { useState, useMemo, useRef, useEffect } from 'react'
import type { MessageLogEntry } from '../types/messages'

interface Props {
  messages: MessageLogEntry[]
  onClear: () => void
}

interface Filters {
  type: string
  documentId: string
  direction: '' | 'incoming' | 'outgoing'
}

export function MessageLog({ messages, onClear }: Props) {
  const [filters, setFilters] = useState<Filters>({
    type: '',
    documentId: '',
    direction: '',
  })
  const [autoScroll, setAutoScroll] = useState(true)
  const listRef = useRef<HTMLDivElement>(null)

  const messageTypes = useMemo(
    () => [...new Set(messages.map((m) => m.type))].sort(),
    [messages],
  )

  const documentIds = useMemo(
    () =>
      [...new Set(messages.map((m) => m.documentId).filter(Boolean))].sort() as string[],
    [messages],
  )

  const filteredMessages = useMemo(
    () =>
      messages.filter((msg) => {
        if (filters.type && msg.type !== filters.type) return false
        if (filters.documentId && msg.documentId !== filters.documentId) return false
        if (filters.direction && msg.direction !== filters.direction) return false
        return true
      }),
    [messages, filters],
  )

  useEffect(() => {
    if (autoScroll && listRef.current) {
      listRef.current.scrollTop = listRef.current.scrollHeight
    }
  }, [filteredMessages, autoScroll])

  const formatTime = (date: Date) => date.toISOString().slice(11, 23)

  const formatSize = (bytes?: number) => {
    if (bytes === undefined) return ''
    if (bytes < 1024) return `${bytes}B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`
    return `${(bytes / (1024 * 1024)).toFixed(1)}MB`
  }

  return (
    <div className="message-log">
      <div className="log-header">
        <h2>
          Communication Log
          <span className="count">
            ({filteredMessages.length}/{messages.length})
          </span>
        </h2>
        <div className="log-actions">
          <label className="auto-scroll">
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={(e) => setAutoScroll(e.target.checked)}
            />
            Auto-scroll
          </label>
          <button onClick={onClear} disabled={messages.length === 0}>
            Clear
          </button>
        </div>
      </div>

      <div className="log-filters">
        <select
          value={filters.type}
          onChange={(e) => setFilters({ ...filters, type: e.target.value })}
        >
          <option value="">All Types</option>
          {messageTypes.map((t) => (
            <option key={t} value={t}>
              {t}
            </option>
          ))}
        </select>

        <select
          value={filters.direction}
          onChange={(e) =>
            setFilters({ ...filters, direction: e.target.value as Filters['direction'] })
          }
        >
          <option value="">All Directions</option>
          <option value="incoming">Incoming</option>
          <option value="outgoing">Outgoing</option>
        </select>

        <select
          value={filters.documentId}
          onChange={(e) => setFilters({ ...filters, documentId: e.target.value })}
        >
          <option value="">All Documents</option>
          {documentIds.map((d) => (
            <option key={d} value={d}>
              {d.slice(0, 20)}…
            </option>
          ))}
        </select>
      </div>

      <div className="log-messages" ref={listRef}>
        {filteredMessages.length === 0 ? (
          <p className="empty">
            {messages.length === 0
              ? 'No messages yet. Subscribe to a document to start seeing sync traffic.'
              : 'No messages match the current filters.'}
          </p>
        ) : (
          filteredMessages.map((msg) => (
            <div key={msg.id} className={`log-entry ${msg.direction}`}>
              <span className="time">{formatTime(msg.timestamp)}</span>
              <span className={`direction ${msg.direction}`}>
                {msg.direction === 'incoming' ? '<<<' : '>>>'}
              </span>
              <span className="type">{msg.type}</span>
              {msg.documentId && (
                <span className="doc-id" title={msg.documentId}>
                  {msg.documentId.slice(0, 12)}…
                </span>
              )}
              {msg.dataSize !== undefined && msg.dataSize > 0 && (
                <span className="size">{formatSize(msg.dataSize)}</span>
              )}
            </div>
          ))
        )}
      </div>
    </div>
  )
}
