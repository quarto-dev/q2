/**
 * The toolbar above the document, as in a word processor (the design is
 * the toolbar of `mockups/block-annotations.html`, with nothing in the
 * margin, no breadcrumb, and no outline on the document while its menu is
 * open). It follows the caret: the style box shows the style of the
 * caret's line in the body (Paragraph, Heading 1, …), and its menu changes
 * it; the bulleted list button is pressed while the caret is on a bullet
 * list's item, and makes the line an item (a bullet one, from a numbered
 * one too) or a paragraph again, and the numbered list button does the
 * same for numbered lists; the quote button
 * is pressed while the caret is in a quote, and quotes or unquotes the
 * line. With a selection, they change every line it is on. A heading can't
 * be in a quote yet, so headings can't be chosen in a quote, and a heading
 * can't be quoted. Nor can a list item be a heading or a quote yet, so on
 * an item neither can be chosen, and a heading or a quote can't be made an
 * item. Between the style box and the list buttons are the formatting
 * buttons: bold, italic, underline and highlight, pressed while what's
 * selected (or what typing at the caret gives) has the format, and turning
 * it on or off; and, after a separator, the link button, pressed while the
 * caret is in a link, which opens the link form. Outside the body, it's
 * disabled, but for its undo and redo buttons, which come first and work
 * wherever the caret is, as the keyboard's undo and redo do. At its right
 * end, where the page has one, is the Developer tools button, which shows
 * and hides the source pane.
 *
 * Resting the mouse on a button shows a tooltip with its name and keyboard
 * shortcut (`Tooltip.tsx`), and the style menu gives each style's shortcut
 * (`shortcuts.ts`). What the style menu and the list and quote buttons do,
 * and when they can, is `editing/lineStyles.ts`, shared with their
 * shortcuts.
 *
 * It lives outside the document's frame. Pressing it doesn't take the
 * focus, so the caret stays where it was.
 */

import { useEffect, useRef, useState } from 'react'
import { BULLET, NUMBERED } from './editing/body.ts'
import { EMPH, HIGHLIGHT, STRONG, UNDERLINE } from './editing/inlines.ts'
import { canChoose, canList, canQuote, caretLine, chooseStyle, toggleList, toggleQuote } from './editing/lineStyles.ts'
import { type State } from './engine/model.ts'
import { ariaKeyshortcuts, SHORTCUTS, shortcutText, styleShortcut, type Shortcut } from './shortcuts.ts'
import { useTooltips } from './Tooltip.tsx'

interface ToolbarProps {
  state: State
  /** Whether the document can be edited now. */
  writable: boolean
  /** The document's frame, whose clicks and keys close the style menu too. */
  frameDoc: Document | null
  /** Give each line the selection is on the prefix `prefixFor` returns for its own. */
  setPrefixes: (prefixFor: (prefix: string) => string) => void
  /** Whether there is an edit to undo, and one to redo. */
  canUndo: boolean
  canRedo: boolean
  undo: () => void
  redo: () => void
  /** The formats of what's selected (null: the selection can't be formatted). */
  marks: number | null
  /** Turn a format on or off. */
  toggleMark: (format: number) => void
  /** Whether the caret is in a link. */
  inLink: boolean
  /** Open the link form. */
  onLink: () => void
  /** Whether the source pane shows, and show or hide it (absent: no source pane to show). */
  devTools?: { open: boolean; toggle: () => void }
}

// Material Icons (Apache 2.0), as in Google Docs' toolbar.
const icon = (d: string) => (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <path d={d} />
  </svg>
)

/** The formatting buttons: name, format, keyboard shortcut and icon. */
const FORMATS = [
  {
    name: 'Bold',
    format: STRONG,
    shortcut: SHORTCUTS.bold,
    icon: icon('M15.6 10.79c.97-.67 1.65-1.77 1.65-2.79 0-2.26-1.75-4-4-4H7v14h7.04c2.09 0 3.71-1.7 3.71-3.79 0-1.52-.86-2.82-2.15-3.42zM10 6.5h3c.83 0 1.5.67 1.5 1.5s-.67 1.5-1.5 1.5h-3v-3zm3.5 9H10v-3h3.5c.83 0 1.5.67 1.5 1.5s-.67 1.5-1.5 1.5z'),
  },
  { name: 'Italic', format: EMPH, shortcut: SHORTCUTS.italic, icon: icon('M10 4v3h2.21l-3.42 8H6v3h8v-3h-2.21l3.42-8H18V4z') },
  {
    name: 'Underline',
    format: UNDERLINE,
    shortcut: SHORTCUTS.underline,
    icon: icon('M12 17c3.31 0 6-2.69 6-6V3h-2.5v8c0 1.93-1.57 3.5-3.5 3.5S8.5 12.93 8.5 11V3H6v8c0 3.31 2.69 6 6 6zm-7 2v2h14v-2H5z'),
  },
  {
    name: 'Highlight',
    format: HIGHLIGHT,
    shortcut: SHORTCUTS.highlight,
    // "border_color": a marker pen over a bar.
    icon: icon('M16.81 8.94l-3.75-3.75L4 14.25V18h3.75l9.06-9.06zM6 16v-.92l7.06-7.06.92.92L6.92 16H6zm13.71-9.96c.39-.39.39-1.02 0-1.41l-2.34-2.34a.9959.9959 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83zM2 20h20v4H2z'),
  },
]

const LINK_ICON = icon(
  'M3.9 12c0-1.71 1.39-3.1 3.1-3.1h4V7H7c-2.76 0-5 2.24-5 5s2.24 5 5 5h4v-1.9H7c-1.71 0-3.1-1.39-3.1-3.1zM8 13h8v-2H8v2zm9-6h-4v1.9h4c1.71 0 3.1 1.39 3.1 3.1s-1.39 3.1-3.1 3.1h-4V17h4c2.76 0 5-2.24 5-5s-2.24-5-5-5z',
)

const STYLES = [
  { name: 'Paragraph', level: 0 },
  { name: 'Heading 1', level: 1 },
  { name: 'Heading 2', level: 2 },
  { name: 'Heading 3', level: 3 },
  { name: 'Heading 4', level: 4 },
]

// Lucide's "undo-2" and "redo-2" (ISC): an arrow that turns back on itself.
const UNDO_ICON = (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="M9 14 4 9l5-5" />
    <path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11" />
  </svg>
)

const REDO_ICON = (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="m15 14 5-5-5-5" />
    <path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13" />
  </svg>
)

// Material Icons' "format_list_bulleted" (Apache 2.0), as in Google Docs' toolbar.
const LIST_ICON = (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <path d="M4 10.5c-.83 0-1.5.67-1.5 1.5s.67 1.5 1.5 1.5 1.5-.67 1.5-1.5-.67-1.5-1.5-1.5zm0-6c-.83 0-1.5.67-1.5 1.5S3.17 7.5 4 7.5 5.5 6.83 5.5 6 4.83 4.5 4 4.5zm0 12c-.83 0-1.5.68-1.5 1.5s.68 1.5 1.5 1.5 1.5-.68 1.5-1.5-.67-1.5-1.5-1.5zM7 19h14v-2H7v2zm0-6h14v-2H7v2zm0-8v2h14V5H7z" />
  </svg>
)

// Material Icons' "format_list_numbered" (Apache 2.0), as in Google Docs' toolbar.
const NUMBERED_ICON = (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <path d="M2 17h2v.5H3v1h1v.5H2v1h3v-4H2v1zm1-9h1V4H2v1h1v3zm-1 3h1.8L2 13.1v.9h3v-1H3.2L5 10.9V10H2v1zm5-6v2h14V5H7zm0 14h14v-2H7v2zm0-6h14v-2H7v2z" />
  </svg>
)

// Lucide's "code-xml" (ISC): angle brackets around a slash.
const DEV_TOOLS_ICON = (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    <path d="m18 16 4-4-4-4" />
    <path d="m6 8-4 4 4 4" />
    <path d="m14.5 4-5 16" />
  </svg>
)

// Material Icons' "format_quote" (Apache 2.0), as in Google Docs' toolbar.
const QUOTE_ICON = (
  <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <path d="M6 17h3l2-4V7H5v6h3zm8 0h3l2-4V7h-6v6h3z" />
  </svg>
)

/**
 * A button's tooltip (its name, and its shortcut if it has one), and its
 * shortcut for assistive technology.
 */
const tip = (name: string, shortcut?: Shortcut) => ({
  'data-tip': name,
  'data-keys': shortcut && shortcutText(shortcut),
  'aria-keyshortcuts': shortcut && ariaKeyshortcuts(shortcut),
})

const CHEVRON = (
  <svg viewBox="0 0 8 8" aria-hidden="true">
    <path d="M1 2.5 4 5.5 7 2.5" fill="none" stroke="currentColor" strokeWidth="1.4" />
  </svg>
)

export function Toolbar({
  state,
  writable,
  frameDoc,
  setPrefixes,
  canUndo,
  canRedo,
  undo,
  redo,
  marks,
  toggleMark,
  inLink,
  onLink,
  devTools,
}: ToolbarProps) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const tooltips = useTooltips(ref)

  // The caret's line in the body (null: the caret isn't in the body).
  const line = caretLine(state, writable)
  // A caption has no style to choose (see `canChoose`).
  const disabled = line === null || line.caption
  const level = line?.level ?? 0
  const style = line?.caption ? 'Caption' : level > 0 ? `Heading ${level}` : 'Paragraph'

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

  // Change the lines the selection is on, closing the menu.
  const change = (prefixFor: (prefix: string) => string) => {
    setOpen(false)
    setPrefixes(prefixFor)
  }

  return (
    <div
      ref={ref}
      className="qe-toolbar"
      role="toolbar"
      aria-label="Formatting"
      onMouseDown={(e) => {
        // Keep the focus, and so the caret, in the document.
        e.preventDefault()
        tooltips.onMouseDown()
      }}
      onMouseMove={tooltips.onMouseMove}
      onMouseLeave={tooltips.onMouseLeave}
    >
      <button
        type="button"
        className="qe-icon"
        aria-label="Undo"
        {...tip('Undo', SHORTCUTS.undo)}
        disabled={!writable || !canUndo}
        onClick={() => {
          setOpen(false)
          undo()
        }}
      >
        {UNDO_ICON}
      </button>
      <button
        type="button"
        className="qe-icon"
        aria-label="Redo"
        {...tip('Redo', SHORTCUTS.redo)}
        disabled={!writable || !canRedo}
        onClick={() => {
          setOpen(false)
          redo()
        }}
      >
        {REDO_ICON}
      </button>
      <span className="qe-sep" />
      <div className="qe-style-box">
        <button
          type="button"
          className={`qe-style${open ? ' open' : ''}`}
          aria-label="Text style"
          // No tooltip while its menu is open.
          {...(open ? {} : tip('Text style'))}
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
                aria-label={s.name}
                aria-keyshortcuts={ariaKeyshortcuts(styleShortcut(s.level))}
                className={`qe-item qe-item-${s.level}`}
                disabled={line !== null && !canChoose(line, s.level)}
                onClick={() => change(chooseStyle(s.level))}
              >
                <span>{s.name}</span>
                <kbd>{shortcutText(styleShortcut(s.level))}</kbd>
              </button>
            ))}
          </div>
        )}
      </div>
      <span className="qe-sep" />
      {FORMATS.map((f) => (
        <button
          key={f.name}
          type="button"
          className="qe-icon"
          aria-label={f.name}
          {...tip(f.name, f.shortcut)}
          aria-pressed={marks !== null && (marks & (1 << f.format)) !== 0}
          disabled={disabled || marks === null}
          onClick={() => {
            setOpen(false)
            toggleMark(f.format)
          }}
        >
          {f.icon}
        </button>
      ))}
      <span className="qe-sep" />
      <button
        type="button"
        className="qe-icon"
        aria-label="Link"
        {...tip('Link', SHORTCUTS.link)}
        aria-pressed={inLink}
        disabled={disabled || marks === null}
        onClick={() => {
          setOpen(false)
          onLink()
        }}
      >
        {LINK_ICON}
      </button>
      <span className="qe-sep" />
      <button
        type="button"
        className="qe-icon"
        aria-label="Bulleted list"
        {...tip('Bulleted list', SHORTCUTS.bulletList)}
        aria-pressed={line?.bulleted ?? false}
        disabled={line === null || !canList(line)}
        onClick={() => line && change(toggleList(line, BULLET))}
      >
        {LIST_ICON}
      </button>
      <button
        type="button"
        className="qe-icon"
        aria-label="Numbered list"
        {...tip('Numbered list', SHORTCUTS.numberedList)}
        aria-pressed={line?.numbered ?? false}
        disabled={line === null || !canList(line)}
        onClick={() => line && change(toggleList(line, NUMBERED))}
      >
        {NUMBERED_ICON}
      </button>
      <button
        type="button"
        className="qe-icon"
        aria-label="Quote"
        {...tip('Quote', SHORTCUTS.quote)}
        aria-pressed={line?.quoted ?? false}
        disabled={line === null || !canQuote(line)}
        onClick={() => line && change(toggleQuote(line))}
      >
        {QUOTE_ICON}
      </button>
      {devTools && (
        <button
          type="button"
          className="qe-icon qe-dev-tools"
          aria-label="Developer tools"
          {...tip(devTools.open ? 'Hide source' : 'Show source')}
          aria-pressed={devTools.open}
          onClick={() => {
            setOpen(false)
            devTools.toggle()
          }}
        >
          {DEV_TOOLS_ICON}
        </button>
      )}
      {tooltips.element}
    </div>
  )
}
