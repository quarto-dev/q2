/**
 * React bindings: `useFieldEditor` turns an element into the editing root
 * and owns every edit made in it, and `Field` renders one field's text
 * inside it.
 *
 * The browser moves the caret and selection (arrows, Shift-select,
 * dragging, the goal column across lines), except that an arrow key never
 * takes the caret out of the fields: one that would goes to the next field
 * that way, or nowhere. Mouse selections that reach outside the fields
 * (into the author grid, say) are left as they are; edits apply to the
 * parts of the fields they cover, and to nothing when they cover none. Every edit is intercepted
 * before the browser makes it (`beforeinput`, paste, cut, drop), turned
 * into a model edit, and rendered back, with the selection restored from
 * the model. The one exception is IME composition, which can't be
 * cancelled: the browser edits the field's text itself, and the result is
 * read back when the composition ends.
 *
 * The fields belong to a document (`FieldSource`). Every edit is written
 * to it as it's made, and a change to it from elsewhere (a collaborator)
 * is made to the editor's state too, keeping the caret, the selection and
 * the undo history (`rebase.ts`). Nothing can be edited while the document
 * isn't writable.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import { flushSync } from 'react-dom'
import { deleteBackward, deleteForward, enter, insertText, paste, select, selectAllStep } from './commands.ts'
import {
  edge,
  fieldElements,
  fieldOf,
  markIslands,
  pointOnLine,
  positionAt,
  readMouseSelection,
  readSelection,
  selectionOutside,
  targetRange,
  writeSelection,
} from './dom.ts'
import { apply, createEditor, fieldChanges, redo, settle, undo, type ApplyOptions, type Editor } from './editor.ts'
import { caret, contentChanged, presentFields, selectedText, type Schema, type State } from './model.ts'
import { rebaseHistory } from './rebase.ts'
import type { FieldSource } from './source.ts'

export interface FieldProps {
  name: string
  /** The field's text; the field isn't rendered when it's undefined. */
  text: string | undefined
  /** Shown, faintly, while the field is empty (style `[data-placeholder]`). */
  placeholder?: string
  className?: string
}

/**
 * One field. React never renders its children: its text is written to the
 * DOM only when it differs from the model, so the browser's own text
 * (during IME composition) is left alone.
 *
 * Text ending in a line break gets a trailing `<br>`: without one, the
 * browser draws no empty last line, so the caret can't sit on it until
 * something is typed. A `<br>` adds no text, so offsets are unchanged.
 */
export function Field({ name, text, placeholder, className }: FieldProps) {
  const ref = useRef<HTMLSpanElement | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el || text === undefined) return
    const wantsBreak = text.endsWith('\n')
    const hasBreak = el.lastChild?.nodeName === 'BR'
    if (el.textContent === text && hasBreak === wantsBreak) return
    el.replaceChildren(...(text === '' ? [] : [text]))
    if (wantsBreak) el.append(el.ownerDocument.createElement('br'))
  }, [text])
  if (text === undefined) return null
  return <span ref={ref} className={className} data-field={name} data-placeholder={placeholder} />
}

export interface FieldEditor {
  state: State
  editor: Editor
  /** Whether edits are accepted now (see `FieldSource.writable`). */
  writable: boolean
}

export interface FieldEditorOptions {
  /** Put the caret at the start of the first field once `root` appears. */
  autoFocus?: boolean
}

/**
 * Edit `schema`'s fields of the document `source` inside `root`. Another
 * `source` starts over, clearing the history.
 */
export function useFieldEditor(
  root: HTMLElement | null,
  schema: Schema,
  source: FieldSource,
  { autoFocus = false }: FieldEditorOptions = {},
): FieldEditor {
  const [edited, setEdited] = useState(source)
  const [editor, setEditor] = useState(() => createEditor(source.get()))
  // The document's fields as the editor last saw them.
  const base = useRef(source.get())
  if (edited !== source) {
    setEdited(source)
    setEditor(createEditor(source.get()))
    base.current = source.get()
  }
  const writable = useSyncExternalStore(source.subscribe, source.writable)

  // Events can arrive faster than React renders; they read and write this.
  const latest = useRef(editor)
  latest.current = editor
  // Whether the next render puts the page's selection back from the model.
  const restore = useRef(false)
  // Whether the editor is writing to the document, or the browser is
  // composing text (IME) in a field.
  const writing = useRef(false)
  const composing = useRef(false)

  /**
   * Make `next` the editor's state. A change to the fields' text is
   * written to the document, unless it came from the document (`from`).
   */
  const commit = useCallback(
    (next: Editor, restoreSelection: boolean, from: 'editor' | 'document' = 'editor') => {
      const previous = latest.current
      if (next === previous) return
      if (from === 'editor' && contentChanged(previous.present.fields, next.present.fields)) {
        writing.current = true
        try {
          source.set(fieldChanges(schema, previous.present.fields, next.present.fields))
        } finally {
          writing.current = false
        }
        base.current = source.get()
      }
      next = settle(schema, next, base.current)
      latest.current = next
      if (restoreSelection) restore.current = true
      setEditor(next)
    },
    [schema, source],
  )

  // A change to the document that isn't the editor's own: make it to the
  // editor's state too. (Not while the browser is composing text in a
  // field: it's made when the composition ends.)
  const followDocument = useCallback(() => {
    if (writing.current || composing.current) return
    const before = base.current
    const now = source.get()
    if (now === before) return
    base.current = now
    const next = rebaseHistory(schema, latest.current, before, now)
    const focused = root !== null && root.ownerDocument.activeElement === root
    commit(next, focused && next.present.selection !== null, 'document')
  }, [schema, source, root, commit])

  useEffect(() => {
    const unsubscribe = source.subscribe(followDocument)
    followDocument()
    return unsubscribe
  }, [source, followDocument])

  // Once per mount, not per reset: a source change mustn't steal focus.
  const autoFocused = useRef(false)
  useEffect(() => {
    if (!root || !autoFocus || autoFocused.current) return
    autoFocused.current = true
    const h = latest.current
    const [first] = presentFields(schema, h.present.fields)
    if (first !== undefined) commit(apply(h, select(schema, h.present, caret(first, 0))), true)
  }, [root, autoFocus, schema, commit])

  useLayoutEffect(() => {
    if (!root) return
    root.contentEditable = 'true'
    root.spellcheck = false
    return () => {
      root.removeAttribute('contenteditable')
    }
  }, [root])

  // After every render: keep the markup around the fields non-editable and
  // put the selection back after our own edits. (Runs after `Field`s have
  // written their text.)
  useLayoutEffect(() => {
    if (!root) return
    markIslands(root)
    if (!restore.current) return
    restore.current = false
    const selection = editor.present.selection
    if (selection) writeSelection(root, selection)
  })

  useEffect(() => {
    if (!root) return
    const doc = root.ownerDocument
    // Whether the mouse button is down, and whether the page's selection
    // was last set by the mouse (rather than the keyboard).
    let dragging = false
    let byMouse = false

    /** The page's selection in model terms, snapped the way it was made. */
    const currentSelection = () =>
      byMouse ? readMouseSelection(root) : readSelection(root, latest.current.present.selection)

    /** Apply a command to the state with the page's current selection. */
    const edit = (command: (state: State) => State, options: Omit<ApplyOptions, 'before'> = {}) => {
      // Nothing to edit with the selection outside the fields, or while
      // the document can't be written.
      if (selectionOutside(root) === 'all' || !source.writable()) return
      const h = latest.current
      const before = currentSelection() ?? h.present.selection
      const at = select(schema, h.present, before)
      commit(apply(h, command(at), { ...options, before }), true)
    }

    const history = (step: (editor: Editor) => Editor) => {
      if (source.writable()) commit(step(latest.current), true)
    }

    const onBeforeInput = (e: InputEvent) => {
      if (composing.current || e.inputType === 'insertCompositionText') return
      e.preventDefault()
      const type = e.inputType
      const target = () => targetRange(root, e)
      if (type === 'historyUndo') history(undo)
      else if (type === 'historyRedo') history(redo)
      else if (type === 'insertText') edit((s) => insertText(schema, s, e.data ?? ''), { inputType: type })
      else if (type === 'insertReplacementText') {
        // Autocorrect or a spelling suggestion: replace the target range.
        const range = target()
        const text = e.dataTransfer?.getData('text/plain') ?? e.data ?? ''
        edit(
          (s) => insertText(schema, range ? select(schema, s, { anchor: range.from, focus: range.to }) : s, text),
          { inputType: type },
        )
      } else if (type === 'insertParagraph') edit((s) => enter(schema, s), { inputType: type })
      else if (type.startsWith('delete') && type.endsWith('Backward')) {
        edit((s) => deleteBackward(schema, s, target()), { inputType: type })
      } else if (type.startsWith('delete') && type.endsWith('Forward')) {
        edit((s) => deleteForward(schema, s, target()), { inputType: type })
      }
      // Everything else (line breaks, formatting, drag and drop) is ignored.
    }

    const onCompositionStart = () => {
      composing.current = true
    }
    const onCompositionEnd = () => {
      composing.current = false
      // The browser has already changed the field's text; read it back.
      const h = latest.current
      const selection = readSelection(root)
      const field = selection?.focus.field
      const el = field && root.querySelector<HTMLElement>(`[data-field="${field}"]`)
      if (selection && field && el) {
        const text = el.textContent ?? ''
        if (text !== h.present.fields[field]) {
          if (source.writable()) {
            commit(
              apply(h, { fields: { ...h.present.fields, [field]: text }, selection }, { inputType: 'insertText' }),
              true,
            )
          } else {
            // Not accepted: put the field's text back.
            el.textContent = h.present.fields[field] ?? ''
          }
        }
      }
      // Changes to the document that arrived meanwhile.
      followDocument()
    }

    const onPaste = (e: ClipboardEvent) => {
      e.preventDefault()
      const text = e.clipboardData?.getData('text/plain') ?? ''
      edit((s) => paste(schema, s, text), { inputType: 'insertFromPaste' })
    }

    /**
     * Put the selected text on the clipboard: the fields' text, one line
     * per field, or what the page shows selected when the selection reaches
     * outside the fields.
     */
    const copySelection = (e: ClipboardEvent) => {
      e.preventDefault()
      const { present } = latest.current
      const selection = selectionOutside(root) === 'none' ? currentSelection() : null
      const text = selection ? selectedText(schema, present.fields, selection) : String(doc.getSelection() ?? '')
      e.clipboardData?.setData('text/plain', text)
    }
    const onCopy = (e: ClipboardEvent) => copySelection(e)
    const onCut = (e: ClipboardEvent) => {
      copySelection(e)
      edit((s) => insertText(schema, s, ''), { inputType: 'deleteByCut' })
    }
    const onDrop = (e: DragEvent) => e.preventDefault()

    /**
     * Move the caret for an arrow key, the way the browser would, except
     * that it never leaves the fields: a move that would goes to the next
     * field that way (on the nearest line, at the same x, for Up / Down),
     * or doesn't happen. Returns false to leave the key to the browser.
     */
    const arrow = (e: KeyboardEvent): boolean => {
      const key = ARROW_KEYS[e.key]
      const selection = doc.getSelection()
      if (!key || !selection?.anchorNode || !selection.focusNode) return false
      const origin = fieldOf(root, selection.focusNode)
      if (!origin) {
        // The caret is outside the fields (after a click on an author): go
        // to the nearest field that way.
        const to = positionAt(root, selection.focusNode, selection.focusOffset, key.forward ? 'forward' : 'backward')
        if (!to) return false
        e.preventDefault()
        writeSelection(root, caret(to.field, to.offset))
        return true
      }
      const granularity = arrowGranularity(e, key.vertical)
      if (!granularity) return false
      e.preventDefault()

      const saved = {
        anchor: { node: selection.anchorNode, offset: selection.anchorOffset },
        focus: { node: selection.focusNode, offset: selection.focusOffset },
      }
      const x = caretX(doc, saved.focus.node, saved.focus.offset) ?? origin.getBoundingClientRect().left
      selection.modify(e.shiftKey ? 'extend' : 'move', key.direction, granularity)
      if (fieldOf(root, selection.focusNode)) return true

      const fields = fieldElements(root)
      const target = fields[fields.indexOf(origin) + (key.forward ? 1 : -1)]
      let to = saved.focus
      if (target) {
        to =
          key.vertical && granularity === 'line'
            ? pointOnLine(target, x, key.forward ? 'first' : 'last')
            : edge(target, key.forward ? 'start' : 'end')
      }
      const from = e.shiftKey || !target ? saved.anchor : to
      selection.setBaseAndExtent(from.node, from.offset, to.node, to.offset)
      return true
    }

    const onKeyDown = (e: KeyboardEvent) => {
      const key = e.key.toLowerCase()
      const primary = IS_MAC ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
      if (NAVIGATION_KEYS.has(e.key)) byMouse = false
      if (primary && key === 'z') {
        e.preventDefault()
        history(e.shiftKey ? redo : undo)
      } else if (primary && !IS_MAC && key === 'y') {
        e.preventDefault()
        history(redo)
      } else if (doc.activeElement !== root || composing.current) {
        return
      } else if (primary && key === 'a') {
        e.preventDefault()
        edit((s) => selectAllStep(schema, s))
      } else if (arrow(e)) {
        syncSelection()
      }
    }

    // Reads the page's selection into the model. `selectionchange` arrives
    // up to a frame late, so `keyup` and `mouseup`, which follow the
    // browser's caret move, sync it as soon as it has happened.
    const syncSelection = () => {
      if (composing.current || doc.activeElement !== root) return
      const h = latest.current
      const selection = currentSelection()
      // Rendered at once, like an edit: an empty field the caret just left
      // disappears in the same frame.
      flushSync(() => commit(apply(h, select(schema, h.present, selection)), false))
      // A keyboard move the arrow handling didn't cover (Home, Page Down)
      // put the caret outside the fields: put it where the model has it. A
      // mouse selection there is the user's, and is left alone.
      if (selection && !byMouse && selectionOutside(root) !== 'none') writeSelection(root, selection)
    }
    const onMouseDown = () => {
      dragging = true
      byMouse = true
    }
    const onMouseUp = () => {
      if (!dragging) return
      dragging = false
      syncSelection()
    }
    const onBlur = () => {
      const h = latest.current
      commit(apply(h, select(schema, h.present, null)), false)
    }

    root.addEventListener('beforeinput', onBeforeInput)
    root.addEventListener('compositionstart', onCompositionStart)
    root.addEventListener('compositionend', onCompositionEnd)
    root.addEventListener('paste', onPaste)
    root.addEventListener('copy', onCopy)
    root.addEventListener('cut', onCut)
    root.addEventListener('drop', onDrop)
    root.addEventListener('blur', onBlur)
    root.addEventListener('keyup', syncSelection)
    root.addEventListener('mousedown', onMouseDown)
    doc.addEventListener('mouseup', onMouseUp)
    doc.addEventListener('keydown', onKeyDown)
    doc.addEventListener('selectionchange', syncSelection)
    return () => {
      root.removeEventListener('beforeinput', onBeforeInput)
      root.removeEventListener('compositionstart', onCompositionStart)
      root.removeEventListener('compositionend', onCompositionEnd)
      root.removeEventListener('paste', onPaste)
      root.removeEventListener('copy', onCopy)
      root.removeEventListener('cut', onCut)
      root.removeEventListener('drop', onDrop)
      root.removeEventListener('blur', onBlur)
      root.removeEventListener('keyup', syncSelection)
      root.removeEventListener('mousedown', onMouseDown)
      doc.removeEventListener('mouseup', onMouseUp)
      doc.removeEventListener('keydown', onKeyDown)
      doc.removeEventListener('selectionchange', syncSelection)
    }
  }, [root, schema, source, commit, followDocument])

  return { state: editor.present, editor, writable }
}

/** Keys that move the selection: after one, it is no longer the mouse's. */
const NAVIGATION_KEYS = new Set([
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'ArrowDown',
  'Home',
  'End',
  'PageUp',
  'PageDown',
])

const IS_MAC = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)

const ARROW_KEYS: Record<
  string,
  { direction: 'left' | 'right' | 'backward' | 'forward'; forward: boolean; vertical: boolean }
> = {
  ArrowLeft: { direction: 'left', forward: false, vertical: false },
  ArrowRight: { direction: 'right', forward: true, vertical: false },
  ArrowUp: { direction: 'backward', forward: false, vertical: true },
  ArrowDown: { direction: 'forward', forward: true, vertical: true },
}

/** How far an arrow key moves with its modifiers, or null to leave it to the browser. */
function arrowGranularity(e: KeyboardEvent, vertical: boolean): string | null {
  const word = IS_MAC ? e.altKey && !e.metaKey && !e.ctrlKey : e.ctrlKey && !e.altKey && !e.metaKey
  const boundary = IS_MAC && e.metaKey && !e.altKey && !e.ctrlKey
  const plain = !e.altKey && !e.metaKey && !e.ctrlKey
  if (vertical) return plain ? 'line' : boundary ? 'documentboundary' : null
  return plain ? 'character' : word ? 'word' : boundary ? 'lineboundary' : null
}

/** The horizontal position of a caret at a DOM point. */
function caretX(doc: Document, node: Node, offset: number): number | null {
  const range = doc.createRange()
  range.setStart(node, offset)
  const rects = range.getClientRects()
  return rects.length > 0 ? rects[0].left : null
}
