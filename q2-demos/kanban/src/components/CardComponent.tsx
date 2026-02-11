/**
 * Renders a single kanban card.
 */

import type { KanbanCard, CardStatus } from '../types.ts'
import type { Annotated_Inline } from '@quarto/pandoc-types'

interface CardComponentProps {
  card: KanbanCard
  onStatusChange?: (cardId: string, newStatus: CardStatus) => void
}

const TYPE_COLORS: Record<string, string> = {
  feature: '#e3f2fd',
  milestone: '#fff3e0',
  bug: '#fce4ec',
  task: '#e8f5e9',
}

export function CardComponent({ card, onStatusChange }: CardComponentProps) {
  const bgColor = card.type ? TYPE_COLORS[card.type] ?? '#f5f5f5' : '#f5f5f5'

  return (
    <div style={{
      border: '1px solid #ddd',
      borderRadius: '6px',
      padding: '12px',
      marginBottom: '8px',
      background: bgColor,
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '4px' }}>
        <select
          value={card.status ?? ''}
          onChange={(e) => {
            const val = e.target.value as CardStatus
            if (val && onStatusChange) {
              onStatusChange(card.id, val)
            }
          }}
          style={{ fontSize: '12px', padding: '2px 4px', flexShrink: 0 }}
        >
          <option value="">—</option>
          <option value="todo">todo</option>
          <option value="doing">doing</option>
          <option value="done">done</option>
        </select>
        <strong style={{ flex: 1, fontSize: '14px' }}>{card.title}</strong>
        {card.type && (
          <span style={{
            fontSize: '11px',
            padding: '1px 6px',
            borderRadius: '3px',
            background: 'rgba(0,0,0,0.08)',
          }}>
            {card.type}
          </span>
        )}
      </div>

      {card.deadline && (
        <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>
          {card.deadline}
        </div>
      )}

      <BodyPreview bodyBlocks={card.bodyBlocks} />
    </div>
  )
}

/**
 * Render a brief text preview of body blocks (first paragraph only).
 */
function BodyPreview({ bodyBlocks }: { bodyBlocks: KanbanCard['bodyBlocks'] }) {
  if (bodyBlocks.length === 0) return null

  // Find the first Para or Plain block
  const textBlock = bodyBlocks.find(b => b.t === 'Para' || b.t === 'Plain')
  if (!textBlock) return null

  const text = inlinesToText(textBlock.c as Annotated_Inline[])
  if (!text) return null

  return (
    <div style={{
      fontSize: '12px',
      color: '#555',
      marginTop: '4px',
      overflow: 'hidden',
      textOverflow: 'ellipsis',
      whiteSpace: 'nowrap',
    }}>
      {text.length > 100 ? text.slice(0, 100) + '...' : text}
    </div>
  )
}

function inlinesToText(inlines: Annotated_Inline[]): string {
  const parts: string[] = []
  for (const inline of inlines) {
    switch (inline.t) {
      case 'Str':
        parts.push(inline.c as string)
        break
      case 'Space':
      case 'SoftBreak':
        parts.push(' ')
        break
      case 'Emph':
      case 'Strong':
      case 'Strikeout':
      case 'Underline':
        parts.push(inlinesToText(inline.c as Annotated_Inline[]))
        break
      case 'Code':
        parts.push((inline.c as [unknown, string])[1])
        break
      default:
        break
    }
  }
  return parts.join('')
}
