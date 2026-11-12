/**
 * A single line of plain text, edited in place.
 *
 * Renders a `contenteditable` span that inherits all of its styling from
 * the element it sits in (e.g. `h1.title`), so it looks exactly like the
 * rendered text and the caret matches the font. It adds no focus or hover
 * styling, so placing the caret changes nothing on the page.
 *
 * React never renders the span's children: the browser owns the text while
 * the user types, and `value` is written to the DOM only when it differs
 * (after an undo, say), so typing never moves the caret.
 *
 * Undo and redo are routed to `onUndo` / `onRedo` instead of the browser's
 * own undo stack, which can't see edits made outside this span.
 */

import { useLayoutEffect, useRef } from 'react'
import { getTextRange, type TextRange } from './textRange.ts'

export interface EditableTextProps {
  value: string
  /** The new text, with the selection just before and just after the edit. */
  onEdit: (text: string, before: TextRange | null, after: TextRange | null) => void
  onUndo: () => void
  onRedo: () => void
  /** Called when the field loses focus (click-away, Enter, or Escape). */
  onLeave?: () => void
  /** Receives the span, so the owner can restore the selection after an undo. */
  elementRef?: (el: HTMLElement | null) => void
}

export function EditableText({ value, onEdit, onUndo, onRedo, onLeave, elementRef }: EditableTextProps) {
  const ref = useRef<HTMLSpanElement | null>(null)
  const before = useRef<TextRange | null>(null)

  useLayoutEffect(() => {
    const el = ref.current
    if (el && el.textContent !== value) el.textContent = value
  }, [value])

  // A native listener: React's onBeforeInput is a polyfill without
  // `inputType`.
  const handlers = useRef({ onUndo, onRedo })
  handlers.current = { onUndo, onRedo }
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const onBeforeInput = (e: InputEvent) => {
      switch (e.inputType) {
        // From the Edit menu, or a platform shortcut we didn't catch.
        case 'historyUndo':
          e.preventDefault()
          handlers.current.onUndo()
          return
        case 'historyRedo':
          e.preventDefault()
          handlers.current.onRedo()
          return
        // Single line.
        case 'insertParagraph':
        case 'insertLineBreak':
          e.preventDefault()
          return
      }
      before.current = getTextRange(el)
    }
    el.addEventListener('beforeinput', onBeforeInput)
    return () => el.removeEventListener('beforeinput', onBeforeInput)
  }, [])

  return (
    <span
      ref={(el) => {
        ref.current = el
        elementRef?.(el)
      }}
      className="qe-field"
      contentEditable="plaintext-only"
      suppressContentEditableWarning
      spellCheck={false}
      onKeyDown={(e) => {
        const mod = e.metaKey || e.ctrlKey
        const key = e.key.toLowerCase()
        if (mod && key === 'z') {
          e.preventDefault()
          if (e.shiftKey) onRedo()
          else onUndo()
        } else if (e.ctrlKey && !e.metaKey && key === 'y') {
          e.preventDefault()
          onRedo()
        } else if (e.key === 'Enter' || e.key === 'Escape') {
          e.preventDefault()
          e.currentTarget.blur()
        }
      }}
      onInput={(e) => {
        const el = e.currentTarget
        const text = el.textContent ?? ''
        // An emptied contenteditable can keep a stray <br>.
        if (text === '' && el.firstChild) el.replaceChildren()
        onEdit(text, before.current, getTextRange(el))
        before.current = null
      }}
      onPaste={(e) => {
        e.preventDefault()
        const text = e.clipboardData.getData('text/plain').replace(/\s*[\r\n]+\s*/g, ' ')
        e.currentTarget.ownerDocument.execCommand('insertText', false, text)
      }}
      onBlur={onLeave}
    />
  )
}
