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
      {activeConnection ? (
        <KanbanApp
          syncServer={activeConnection.syncServer}
          indexDocId={activeConnection.indexDocId}
          filePath={activeConnection.filePath}
          onDisconnect={handleBack}
        />
      ) : (
        <>
          <h1 style={{ fontSize: '24px' }}>Quarto Hub - Kanban</h1>
          <ConnectionSelector onSelect={handleSelect} />
        </>
      )}
    </div>
  )
}
