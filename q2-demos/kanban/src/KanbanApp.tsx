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
}

export function KanbanApp({ syncServer, indexDocId, filePath }: KanbanAppProps) {
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

  return <KanbanBoard ast={ast} filePath={filePath} updateAst={updateAst} />
}

interface KanbanBoardProps {
  ast: RustQmdJson
  filePath: string
  updateAst: ((ast: RustQmdJson) => void) | null
}

function KanbanBoard({ ast, filePath, updateAst }: KanbanBoardProps) {
  const board = useMemo(() => buildBoard(ast), [ast])
  const [selectedCard, setSelectedCard] = useState<KanbanCard | null>(null)
  const [viewMode, setViewMode] = useState<ViewMode>('board')
  const [showNewCardForm, setShowNewCardForm] = useState(false)

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

  // Keep the selected card in sync with the latest AST data
  const currentSelectedCard = selectedCard
    ? board.cards.find(c => c.id === selectedCard.id) ?? null
    : null

  return (
    <div>
      {/* Toolbar: info + view switcher + new card button */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        marginBottom: '12px',
      }}>
        <p style={{ fontSize: '12px', color: '#888', margin: 0 }}>
          Live from <code>{filePath}</code> — {board.cards.length} card{board.cards.length !== 1 ? 's' : ''}
        </p>
        <div style={{ display: 'flex', gap: '4px', alignItems: 'center' }}>
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
                marginRight: '8px',
              }}
            >
              + New Card
            </button>
          )}
          <button
            onClick={() => setViewMode('board')}
            style={viewMode === 'board' ? activeTabStyle : tabStyle}
          >
            Board
          </button>
          <button
            onClick={() => setViewMode('calendar')}
            style={viewMode === 'calendar' ? activeTabStyle : tabStyle}
          >
            Calendar
          </button>
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

const tabStyle: React.CSSProperties = {
  background: 'none',
  border: '1px solid #ccc',
  borderRadius: '4px',
  padding: '4px 12px',
  cursor: 'pointer',
  fontSize: '13px',
  color: '#666',
}

const activeTabStyle: React.CSSProperties = {
  ...tabStyle,
  background: '#2563eb',
  borderColor: '#2563eb',
  color: '#fff',
}
