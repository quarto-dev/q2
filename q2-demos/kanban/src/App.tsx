/**
 * Root app component — state machine switching between connection selector and kanban board.
 */

import { useState, useCallback } from 'react'
import type { ConnectionEntry } from './connectionStorage.ts'
import { ConnectionSelector } from './components/ConnectionSelector.tsx'
import { KanbanApp } from './KanbanApp.tsx'

export function App() {
  const [activeConnection, setActiveConnection] = useState<ConnectionEntry | null>(null)

  const handleSelect = useCallback((conn: ConnectionEntry) => {
    setActiveConnection(conn)
  }, [])

  const handleBack = useCallback(() => {
    setActiveConnection(null)
  }, [])

  return (
    <div style={{ fontFamily: 'system-ui, sans-serif', maxWidth: '1200px', margin: '0 auto', padding: '20px' }}>
      <h1 style={{ fontSize: '24px' }}>Quarto Hub - Kanban</h1>

      {activeConnection ? (
        <>
          <div style={{
            marginBottom: '12px',
            padding: '8px',
            background: '#f0f0f0',
            borderRadius: '4px',
            fontSize: '13px',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
          }}>
            <div>
              <strong>{activeConnection.description}</strong>
              {' — '}
              <code>{activeConnection.filePath}</code>
              {' @ '}
              <span style={{ color: '#666' }}>{activeConnection.syncServer}</span>
            </div>
            <button
              onClick={handleBack}
              style={{
                padding: '4px 12px',
                background: 'none',
                border: '1px solid #ccc',
                borderRadius: '4px',
                cursor: 'pointer',
                fontSize: '13px',
              }}
            >
              Disconnect
            </button>
          </div>
          <KanbanApp
            syncServer={activeConnection.syncServer}
            indexDocId={activeConnection.indexDocId}
            filePath={activeConnection.filePath}
          />
        </>
      ) : (
        <ConnectionSelector onSelect={handleSelect} />
      )}
    </div>
  )
}
