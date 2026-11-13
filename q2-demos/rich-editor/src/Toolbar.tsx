/**
 * The toolbar above the document, as in a word processor (the design is
 * the toolbar of `mockups/block-annotations.html`, with nothing in the
 * margin, no breadcrumb, and no outline on the document while its menu is
 * open). It follows the caret: the style box shows the style of the
 * caret's line in the body (Paragraph, Heading 1, …), and its menu changes
 * it; the quote button is pressed while the caret is in a quote, and
 * quotes or unquotes the line. With a selection, they change every line it
 * is on. A heading can't be in a quote yet, so headings can't be chosen
 * in a quote, and a heading can't be quoted. Outside the body, it's
 * disabled.
 *
 * It lives outside the document's frame. Pressing it doesn't take the
 * focus, so the caret stays where it was.
 */

import { useEffect, useRef, useState } from 'react'
import { headingLevel, headingPrefix, isQuote, QUOTE } from './editing/body.ts'
import { linePrefix, type State } from './engine/model.ts'

interface ToolbarProps {
  state: State
  /** Whether the document can be edited now. */
  writable: boolean
  /** The document's frame, whose clicks and keys close the style menu too. */
  frameDoc: Document | null
  /** Give each line the selection is on the prefix `prefixFor` returns for its own. */
  setPrefixes: (prefixFor: (prefix: string) => string) => void
}

const STYLES = [
  { name: 'Paragraph', level: 0 },
  { name: 'Heading 1', level: 1 },
  { name: 'Heading 2', level: 2 },
  { name: 'Heading 3', level: 3 },
  { name: 'Heading 4', level: 4 },
]

// Material Icons' "format_quote" (Apache 2.0), as in Google Docs' toolbar.
const QUOTE_ICON = (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <path d="M6 17h3l2-4V7H5v6h3zm8 0h3l2-4V7h-6v6h3z" />
  </svg>
)

const CHEVRON = (
  <svg viewBox="0 0 8 8" aria-hidden="true">
    <path d="M1 2.5 4 5.5 7 2.5" fill="none" stroke="currentColor" strokeWidth="1.4" />
  </svg>
)

export function Toolbar({ state, writable, frameDoc, setPrefixes }: ToolbarProps) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  // The caret's line in the body, by its prefix (null: the caret isn't in the body).
  const focus = state.selection?.focus
  const prefix = writable && focus?.field === 'body' ? linePrefix(state.fields.body ?? '', focus.offset) : null
  const disabled = prefix === null
  const level = prefix === null ? 0 : headingLevel(prefix)
  const quoted = prefix !== null && isQuote(prefix)
  const style = level > 0 ? `Heading ${level}` : 'Paragraph'

  useEffect(() => {
    if (disabled) setOpen(false)
  }, [disabled])

  // A press anywhere else, or Escape, closes the menu.
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.preventDefault()
      setOpen(false)
    }
    const docs = [document, frameDoc].filter((d): d is Document => d !== null)
    for (const d of docs) {
      d.addEventListener('mousedown', onDown, true)
      d.addEventListener('keydown', onKey, true)
    }
    return () => {
      for (const d of docs) {
        d.removeEventListener('mousedown', onDown, true)
        d.removeEventListener('keydown', onKey, true)
      }
    }
  }, [open, frameDoc])

  const choose = (to: number) => {
    setOpen(false)
    // A quote's line is a paragraph, and stays one.
    setPrefixes((p) => (isQuote(p) ? p : to > 0 ? headingPrefix(to) : ''))
  }

  const toggleQuote = () => {
    setOpen(false)
    setPrefixes((p) => (quoted ? (isQuote(p) ? '' : p) : p === '' ? QUOTE : p))
  }

  return (
    <div
      ref={ref}
      className="qe-toolbar"
      role="toolbar"
      aria-label="Formatting"
      // Keep the focus, and so the caret, in the document.
      onMouseDown={(e) => e.preventDefault()}
    >
      <div className="qe-style-box">
        <button
          type="button"
          className={`qe-style${open ? ' open' : ''}`}
          aria-label="Text style"
          aria-haspopup="menu"
          aria-expanded={open}
          disabled={disabled}
          onClick={() => setOpen((o) => !o)}
        >
          <span>{style}</span>
          {CHEVRON}
        </button>
        {open && (
          <div className="qe-menu" role="menu" aria-label="Text style">
            {STYLES.map((s) => (
              <button
                key={s.level}
                type="button"
                role="menuitemradio"
                aria-checked={s.level === level}
                className={`qe-item qe-item-${s.level}`}
                disabled={quoted && s.level > 0}
                onClick={() => choose(s.level)}
              >
                {s.name}
              </button>
            ))}
          </div>
        )}
      </div>
      <span className="qe-sep" />
      <button
        type="button"
        className="qe-icon"
        aria-label="Quote"
        title="Quote"
        aria-pressed={quoted}
        disabled={disabled || level > 0}
        onClick={toggleQuote}
      >
        {QUOTE_ICON}
      </button>
    </div>
  )
}
