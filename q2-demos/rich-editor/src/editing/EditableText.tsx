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
 * own undo stack, which can't see edits made outside this span. Enter, Up /
 * Down on the first / last line, and Left / Right at the start / end are
 * routed to the owner, which moves the caret between fields.
 */

import { useLayoutEffect, useRef } from 'react'
import { caretLine } from './lines.ts'
import { getTextRange, type TextRange } from './textRange.ts'

export type ArrowDirection = 'up' | 'down' | 'left' | 'right'

export interface EditableTextProps {
  value: string
  /** Shown, faintly, while the field is empty. */
  placeholder?: string
  /**
   * The new text, the edit's `InputEvent.inputType`, and the selection just
   * before and just after the edit.
   */
  onEdit: (text: string, inputType: string, before: TextRange | null, after: TextRange | null) => void
  onUndo: () => void
  onRedo: () => void
  /** Enter was pressed. The field never inserts a line break itself. */
  onEnter?: () => void
  /**
   * The caret is about to leave the field: Up on the first line, Down on
   * the last, Left at the start, or Right at the end. `x` is the caret's
   * horizontal position. Return true when the owner moved the caret
   * elsewhere; otherwise the browser's default applies.
   */
  onArrowOut?: (direction: ArrowDirection, x: number) => boolean
  /** Called when the field loses focus. */
  onLeave?: () => void
  /** Receives the span, so the owner can restore the selection after an undo. */
  elementRef?: (el: HTMLElement | null) => void
}

const ARROWS: Record<string, ArrowDirection> = {
  ArrowUp: 'up',
  ArrowDown: 'down',
  ArrowLeft: 'left',
  ArrowRight: 'right',
}

export function EditableText({
  value,
  placeholder,
  onEdit,
  onUndo,
  onRedo,
  onEnter,
  onArrowOut,
  onLeave,
  elementRef,
}: EditableTextProps) {
  const ref = useRef<HTMLSpanElement | null>(null)
  // The edit in progress, from `beforeinput` until `input`.
  const pending = useRef<{ inputType: string; before: TextRange | null } | null>(null)
  // Set while a paste inserts its text, which reports as `insertText`.
  const pasting = useRef(false)

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
      pending.current = {
        inputType: pasting.current ? 'insertFromPaste' : e.inputType,
        before: getTextRange(el),
      }
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
      data-placeholder={placeholder}
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
        } else if (e.key === 'Enter') {
          e.preventDefault()
          if (!e.shiftKey && !mod) onEnter?.()
        } else if (e.key in ARROWS && !e.shiftKey && !mod && !e.altKey && onArrowOut) {
          const el = e.currentTarget
          const line = caretLine(el)
          if (!line) return
          const direction = ARROWS[e.key]
          const range = getTextRange(el)
          const atEdge = {
            up: line.onFirstLine,
            down: line.onLastLine,
            left: range?.start === 0,
            right: range?.end === (el.textContent ?? '').length,
          }[direction]
          if (atEdge && onArrowOut(direction, line.x)) e.preventDefault()
        }
      }}
      onInput={(e) => {
        const el = e.currentTarget
        const text = el.textContent ?? ''
        // An emptied contenteditable can keep a stray <br>.
        if (text === '' && el.firstChild) el.replaceChildren()
        const { inputType, before } = pending.current ?? { inputType: '', before: null }
        pending.current = null
        onEdit(text, inputType, before, getTextRange(el))
      }}
      onPaste={(e) => {
        e.preventDefault()
        const text = e.clipboardData.getData('text/plain').replace(/\s*[\r\n]+\s*/g, ' ')
        pasting.current = true
        try {
          e.currentTarget.ownerDocument.execCommand('insertText', false, text)
        } finally {
          pasting.current = false
        }
      }}
      onBlur={onLeave}
    />
  )
}
