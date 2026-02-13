/**
 * Board view: renders cards in horizontal status rows (todo / doing / done / unset).
 * Cards can be dragged between status sections to change their status.
 */

import { useState } from 'react'
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  KeyboardSensor,
  useSensor,
  useSensors,
  useDraggable,
  useDroppable,
} from '@dnd-kit/core'
import type { DragStartEvent, DragEndEvent } from '@dnd-kit/core'
import type { KanbanCard, CardStatus } from '../types.ts'
import { CardComponent } from './CardComponent.tsx'

interface BoardViewProps {
  cards: KanbanCard[]
  onStatusChange?: (cardId: string, newStatus: CardStatus) => void
  onCardClick?: (card: KanbanCard) => void
}

/** Status section definitions. null status means "unset". */
const ROWS: { status: CardStatus | null; label: string; droppableId: string }[] = [
  { status: 'todo', label: 'Todo', droppableId: 'section-todo' },
  { status: 'doing', label: 'Doing', droppableId: 'section-doing' },
  { status: 'done', label: 'Done', droppableId: 'section-done' },
  { status: null, label: 'Unset', droppableId: 'section-unset' },
]

/** Map from droppable id to the CardStatus value (or undefined for unset). */
const DROPPABLE_TO_STATUS: Record<string, CardStatus | undefined> = {
  'section-todo': 'todo',
  'section-doing': 'doing',
  'section-done': 'done',
  'section-unset': undefined,
}

/**
 * Creates a DragEnd handler that maps droppable section ids to status values
 * and calls onStatusChange. Exported for unit testing.
 */
export function makeDragEndHandler(
  onStatusChange: ((cardId: string, newStatus: CardStatus) => void) | undefined
) {
  return (event: DragEndEvent) => {
    const { active, over } = event
    if (!over || !onStatusChange) return

    const cardId = active.id as string
    const droppableId = over.id as string

    if (droppableId in DROPPABLE_TO_STATUS) {
      const newStatus = DROPPABLE_TO_STATUS[droppableId]
      onStatusChange(cardId, newStatus as CardStatus)
    }
  }
}

export function BoardView({ cards, onStatusChange, onCardClick }: BoardViewProps) {
  const [activeCardId, setActiveCardId] = useState<string | null>(null)

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor),
  )

  const activeCard = activeCardId ? cards.find(c => c.id === activeCardId) : null

  function handleDragStart(event: DragStartEvent) {
    setActiveCardId(event.active.id as string)
  }

  const handleDragEnd = (event: DragEndEvent) => {
    setActiveCardId(null)
    makeDragEndHandler(onStatusChange)(event)
  }

  function handleDragCancel() {
    setActiveCardId(null)
  }

  return (
    <DndContext
      sensors={sensors}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
      onDragCancel={handleDragCancel}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
        {ROWS.map(({ status, label, droppableId }) => {
          const rowCards = cards.filter(c =>
            status === null ? c.status === undefined : c.status === status
          )
          return (
            <DroppableSection
              key={droppableId}
              droppableId={droppableId}
              label={label}
              statusKey={status === null ? 'unset' : status}
              cardCount={rowCards.length}
            >
              {rowCards.map(card => (
                <DraggableCard
                  key={card.id}
                  card={card}
                  onCardClick={onCardClick}
                  isDragOverlay={false}
                />
              ))}
            </DroppableSection>
          )
        })}
      </div>

      <DragOverlay>
        {activeCard ? (
          <CardComponent
            card={activeCard}
            showStatusDropdown={false}
          />
        ) : null}
      </DragOverlay>
    </DndContext>
  )
}

/** A droppable status section that accepts cards. */
function DroppableSection({
  droppableId,
  label,
  statusKey,
  cardCount,
  children,
}: {
  droppableId: string
  label: string
  statusKey: string
  cardCount: number
  children: React.ReactNode
}) {
  const { setNodeRef, isOver } = useDroppable({ id: droppableId })

  return (
    <div
      ref={setNodeRef}
      data-status={statusKey}
      style={{
        padding: '12px 0',
        borderRadius: '8px',
        transition: 'background-color 150ms ease',
        backgroundColor: isOver ? 'rgba(59, 130, 246, 0.08)' : 'transparent',
      }}
    >
      <h3 style={{
        fontSize: '14px',
        fontWeight: 600,
        marginBottom: '12px',
        paddingBottom: '8px',
        borderBottom: isOver ? '2px solid rgba(59, 130, 246, 0.4)' : '2px solid #eee',
        transition: 'border-color 150ms ease',
      }}>
        {label}
        <span style={{ color: '#999', fontWeight: 400, marginLeft: '6px' }}>
          ({cardCount})
        </span>
      </h3>
      <div style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(2, 1fr)',
        gap: '8px',
      }}>
        {children}
      </div>
    </div>
  )
}

/** A draggable card wrapper. */
function DraggableCard({
  card,
  onCardClick,
  isDragOverlay,
}: {
  card: KanbanCard
  onCardClick?: (card: KanbanCard) => void
  isDragOverlay: boolean
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    isDragging,
  } = useDraggable({ id: card.id })

  const style: React.CSSProperties = {
    cursor: 'grab',
    opacity: isDragging ? 0.4 : 1,
    ...(transform && !isDragOverlay ? {
      transform: `translate3d(${transform.x}px, ${transform.y}px, 0)`,
    } : {}),
  }

  return (
    <div
      ref={setNodeRef}
      data-card-id={card.id}
      style={style}
      {...listeners}
      {...attributes}
    >
      <CardComponent
        card={card}
        showStatusDropdown={false}
        onCardClick={onCardClick}
      />
    </div>
  )
}
