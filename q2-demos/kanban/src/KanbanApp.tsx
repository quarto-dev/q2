/**
 * Main kanban app component.
 * Connects AST from useSyncedAst to board view via astHelpers.
 */

import type { RustQmdJson } from '@quarto/pandoc-types'
import type { KanbanCard, CardStatus } from './types.ts'
import { useSyncedAst } from './useSyncedAst.ts'
import { buildBoard, setCardStatus, addCard } from './astHelpers.ts'
import { BoardView } from './components/BoardView.tsx'
import { CalendarView } from './components/CalendarView.tsx'
import { CardDetailView } from './components/CardDetailView.tsx'
import { NewCardForm } from './components/NewCardForm.tsx'
import type { NewCardData } from './components/NewCardForm.tsx'
import { useState, useMemo, useCallback } from 'react'

type ViewMode = 'board' | 'calendar'

interface KanbanAppProps {
  syncServer: string
  indexDocId: string
  filePath: string
  onDisconnect: () => void
}

export function KanbanApp({ syncServer, indexDocId, filePath, onDisconnect }: KanbanAppProps) {
  const params = useMemo(
    () => ({ syncServer, indexDocId, filePath }),
    [syncServer, indexDocId, filePath],
  )
  const { ast, connected, error, connecting, updateAst } = useSyncedAst(params)

  if (error) {
    return (
      <div style={{ padding: '16px' }}>
        <h2>Connection Error</h2>
        <p style={{ color: 'red' }}>{error}</p>
      </div>
    )
  }

  if (connecting) {
    return (
      <div style={{ padding: '16px' }}>
        <p>Connecting to sync server...</p>
      </div>
    )
  }

  if (!connected) {
    return (
      <div style={{ padding: '16px' }}>
        <p>Disconnected.</p>
      </div>
    )
  }

  if (!ast) {
    return (
      <div style={{ padding: '16px' }}>
        <p>Waiting for document...</p>
      </div>
    )
  }

  return (
    <KanbanBoard
      ast={ast}
      filePath={filePath}
      indexDocId={indexDocId}
      updateAst={updateAst}
      onDisconnect={onDisconnect}
    />
  )
}

interface KanbanBoardProps {
  ast: RustQmdJson
  filePath: string
  indexDocId: string
  updateAst: ((ast: RustQmdJson) => void) | null
  onDisconnect: () => void
}

function KanbanBoard({ ast, filePath, indexDocId, updateAst, onDisconnect }: KanbanBoardProps) {
  const board = useMemo(() => buildBoard(ast), [ast])
  const [selectedCard, setSelectedCard] = useState<KanbanCard | null>(null)
  const [viewMode, setViewMode] = useState<ViewMode>('board')
  const [showNewCardForm, setShowNewCardForm] = useState(false)
  const [copiedDocId, setCopiedDocId] = useState(false)

  const onStatusChange = useCallback((cardId: string, newStatus: CardStatus) => {
    if (!updateAst) return
    const newAst = setCardStatus(ast, cardId, newStatus)
    if (newAst) {
      updateAst(newAst)
    }
  }, [ast, updateAst])

  const onCardClick = useCallback((card: KanbanCard) => {
    setSelectedCard(card)
  }, [])

  const onNewCard = useCallback((data: NewCardData) => {
    if (!updateAst) return
    const newAst = addCard(ast, data)
    if (newAst) {
      updateAst(newAst)
      setShowNewCardForm(false)
    }
  }, [ast, updateAst])

  const copyDocId = useCallback(() => {
    navigator.clipboard.writeText(indexDocId).then(() => {
      setCopiedDocId(true)
      setTimeout(() => setCopiedDocId(false), 1500)
    })
  }, [indexDocId])

  // Keep the selected card in sync with the latest AST data
  const currentSelectedCard = selectedCard
    ? board.cards.find(c => c.id === selectedCard.id) ?? null
    : null

  const cardCount = board.cards.length

  return (
    <div>
      {/* Unified toolbar */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        marginBottom: '12px',
        padding: '8px 12px',
        background: '#f0f0f0',
        borderRadius: '6px',
        gap: '12px',
      }}>
        {/* Left: title + info */}
        <div style={{ display: 'flex', alignItems: 'center', gap: '8px', fontSize: '13px', minWidth: 0 }}>
          <strong style={{ flexShrink: 0 }}>Kanban</strong>
          <span style={{ color: '#999' }}>&mdash;</span>
          <code style={{ fontSize: '12px' }}>{filePath}</code>
          <span style={{ color: '#999' }}>&mdash;</span>
          <span style={{ color: '#666', flexShrink: 0 }}>{cardCount} card{cardCount !== 1 ? 's' : ''}</span>
          <span style={{ color: '#999' }}>&middot;</span>
          <span
            onClick={copyDocId}
            title="Click to copy index document ID"
            style={{
              color: copiedDocId ? '#16a34a' : '#888',
              cursor: 'pointer',
              fontSize: '11px',
              fontFamily: 'monospace',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {copiedDocId ? 'Copied!' : indexDocId}
          </span>
        </div>

        {/* Right: actions */}
        <div style={{ display: 'flex', gap: '8px', alignItems: 'center', flexShrink: 0 }}>
          <button
            onClick={onDisconnect}
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
          {updateAst && (
            <button
              onClick={() => setShowNewCardForm(true)}
              style={{
                padding: '4px 12px',
                border: 'none',
                borderRadius: '4px',
                background: '#16a34a',
                color: '#fff',
                cursor: 'pointer',
                fontSize: '13px',
              }}
            >
              + New Card
            </button>
          )}
          {/* Joined Board/Calendar toggle group */}
          <div style={{ display: 'flex' }}>
            <button
              onClick={() => setViewMode('board')}
              style={{
                padding: '4px 12px',
                fontSize: '13px',
                cursor: 'pointer',
                border: '1px solid #ccc',
                borderRadius: '4px 0 0 4px',
                borderRight: 'none',
                background: viewMode === 'board' ? '#2563eb' : '#fff',
                color: viewMode === 'board' ? '#fff' : '#666',
              }}
            >
              Board
            </button>
            <button
              onClick={() => setViewMode('calendar')}
              style={{
                padding: '4px 12px',
                fontSize: '13px',
                cursor: 'pointer',
                border: '1px solid #ccc',
                borderRadius: '0 4px 4px 0',
                background: viewMode === 'calendar' ? '#2563eb' : '#fff',
                color: viewMode === 'calendar' ? '#fff' : '#666',
              }}
            >
              Calendar
            </button>
          </div>
        </div>
      </div>

      {viewMode === 'board' ? (
        <BoardView
          cards={board.cards}
          onStatusChange={updateAst ? onStatusChange : undefined}
          onCardClick={onCardClick}
        />
      ) : (
        <CalendarView
          cards={board.cards}
          onCardClick={onCardClick}
        />
      )}

      {currentSelectedCard && (
        <CardDetailView
          card={currentSelectedCard}
          onClose={() => setSelectedCard(null)}
          onStatusChange={updateAst ? onStatusChange : undefined}
        />
      )}

      {showNewCardForm && (
        <NewCardForm
          onSubmit={onNewCard}
          onClose={() => setShowNewCardForm(false)}
        />
      )}
    </div>
  )
}
