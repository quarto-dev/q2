/**
 * The row of "+" items that add the title-block fields a document doesn't
 * have ("+ Title  + Subtitle  + Abstract"), above the title block.
 *
 * It stays out of the way. It's shown only while the caret is in the title
 * block, the pointer is over it, the focus is on one of its items, or the
 * document is empty; and it never takes space of its own: it's laid over
 * the space above the title block (or above the body, when there's no title
 * block), so showing it moves nothing and the page is laid out exactly as
 * `q2 render` lays it out.
 *
 * It's outside the editing surface, but the keyboard reaches it: Left at
 * the start of the first field or Up on its first line goes to it, and so
 * does Backspace removing the first field (to that field's item). In the
 * row, Left and Right move along it (Left stops at the first item), Enter
 * adds the item's field, and Right past the last item, Down or Escape go
 * back to the start of the first field.
 */

import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react'
import { createPortal } from 'react-dom'

/** Where the focus goes in the row, when the caret leaves the fields for it. */
export interface WidgetFocus {
  /** The item for this field (Backspace removed it). */
  field?: string
  /** The item nearest this horizontal position (Up from the first line). */
  x?: number
}

interface TitleBlockWidgetProps {
  /** The editor's frame document, where the row is drawn. */
  doc: Document
  /** The document's content element (`main`), which the row sits above. */
  content: HTMLElement
  /** The fields the row can add, in reading order, with their labels. */
  fields: readonly { name: string; label: string }[]
  /** The fields the document has. */
  present: ReadonlySet<string>
  /** Whether the caret is in the title block. */
  caretInTitleBlock: boolean
  /** Whether the document is empty: no title block and no body. */
  empty: boolean
  /** Where to put the focus in the row, once it's drawn; null: leave it. */
  focus: WidgetFocus | null
  /** Called once `focus` has been put. */
  onFocused: () => void
  onAdd: (field: string) => void
  /** Leave the row for the start of the first field. */
  onExit: () => void
}

const CSS = `
.qe-widget { position: absolute; z-index: 1; display: flex; flex-wrap: wrap; gap: 2px;
  opacity: 0; pointer-events: none; transition: opacity .12s; }
.qe-widget.qe-shown, .qe-widget:focus-within { opacity: 1; pointer-events: auto; }
.qe-widget button { font: 12px/12px system-ui, -apple-system, "Segoe UI", Roboto, sans-serif; color: #6c757d;
  background: none; border: 0; padding: 2px 6px; border-radius: 4px; cursor: pointer; }
.qe-widget button:hover, .qe-widget button:focus { background: #eef0f3; color: #212529; outline: none; }
`

export function TitleBlockWidget({
  doc,
  content,
  fields,
  present,
  caretInTitleBlock,
  empty,
  focus,
  onFocused,
  onAdd,
  onExit,
}: TitleBlockWidgetProps) {
  const row = useRef<HTMLDivElement | null>(null)
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null)
  const hovered = useHovered(doc, content, row)
  const missing = fields.filter((f) => !present.has(f.name))

  useEffect(() => {
    const style = doc.createElement('style')
    style.textContent = CSS
    doc.head.appendChild(style)
    return () => style.remove()
  }, [doc])

  // Lay the row over the space above the title block, or above the body
  // when there's none, after every render and whenever the page reflows.
  useLayoutEffect(() => {
    const place = () => {
      const el = row.current
      if (!el) return
      const anchor = content.querySelector('#title-block-header') ?? content
      const box = anchor.getBoundingClientRect()
      const scroll = { x: doc.defaultView?.scrollX ?? 0, y: doc.defaultView?.scrollY ?? 0 }
      const next = { top: box.top + scroll.y - el.offsetHeight - 2, left: content.getBoundingClientRect().left + scroll.x - 6 }
      setPosition((p) => (p && p.top === next.top && p.left === next.left ? p : next))
    }
    place()
    const observer = new ResizeObserver(place)
    observer.observe(content)
    return () => observer.disconnect()
  })

  // Put the focus where the caret left the fields for, once the row has
  // the item (a field Backspace just removed has its item only now).
  useLayoutEffect(() => {
    const el = row.current
    if (!focus || !el) return
    const items = [...el.querySelectorAll<HTMLButtonElement>('button')]
    if (items.length === 0) return
    const item =
      (focus.field !== undefined && items.find((b) => b.dataset.add === focus.field)) ||
      (focus.x !== undefined ? nearest(items, focus.x) : items[items.length - 1])
    item.focus()
    onFocused()
  })

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    const items = [...(row.current?.querySelectorAll<HTMLButtonElement>('button') ?? [])]
    const at = items.indexOf(e.currentTarget)
    // The row's keys are the row's: once one has put the caret back in a
    // field, the editor mustn't take it as a key of its own and move on.
    if (['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Escape'].includes(e.key)) e.stopPropagation()
    if (e.key === 'ArrowLeft') {
      e.preventDefault()
      items[at - 1]?.focus()
    } else if (e.key === 'ArrowRight') {
      e.preventDefault()
      if (items[at + 1]) items[at + 1].focus()
      else onExit()
    } else if (e.key === 'ArrowDown' || e.key === 'Escape') {
      e.preventDefault()
      onExit()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
    }
  }

  const shown = caretInTitleBlock || hovered || empty
  return createPortal(
    <div
      ref={row}
      className={`qe-widget${shown ? ' qe-shown' : ''}`}
      role="toolbar"
      aria-label="Add to the title block"
      style={position ? { top: position.top, left: position.left } : { visibility: 'hidden' }}
    >
      {missing.map((f) => (
        <button
          key={f.name}
          type="button"
          data-add={f.name}
          tabIndex={-1}
          // Keep the caret where it is until the click adds the field.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => onAdd(f.name)}
          onKeyDown={onKeyDown}
        >
          + {f.label}
        </button>
      ))}
    </div>,
    doc.body,
  )
}

/** The element whose horizontal middle is nearest `x`. */
function nearest<T extends Element>(items: T[], x: number): T {
  const distance = (el: T) => {
    const r = el.getBoundingClientRect()
    return x < r.left ? r.left - x : x > r.right ? x - r.right : 0
  }
  return items.reduce((a, b) => (distance(b) < distance(a) ? b : a))
}

/**
 * Whether the pointer is over the title block, or the space above it where
 * the row is (over the top of the body when there's no title block).
 */
function useHovered(doc: Document, content: HTMLElement, row: { current: HTMLElement | null }): boolean {
  const [hovered, setHovered] = useState(false)
  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const header = content.querySelector('#title-block-header')
      const top = row.current?.getBoundingClientRect().top ?? content.getBoundingClientRect().top
      const bottom = header ? header.getBoundingClientRect().bottom : content.getBoundingClientRect().top + 8
      const { left, right } = content.getBoundingClientRect()
      setHovered(e.clientY >= top && e.clientY <= bottom && e.clientX >= left - 8 && e.clientX <= right)
    }
    const onLeave = () => setHovered(false)
    doc.addEventListener('mousemove', onMove)
    doc.documentElement.addEventListener('mouseleave', onLeave)
    return () => {
      doc.removeEventListener('mousemove', onMove)
      doc.documentElement.removeEventListener('mouseleave', onLeave)
    }
  }, [doc, content, row])
  return hovered
}
