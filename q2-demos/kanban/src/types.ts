/**
 * Kanban board types extracted from QMD document AST.
 */

import type { Annotated_Block } from '@quarto/pandoc-types'

/** Known card types, corresponding to header classes. */
export type CardType = 'feature' | 'milestone' | 'bug' | 'task'

/** Known card statuses, stored as key-value attribute on the header. */
export type CardStatus = 'todo' | 'doing' | 'done'

/**
 * A single kanban card, extracted from a level-2 header and its body.
 */
export interface KanbanCard {
  /** Slug id from the header (e.g., "project-export"). */
  id: string
  /** Human-readable title from the header inlines. */
  title: string
  /** Card type from the first recognized class, or undefined. */
  type: CardType | undefined
  /** Card status from the "status" key-value attribute, or undefined. */
  status: CardStatus | undefined
  /** ISO timestamp from the "created" attribute, or undefined. */
  created: string | undefined
  /** ISO date from the "deadline" attribute, or undefined. */
  deadline: string | undefined
  /** Priority from the "priority" attribute, or undefined. */
  priority: string | undefined
  /** AST blocks forming the card's body (between this header and the next). */
  bodyBlocks: Annotated_Block[]
  /** Index of this card's Header block in the top-level blocks array. */
  headerBlockIndex: number
}

/**
 * A cross-reference between cards, found in bullet list items.
 */
export interface CardRef {
  /** Card id of the card containing the reference. */
  sourceCardId: string
  /** Card id being referenced (the link target, without "#"). */
  targetCardId: string
  /** Display text of the link. */
  label: string
  /** Whether this is a checkbox item (has a Span before the Link). */
  isCheckbox: boolean
  /** If isCheckbox, whether the checkbox is checked. */
  checked: boolean
  /** Index of this item within its BulletList. */
  itemIndex: number
  /** Index of the BulletList block within the card's bodyBlocks. */
  bulletListBodyIndex: number
}

/**
 * The full board extracted from a QMD document.
 */
export interface KanbanBoard {
  /** All cards in document order. */
  cards: KanbanCard[]
  /** All cross-references between cards. */
  refs: CardRef[]
}
