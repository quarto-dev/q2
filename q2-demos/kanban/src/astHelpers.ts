/**
 * Extract kanban cards and cross-references from a Pandoc AST.
 *
 * Expected document structure:
 *
 *   # Document Title
 *
 *   ## Card Title {.type key="value"}
 *
 *   Card body content...
 *
 *   ## Another Card {.feature created="2026-02-10"}
 *
 * Each level-2 header defines a card. The body is everything between
 * this header and the next level-2 (or higher) header.
 */

import type { RustQmdJson, Annotated_Block, Annotated_Inline } from '@quarto/pandoc-types'
import type { KanbanCard, KanbanBoard, CardRef, CardType, CardStatus } from './types.ts'

const KNOWN_CARD_TYPES: Set<string> = new Set(['feature', 'milestone', 'bug', 'task'])
const KNOWN_STATUSES: Set<string> = new Set(['todo', 'doing', 'done'])

/**
 * Extract all kanban cards from the AST.
 *
 * A card is a level-2 Header block plus all subsequent blocks until
 * the next level-2 (or higher) Header or end of document.
 */
export function extractCards(ast: RustQmdJson): KanbanCard[] {
  const cards: KanbanCard[] = []
  const blocks = ast.blocks

  for (let i = 0; i < blocks.length; i++) {
    const block = blocks[i]
    if (block.t !== 'Header') continue

    const [level, attr, inlines] = block.c as [number, [string, string[], [string, string][]], Annotated_Inline[]]
    if (level !== 2) continue

    const [id, classes, kvs] = attr

    // Collect body blocks until the next level-2+ header
    const bodyBlocks: Annotated_Block[] = []
    for (let j = i + 1; j < blocks.length; j++) {
      const next = blocks[j]
      if (next.t === 'Header') {
        const nextLevel = (next.c as [number, unknown, unknown])[0]
        if (nextLevel <= 2) break
      }
      bodyBlocks.push(next)
    }

    // Extract metadata from key-value pairs
    const kvsMap = new Map(kvs)

    // Find the card type from classes
    const cardType = classes.find(c => KNOWN_CARD_TYPES.has(c)) as CardType | undefined

    // Extract status
    const statusStr = kvsMap.get('status')
    const status = statusStr && KNOWN_STATUSES.has(statusStr) ? statusStr as CardStatus : undefined

    cards.push({
      id,
      title: inlinesToText(inlines),
      type: cardType,
      status,
      created: kvsMap.get('created'),
      deadline: kvsMap.get('deadline'),
      priority: kvsMap.get('priority'),
      bodyBlocks,
      headerBlockIndex: i,
    })
  }

  return cards
}

/**
 * Extract cross-references from a card's body blocks.
 *
 * Looks for BulletList items containing Link inlines with "#" targets.
 * Items with a Span before the Link are checkbox items (from `- [ ]` syntax).
 */
export function extractCardRefs(card: KanbanCard): CardRef[] {
  const refs: CardRef[] = []

  for (let bodyIdx = 0; bodyIdx < card.bodyBlocks.length; bodyIdx++) {
    const block = card.bodyBlocks[bodyIdx]
    if (block.t !== 'BulletList') continue

    const listItems = block.c as Annotated_Block[][]
    for (let itemIdx = 0; itemIdx < listItems.length; itemIdx++) {
      const item = listItems[itemIdx]
      if (!item || item.length === 0) continue

      const firstBlock = item[0]
      if (firstBlock.t !== 'Plain' && firstBlock.t !== 'Para') continue

      const inlines = firstBlock.c as Annotated_Inline[]
      const ref = extractRefFromInlines(inlines, card.id, itemIdx, bodyIdx)
      if (ref) refs.push(ref)
    }
  }

  return refs
}

/**
 * Build a complete KanbanBoard from the AST.
 */
export function buildBoard(ast: RustQmdJson): KanbanBoard {
  const cards = extractCards(ast)
  const refs: CardRef[] = []
  for (const card of cards) {
    refs.push(...extractCardRefs(card))
  }
  return { cards, refs }
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

/**
 * Set or update a card's status attribute. Returns a new (cloned) AST,
 * or null if the card is not found.
 */
export function setCardStatus(ast: RustQmdJson, cardId: string, newStatus: CardStatus): RustQmdJson | null {
  const cards = extractCards(ast)
  const card = cards.find(c => c.id === cardId)
  if (!card) return null

  const cloned: RustQmdJson = JSON.parse(JSON.stringify(ast))
  const header = cloned.blocks[card.headerBlockIndex]
  if (header.t !== 'Header') return null
  const attr = header.c[1] as [string, string[], [string, string][]]
  const kvs = attr[2]

  // Find existing status entry
  const statusIdx = kvs.findIndex(([k]) => k === 'status')
  if (statusIdx >= 0) {
    kvs[statusIdx] = ['status', newStatus]
  } else {
    kvs.push(['status', newStatus])
  }

  return cloned
}

/**
 * Toggle a milestone checkbox item. Returns a new (cloned) AST,
 * or null if the card/item is not found.
 */
export function toggleMilestoneItem(ast: RustQmdJson, cardId: string, itemIndex: number): RustQmdJson | null {
  const cards = extractCards(ast)
  const card = cards.find(c => c.id === cardId)
  if (!card) return null

  // Find the first BulletList in the card's body
  const bulletListBodyIdx = card.bodyBlocks.findIndex(b => b.t === 'BulletList')
  if (bulletListBodyIdx < 0) return null

  const cloned: RustQmdJson = JSON.parse(JSON.stringify(ast))

  // Navigate to the BulletList in the cloned AST
  // The body blocks start at headerBlockIndex + 1
  const bulletListBlockIdx = card.headerBlockIndex + 1 + bulletListBodyIdx
  const bulletList = cloned.blocks[bulletListBlockIdx]
  if (!bulletList || bulletList.t !== 'BulletList') return null

  const listItems = bulletList.c as Annotated_Block[][]
  if (itemIndex < 0 || itemIndex >= listItems.length) return null

  const item = listItems[itemIndex]
  if (!item || item.length === 0) return null

  const firstBlock = item[0]
  if (firstBlock.t !== 'Plain' && firstBlock.t !== 'Para') return null

  const inlines = firstBlock.c as Annotated_Inline[]

  // Find the Span (checkbox)
  const span = inlines.find(i => i.t === 'Span')
  if (!span) return null

  const spanContent = (span.c as [unknown, Annotated_Inline[]])[1]
  const isChecked = spanContent.length > 0 && spanContent.some(
    (i: Annotated_Inline) => i.t === 'Str' && i.c === 'x'
  )

  if (isChecked) {
    ;(span.c as [unknown, Annotated_Inline[]])[1] = []
  } else {
    ;(span.c as [unknown, Annotated_Inline[]])[1] = [{ t: 'Str', c: 'x', s: 0 } as Annotated_Inline]
  }

  return cloned
}

/**
 * Options for creating a new card.
 */
export interface AddCardOptions {
  title: string
  type?: CardType
  status?: CardStatus
  deadline?: string  // YYYY-MM-DD
}

/**
 * Add a new card at the end of the document. Returns a new (cloned) AST.
 */
export function addCard(ast: RustQmdJson, options: AddCardOptions): RustQmdJson | null {
  const { title, type, status, deadline } = options
  const cloned: RustQmdJson = JSON.parse(JSON.stringify(ast))

  // Generate a slug from the title
  let slug = title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  // Ensure uniqueness against existing card ids
  const existingIds = new Set(extractCards(ast).map(c => c.id))
  if (existingIds.has(slug)) {
    let counter = 1
    while (existingIds.has(`${slug}-${counter}`)) counter++
    slug = `${slug}-${counter}`
  }

  const classes = type ? [type] : []
  const now = new Date().toISOString().replace(/:\d{2}\.\d+Z$/, '')
  const kvs: [string, string][] = [['created', now]]
  if (status) kvs.push(['status', status])
  if (deadline) kvs.push(['deadline', deadline])

  const inlineParts: Annotated_Inline[] = []
  const words = title.split(' ')
  for (let i = 0; i < words.length; i++) {
    if (i > 0) inlineParts.push({ t: 'Space', s: 0 } as Annotated_Inline)
    inlineParts.push({ t: 'Str', c: words[i], s: 0 } as Annotated_Inline)
  }

  const header: Annotated_Block = {
    t: 'Header',
    c: [2, [slug, classes, kvs], inlineParts],
    s: 0,
    attrS: { classes: [], id: null, kvs: [] },
  } as unknown as Annotated_Block

  cloned.blocks.push(header)
  return cloned
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Extract a CardRef from a list item's inlines, if the item contains
 * a Link with a "#" target.
 */
function extractRefFromInlines(
  inlines: Annotated_Inline[],
  sourceCardId: string,
  itemIndex: number,
  bulletListBodyIndex: number,
): CardRef | null {
  let hasCheckbox = false
  let checked = false

  for (const inline of inlines) {
    if (inline.t === 'Span') {
      hasCheckbox = true
      const spanContent = (inline.c as [unknown, Annotated_Inline[]])[1]
      checked = spanContent.length > 0 && spanContent.some(
        (i: Annotated_Inline) => i.t === 'Str' && i.c === 'x'
      )
    }

    if (inline.t === 'Link') {
      const linkContent = inline.c as [unknown, Annotated_Inline[], [string, string]]
      const target = linkContent[2][0]
      if (!target.startsWith('#')) continue

      const targetId = target.slice(1)
      const label = inlinesToText(linkContent[1])

      return {
        sourceCardId,
        targetCardId: targetId,
        label,
        isCheckbox: hasCheckbox,
        checked,
        itemIndex,
        bulletListBodyIndex,
      }
    }
  }

  return null
}

/**
 * Concatenate inline nodes into plain text.
 */
function inlinesToText(inlines: Annotated_Inline[]): string {
  const parts: string[] = []
  for (const inline of inlines) {
    switch (inline.t) {
      case 'Str':
        parts.push(inline.c as string)
        break
      case 'Space':
        parts.push(' ')
        break
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
