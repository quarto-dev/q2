/**
 * Board view: renders cards in horizontal status rows (todo / doing / done / unset).
 */

import type { KanbanCard, CardStatus } from '../types.ts'
import { CardComponent } from './CardComponent.tsx'

interface BoardViewProps {
  cards: KanbanCard[]
  onStatusChange?: (cardId: string, newStatus: CardStatus) => void
  onCardClick?: (card: KanbanCard) => void
}

const ROWS: { status: CardStatus | null; label: string }[] = [
  { status: 'todo', label: 'Todo' },
  { status: 'doing', label: 'Doing' },
  { status: 'done', label: 'Done' },
  { status: null, label: 'Unset' },
]

export function BoardView({ cards, onStatusChange, onCardClick }: BoardViewProps) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
      {ROWS.map(({ status, label }) => {
        const rowCards = cards.filter(c =>
          status === null ? c.status === undefined : c.status === status
        )
        return (
          <div key={label} style={{
            background: '#fafafa',
            borderRadius: '8px',
            padding: '12px',
          }}>
            <h3 style={{
              fontSize: '14px',
              fontWeight: 600,
              marginBottom: '12px',
              paddingBottom: '8px',
              borderBottom: '2px solid #eee',
            }}>
              {label}
              <span style={{ color: '#999', fontWeight: 400, marginLeft: '6px' }}>
                ({rowCards.length})
              </span>
            </h3>
            <div style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(2, 1fr)',
              gap: '8px',
            }}>
              {rowCards.map(card => (
                <div key={card.id}>
                  <CardComponent
                    card={card}
                    onStatusChange={onStatusChange}
                    onCardClick={onCardClick}
                  />
                </div>
              ))}
            </div>
          </div>
        )
      })}
    </div>
  )
}
