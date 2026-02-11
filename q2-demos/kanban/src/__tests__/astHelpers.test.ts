/**
 * Layer 1 tests for astHelpers — pure AST extraction and mutation.
 *
 * Uses pre-generated AST JSON fixtures (no WASM, no DOM).
 */

import { describe, it, expect } from 'vitest'
import type { RustQmdJson } from '@quarto/pandoc-types'
import { extractCards, extractCardRefs, buildBoard, setCardStatus, toggleMilestoneItem, addCard } from '../astHelpers.ts'
import example1Fixture from './fixtures/example1.json'

const ast = example1Fixture as unknown as RustQmdJson

// ---------------------------------------------------------------------------
// extractCards
// ---------------------------------------------------------------------------

describe('extractCards', () => {
  const cards = extractCards(ast)

  it('extracts all level-2 header cards', () => {
    expect(cards).toHaveLength(5)
  })

  it('extracts card ids from header slugs', () => {
    expect(cards.map(c => c.id)).toEqual([
      'work-week',
      'positconf',
      'project-export',
      'connect-cloud-integration',
      'acls-for-automerge',
    ])
  })

  it('extracts card titles from header inlines', () => {
    expect(cards.map(c => c.title)).toEqual([
      'Work Week',
      'Posit::conf',
      'Project Export',
      'Connect cloud integration',
      'ACLs for automerge',
    ])
  })

  it('extracts card types from header classes', () => {
    expect(cards.map(c => c.type)).toEqual([
      'milestone',
      'milestone',
      'feature',
      'feature',
      'feature',
    ])
  })

  it('extracts deadline attribute on milestones', () => {
    const workWeek = cards[0]
    expect(workWeek.deadline).toBe('2026-03-25')

    const positconf = cards[1]
    expect(positconf.deadline).toBe('2026-07-25')
  })

  it('extracts created attribute', () => {
    expect(cards[0].created).toBe('2026-02-10T08:56')
    expect(cards[2].created).toBe('2026-02-10T14:30')
    expect(cards[4].created).toBe('2026-02-10T14:40')
  })

  it('returns undefined for missing attributes', () => {
    // No cards in example1 have status or priority
    for (const card of cards) {
      expect(card.status).toBeUndefined()
      expect(card.priority).toBeUndefined()
    }
  })

  it('collects body blocks between headers', () => {
    // "Work Week" milestone has a Para ("Items:") and a BulletList
    const workWeek = cards[0]
    expect(workWeek.bodyBlocks).toHaveLength(2)
    expect(workWeek.bodyBlocks[0].t).toBe('Para')
    expect(workWeek.bodyBlocks[1].t).toBe('BulletList')

    // "Posit::conf" has a Para and a BulletList
    const positconf = cards[1]
    expect(positconf.bodyBlocks).toHaveLength(2)

    // "Project Export" has an empty body (next block is another Header)
    const projectExport = cards[2]
    expect(projectExport.bodyBlocks).toHaveLength(0)

    // "Connect cloud integration" has a Para and a BulletList (dependency)
    const connect = cards[3]
    expect(connect.bodyBlocks).toHaveLength(2)

    // "ACLs for automerge" has a Para (the description paragraph)
    const acls = cards[4]
    expect(acls.bodyBlocks).toHaveLength(1)
    expect(acls.bodyBlocks[0].t).toBe('Para')
  })

  it('records header block indices', () => {
    // Block 0 is the level-1 header "# Cards"
    // Block 1 is "## Work Week" => headerBlockIndex = 1
    expect(cards[0].headerBlockIndex).toBe(1)
    // Block 4 is "## Posit::conf"
    expect(cards[1].headerBlockIndex).toBe(4)
  })
})

// ---------------------------------------------------------------------------
// extractCardRefs
// ---------------------------------------------------------------------------

describe('extractCardRefs', () => {
  const cards = extractCards(ast)

  it('extracts checkbox refs from milestone items', () => {
    const workWeek = cards[0]
    const refs = extractCardRefs(workWeek)
    expect(refs).toHaveLength(1)
    expect(refs[0].sourceCardId).toBe('work-week')
    expect(refs[0].targetCardId).toBe('project-export')
    expect(refs[0].label).toBe('Project Export')
    expect(refs[0].isCheckbox).toBe(true)
    expect(refs[0].checked).toBe(false)
  })

  it('extracts checkbox refs from second milestone', () => {
    const positconf = cards[1]
    const refs = extractCardRefs(positconf)
    expect(refs).toHaveLength(1)
    expect(refs[0].targetCardId).toBe('connect-cloud-integration')
    expect(refs[0].label).toBe('Connect cloud integration')
    expect(refs[0].isCheckbox).toBe(true)
    expect(refs[0].checked).toBe(false)
  })

  it('extracts dependency refs (non-checkbox links)', () => {
    const connect = cards[3]
    const refs = extractCardRefs(connect)
    expect(refs).toHaveLength(1)
    expect(refs[0].sourceCardId).toBe('connect-cloud-integration')
    expect(refs[0].targetCardId).toBe('acls-for-automerge')
    expect(refs[0].label).toBe('ACLs for automerge')
    expect(refs[0].isCheckbox).toBe(false)
    expect(refs[0].checked).toBe(false)
  })

  it('returns empty array for cards with no refs', () => {
    const projectExport = cards[2]
    expect(extractCardRefs(projectExport)).toEqual([])

    const acls = cards[4]
    expect(extractCardRefs(acls)).toEqual([])
  })
})

// ---------------------------------------------------------------------------
// buildBoard
// ---------------------------------------------------------------------------

describe('buildBoard', () => {
  const board = buildBoard(ast)

  it('extracts all cards', () => {
    expect(board.cards).toHaveLength(5)
  })

  it('collects all refs from all cards', () => {
    // 1 from Work Week + 1 from Posit::conf + 1 from Connect = 3 total
    expect(board.refs).toHaveLength(3)
  })

  it('refs point to valid card ids', () => {
    const cardIds = new Set(board.cards.map(c => c.id))
    for (const ref of board.refs) {
      expect(cardIds.has(ref.sourceCardId)).toBe(true)
      expect(cardIds.has(ref.targetCardId)).toBe(true)
    }
  })
})

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

describe('edge cases', () => {
  it('handles a document with no level-2 headers', () => {
    const emptyAst: RustQmdJson = {
      blocks: [
        { t: 'Header', c: [1, ['title', [], []], [{ t: 'Str', c: 'Title', s: 0 }]], s: 0, attrS: { classes: [], id: null, kvs: [] } },
        { t: 'Para', c: [{ t: 'Str', c: 'Some text', s: 0 }], s: 0 },
      ],
      meta: {},
      'pandoc-api-version': [1, 23, 1],
      astContext: { files: [], sourceInfoPool: [] },
    } as unknown as RustQmdJson
    const board = buildBoard(emptyAst)
    expect(board.cards).toHaveLength(0)
    expect(board.refs).toHaveLength(0)
  })

  it('handles a card with no class (generic card)', () => {
    const ast: RustQmdJson = {
      blocks: [
        { t: 'Header', c: [2, ['my-card', [], []], [{ t: 'Str', c: 'My Card', s: 0 }]], s: 0, attrS: { classes: [], id: null, kvs: [] } },
      ],
      meta: {},
      'pandoc-api-version': [1, 23, 1],
      astContext: { files: [], sourceInfoPool: [] },
    } as unknown as RustQmdJson
    const cards = extractCards(ast)
    expect(cards).toHaveLength(1)
    expect(cards[0].type).toBeUndefined()
    expect(cards[0].title).toBe('My Card')
  })

  it('handles a card with unknown class gracefully', () => {
    const ast: RustQmdJson = {
      blocks: [
        { t: 'Header', c: [2, ['my-card', ['exotic'], []], [{ t: 'Str', c: 'Card', s: 0 }]], s: 0, attrS: { classes: [], id: null, kvs: [] } },
      ],
      meta: {},
      'pandoc-api-version': [1, 23, 1],
      astContext: { files: [], sourceInfoPool: [] },
    } as unknown as RustQmdJson
    const cards = extractCards(ast)
    expect(cards).toHaveLength(1)
    expect(cards[0].type).toBeUndefined()
  })
})

// ---------------------------------------------------------------------------
// setCardStatus
// ---------------------------------------------------------------------------

describe('setCardStatus', () => {
  it('adds status attribute when missing', () => {
    const result = setCardStatus(ast, 'project-export', 'doing')
    expect(result).not.toBeNull()

    const cards = extractCards(result!)
    const card = cards.find(c => c.id === 'project-export')
    expect(card?.status).toBe('doing')
  })

  it('updates existing status attribute', () => {
    // First add a status, then change it
    const withStatus = setCardStatus(ast, 'project-export', 'doing')!
    const updated = setCardStatus(withStatus, 'project-export', 'done')
    expect(updated).not.toBeNull()

    const cards = extractCards(updated!)
    const card = cards.find(c => c.id === 'project-export')
    expect(card?.status).toBe('done')
  })

  it('preserves other attributes when adding status', () => {
    const result = setCardStatus(ast, 'work-week', 'doing')!
    const cards = extractCards(result)
    const card = cards.find(c => c.id === 'work-week')
    expect(card?.status).toBe('doing')
    expect(card?.deadline).toBe('2026-03-25')
    expect(card?.created).toBe('2026-02-10T08:56')
  })

  it('does not mutate the original AST', () => {
    const originalCards = extractCards(ast)
    setCardStatus(ast, 'project-export', 'doing')
    const afterCards = extractCards(ast)
    expect(afterCards[2].status).toBeUndefined()
    expect(originalCards[2].status).toBeUndefined()
  })

  it('returns null for unknown card id', () => {
    const result = setCardStatus(ast, 'nonexistent', 'todo')
    expect(result).toBeNull()
  })

  it('preserves other cards unchanged', () => {
    const result = setCardStatus(ast, 'project-export', 'doing')!
    const cards = extractCards(result)
    // Other cards should be untouched
    expect(cards[0].status).toBeUndefined()
    expect(cards[1].status).toBeUndefined()
    expect(cards[3].status).toBeUndefined()
    expect(cards[4].status).toBeUndefined()
  })
})

// ---------------------------------------------------------------------------
// toggleMilestoneItem
// ---------------------------------------------------------------------------

describe('toggleMilestoneItem', () => {
  it('checks an unchecked milestone item', () => {
    const result = toggleMilestoneItem(ast, 'work-week', 0)
    expect(result).not.toBeNull()

    const cards = extractCards(result!)
    const refs = extractCardRefs(cards[0])
    expect(refs[0].checked).toBe(true)
  })

  it('unchecks a checked milestone item', () => {
    // First check it, then uncheck
    const checked = toggleMilestoneItem(ast, 'work-week', 0)!
    const unchecked = toggleMilestoneItem(checked, 'work-week', 0)
    expect(unchecked).not.toBeNull()

    const cards = extractCards(unchecked!)
    const refs = extractCardRefs(cards[0])
    expect(refs[0].checked).toBe(false)
  })

  it('does not mutate the original AST', () => {
    toggleMilestoneItem(ast, 'work-week', 0)
    const cards = extractCards(ast)
    const refs = extractCardRefs(cards[0])
    expect(refs[0].checked).toBe(false)
  })

  it('returns null for unknown card id', () => {
    expect(toggleMilestoneItem(ast, 'nonexistent', 0)).toBeNull()
  })

  it('returns null for out-of-range item index', () => {
    expect(toggleMilestoneItem(ast, 'work-week', 99)).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// addCard
// ---------------------------------------------------------------------------

describe('addCard', () => {
  it('appends a new card at the end of the document', () => {
    const result = addCard(ast, 'New Feature', 'feature')
    expect(result).not.toBeNull()

    const cards = extractCards(result!)
    expect(cards).toHaveLength(6)
    expect(cards[5].title).toBe('New Feature')
    expect(cards[5].type).toBe('feature')
    expect(cards[5].id).toBe('new-feature')
  })

  it('sets created timestamp', () => {
    const result = addCard(ast, 'Test Card', 'task')!
    const cards = extractCards(result)
    const newCard = cards[cards.length - 1]
    expect(newCard.created).toBeDefined()
    // Should be an ISO-ish timestamp
    expect(newCard.created).toMatch(/^\d{4}-\d{2}-\d{2}T/)
  })

  it('does not mutate the original AST', () => {
    addCard(ast, 'Test', 'bug')
    expect(extractCards(ast)).toHaveLength(5)
  })

  it('generates unique slugs for duplicate titles', () => {
    // Add a card with a title that would clash with "Project Export"
    const result = addCard(ast, 'Project Export', 'feature')!
    const cards = extractCards(result)
    const ids = cards.map(c => c.id)
    // Should have 6 unique ids
    expect(new Set(ids).size).toBe(6)
  })

  it('creates card with no type', () => {
    const result = addCard(ast, 'Generic Card')!
    const cards = extractCards(result)
    const newCard = cards[cards.length - 1]
    expect(newCard.type).toBeUndefined()
    expect(newCard.title).toBe('Generic Card')
  })
})
