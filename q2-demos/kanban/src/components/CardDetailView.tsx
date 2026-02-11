/**
 * Modal overlay showing full details of a single kanban card.
 */

import type { KanbanCard, CardStatus } from '../types.ts'
import type { Annotated_Inline } from '@quarto/pandoc-types'

interface CardDetailViewProps {
  card: KanbanCard
  onClose: () => void
  onStatusChange?: (cardId: string, newStatus: CardStatus) => void
}

const TYPE_COLORS: Record<string, string> = {
  feature: '#e3f2fd',
  milestone: '#fff3e0',
  bug: '#fce4ec',
  task: '#e8f5e9',
}

export function CardDetailView({ card, onClose, onStatusChange }: CardDetailViewProps) {
  return (
    <div
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        background: 'rgba(0, 0, 0, 0.5)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div style={{
        background: '#fff',
        borderRadius: '8px',
        padding: '24px',
        maxWidth: '600px',
        width: '90%',
        maxHeight: '80vh',
        overflow: 'auto',
        boxShadow: '0 4px 24px rgba(0, 0, 0, 0.2)',
      }}>
        {/* Header */}
        <div style={{ display: 'flex', alignItems: 'flex-start', gap: '12px', marginBottom: '16px' }}>
          <h2 style={{ flex: 1, margin: 0, fontSize: '20px' }}>{card.title}</h2>
          <button
            onClick={onClose}
            style={{
              background: 'none',
              border: 'none',
              fontSize: '20px',
              cursor: 'pointer',
              color: '#666',
              padding: '0 4px',
              lineHeight: 1,
            }}
          >
            &times;
          </button>
        </div>

        {/* Metadata grid */}
        <div style={{
          display: 'grid',
          gridTemplateColumns: 'auto 1fr',
          gap: '8px 16px',
          marginBottom: '16px',
          fontSize: '14px',
        }}>
          {card.type && (
            <>
              <span style={{ color: '#888' }}>Type</span>
              <span>
                <span style={{
                  display: 'inline-block',
                  padding: '2px 8px',
                  borderRadius: '4px',
                  background: TYPE_COLORS[card.type] ?? '#f5f5f5',
                  fontSize: '12px',
                }}>
                  {card.type}
                </span>
              </span>
            </>
          )}

          <span style={{ color: '#888' }}>Status</span>
          <span>
            <select
              value={card.status ?? ''}
              onChange={(e) => {
                const val = e.target.value as CardStatus
                if (val && onStatusChange) {
                  onStatusChange(card.id, val)
                }
              }}
              style={{ fontSize: '13px', padding: '2px 6px' }}
            >
              <option value="">—</option>
              <option value="todo">todo</option>
              <option value="doing">doing</option>
              <option value="done">done</option>
            </select>
          </span>

          {card.created && (
            <>
              <span style={{ color: '#888' }}>Created</span>
              <span>{card.created}</span>
            </>
          )}

          {card.deadline && (
            <>
              <span style={{ color: '#888' }}>Deadline</span>
              <span>{card.deadline}</span>
            </>
          )}

          {card.priority && (
            <>
              <span style={{ color: '#888' }}>Priority</span>
              <span>{card.priority}</span>
            </>
          )}
        </div>

        {/* Body */}
        {card.bodyBlocks.length > 0 && (
          <div style={{
            borderTop: '1px solid #eee',
            paddingTop: '16px',
          }}>
            <BodyContent bodyBlocks={card.bodyBlocks} />
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * Render all body blocks as readable text.
 */
function BodyContent({ bodyBlocks }: { bodyBlocks: KanbanCard['bodyBlocks'] }) {
  const paragraphs: string[] = []

  for (const block of bodyBlocks) {
    if (block.t === 'Para' || block.t === 'Plain') {
      const text = inlinesToText(block.c as Annotated_Inline[])
      if (text) paragraphs.push(text)
    } else if (block.t === 'BulletList') {
      const items = block.c as unknown[][]
      for (const item of items) {
        if (item.length > 0) {
          const firstBlock = item[0] as { t: string; c: unknown }
          if (firstBlock.t === 'Plain' || firstBlock.t === 'Para') {
            const text = inlinesToText(firstBlock.c as Annotated_Inline[])
            if (text) paragraphs.push('- ' + text)
          }
        }
      }
    }
  }

  if (paragraphs.length === 0) return null

  return (
    <div style={{ fontSize: '14px', color: '#444', lineHeight: 1.6 }}>
      {paragraphs.map((p, i) => (
        <p key={i} style={{ margin: '0 0 8px 0' }}>{p}</p>
      ))}
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
      case 'Link': {
        const linkContent = inline.c as [unknown, Annotated_Inline[], [string, string]]
        parts.push(inlinesToText(linkContent[1]))
        break
      }
      default:
        break
    }
  }
  return parts.join('')
}
