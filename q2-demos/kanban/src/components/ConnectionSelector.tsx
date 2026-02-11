/**
 * Connection selector: form to connect to a project + list of recent connections.
 */

import { useState, useEffect, useCallback } from 'react'
import type { ConnectionEntry } from '../connectionStorage.ts'
import * as storage from '../connectionStorage.ts'

interface ConnectionSelectorProps {
  onSelect: (conn: ConnectionEntry) => void
}

const DEFAULT_SYNC_SERVER = 'wss://sync.automerge.org'

export function ConnectionSelector({ onSelect }: ConnectionSelectorProps) {
  const [connections, setConnections] = useState<ConnectionEntry[]>([])
  const [loading, setLoading] = useState(true)

  // Form state
  const [syncServer, setSyncServer] = useState(DEFAULT_SYNC_SERVER)
  const [indexDocId, setIndexDocId] = useState('')
  const [filePath, setFilePath] = useState('')
  const [description, setDescription] = useState('')
  const [formError, setFormError] = useState<string | null>(null)

  const loadConnections = useCallback(async () => {
    setLoading(true)
    try {
      const entries = await storage.listConnections()
      setConnections(entries)
    } catch (err) {
      console.error('Failed to load connections:', err)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { loadConnections() }, [loadConnections])

  const handleConnect = async (e: React.FormEvent) => {
    e.preventDefault()
    setFormError(null)

    if (!indexDocId.trim()) {
      setFormError('Document ID is required')
      return
    }
    if (!filePath.trim()) {
      setFormError('File path is required')
      return
    }
    if (!syncServer.trim()) {
      setFormError('Sync server URL is required')
      return
    }

    try {
      const conn = await storage.addConnection(
        syncServer.trim(),
        indexDocId.trim(),
        filePath.trim(),
        description.trim() || undefined,
      )
      await loadConnections()
      onSelect(conn)
    } catch (err) {
      console.error('Failed to add connection:', err)
      setFormError('Failed to save connection.')
    }
  }

  const handleSelectExisting = async (conn: ConnectionEntry) => {
    await storage.touchConnection(conn.id)
    onSelect(conn)
  }

  const handleDelete = async (e: React.MouseEvent, conn: ConnectionEntry) => {
    e.stopPropagation()
    await storage.deleteConnection(conn.id)
    await loadConnections()
  }

  return (
    <div style={{ maxWidth: '600px', margin: '0 auto' }}>
      <h2 style={{ fontSize: '18px', marginBottom: '16px' }}>Connect to a Project</h2>

      {/* Connect form */}
      <form onSubmit={handleConnect} style={{
        background: '#f8f8f8',
        borderRadius: '8px',
        padding: '16px',
        marginBottom: '24px',
      }}>
        {formError && (
          <div style={{ color: 'red', marginBottom: '12px', fontSize: '13px' }}>
            {formError}
          </div>
        )}

        <div style={{ marginBottom: '10px' }}>
          <label htmlFor="syncServer" style={labelStyle}>Sync Server URL</label>
          <input
            id="syncServer"
            type="text"
            value={syncServer}
            onChange={e => setSyncServer(e.target.value)}
            placeholder="wss://sync.automerge.org"
            style={inputStyle}
          />
        </div>

        <div style={{ marginBottom: '10px' }}>
          <label htmlFor="indexDocId" style={labelStyle}>Document ID</label>
          <input
            id="indexDocId"
            type="text"
            value={indexDocId}
            onChange={e => setIndexDocId(e.target.value)}
            placeholder="Automerge document ID"
            style={inputStyle}
          />
        </div>

        <div style={{ marginBottom: '10px' }}>
          <label htmlFor="filePath" style={labelStyle}>File Path</label>
          <input
            id="filePath"
            type="text"
            value={filePath}
            onChange={e => setFilePath(e.target.value)}
            placeholder="kanban.qmd"
            style={inputStyle}
          />
        </div>

        <div style={{ marginBottom: '14px' }}>
          <label htmlFor="description" style={labelStyle}>Description (optional)</label>
          <input
            id="description"
            type="text"
            value={description}
            onChange={e => setDescription(e.target.value)}
            placeholder="My Kanban Board"
            style={inputStyle}
          />
        </div>

        <button type="submit" style={{
          padding: '8px 20px',
          background: '#2563eb',
          color: 'white',
          border: 'none',
          borderRadius: '4px',
          cursor: 'pointer',
          fontSize: '14px',
        }}>
          Connect
        </button>
      </form>

      {/* Recent connections */}
      <h3 style={{ fontSize: '15px', marginBottom: '10px', color: '#555' }}>
        Recent Connections
      </h3>

      {loading ? (
        <p style={{ color: '#999', fontSize: '13px' }}>Loading...</p>
      ) : connections.length === 0 ? (
        <p style={{ color: '#999', fontSize: '13px' }}>No saved connections yet.</p>
      ) : (
        <ul style={{ listStyle: 'none', padding: 0, margin: 0 }}>
          {connections.map(conn => (
            <li
              key={conn.id}
              onClick={() => handleSelectExisting(conn)}
              style={{
                padding: '10px 12px',
                background: '#fafafa',
                borderRadius: '6px',
                marginBottom: '6px',
                cursor: 'pointer',
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'center',
                border: '1px solid #eee',
              }}
            >
              <div>
                <div style={{ fontWeight: 500, fontSize: '14px' }}>{conn.description}</div>
                <div style={{ fontSize: '12px', color: '#888', marginTop: '2px' }}>
                  {conn.filePath} &middot;{' '}
                  <span title={conn.indexDocId}>
                    {conn.indexDocId.slice(0, 12)}...
                  </span>
                </div>
              </div>
              <button
                onClick={e => handleDelete(e, conn)}
                title="Delete connection"
                style={{
                  background: 'none',
                  border: 'none',
                  cursor: 'pointer',
                  fontSize: '18px',
                  color: '#ccc',
                  padding: '0 4px',
                  lineHeight: 1,
                }}
              >
                &times;
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

const labelStyle: React.CSSProperties = {
  display: 'block',
  fontSize: '13px',
  fontWeight: 500,
  marginBottom: '4px',
  color: '#444',
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  padding: '6px 10px',
  fontSize: '14px',
  border: '1px solid #ddd',
  borderRadius: '4px',
  boxSizing: 'border-box',
}
