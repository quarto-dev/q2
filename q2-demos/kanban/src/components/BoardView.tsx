/**
 * Board view: renders cards in status columns (todo / doing / done / unset).
 */

import type { KanbanCard, CardStatus } from '../types.ts'
import { CardComponent } from './CardComponent.tsx'

interface BoardViewProps {
  cards: KanbanCard[]
  onStatusChange?: (cardId: string, newStatus: CardStatus) => void
}

const COLUMNS: { status: CardStatus | null; label: string }[] = [
  { status: 'todo', label: 'Todo' },
  { status: 'doing', label: 'Doing' },
  { status: 'done', label: 'Done' },
  { status: null, label: 'Unset' },
]

export function BoardView({ cards, onStatusChange }: BoardViewProps) {
  return (
    <div style={{
      display: 'grid',
      gridTemplateColumns: `repeat(${COLUMNS.length}, 1fr)`,
      gap: '16px',
    }}>
      {COLUMNS.map(({ status, label }) => {
        const columnCards = cards.filter(c =>
          status === null ? c.status === undefined : c.status === status
        )
        return (
          <div key={label} style={{
            background: '#fafafa',
            borderRadius: '8px',
            padding: '12px',
            minHeight: '200px',
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
                ({columnCards.length})
              </span>
            </h3>
            {columnCards.map(card => (
              <CardComponent
                key={card.id}
                card={card}
                onStatusChange={onStatusChange}
              />
            ))}
          </div>
        )
      })}
    </div>
  )
}
