/**
 * Main kanban app component.
 * Connects AST from useSyncedAst to board view via astHelpers.
 */

import type { RustQmdJson } from '@quarto/pandoc-types'
import type { KanbanCard, CardStatus } from './types.ts'
import { useSyncedAst } from './useSyncedAst.ts'
import { buildBoard, setCardStatus } from './astHelpers.ts'
import { BoardView } from './components/BoardView.tsx'
import { CardDetailView } from './components/CardDetailView.tsx'
import { useState, useMemo, useCallback } from 'react'

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

  // Keep the selected card in sync with the latest AST data
  const currentSelectedCard = selectedCard
    ? board.cards.find(c => c.id === selectedCard.id) ?? null
    : null

  return (
    <div>
      <p style={{ fontSize: '12px', color: '#888', marginBottom: '12px' }}>
        Live from <code>{filePath}</code> — {board.cards.length} card{board.cards.length !== 1 ? 's' : ''}
      </p>
      <BoardView
        cards={board.cards}
        onStatusChange={updateAst ? onStatusChange : undefined}
        onCardClick={onCardClick}
      />
      {currentSelectedCard && (
        <CardDetailView
          card={currentSelectedCard}
          onClose={() => setSelectedCard(null)}
          onStatusChange={updateAst ? onStatusChange : undefined}
        />
      )}
    </div>
  )
}
