/**
 * Layer 2 component rendering tests (jsdom + React Testing Library).
 *
 * Tests that React components render the correct DOM given specific
 * KanbanBoard data, and that interactions fire the right callbacks.
 */

import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { userEvent } from '@testing-library/user-event'
import { CardComponent } from '../components/CardComponent.tsx'
import { BoardView } from '../components/BoardView.tsx'
import type { KanbanCard } from '../types.ts'
import type { Annotated_Block } from '@quarto/pandoc-types'

function makeCard(overrides: Partial<KanbanCard> = {}): KanbanCard {
  return {
    id: 'test-card',
    title: 'Test Card',
    type: 'feature',
    status: undefined,
    created: '2026-02-10T14:30',
    deadline: undefined,
    priority: undefined,
    bodyBlocks: [],
    headerBlockIndex: 0,
    ...overrides,
  }
}

// ---------------------------------------------------------------------------
// CardComponent
// ---------------------------------------------------------------------------

describe('CardComponent', () => {
  it('renders the card title', () => {
    render(<CardComponent card={makeCard({ title: 'My Feature' })} />)
    expect(screen.getByText('My Feature')).toBeDefined()
  })

  it('renders the card type as a badge', () => {
    render(<CardComponent card={makeCard({ type: 'milestone' })} />)
    expect(screen.getByText('milestone')).toBeDefined()
  })

  it('renders deadline when present', () => {
    render(<CardComponent card={makeCard({ deadline: '2026-03-25' })} />)
    expect(screen.getByText(/2026-03-25/)).toBeDefined()
  })

  it('does not render deadline when absent', () => {
    const { container } = render(<CardComponent card={makeCard()} />)
    expect(container.textContent).not.toContain('deadline')
  })

  it('renders a status selector', () => {
    render(<CardComponent card={makeCard({ status: 'doing' })} />)
    const select = screen.getByRole('combobox')
    expect(select).toBeDefined()
    expect((select as HTMLSelectElement).value).toBe('doing')
  })

  it('calls onStatusChange when status is changed', async () => {
    const onStatusChange = vi.fn()
    render(
      <CardComponent
        card={makeCard({ id: 'my-card', status: 'todo' })}
        onStatusChange={onStatusChange}
      />
    )
    const select = screen.getByRole('combobox')
    await userEvent.selectOptions(select, 'done')
    expect(onStatusChange).toHaveBeenCalledWith('my-card', 'done')
  })

  it('renders body preview when card has body blocks', () => {
    const bodyBlocks: Annotated_Block[] = [
      {
        t: 'Para',
        c: [{ t: 'Str', c: 'Some description text', s: 0 }],
        s: 0,
      } as Annotated_Block,
    ]
    render(<CardComponent card={makeCard({ bodyBlocks })} />)
    expect(screen.getByText('Some description text')).toBeDefined()
  })
})

// ---------------------------------------------------------------------------
// BoardView
// ---------------------------------------------------------------------------

describe('BoardView', () => {
  const cards: KanbanCard[] = [
    makeCard({ id: 'card-1', title: 'Todo Card', status: 'todo' }),
    makeCard({ id: 'card-2', title: 'Doing Card', status: 'doing' }),
    makeCard({ id: 'card-3', title: 'Done Card', status: 'done' }),
    makeCard({ id: 'card-4', title: 'Unset Card', status: undefined }),
  ]

  it('renders all status columns', () => {
    render(<BoardView cards={cards} />)
    expect(screen.getByText('Todo')).toBeDefined()
    expect(screen.getByText('Doing')).toBeDefined()
    expect(screen.getByText('Done')).toBeDefined()
    expect(screen.getByText('Unset')).toBeDefined()
  })

  it('places cards in the correct columns', () => {
    render(<BoardView cards={cards} />)
    // Each card title should appear
    expect(screen.getByText('Todo Card')).toBeDefined()
    expect(screen.getByText('Doing Card')).toBeDefined()
    expect(screen.getByText('Done Card')).toBeDefined()
    expect(screen.getByText('Unset Card')).toBeDefined()
  })

  it('renders empty columns gracefully', () => {
    const onlyTodo = [makeCard({ id: 'card-1', title: 'One Card', status: 'todo' })]
    render(<BoardView cards={onlyTodo} />)
    // Should still render all columns
    expect(screen.getByText('Todo')).toBeDefined()
    expect(screen.getByText('Doing')).toBeDefined()
    expect(screen.getByText('Done')).toBeDefined()
  })

  it('passes onStatusChange through to cards', async () => {
    const onStatusChange = vi.fn()
    render(<BoardView cards={cards} onStatusChange={onStatusChange} />)
    // Find the first combobox and change it
    const selects = screen.getAllByRole('combobox')
    await userEvent.selectOptions(selects[0], 'done')
    expect(onStatusChange).toHaveBeenCalled()
  })
})
