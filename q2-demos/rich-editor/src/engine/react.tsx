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
 *
 * Whatever comes before the first field (a row of buttons that add
 * fields, say) is the caller's: an arrow key that would take the caret
 * back past the first field, or Backspace that removes the first field,
 * goes there through `onLeaveStart`.
 */

import {
  createElement,
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react'
import { flushSync } from 'react-dom'
import { addField, deleteBackward, deleteForward, enter, insertText, paste, select, selectAllStep } from './commands.ts'
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
  shownRange,
  targetRange,
  writeSelection,
} from './dom.ts'
import { apply, createEditor, fieldChanges, redo, settle, undo, type ApplyOptions, type Editor } from './editor.ts'
import { ATOM, caret, documentChanged, presentFields, selectedText, type Schema, type State } from './model.ts'
import { rebaseHistory } from './rebase.ts'
import type { FieldSource } from './source.ts'

export interface FieldProps {
  name: string
  /** The field's text; the field isn't rendered when it's undefined. */
  text: string | undefined
  /** Shown, faintly, while the field is empty (style `[data-placeholder]`). */
  placeholder?: string
  className?: string
  /** The element to render (a `span` unless given). */
  as?: string
  /** For one line of a field: where the line starts in the field's text. */
  start?: number
}

/**
 * One field. React never renders its children: its text is written to the
 * DOM only when it differs from the model, so the browser's own text
 * (during IME composition) is left alone.
 *
 * Text ending in a line break gets a trailing `<br>`: without one, the
 * browser draws no empty last line, so the caret can't sit on it until
 * something is typed. A `<br>` adds no text, so offsets are unchanged.
 * An empty field, or an empty line of a multi-line field, gets one too:
 * Firefox's arrow keys skip a block with nothing in it (an empty subtitle
 * between the title and the body, say). So an empty field isn't `:empty`;
 * it has `data-empty`, for styling its placeholder.
 */
export function Field({ name, text, placeholder, className, as = 'span', start }: FieldProps) {
  const ref = useRef<HTMLElement | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el || text === undefined) return
    const wantsBreak = text.endsWith('\n') || text === ''
    const hasBreak = el.lastChild?.nodeName === 'BR'
    if (el.textContent === text && hasBreak === wantsBreak) return
    el.replaceChildren(...(text === '' ? [] : [text]))
    if (wantsBreak) el.append(el.ownerDocument.createElement('br'))
  }, [text, start])
  if (text === undefined) return null
  return createElement(as, {
    ref,
    className,
    'data-field': name,
    'data-start': start,
    'data-placeholder': placeholder,
    'data-empty': text === '' ? '' : undefined,
  })
}

export interface LinesProps {
  name: string
  /** The field's text; nothing is rendered when it's undefined. */
  text: string | undefined
  className?: string
  /** The element to render for each line (a `p` unless given). */
  as?: string
  /** What to show for the `index`th line that can't be edited (`ATOM`). */
  renderAtom: (index: number) => ReactNode
}

/**
 * A multi-line field as one element per line. The lines that can't be
 * edited are rendered by the caller, and get no caret.
 */
export function Lines({ name, text, className, as = 'p', renderAtom }: LinesProps) {
  if (text === undefined) return null
  let start = 0
  let atoms = 0
  return text.split('\n').map((line, i) => {
    const at = start
    start += line.length + 1
    return line === ATOM ? (
      <Fragment key={i}>{renderAtom(atoms++)}</Fragment>
    ) : (
      <Field key={i} name={name} text={line} className={className} as={as} start={at} />
    )
  })
}

export interface FieldEditor {
  state: State
  editor: Editor
  /** Whether edits are accepted now (see `FieldSource.writable`). */
  writable: boolean
  /** Add `field`, empty, if it's absent, and put the caret at its start. An edit like any other. */
  add(field: string): void
  /** Put the caret at the start of the first field (none: nothing happens). */
  focusStart(): void
}

/** How the caret left the fields before the first one. */
export interface LeaveStart {
  /** Up from the first line: the caret's horizontal position. Left or Backspace: none. */
  x?: number
  /** Backspace removed the first field, which was empty: its name. */
  removed?: string
}

export interface FieldEditorOptions {
  /** Put the caret at the start of the first field once `root` appears. */
  autoFocus?: boolean
  /**
   * Left at the start of the first field, Up on its first line, or
   * Backspace removing it, empty: the caret leaves the fields. The caller
   * takes the focus somewhere before them; without this, the caret stays.
   */
  onLeaveStart?: (how: LeaveStart) => void
}

/**
 * Edit `schema`'s fields of the document `source` inside `root`. Another
 * `source` starts over, clearing the history.
 */
export function useFieldEditor(
  root: HTMLElement | null,
  schema: Schema,
  source: FieldSource,
  { autoFocus = false, onLeaveStart }: FieldEditorOptions = {},
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
  // Whether the last key down was Backspace, held down and repeating.
  const repeatingBackspace = useRef(false)
  const leaveStart = useRef(onLeaveStart)
  leaveStart.current = onLeaveStart

  /**
   * Make `next` the editor's state. A change to the fields' text is
   * written to the document, unless it came from the document (`from`).
   */
  const commit = useCallback(
    (next: Editor, restoreSelection: boolean, from: 'editor' | 'document' = 'editor') => {
      const previous = latest.current
      if (next === previous) return
      if (from === 'editor' && documentChanged(schema, previous.present.fields, next.present.fields)) {
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
    if (first !== undefined) commit(apply(h, select(schema, h.present, caret(first, 0)), { schema }), true)
  }, [root, autoFocus, schema, commit])

  const add = useCallback(
    (field: string) => {
      if (!source.writable()) return
      const h = latest.current
      commit(apply(h, addField(schema, h.present, field), { schema, before: h.present.selection }), true)
    },
    [schema, source, commit],
  )

  const focusStart = useCallback(() => {
    const h = latest.current
    const [first] = presentFields(schema, h.present.fields)
    if (first !== undefined) commit(apply(h, select(schema, h.present, caret(first, 0)), { schema }), true)
  }, [schema, commit])

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
      commit(apply(h, command(at), { ...options, before, schema }), true)
    }

    /** Delete backward (`inputType`), over `target` when the browser gave one. */
    const backspace = (inputType: string, target?: ReturnType<typeof targetRange>) => {
      const was = latest.current.present
      const repeat = repeatingBackspace.current
      edit((s) => deleteBackward(schema, s, target, { repeat }), { inputType })
      const now = latest.current.present
      if (was.selection && !now.selection) {
        // Backspace removed the first field: the caret has left the fields.
        const removed = schema.order.find((f) => was.fields[f] !== undefined && now.fields[f] === undefined)
        leaveStart.current?.({ removed })
      }
    }

    /** Whether the caret is at the very start of the first field (nothing before it to delete). */
    const atDocumentStart = () => {
      const selection = currentSelection()
      const [first] = presentFields(schema, latest.current.present.fields)
      return (
        selection !== null &&
        selection.anchor.field === first &&
        selection.anchor.offset === 0 &&
        selection.focus.field === first &&
        selection.focus.offset === 0
      )
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
        backspace(type, target())
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
      const el = fieldOf(root, root.ownerDocument.getSelection()?.focusNode ?? null)
      if (selection && field && el) {
        // The element shows all of the field's text, or one line of it.
        const was = h.present.fields[field] ?? ''
        const shown = shownRange(el, was)
        const text = was.slice(0, shown.start) + (el.textContent ?? '') + was.slice(shown.end)
        if (text !== was) {
          if (source.writable()) {
            let edited = apply(
              h,
              { fields: { ...h.present.fields, [field]: text }, selection },
              { inputType: 'insertText', schema },
            )
            // A change to the document that arrived meanwhile: make it to
            // the editor's state as it was, then to the composed text too,
            // so that what's written has both.
            const before = base.current
            const now = source.get()
            if (now !== before) {
              followDocument()
              edited = rebaseHistory(schema, edited, before, now)
            }
            commit(edited, true)
          } else {
            // Not accepted: put the element's text back.
            el.textContent = was.slice(shown.start, shown.end)
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
      const fromModel = selection ? selectedText(schema, present.fields, selection) : null
      // What the page shows, when the selection covers something that
      // isn't the fields' text.
      const text = fromModel === null || fromModel.includes(ATOM) ? String(doc.getSelection() ?? '') : fromModel
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
     * or doesn't happen. Returns false to leave the key to the browser, and
     * 'left' when the caret left the fields (for what's before them).
     */
    const arrow = (e: KeyboardEvent): boolean | 'left' => {
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
      const fields = fieldElements(root)
      if (granularity === 'documentboundary') {
        // The start of the first field, or the end of the last.
        const to = key.forward ? edge(fields[fields.length - 1], 'end') : edge(fields[0], 'start')
        const from = e.shiftKey ? saved.anchor : to
        selection.setBaseAndExtent(from.node, from.offset, to.node, to.offset)
        return true
      }
      if (granularity === 'character' && !e.shiftKey && !selection.isCollapsed) {
        // Left and Right put the caret at that end of a selection.
        // (Firefox's `modify` would move it a character from the focus.)
        const range = selection.getRangeAt(0)
        if (key.forward) selection.collapse(range.endContainer, range.endOffset)
        else selection.collapse(range.startContainer, range.startOffset)
        return true
      }
      const x = caretX(doc, saved.focus.node, saved.focus.offset) ?? origin.getBoundingClientRect().left
      // Back past the first field (Left at its start, Up on its first
      // line): out of the fields, to what's before them. Checked before the
      // browser moves the caret, which at the very start may not move at all.
      const leaving =
        !key.forward &&
        !e.shiftKey &&
        fields.indexOf(origin) === 0 &&
        (key.vertical
          ? granularity === 'line' && onFirstLine(origin, saved.focus.node, saved.focus.offset)
          : // Not the key for the line's start (like Home, it stays in the line).
            granularity !== 'lineboundary' && atStart(origin, saved.focus.node, saved.focus.offset))
      if (leaving && leaveStart.current) {
        leaveStart.current(key.vertical ? { x } : {})
        return 'left'
      }
      selection.modify(e.shiftKey ? 'extend' : 'move', key.direction, granularity)
      if (fieldOf(root, selection.focusNode)) return true

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
      repeatingBackspace.current = e.key === 'Backspace' && e.repeat
      if (primary && key === 'z') {
        e.preventDefault()
        history(e.shiftKey ? redo : undo)
      } else if (primary && !IS_MAC && key === 'y') {
        e.preventDefault()
        history(redo)
      } else if (doc.activeElement !== root || composing.current) {
        return
      } else if (e.key === 'Enter' && e.shiftKey) {
        // A line break within a line: there are none. (WebKit would
        // report it as a new paragraph.)
        e.preventDefault()
      } else if (primary && key === 'a') {
        e.preventDefault()
        edit((s) => selectAllStep(schema, s))
      } else if (e.key === 'Backspace' && !e.altKey && !e.metaKey && !e.ctrlKey && atDocumentStart()) {
        // WebKit sends no `beforeinput` for a Backspace with nothing before
        // the caret to delete, but here it still has work: an empty first
        // field is removed.
        e.preventDefault()
        backspace('deleteContentBackward')
      } else {
        // Not once the caret has left the fields: reading the page's
        // selection back would put the caret (and the focus) back in them.
        const moved = arrow(e)
        if (moved === true) syncSelection()
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
      flushSync(() => commit(apply(h, select(schema, h.present, selection), { schema }), false))
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
      commit(apply(h, select(schema, h.present, null), { schema }), false)
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

  return { state: editor.present, editor, writable, add, focusStart }
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

/** Whether a DOM point is at the very start of `field`'s text. */
function atStart(field: HTMLElement, node: Node, offset: number): boolean {
  const range = field.ownerDocument.createRange()
  range.selectNodeContents(field)
  range.setEnd(node, offset)
  return range.toString() === ''
}

/** Whether a caret at a DOM point is on `field`'s first line. */
function onFirstLine(field: HTMLElement, node: Node, offset: number): boolean {
  const range = field.ownerDocument.createRange()
  range.setStart(node, offset)
  const caretTop = range.getClientRects()[0]?.top ?? field.getBoundingClientRect().top
  const style = getComputedStyle(field)
  const lineHeight = parseFloat(style.lineHeight) || parseFloat(style.fontSize) * 1.2
  return caretTop - field.getBoundingClientRect().top < lineHeight * 0.75
}

/** The horizontal position of a caret at a DOM point. */
function caretX(doc: Document, node: Node, offset: number): number | null {
  const range = doc.createRange()
  range.setStart(node, offset)
  const rects = range.getClientRects()
  return rects.length > 0 ? rects[0].left : null
}
